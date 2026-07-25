//! Casa 13 — DSP components (GrooveClock, IntensityAnalyzer, PercussionSynth).
//! All components are RT-safe: no allocation, no locks, no panics in the hot path.

pub mod bass;
pub mod clock;
pub mod intensity;
pub mod synth;

pub use bass::{BassSynth, BassVoice};
pub use clock::GrooveClock;
pub use intensity::IntensityAnalyzer;
pub use synth::PercussionSynth;
