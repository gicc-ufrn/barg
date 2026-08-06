// SPDX-License-Identifier: Apache-2.0
//! Detector de onset por canal — envelope follower com limiar adaptativo + refratário.
//! Reusa a estratégia do `IntensityAnalyzer` (envelope rápido vs. lento). Denormal-safe.
//!
//! TODO(calibração): velocity mais fiel via peak-hold curto; pré-filtro band-limited por
//! peça (passa-baixa p/ Kick, passa-alta p/ OHs) antes do envelope.

#[inline]
fn one_pole(sr: f64, ms: f64) -> f32 {
    let t = ms * 0.001;
    (-1.0 / (t * sr)).exp() as f32
}

/// Anti-denormal: zera valores minúsculos que causam spikes de CPU no ARM (§7 do doc).
#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-30 {
        0.0
    } else {
        x
    }
}

pub struct OnsetDetector {
    fast_env: f32,
    slow_env: f32,
    a_fast: f32,
    a_slow: f32,
    onset_factor: f32,
    refractory: i32,
    refractory_samples: i32,
    armed: bool,
}

impl OnsetDetector {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            fast_env: 0.0,
            slow_env: 0.0,
            a_fast: one_pole(sample_rate, 3.0),
            a_slow: one_pole(sample_rate, 120.0),
            onset_factor: 1.6,
            refractory: 0,
            refractory_samples: (0.020 * sample_rate) as i32, // 20 ms
            armed: true,
        }
    }

    /// Processa um sample. Retorna `Some(velocity 0..1)` no frame do disparo (baixa
    /// latência: emite na detecção, não no fim do transiente).
    #[inline]
    pub fn process(&mut self, x: f32) -> Option<f32> {
        let e = x * x;
        self.fast_env = flush(e + self.a_fast * (self.fast_env - e));
        self.slow_env = flush(e + self.a_slow * (self.slow_env - e));

        if self.refractory > 0 {
            self.refractory -= 1;
        }
        let over = self.fast_env > self.slow_env * self.onset_factor + 1e-9;

        let mut hit = None;
        if self.armed && over && self.refractory == 0 {
            self.refractory = self.refractory_samples;
            self.armed = false;
            // TODO(calibração): velocity via peak-hold; proxy = nível do envelope rápido.
            hit = Some(self.fast_env.sqrt().clamp(0.0, 1.0));
        }
        if !over {
            self.armed = true;
        }
        hit
    }

    /// Energia suavizada do canal (~RMS), para o snapshot `DrumEnergies`.
    #[inline]
    pub fn energy(&self) -> f32 {
        self.slow_env.sqrt()
    }

    pub fn set_sensitivity(&mut self, onset_factor: f32) {
        self.onset_factor = onset_factor;
    }

    pub fn reset(&mut self) {
        self.fast_env = 0.0;
        self.slow_env = 0.0;
        self.refractory = 0;
        self.armed = true;
    }
}
