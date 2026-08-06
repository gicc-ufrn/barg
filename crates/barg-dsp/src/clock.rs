// SPDX-License-Identifier: Apache-2.0
//! GrooveClock — relógio sample-accurate do Casa 13.
//! Emite steps (semicolcheias) com frame offset exato no buffer de áudio.

use barg_types::StepEvent;

pub struct GrooveClock {
    sample_rate: f64,
    steps_per_bar: i32,
    bpm: f64,
    samples_per_step: f64,
    phase_frames: f64,
    absolute_step: i64,
}

impl GrooveClock {
    pub fn new(sample_rate: f64, steps_per_bar: i32) -> Self {
        let mut clock = Self {
            sample_rate,
            steps_per_bar,
            bpm: 100.0,
            samples_per_step: 0.0,
            phase_frames: 0.0,
            absolute_step: 0,
        };
        clock.set_tempo(100.0);
        clock
    }

    pub fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
        let beats_per_bar = 4.0;
        let steps_per_beat = self.steps_per_bar as f64 / beats_per_bar;
        let steps_per_second = (bpm / 60.0) * steps_per_beat;
        self.samples_per_step = self.sample_rate / steps_per_second;
    }

    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    pub fn steps_per_bar(&self) -> i32 {
        self.steps_per_bar
    }

    pub fn samples_per_step(&self) -> f64 {
        self.samples_per_step
    }

    pub fn current_bar(&self) -> i64 {
        self.absolute_step / self.steps_per_bar as i64
    }

    /// Processa um bloco de frames, chamando on_step em cada fronteira de step.
    /// RT-safe: sem alocação.
    pub fn process<F>(&mut self, num_frames: u32, mut on_step: F)
    where
        F: FnMut(StepEvent),
    {
        let mut frame: u32 = 0;
        while frame < num_frames {
            let frames_until_next = self.samples_per_step - self.phase_frames;
            let step_frame = frames_until_next as u32;
            if step_frame < (num_frames - frame) {
                frame += step_frame;
                self.phase_frames = 0.0;
                let step_in_bar = (self.absolute_step % self.steps_per_bar as i64) as u8;
                let bar = self.absolute_step / self.steps_per_bar as i64;
                on_step(StepEvent {
                    step_in_bar,
                    bar,
                    frame_offset: frame,
                });
                self.absolute_step += 1;
            } else {
                self.phase_frames += (num_frames - frame) as f64;
                frame = num_frames;
            }
        }
    }

    pub fn reset(&mut self) {
        self.phase_frames = 0.0;
        self.absolute_step = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_16_steps_per_bar() {
        let mut clock = GrooveClock::new(48000.0, 16);
        clock.set_tempo(120.0);
        // 120 BPM → 2s/bar → 96000 frames/bar. Add 1 extra frame to catch the last step.
        let frames_per_bar = (48000.0 * 2.0) as u32 + 1;
        let mut steps = Vec::new();
        clock.process(frames_per_bar, |ev| steps.push(ev));
        assert_eq!(steps.len(), 16);
        assert_eq!(steps[0].step_in_bar, 0);
        assert_eq!(steps[15].step_in_bar, 15);
        // Frame offsets devem ser crescentes
        for w in steps.windows(2) {
            assert!(w[1].frame_offset > w[0].frame_offset);
        }
    }

    #[test]
    fn determinism_across_block_sizes() {
        let mut clock1 = GrooveClock::new(48000.0, 16);
        let mut clock2 = GrooveClock::new(48000.0, 16);
        clock1.set_tempo(100.0);
        clock2.set_tempo(100.0);

        // Processar 4800 frames em um bloco
        let mut steps1 = Vec::new();
        clock1.process(4800, |ev| steps1.push(ev));

        // Processar 4800 frames em blocos de 128
        let mut steps2 = Vec::new();
        let mut processed = 0u32;
        while processed < 4800 {
            let block = 128.min(4800 - processed);
            clock2.process(block, |ev| {
                steps2.push(StepEvent {
                    step_in_bar: ev.step_in_bar,
                    bar: ev.bar,
                    frame_offset: ev.frame_offset + processed,
                });
            });
            processed += block;
        }

        assert_eq!(steps1.len(), steps2.len());
        for (a, b) in steps1.iter().zip(steps2.iter()) {
            assert_eq!(a.step_in_bar, b.step_in_bar);
            assert_eq!(a.bar, b.bar);
            assert_eq!(a.frame_offset, b.frame_offset);
        }
    }

    #[test]
    fn tempo_change_takes_effect() {
        let mut clock = GrooveClock::new(48000.0, 16);
        clock.set_tempo(60.0);
        let sps_slow = clock.samples_per_step();
        clock.set_tempo(120.0);
        let sps_fast = clock.samples_per_step();
        assert!(sps_fast < sps_slow);
        assert!((sps_fast - sps_slow / 2.0).abs() < 1.0);
    }
}
