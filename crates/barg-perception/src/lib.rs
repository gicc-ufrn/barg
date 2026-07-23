//! Casa 13 — camada de percepção (V4): transdução acústico → simbólica.
//!
//! Adapter na **porta de entrada** (Ports & Adapters, ver `docs/architecture-v4.md`):
//! converte áudio multicanal dos microfones da bateria em eventos simbólicos limpos
//! (`DrumHit`) mais um snapshot de energia por canal (`DrumEnergies`). O domínio nunca vê áudio.
//!
//! Hardware alvo (P0 resolvido): a **Flow 8 entrega 10 canais USB** — os **8 isolados**
//! (índices 0–7, pré-fader) são os mics; **8–9 são o master** e são IGNORADOS aqui.
//!
//! O mapa **canal físico → peça** (`DrumVoice`) é **configurável em runtime** — o performer
//! define, na garagem, qual mic está em qual entrada.
//!
//! **RT-safe:** roda no audio thread — sem alocação, sem locks, sem panic. ESQUELETO: as
//! costuras são reais e testadas; o DSP de onset e as regras do árbitro têm TODOs de calibração.

mod arbiter;
mod onset;

pub use arbiter::CrossChannelArbiter;
pub use onset::OnsetDetector;

/// Canais de entrada ISOLADOS processados (os 8 mics da Flow 8). Os índices USB 8–9
/// (master L/R) ficam de fora — a percepção só olha os 8 primeiros.
pub const NUM_INPUT_CHANNELS: usize = 8;

/// Peça da bateria atribuída a um canal físico. Mapeamento definido pelo performer.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrumVoice {
    Kick = 0,
    Snare = 1,
    HiHat = 2,
    Tom = 3,
    Ride = 4,
    Crash = 5,
    Overhead = 6,
    Other = 7,
}

/// Prioridade da peça na arbitragem de bleed (fontes "duras" vencem; pratos/OH recebem bleed).
/// Canal não mapeado = prioridade média.
fn voice_priority(v: Option<DrumVoice>) -> u8 {
    match v {
        Some(DrumVoice::Kick) => 6,
        Some(DrumVoice::Snare) => 5,
        Some(DrumVoice::Tom) => 4,
        Some(DrumVoice::HiHat) => 3,
        Some(DrumVoice::Ride) | Some(DrumVoice::Crash) => 2,
        Some(DrumVoice::Overhead) => 1,
        Some(DrumVoice::Other) | None => 3,
    }
}

/// Snapshot de energia por canal físico (0–7) — LATEST-VALUE.
/// Publicado à Malha Lenta por `triple_buffer` (wait-free, sem torn-read). Ver doc §4/§7.
#[derive(Clone, Copy, Debug, Default)]
pub struct DrumEnergies {
    pub per_channel: [f32; NUM_INPUT_CHANNELS],
}

impl DrumEnergies {
    #[inline]
    pub fn get(&self, channel: usize) -> f32 {
        self.per_channel.get(channel).copied().unwrap_or(0.0)
    }
}

/// Golpe discreto já arbitrado — STREAM (SPSC).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DrumHit {
    pub channel: u8,              // canal físico 0–7
    pub voice: Option<DrumVoice>, // peça mapeada (None = canal ainda não configurado)
    pub velocity: f32,           // 0..1, do envelope no disparo
    pub frame_timestamp: u64,    // frame absoluto no GrooveClock
}

/// Porta de entrada da percepção (Ports & Adapters). Substituível: mics, MIDI, mock offline.
pub trait PerceptionSource {
    /// `channels[i]` é o buffer do canal físico `i` (0–7 são usados; 8+ ignorados).
    /// RT-safe: `emit` não deve alocar (tipicamente um push em SPSC).
    fn process_block(
        &mut self,
        channels: &[&[f32]],
        frame_base: u64,
        emit: &mut dyn FnMut(DrumHit),
    ) -> DrumEnergies;

    fn reset(&mut self);
}

/// Adapter acústico: onset por canal + arbitragem de bleed + mapa canal→peça.
pub struct AcousticPerception {
    detectors: [OnsetDetector; NUM_INPUT_CHANNELS],
    arbiter: CrossChannelArbiter,
    channel_voice: [Option<DrumVoice>; NUM_INPUT_CHANNELS],
}

impl AcousticPerception {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            detectors: core::array::from_fn(|_| OnsetDetector::new(sample_rate)),
            arbiter: CrossChannelArbiter::new(sample_rate),
            channel_voice: [None; NUM_INPUT_CHANNELS],
        }
    }

    /// Atribui (ou limpa) a peça de um canal físico. Chamado pela camada de controle —
    /// é o que o app configura na garagem (canal→mic→peça).
    pub fn set_channel_voice(&mut self, channel: usize, voice: Option<DrumVoice>) {
        if channel < NUM_INPUT_CHANNELS {
            self.channel_voice[channel] = voice;
        }
    }

    pub fn channel_voice(&self, channel: usize) -> Option<DrumVoice> {
        self.channel_voice.get(channel).copied().flatten()
    }
}

impl PerceptionSource for AcousticPerception {
    fn process_block(
        &mut self,
        channels: &[&[f32]],
        frame_base: u64,
        emit: &mut dyn FnMut(DrumHit),
    ) -> DrumEnergies {
        let mut energies = DrumEnergies::default();

        for (ci, det) in self.detectors.iter_mut().enumerate() {
            if ci >= channels.len() {
                break; // menos canais que o esperado; ignora o resto
            }
            let voice = self.channel_voice[ci];
            let priority = voice_priority(voice);
            for (j, &x) in channels[ci].iter().enumerate() {
                if let Some(velocity) = det.process(x) {
                    let frame = frame_base + j as u64;
                    if self.arbiter.accept(ci, priority, frame, velocity) {
                        emit(DrumHit {
                            channel: ci as u8,
                            voice,
                            velocity,
                            frame_timestamp: frame,
                        });
                    }
                }
            }
            energies.per_channel[ci] = det.energy();
        }
        energies
    }

    fn reset(&mut self) {
        for d in self.detectors.iter_mut() {
            d.reset();
        }
        self.arbiter.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_on_channel0_emits_hit_with_mapped_voice() {
        let mut p = AcousticPerception::new(48000.0);
        p.set_channel_voice(0, Some(DrumVoice::Kick));

        let mut ch0 = vec![0.0f32; 2048];
        for k in ch0.iter_mut().skip(512).take(48) {
            *k = 0.8;
        }
        let silent = vec![0.0f32; 2048];
        let channels: [&[f32]; NUM_INPUT_CHANNELS] =
            [&ch0, &silent, &silent, &silent, &silent, &silent, &silent, &silent];

        let mut hits: Vec<DrumHit> = Vec::new();
        let energies = p.process_block(&channels, 0, &mut |h| hits.push(h));

        assert!(hits.iter().any(|h| h.channel == 0 && h.voice == Some(DrumVoice::Kick)));
        assert!(energies.get(0) > 0.0);
    }

    #[test]
    fn master_channels_are_ignored() {
        // Mesmo passando 10 buffers (com sinal em 8 e 9), só 0–7 são processados.
        let mut p = AcousticPerception::new(48000.0);
        let mut loud = vec![0.0f32; 1024];
        for k in loud.iter_mut().skip(64).take(64) {
            *k = 0.9;
        }
        let silent = vec![0.0f32; 1024];
        let channels: [&[f32]; 10] = [
            &silent, &silent, &silent, &silent, &silent, &silent, &silent, &silent,
            &loud, &loud, // índices 8–9 (master) — devem ser ignorados
        ];
        let mut hits: Vec<DrumHit> = Vec::new();
        let _ = p.process_block(&channels, 0, &mut |h| hits.push(h));
        assert!(hits.iter().all(|h| h.channel < NUM_INPUT_CHANNELS as u8));
    }

    #[test]
    fn arbiter_suppresses_weak_coincident_bleed() {
        let mut arb = CrossChannelArbiter::new(48000.0);
        // Canal 0 (Kick, prioridade alta) forte no frame 1000.
        assert!(arb.accept(0, voice_priority(Some(DrumVoice::Kick)), 1000, 0.9));
        // Canal 6 (Overhead, prioridade baixa) fraco e coincidente: bleed → suprimido.
        assert!(!arb.accept(6, voice_priority(Some(DrumVoice::Overhead)), 1010, 0.1));
        // Canal 6 forte: golpe real → aceito.
        assert!(arb.accept(6, voice_priority(Some(DrumVoice::Overhead)), 1010, 0.8));
    }
}
