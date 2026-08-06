// SPDX-License-Identifier: Apache-2.0
//! IntensityAnalyzer — extrai intensidade (RMS) e densidade (onsets/s).
//! RT-safe: sem alocação no path de process().

/// Coeficiente one-pole para constante de tempo em ms.
#[inline]
fn one_pole(sr: f64, ms: f64) -> f32 {
    let t = ms * 0.001;
    (-1.0 / (t * sr)).exp() as f32
}

pub struct IntensityAnalyzer {
    // Envelopes de energia
    fast_env: f32,
    slow_env: f32,
    rms_env: f32,
    a_fast: f32,
    a_slow: f32,
    a_rms: f32,
    // Intensidade
    ref_level: f32,
    intensity_smoothed: f32,
    // Densidade (onsets)
    onset_factor: f32,
    refractory: i32,
    refractory_samples: i32,
    density_accum: f32,
    density_decay: f32,
    density_smoothed: f32,
    a_density: f32,
    armed: bool,
}

impl IntensityAnalyzer {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            fast_env: 0.0,
            slow_env: 0.0,
            rms_env: 0.0,
            a_fast: one_pole(sample_rate, 3.0),
            a_slow: one_pole(sample_rate, 120.0),
            a_rms: one_pole(sample_rate, 150.0),
            ref_level: 0.05,
            intensity_smoothed: 0.0,
            onset_factor: 1.6,
            refractory: 0,
            refractory_samples: (0.030 * sample_rate) as i32,
            density_accum: 0.0,
            density_decay: one_pole(sample_rate, 700.0),
            density_smoothed: 0.0,
            a_density: one_pole(sample_rate, 300.0),
            armed: true,
        }
    }

    /// Processa bloco mono de áudio. RT-safe.
    pub fn process(&mut self, input: &[f32]) {
        for &x in input {
            let e = x * x;

            self.fast_env = e + self.a_fast * (self.fast_env - e);
            self.slow_env = e + self.a_slow * (self.slow_env - e);
            self.rms_env = e + self.a_rms * (self.rms_env - e);

            if self.refractory > 0 {
                self.refractory -= 1;
            }
            let over = self.fast_env > (self.slow_env * self.onset_factor + 1e-9);
            if self.armed && over && self.refractory == 0 {
                self.density_accum += 1.0;
                self.refractory = self.refractory_samples;
                self.armed = false;
            }
            if !over {
                self.armed = true;
            }

            self.density_accum *= self.density_decay;
            let inst_rate = self.density_accum * (1.0 / 0.7);
            self.density_smoothed =
                inst_rate + self.a_density * (self.density_smoothed - inst_rate);
        }

        let rms = self.rms_env.max(0.0).sqrt();
        let inten = (rms / (self.ref_level + 1e-9)).clamp(0.0, 1.0);
        self.intensity_smoothed = inten;
    }

    pub fn intensity(&self) -> f32 {
        self.intensity_smoothed
    }

    pub fn density(&self) -> f32 {
        self.density_smoothed
    }

    pub fn set_reference_level(&mut self, rms: f32) {
        self.ref_level = rms;
    }

    pub fn set_onset_sensitivity(&mut self, s: f32) {
        self.onset_factor = s;
    }

    pub fn reset(&mut self) {
        self.fast_env = 0.0;
        self.slow_env = 0.0;
        self.rms_env = 0.0;
        self.intensity_smoothed = 0.0;
        self.density_smoothed = 0.0;
        self.density_accum = 0.0;
        self.refractory = 0;
        self.armed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intensity_bounds() {
        let mut a = IntensityAnalyzer::new(48000.0);
        // Silence → 0
        let silence = [0.0f32; 128];
        a.process(&silence);
        assert_eq!(a.intensity(), 0.0);
        assert!(a.density() >= 0.0);

        // Loud signal → clamped to 1.0
        let loud = [1.0f32; 4800];
        a.process(&loud);
        assert!(a.intensity() <= 1.0);
        assert!(a.intensity() > 0.0);
    }

    #[test]
    fn onset_detection() {
        let mut a = IntensityAnalyzer::new(48000.0);
        a.set_reference_level(0.2);
        // Feed silence then impulse
        let silence = [0.0f32; 4800];
        a.process(&silence);
        assert!(a.density() < 0.1);

        // Impulse
        let mut impulse = [0.0f32; 128];
        impulse[0] = 0.5;
        a.process(&impulse);
        // Density should increase
        let d_after = a.density();
        assert!(d_after > 0.0);
    }

    #[test]
    fn no_nan_inf() {
        let mut a = IntensityAnalyzer::new(48000.0);
        let data: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin()).collect();
        a.process(&data);
        assert!(a.intensity().is_finite());
        assert!(a.density().is_finite());
    }
}
