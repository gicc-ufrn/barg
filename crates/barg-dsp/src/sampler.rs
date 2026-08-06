// SPDX-License-Identifier: Apache-2.0
//! SamplePlayer — sampler multi-articulação RT-safe para o baixo (Gap C, fase B2/B3).
//!
//! Modelo: um `SampleBank` imutável (zonas mapeando nota/velocity/round-robin → PCM) é
//! carregado FORA do audio thread e publicado por troca atômica de ponteiro (ver
//! `casa13-ffi`). A voz de playback (`SampledBassVoice`) só LÊ buffers pré-carregados —
//! sem alloc/lock/I-O no caminho quente. O pitch é por razão de reprodução (resample on
//! the fly), então a conversão de taxa (amostra 44.1k → engine) é dobrada nessa razão,
//! dispensando pré-resample. Ver `docs/research-bass-sampler-sequencer.md §5.2/§5.3`.

use std::sync::Arc;
use crate::bass::BassVoice;

/// Ataque/release curtos (s) para evitar cliques em note_on / troca de nota.
const ATTACK_S: f32 = 0.003;
const RELEASE_S: f32 = 0.030;

#[inline]
fn midi_to_hz(note: f64) -> f64 {
    440.0 * 2.0_f64.powf((note - 69.0) / 12.0)
}

/// Uma amostra decodificada (mono, PCM f32) + sua taxa nativa.
pub struct Sample {
    pub pcm: Vec<f32>,
    pub sample_rate: f64,
}

/// Zona de mapeamento: uma amostra ativa numa faixa de nota/velocity (+ round-robin).
pub struct SampleZone {
    pub sample: Arc<Sample>,
    pub root_hz: f64, // frequência da nota-raiz (pitch_keycenter)
    pub lo_key: u8,
    pub hi_key: u8,
    pub lo_vel: u8,
    pub hi_vel: u8,
    pub gain: f32,       // volume linear (de `volume` dB)
    pub tune_ratio: f64, // de `tune` (cents) → razão de pitch
}

/// Banco imutável de zonas. Selecionado por (nota, velocity, contador de round-robin).
pub struct SampleBank {
    pub zones: Vec<SampleZone>,
    pub name: String,
}

impl SampleBank {
    /// Índices das zonas que casam com (nota, velocity), na ordem de declaração.
    /// O caller escolhe entre elas por round-robin.
    fn matching(&self, note: u8, vel: u8) -> impl Iterator<Item = usize> + '_ {
        self.zones.iter().enumerate().filter_map(move |(i, z)| {
            if note >= z.lo_key && note <= z.hi_key && vel >= z.lo_vel && vel <= z.hi_vel {
                Some(i)
            } else {
                None
            }
        })
    }

    /// Seleciona a zona para (nota, velocity), rotacionando os round-robins por `rr`.
    /// Público para permitir verificação do mapeamento (ex.: parser SFZ no FFI).
    pub fn select(&self, note: u8, vel: u8, rr: u32) -> Option<usize> {
        let count = self.matching(note, vel).count();
        if count == 0 {
            return None;
        }
        let pick = (rr as usize) % count;
        self.matching(note, vel).nth(pick)
    }
}

#[derive(Clone, Copy)]
struct PoolVoice {
    active: bool,
    zone_idx: usize,
    pos: f64,      // posição de leitura na amostra (frames)
    rate: f64,     // incremento de leitura por frame de saída
    start: i32,    // atraso de latência
    gain: f32,
    env: f32,
    releasing: bool,
    age: u64,      // para voice-stealing
}

impl Default for PoolVoice {
    fn default() -> Self {
        Self {
            active: false,
            zone_idx: 0,
            pos: 0.0,
            rate: 1.0,
            start: 0,
            gain: 0.0,
            env: 0.0,
            releasing: false,
            age: 0,
        }
    }
}

/// Voz sampleada RT-safe: pool de `N` vozes lendo um `SampleBank` imutável.
/// `choke = true` dá comportamento mono-legato (o baixo: solta as vozes ativas ao atacar
/// a nova); `choke = false` é polifônico (temas/acordes — Gap E). Só LÊ buffers
/// pré-carregados: sem alloc/lock/panic no caminho quente.
pub struct SampledVoice<const N: usize> {
    engine_sr: f64,
    latency_frames: i32,
    bank: Option<Arc<SampleBank>>,
    voices: [PoolVoice; N],
    rr_counter: u32,
    age_counter: u64,
    atk_coef: f32,
    rel_coef: f32,
    choke: bool,
}

/// Baixo (Gap C): 6 vozes, mono-legato (choke).
pub type SampledBassVoice = SampledVoice<6>;
/// Tema (Gap E): 16 vozes, polifônico.
pub type SampledPolyVoice = SampledVoice<16>;

impl<const N: usize> SampledVoice<N> {
    pub fn new(sample_rate: f64, choke: bool) -> Self {
        let mut s = Self {
            engine_sr: sample_rate,
            latency_frames: 0,
            bank: None,
            voices: [PoolVoice::default(); N],
            rr_counter: 0,
            age_counter: 0,
            atk_coef: 0.0,
            rel_coef: 0.0,
            choke,
        };
        s.recalc_coefs();
        s
    }

    fn recalc_coefs(&mut self) {
        self.atk_coef = (-1.0 / (ATTACK_S as f64 * self.engine_sr)).exp() as f32;
        self.rel_coef = (-1.0 / (RELEASE_S as f64 * self.engine_sr)).exp() as f32;
    }

    /// Troca o banco ativo. Devolve o antigo para liberação DIFERIDA (fora do audio
    /// thread) — nunca dropar `Arc<SampleBank>` aqui.
    pub fn set_bank(&mut self, bank: Option<Arc<SampleBank>>) -> Option<Arc<SampleBank>> {
        core::mem::replace(&mut self.bank, bank)
    }

    pub fn has_bank(&self) -> bool {
        self.bank.is_some()
    }

    /// Nº de zonas do banco ativo (para o HUD).
    pub fn zone_count(&self) -> usize {
        self.bank.as_ref().map(|b| b.zones.len()).unwrap_or(0)
    }

    fn pick_voice(&self) -> usize {
        // Preferir voz inativa; senão roubar a mais antiga.
        let mut best = 0usize;
        let mut best_age = u64::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if !v.active {
                return i;
            }
            if v.age < best_age {
                best_age = v.age;
                best = i;
            }
        }
        best
    }
}

impl<const N: usize> BassVoice for SampledVoice<N> {
    fn note_on(&mut self, midi: u8, frame_offset: i32, gain: f32) {
        let bank = match &self.bank {
            Some(b) => b,
            None => return, // nenhum instrumento carregado → silêncio
        };
        let vel = (gain.clamp(0.0, 1.0) * 127.0).round() as u8;
        let zi = match bank.select(midi, vel, self.rr_counter) {
            Some(i) => i,
            None => return,
        };
        self.rr_counter = self.rr_counter.wrapping_add(1);
        let zone = &bank.zones[zi];
        let target_hz = midi_to_hz(midi as f64);
        let rate = (target_hz / zone.root_hz)
            * (zone.sample.sample_rate / self.engine_sr)
            * zone.tune_ratio;
        let zgain = gain * zone.gain;

        // Mono-legato: solta as vozes ativas antes de atacar (só quando `choke`).
        if self.choke {
            for v in self.voices.iter_mut() {
                if v.active {
                    v.releasing = true;
                }
            }
        }

        self.age_counter += 1;
        let idx = self.pick_voice();
        self.voices[idx] = PoolVoice {
            active: true,
            zone_idx: zi,
            pos: 0.0,
            rate,
            start: (frame_offset + self.latency_frames).max(0),
            gain: zgain,
            env: 0.0,
            releasing: false,
            age: self.age_counter,
        };
    }

    fn render(&mut self, out: &mut [f32]) {
        // Clona o Arc do banco (bump atômico, RT-ok) para ler as amostras sem conflito
        // de borrow com as vozes; `self.bank` mantém a outra referência viva.
        let bank = match self.bank.clone() {
            Some(b) => b,
            None => return,
        };
        let atk = self.atk_coef;
        let rel = self.rel_coef;
        for v in self.voices.iter_mut() {
            if !v.active {
                continue;
            }
            if v.zone_idx >= bank.zones.len() {
                v.active = false; // banco trocou sob a voz → corta
                continue;
            }
            let pcm = &bank.zones[v.zone_idx].sample.pcm;
            let len = pcm.len();
            if len < 2 {
                v.active = false;
                continue;
            }
            for s in out.iter_mut() {
                if v.start > 0 {
                    v.start -= 1;
                    continue;
                }
                let i = v.pos as usize;
                if i + 1 >= len {
                    v.active = false;
                    break;
                }
                let frac = (v.pos - i as f64) as f32;
                let smp = pcm[i] * (1.0 - frac) + pcm[i + 1] * frac;

                // Envelope: ataque até 1, release exponencial quando solto.
                if v.releasing {
                    v.env *= rel;
                } else if v.env < 1.0 {
                    v.env = 1.0 - (1.0 - v.env) * atk;
                }

                *s += smp * v.gain * v.env;
                v.pos += v.rate;

                if v.releasing && v.env < 1.0e-4 {
                    v.active = false;
                    break;
                }
            }
        }
    }

    fn set_sample_rate(&mut self, sr: f64) {
        self.engine_sr = sr;
        self.recalc_coefs();
    }

    fn set_latency_frames(&mut self, frames: i32) {
        self.latency_frames = frames;
    }

    fn reset(&mut self) {
        for v in self.voices.iter_mut() {
            v.active = false;
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_sample(len: usize, sr: f64) -> Arc<Sample> {
        // Onda triangular simples (não-zero) para testar leitura/interpolação.
        let pcm: Vec<f32> = (0..len).map(|i| ((i % 32) as f32 / 32.0) - 0.5).collect();
        Arc::new(Sample { pcm, sample_rate: sr })
    }

    fn one_zone_bank(root_hz: f64) -> Arc<SampleBank> {
        Arc::new(SampleBank {
            name: "test".into(),
            zones: vec![SampleZone {
                sample: ramp_sample(4096, 48000.0),
                root_hz,
                lo_key: 0,
                hi_key: 127,
                lo_vel: 0,
                hi_vel: 127,
                gain: 1.0,
                tune_ratio: 1.0,
            }],
        })
    }

    #[test]
    fn no_bank_is_silent() {
        let mut v = SampledBassVoice::new(48000.0, true);
        v.note_on(40, 0, 1.0);
        let mut buf = [0.0f32; 512];
        v.render(&mut buf);
        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn plays_sample_when_bank_loaded() {
        let mut v = SampledBassVoice::new(48000.0, true);
        v.set_bank(Some(one_zone_bank(midi_to_hz(40.0))));
        v.note_on(40, 0, 1.0);
        let mut buf = [0.0f32; 512];
        v.render(&mut buf);
        let peak = buf.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak > 0.01, "deveria soar a amostra, peak={peak}");
    }

    #[test]
    fn octave_up_plays_twice_as_fast() {
        // Nota uma oitava acima da raiz → rate ≈ 2× → consome a amostra ~2× mais rápido.
        let root = 40u8;
        let mut lo = SampledBassVoice::new(48000.0, true);
        lo.set_bank(Some(one_zone_bank(midi_to_hz(root as f64))));
        lo.note_on(root, 0, 1.0);

        let mut hi = SampledBassVoice::new(48000.0, true);
        hi.set_bank(Some(one_zone_bank(midi_to_hz(root as f64))));
        hi.note_on(root + 12, 0, 1.0);

        // Posição avançada após 256 frames: a oitava acima deve estar ~2× à frente.
        let mut b = [0.0f32; 256];
        lo.render(&mut b);
        let mut b2 = [0.0f32; 256];
        hi.render(&mut b2);
        assert!(hi.voices[0].pos > lo.voices[0].pos * 1.8);
    }

    #[test]
    fn round_robin_and_velocity_selection() {
        // Duas zonas de velocity + duas de round-robin no mesmo alcance.
        let s = ramp_sample(2048, 48000.0);
        let bank = Arc::new(SampleBank {
            name: "vel+rr".into(),
            zones: vec![
                // soft (vel 0..63), 2 round-robins
                SampleZone { sample: s.clone(), root_hz: midi_to_hz(40.0), lo_key: 0, hi_key: 127, lo_vel: 0, hi_vel: 63, gain: 1.0, tune_ratio: 1.0 },
                SampleZone { sample: s.clone(), root_hz: midi_to_hz(40.0), lo_key: 0, hi_key: 127, lo_vel: 0, hi_vel: 63, gain: 1.0, tune_ratio: 1.0 },
                // loud (vel 64..127)
                SampleZone { sample: s.clone(), root_hz: midi_to_hz(40.0), lo_key: 0, hi_key: 127, lo_vel: 64, hi_vel: 127, gain: 1.0, tune_ratio: 1.0 },
            ],
        });
        // vel baixa seleciona uma das duas soft (índices 0/1), nunca a loud (2).
        assert!(matches!(bank.select(40, 20, 0), Some(0)));
        assert!(matches!(bank.select(40, 20, 1), Some(1)));
        assert!(matches!(bank.select(40, 20, 2), Some(0))); // rotação
        // vel alta seleciona a loud.
        assert_eq!(bank.select(40, 100, 0), Some(2));
    }
}
