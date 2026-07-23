//! Cross-Channel Arbiter — rejeita bleed: 1 golpe físico → 1 `DrumHit` (doc §3.7).
//!
//! Regra (placeholder): se um canal de PRIORIDADE maior disparou dentro de uma janela de
//! coincidência e o candidato é fraco, trata como vazamento (o mic do bumbo escutando a
//! caixa etc.). A prioridade vem da peça mapeada no canal (ver `voice_priority`).
//! TODO(calibração): usar a RAZÃO DE NÍVEL entre mics, não só a velocity absoluta.

use crate::NUM_INPUT_CHANNELS;

/// Abaixo deste nível, um candidato coincidente com um canal de maior prioridade é bleed.
const BLEED_VELOCITY_GATE: f32 = 0.35;

pub struct CrossChannelArbiter {
    last_frame: [Option<u64>; NUM_INPUT_CHANNELS],
    last_priority: [u8; NUM_INPUT_CHANNELS],
    window_frames: u64,
}

impl CrossChannelArbiter {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            last_frame: [None; NUM_INPUT_CHANNELS],
            last_priority: [0; NUM_INPUT_CHANNELS],
            // Janela de coincidência ~8 ms (bleed acústico entre peças próximas).
            window_frames: (0.008 * sample_rate) as u64,
        }
    }

    /// Decide se um onset candidato é golpe real (`true`) ou bleed (`false`).
    /// `priority` é a prioridade da peça mapeada no canal.
    pub fn accept(&mut self, channel: usize, priority: u8, frame: u64, velocity: f32) -> bool {
        let mut is_bleed = false;
        for (other, slot) in self.last_frame.iter().enumerate() {
            if other == channel {
                continue;
            }
            let Some(last) = *slot else { continue };
            if self.last_priority[other] > priority
                && frame.saturating_sub(last) < self.window_frames
                && velocity < BLEED_VELOCITY_GATE
            {
                is_bleed = true;
                break;
            }
        }
        if is_bleed {
            return false;
        }
        if channel < NUM_INPUT_CHANNELS {
            self.last_frame[channel] = Some(frame);
            self.last_priority[channel] = priority;
        }
        true
    }

    pub fn reset(&mut self) {
        self.last_frame = [None; NUM_INPUT_CHANNELS];
        self.last_priority = [0; NUM_INPUT_CHANNELS];
    }
}
