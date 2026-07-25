//! BassSynth — baixo procedural monofônico para o vamp de um acorde (Gap C).
//!
//! Timbre simples e barato: um dente-de-serra (via soma de senos limitada seria caro;
//! usamos um saw por fase acumulada) somado a um sub-seno uma oitava abaixo, moldados
//! por um envelope AD e suavizados por um filtro de um polo (passa-baixa) — o suficiente
//! para estabelecer o vamp com peso e "warmth", sem samples. RT-safe: uma voz, sem
//! alocação, sem panic. Samples multi-articulação ficam como upgrade (concept §2).
//!
//! Monofônico com release curto: uma nova nota rearma o envelope (estilo baixo de dedo
//! ligado/legato). A latência de agendamento espelha a do `PercussionSynth` para o baixo
//! cair em fase com a percussão.

use core::f64::consts::PI;

/// Backend de timbre do baixo — a costura que permite trocar procedural ↔ sampleado
/// ao vivo (Gap C). O gerador simbólico não muda; só quem soa as notas. Ver
/// `docs/research-bass-sampler-sequencer.md §5.1`.
pub trait BassVoice {
    /// Agenda uma nota. `frame_offset` é relativo ao bloco; `gain` ∈ 0..1 (proxy de
    /// velocity — o sampler mapeia para camada/round-robin internamente).
    fn note_on(&mut self, midi: u8, frame_offset: i32, gain: f32);
    /// Renderiza somando em `out`. RT-safe.
    fn render(&mut self, out: &mut [f32]);
    fn set_sample_rate(&mut self, sr: f64);
    fn set_latency_frames(&mut self, frames: i32);
    fn reset(&mut self);
}

/// Constante de decaimento do envelope (s) — nota de baixo com sustain curto.
const DECAY_S: f32 = 0.28;
/// Ataque (s) — rápido, mas evita clique.
const ATTACK_S: f32 = 0.004;
/// Corte do passa-baixa de um polo (Hz) — tira o brilho áspero do saw.
const LP_CUTOFF_HZ: f64 = 1400.0;

pub struct BassSynth {
    sample_rate: f64,
    latency_frames: i32,
    // Voz única.
    active: bool,
    start: i32,      // frames até o início (agendamento)
    freq: f64,       // Hz da fundamental
    gain: f32,
    phase: f64,      // fase do saw [0,1)
    sub_phase: f64,  // fase do sub-seno
    env: f32,        // amplitude do envelope [0,1]
    env_target: f32, // alvo do ataque
    attacking: bool,
    atk_coef: f32,
    dec_coef: f32,
    lp_state: f32,   // estado do passa-baixa
    lp_coef: f32,
}

impl BassSynth {
    pub fn new(sample_rate: f64) -> Self {
        let mut s = Self {
            sample_rate,
            latency_frames: 0,
            active: false,
            start: 0,
            freq: 0.0,
            gain: 0.0,
            phase: 0.0,
            sub_phase: 0.0,
            env: 0.0,
            env_target: 1.0,
            attacking: false,
            atk_coef: 0.0,
            dec_coef: 0.0,
            lp_state: 0.0,
            lp_coef: 0.0,
        };
        s.set_sample_rate(sample_rate);
        s
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr;
        // Coeficientes de envelope (decaimento exponencial por amostra).
        self.atk_coef = (-1.0 / (ATTACK_S as f64 * sr)).exp() as f32;
        self.dec_coef = (-1.0 / (DECAY_S as f64 * sr)).exp() as f32;
        // Passa-baixa de um polo: y += coef*(x - y).
        let rc = 1.0 / (2.0 * PI * LP_CUTOFF_HZ);
        let dt = 1.0 / sr;
        self.lp_coef = (dt / (rc + dt)) as f32;
    }

    pub fn set_latency_frames(&mut self, frames: i32) {
        self.latency_frames = frames;
    }

    /// Converte nota MIDI em frequência (Hz).
    #[inline]
    fn midi_to_hz(note: u8) -> f64 {
        440.0 * 2.0_f64.powf((note as f64 - 69.0) / 12.0)
    }

    /// Agenda uma nota de baixo. `frame_offset` relativo ao início do bloco.
    pub fn schedule(&mut self, midi_note: u8, frame_offset: i32, gain: f32) {
        self.start = (frame_offset + self.latency_frames).max(0);
        self.freq = Self::midi_to_hz(midi_note);
        self.gain = gain;
        self.active = true;
        self.attacking = true;
        // Rearma o envelope a partir do valor atual (legato: sem zerar → sem clique).
        self.env_target = 1.0;
        // Mantém phase/sub_phase para continuidade (legato); zera se estava inativa.
        if self.env < 1.0e-4 {
            self.phase = 0.0;
            self.sub_phase = 0.0;
            self.lp_state = 0.0;
        }
    }

    /// Renderiza a voz somando em `out`. RT-safe.
    pub fn render(&mut self, out: &mut [f32]) {
        if !self.active {
            return;
        }
        let inc = self.freq / self.sample_rate;
        let sub_inc = inc * 0.5; // uma oitava abaixo
        for sample in out.iter_mut() {
            if self.start > 0 {
                self.start -= 1;
                continue;
            }
            // Envelope: ataque rápido até o alvo, depois decaimento.
            if self.attacking {
                self.env = self.env_target - (self.env_target - self.env) * self.atk_coef;
                if self.env >= self.env_target - 1.0e-3 {
                    self.attacking = false;
                }
            } else {
                self.env *= self.dec_coef;
            }

            // Saw em [-1,1] + sub-seno (peso grave).
            let saw = (self.phase * 2.0 - 1.0) as f32;
            let sub = (self.sub_phase * 2.0 * PI).sin() as f32;
            let raw = 0.6 * saw + 0.4 * sub;

            // Passa-baixa de um polo.
            self.lp_state += self.lp_coef * (raw - self.lp_state);

            *sample += self.lp_state * self.env * self.gain;

            self.phase += inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            self.sub_phase += sub_inc;
            if self.sub_phase >= 1.0 {
                self.sub_phase -= 1.0;
            }

            if self.env < 1.0e-4 {
                self.active = false;
                break;
            }
        }
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.env = 0.0;
    }
}

/// O baixo procedural como um `BassVoice` (o baseline/fallback do A/B de timbre).
impl BassVoice for BassSynth {
    fn note_on(&mut self, midi: u8, frame_offset: i32, gain: f32) {
        self.schedule(midi, frame_offset, gain);
    }
    fn render(&mut self, out: &mut [f32]) {
        BassSynth::render(self, out);
    }
    fn set_sample_rate(&mut self, sr: f64) {
        BassSynth::set_sample_rate(self, sr);
    }
    fn set_latency_frames(&mut self, frames: i32) {
        BassSynth::set_latency_frames(self, frames);
    }
    fn reset(&mut self) {
        BassSynth::reset(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_to_hz_a440() {
        assert!((BassSynth::midi_to_hz(69) - 440.0).abs() < 1e-6);
        // A2 = 45 → 110 Hz
        assert!((BassSynth::midi_to_hz(45) - 110.0).abs() < 1e-3);
    }

    #[test]
    fn schedule_produces_audio_then_decays() {
        let mut b = BassSynth::new(48000.0);
        b.set_latency_frames(0);
        b.schedule(38, 0, 0.9); // D2
        let mut buf = [0.0f32; 4096];
        b.render(&mut buf);
        let peak = buf.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak > 0.05, "baixo deveria produzir áudio audível, peak={peak}");

        // Após muitos frames, o envelope decai (silêncio).
        for _ in 0..40 {
            let mut b2 = [0.0f32; 4096];
            b.render(&mut b2);
        }
        assert!(!b.active, "a nota deveria ter decaído e desativado a voz");
    }

    #[test]
    fn latency_delays_onset() {
        let mut b = BassSynth::new(48000.0);
        b.set_latency_frames(100);
        b.schedule(40, 0, 1.0);
        let mut buf = [0.0f32; 64];
        b.render(&mut buf); // 64 frames < 100 de latência → ainda silêncio
        let peak = buf.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak < 1.0e-6, "a nota não deveria soar antes da latência");
    }
}
