//! Casa 13 — camada de percepção (V4): transdução acústico → simbólica.
//!
//! Adapter na **porta de entrada** (Ports & Adapters, ver `docs/architecture-v4.md`):
//! converte áudio multicanal dos microfones da bateria em eventos simbólicos limpos
//! (`DrumHit`) mais um snapshot de energia por peça (`DrumEnergies`), que a Malha Lenta
//! consome. O domínio (geração) nunca vê áudio.
//!
//! **RT-safe:** roda no audio thread — sem alocação, sem locks, sem panic.
//!
//! Este crate é um ESQUELETO: as costuras (porta, tipos, fluxo onset→arbiter) são reais e
//! testáveis; o DSP de onset e as regras do árbitro estão marcados com TODO para calibração.

mod arbiter;
mod onset;

pub use arbiter::CrossChannelArbiter;
pub use onset::OnsetDetector;

/// Peça da bateria a que um golpe é atribuído. Ordem = índice do canal de entrada.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrumChannel {
    Kick = 0,
    Snare = 1,
    HiHat = 2,
    Toms = 3,
    Cymbals = 4,
}

pub const DRUM_CHANNEL_COUNT: usize = 5;

/// Canais em ordem de enum — permite iterar sem `unwrap()` no caminho quente.
pub const DRUM_CHANNELS: [DrumChannel; DRUM_CHANNEL_COUNT] = [
    DrumChannel::Kick,
    DrumChannel::Snare,
    DrumChannel::HiHat,
    DrumChannel::Toms,
    DrumChannel::Cymbals,
];

/// Snapshot de energia por peça — LATEST-VALUE.
/// Publicado à Malha Lenta por `triple_buffer` (wait-free, sem torn-read) — nunca por
/// Seqlock à mão (UB no Rust/ARM). Ver §4 e §7 do doc.
#[derive(Clone, Copy, Debug, Default)]
pub struct DrumEnergies {
    pub per_channel: [f32; DRUM_CHANNEL_COUNT],
}

impl DrumEnergies {
    #[inline]
    pub fn get(&self, ch: DrumChannel) -> f32 {
        self.per_channel[ch as usize]
    }
}

/// Golpe discreto já arbitrado — STREAM.
/// Transportado à Malha Lenta por SPSC ring (`rtrb`): não perder nenhum. Ver §4 do doc.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DrumHit {
    pub drum: DrumChannel,
    pub velocity: f32,        // 0..1, estimada do envelope no disparo
    pub frame_timestamp: u64, // frame absoluto no GrooveClock
}

/// Porta de entrada da percepção (Ports & Adapters). Um adapter consome um bloco
/// multicanal no audio thread, emite os `DrumHit` arbitrados via `emit` e devolve o
/// snapshot de energias.
///
/// Implementações substituíveis: `AcousticPerception` (mics); futuramente um adapter de
/// MIDI (kit eletrônico) ou um mock offline para testes — sem tocar no domínio.
pub trait PerceptionSource {
    /// `channels[i]` é o buffer do canal `i` (mapeado a `DRUM_CHANNELS[i]`).
    /// `frame_base` é o frame absoluto do primeiro sample do bloco.
    /// RT-safe: `emit` não deve alocar (tipicamente um push em SPSC).
    fn process_block(
        &mut self,
        channels: &[&[f32]],
        frame_base: u64,
        emit: &mut dyn FnMut(DrumHit),
    ) -> DrumEnergies;

    fn reset(&mut self);
}

/// Adapter acústico: onset por canal + arbitragem de bleed.
pub struct AcousticPerception {
    detectors: [OnsetDetector; DRUM_CHANNEL_COUNT],
    arbiter: CrossChannelArbiter,
}

impl AcousticPerception {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            detectors: core::array::from_fn(|_| OnsetDetector::new(sample_rate)),
            arbiter: CrossChannelArbiter::new(sample_rate),
        }
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
                break;
            }
            let ch = DRUM_CHANNELS[ci];
            for (j, &x) in channels[ci].iter().enumerate() {
                if let Some(velocity) = det.process(x) {
                    let frame = frame_base + j as u64;
                    if self.arbiter.accept(ch, frame, velocity) {
                        emit(DrumHit {
                            drum: ch,
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
    fn impulse_on_kick_emits_hit() {
        let mut p = AcousticPerception::new(48000.0);

        // Piso de silêncio, depois um burst curto forte no canal Kick (0).
        let mut kick = vec![0.0f32; 2048];
        for k in kick.iter_mut().skip(512).take(48) {
            *k = 0.8;
        }
        let silent = vec![0.0f32; 2048];
        let channels: [&[f32]; DRUM_CHANNEL_COUNT] =
            [&kick, &silent, &silent, &silent, &silent];

        let mut hits: Vec<DrumHit> = Vec::new();
        let energies = p.process_block(&channels, 0, &mut |h| hits.push(h));

        assert!(hits.iter().any(|h| h.drum == DrumChannel::Kick));
        assert!(energies.get(DrumChannel::Kick) > 0.0);
    }

    #[test]
    fn arbiter_suppresses_weak_coincident_bleed() {
        let mut arb = CrossChannelArbiter::new(48000.0);
        // Kick (prioridade alta) dispara forte no frame 1000.
        assert!(arb.accept(DrumChannel::Kick, 1000, 0.9));
        // Cymbals fraco no frame 1010 (coincidente, < janela): é bleed → suprimido.
        assert!(!arb.accept(DrumChannel::Cymbals, 1010, 0.1));
        // Cymbals forte no frame 1010: golpe real → aceito.
        assert!(arb.accept(DrumChannel::Cymbals, 1010, 0.8));
    }

    #[test]
    fn reset_clears_state() {
        let mut p = AcousticPerception::new(48000.0);
        let buf = vec![0.5f32; 256];
        let channels: [&[f32]; DRUM_CHANNEL_COUNT] = [&buf, &buf, &buf, &buf, &buf];
        let _ = p.process_block(&channels, 0, &mut |_| {});
        p.reset();
        let e = p.process_block(&[&vec![0.0f32; 64]], 0, &mut |_| {});
        assert_eq!(e.get(DrumChannel::Snare), 0.0);
    }
}
