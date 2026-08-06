// SPDX-License-Identifier: Apache-2.0
//! PercussionSynth — síntese procedural de 6 vozes afro-brasileiras.
//! RT-safe: sem alocação, voice stealing, latência de agendamento.

use barg_types::{Voice, VOICE_COUNT};
use core::f64::consts::PI;

const MAX_VOICES: usize = 24;

/// Parâmetros de síntese procedural por voz. Tabela de dados em vez de números
/// mágicos espalhados num `match` — legível e afinável num só lugar.
struct VoiceTimbre {
    f0: f64,        // fundamental (Hz)
    f1: f64,        // parcial adicional (0 = ausente)
    f_start: f64,   // início da queda de pitch (usado só se pitch_tau > 0)
    a0: f32,        // amplitude da fundamental
    a1: f32,        // amplitude da parcial
    noise_mix: f32, // 0 = tonal, 1 = ruído
    pitch_tau: f32, // constante de queda de pitch (s); 0 = sem queda
    decay: f32,     // constante de decaimento do envelope (s)
}

/// Ordem = enum `Voice` (Agogo, Tamborim, Ganza, SurdoLow, SurdoResp, Repique).
#[rustfmt::skip]
const TIMBRES: [VoiceTimbre; VOICE_COUNT] = [
    // Agogo
    VoiceTimbre { f0: 780.0, f1: 1180.0, f_start: 0.0, a0: 0.7, a1: 0.5, noise_mix: 0.0, pitch_tau: 0.0, decay: 0.13 },
    // Tamborim
    VoiceTimbre { f0: 1800.0, f1: 0.0, f_start: 0.0, a0: 0.3, a1: 0.0, noise_mix: 0.8, pitch_tau: 0.0, decay: 0.035 },
    // Ganza
    VoiceTimbre { f0: 0.0, f1: 0.0, f_start: 0.0, a0: 0.0, a1: 0.0, noise_mix: 1.0, pitch_tau: 0.0, decay: 0.045 },
    // SurdoLow
    VoiceTimbre { f0: 52.0, f1: 0.0, f_start: 95.0, a0: 1.0, a1: 0.0, noise_mix: 0.05, pitch_tau: 0.03, decay: 0.22 },
    // SurdoResp
    VoiceTimbre { f0: 68.0, f1: 0.0, f_start: 120.0, a0: 0.9, a1: 0.0, noise_mix: 0.05, pitch_tau: 0.03, decay: 0.18 },
    // Repique
    VoiceTimbre { f0: 380.0, f1: 0.0, f_start: 0.0, a0: 0.4, a1: 0.0, noise_mix: 0.7, pitch_tau: 0.0, decay: 0.09 },
];

#[derive(Clone)]
struct VoiceState {
    active: bool,
    voice_type: Voice,
    start: i32,
    gain: f32,
    env: f32,
    env_coef: f32,
    t: f64,
    ph0: f64,
    ph1: f64,
    f0: f64,
    f1: f64,
    f_start: f64,
    a0: f32,
    a1: f32,
    noise_mix: f32,
    pitch_tau: f32,
    rng: u32,
}

impl Default for VoiceState {
    fn default() -> Self {
        Self {
            active: false,
            voice_type: Voice::Agogo,
            start: 0,
            gain: 0.0,
            env: 0.0,
            env_coef: 0.0,
            t: 0.0,
            ph0: 0.0,
            ph1: 0.0,
            f0: 0.0,
            f1: 0.0,
            f_start: 0.0,
            a0: 0.0,
            a1: 0.0,
            noise_mix: 0.0,
            pitch_tau: 0.0,
            rng: 1,
        }
    }
}

pub struct PercussionSynth {
    sample_rate: f64,
    latency_ms: f32,
    latency_frames: i32,
    rng_seed: u32,
    voices: [VoiceState; MAX_VOICES],
}

impl PercussionSynth {
    pub fn new(sample_rate: f64) -> Self {
        let mut s = Self {
            sample_rate,
            latency_ms: 30.0,
            latency_frames: 0,
            rng_seed: 0x1234567,
            voices: core::array::from_fn(|_| VoiceState::default()),
        };
        s.set_latency_ms(30.0);
        s
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr;
        self.set_latency_ms(self.latency_ms);
    }

    pub fn set_latency_ms(&mut self, ms: f32) {
        self.latency_ms = ms;
        self.latency_frames = (ms as f64 * 0.001 * self.sample_rate).round() as i32;
    }

    pub fn latency_frames(&self) -> i32 {
        self.latency_frames
    }

    /// Agenda uma nota. frame_offset pode ser negativo (microtiming antecipado).
    pub fn schedule(&mut self, voice: Voice, frame_offset: i32, gain: f32) {
        let start = (frame_offset + self.latency_frames).max(0);
        self.rng_seed = self.rng_seed.wrapping_add(0x9E3779B9) | 1;
        let rng = self.rng_seed;
        let sr = self.sample_rate;
        let idx = self.pick_index();
        let v = &mut self.voices[idx];
        v.active = true;
        v.voice_type = voice;
        v.start = start;
        v.gain = gain;
        v.env = 1.0;
        v.t = 0.0;
        v.ph0 = 0.0;
        v.ph1 = 0.0;
        v.rng = rng;
        Self::set_params(v, sr);
    }

    /// Renderiza vozes ativas somando em out. RT-safe.
    pub fn render(&mut self, out: &mut [f32]) {
        let sr = self.sample_rate;
        for voice in self.voices.iter_mut() {
            if !voice.active {
                continue;
            }
            for sample in out.iter_mut() {
                if voice.start > 0 {
                    voice.start -= 1;
                    continue;
                }
                *sample += Self::render_sample(voice, sr);
                if voice.env < 1.0e-4 {
                    voice.active = false;
                    break;
                }
            }
        }
    }

    pub fn reset(&mut self) {
        for v in self.voices.iter_mut() {
            v.active = false;
        }
    }

    #[inline]
    fn render_sample(v: &mut VoiceState, sr: f64) -> f32 {
        // xorshift noise
        v.rng ^= v.rng << 13;
        v.rng ^= v.rng >> 17;
        v.rng ^= v.rng << 5;
        let noise = (v.rng as i32) as f32 * (1.0 / 2147483648.0);

        // pitch envelope (surdo/zabumba)
        let pe = if v.pitch_tau > 0.0 {
            (-(v.t as f32) / v.pitch_tau).exp()
        } else {
            0.0
        };
        let f0 = v.f0 + (v.f_start - v.f0) * pe as f64;

        v.ph0 += 2.0 * PI * f0 / sr;
        v.ph1 += 2.0 * PI * v.f1 / sr;

        let tone = v.a0 * v.ph0.sin() as f32 + v.a1 * v.ph1.sin() as f32;
        let s = (tone * (1.0 - v.noise_mix) + noise * v.noise_mix) * v.env * v.gain;

        v.env *= v.env_coef;
        v.t += 1.0 / sr;
        s
    }

    fn set_params(v: &mut VoiceState, sr: f64) {
        let t = &TIMBRES[v.voice_type as usize];
        v.f0 = t.f0;
        v.f1 = t.f1;
        v.a0 = t.a0;
        v.a1 = t.a1;
        v.noise_mix = t.noise_mix;
        v.pitch_tau = t.pitch_tau;
        // Sem queda de pitch, f_start = f0 (evita rampa espúria).
        v.f_start = if t.pitch_tau > 0.0 { t.f_start } else { t.f0 };
        v.env_coef = (-1.0 / (t.decay * sr as f32)).exp();
    }

    fn pick_index(&self) -> usize {
        // Procura voice inativa
        for (i, v) in self.voices.iter().enumerate() {
            if !v.active {
                return i;
            }
        }
        // Voice stealing: rouba a de menor envelope
        let mut idx = 0;
        let mut lo = f32::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if v.env < lo {
                lo = v.env;
                idx = i;
            }
        }
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_stealing() {
        let mut synth = PercussionSynth::new(48000.0);
        // Schedule 25 notes (more than MAX_VOICES=24)
        for i in 0..25 {
            synth.schedule(Voice::Agogo, 0, 0.5 + i as f32 * 0.01);
        }
        // All 24 slots should be active
        let active_count = synth.voices.iter().filter(|v| v.active).count();
        assert_eq!(active_count, MAX_VOICES);
    }

    #[test]
    fn latency_prevents_negative_start() {
        let mut synth = PercussionSynth::new(48000.0);
        synth.set_latency_ms(30.0);
        synth.schedule(Voice::Tamborim, -100, 0.8);
        // start should be >= 0
        let v = synth.voices.iter().find(|v| v.active).unwrap();
        assert!(v.start >= 0);
    }

    #[test]
    fn render_produces_audio() {
        let mut synth = PercussionSynth::new(48000.0);
        synth.set_latency_ms(0.0); // Remove latency for this test
        synth.schedule(Voice::SurdoLow, 0, 0.9);
        let mut buf = [0.0f32; 128];
        synth.render(&mut buf);
        // Should have non-zero output
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(energy > 0.0);
    }

    #[test]
    fn voice_deactivates_after_decay() {
        let mut synth = PercussionSynth::new(48000.0);
        synth.set_latency_ms(0.0); // Remove latency for this test
        synth.schedule(Voice::Tamborim, 0, 0.5);
        // Render enough to decay (tamborim decay=0.035s; env < 1e-4 at ~0.035*ln(1e4)≈0.32s → 15360 samples)
        let mut buf = vec![0.0f32; 48000];
        synth.render(&mut buf);
        let active_count = synth.voices.iter().filter(|v| v.active).count();
        assert_eq!(active_count, 0);
    }
}
