//! Cross-Channel Arbiter — rejeita bleed: 1 golpe físico → 1 `DrumHit` (§3.7 do doc).
//!
//! Regra (placeholder): se um canal de PRIORIDADE maior disparou dentro de uma janela de
//! coincidência e o candidato é fraco, trata como vazamento (o mic do bumbo escutando a
//! caixa etc.). TODO(calibração): usar a RAZÃO DE NÍVEL entre mics, não só a velocity
//! absoluta — é o que os trackers de drum-replacement fazem.

use crate::{DrumChannel, DRUM_CHANNEL_COUNT};

/// Prioridade por canal (maior vence a arbitragem de coincidência). Kick/Snare são fontes
/// "duras"; pratos/OH costumam ser o DESTINO do bleed.
const PRIORITY: [u8; DRUM_CHANNEL_COUNT] = [
    5, // Kick
    4, // Snare
    2, // HiHat
    3, // Toms
    1, // Cymbals
];

/// Abaixo deste nível, um candidato coincidente com um canal de maior prioridade é bleed.
const BLEED_VELOCITY_GATE: f32 = 0.35;

pub struct CrossChannelArbiter {
    last_hit_frame: [Option<u64>; DRUM_CHANNEL_COUNT],
    window_frames: u64,
}

impl CrossChannelArbiter {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            last_hit_frame: [None; DRUM_CHANNEL_COUNT],
            // Janela de coincidência ~8 ms (bleed acústico entre peças próximas).
            window_frames: (0.008 * sample_rate) as u64,
        }
    }

    /// Decide se um onset candidato é golpe real (`true`) ou bleed (`false`).
    pub fn accept(&mut self, ch: DrumChannel, frame: u64, velocity: f32) -> bool {
        let my_priority = PRIORITY[ch as usize];
        let mut is_bleed = false;
        for (other, slot) in self.last_hit_frame.iter().enumerate() {
            let Some(last) = *slot else { continue };
            if PRIORITY[other] > my_priority
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
        self.last_hit_frame[ch as usize] = Some(frame);
        true
    }

    pub fn reset(&mut self) {
        self.last_hit_frame = [None; DRUM_CHANNEL_COUNT];
    }
}
