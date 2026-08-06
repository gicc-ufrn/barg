// SPDX-License-Identifier: Apache-2.0
//! BARG — primitivas de DSP para **análise** do gesto.
//!
//! - [`GrooveClock`] — relógio sample-accurate e determinístico, que é a grade de
//!   referência contra a qual o gesto é situado.
//! - [`IntensityAnalyzer`] — energia e densidade a partir do sinal.
//!
//! Ambos são RT-safe: sem alocação, lock, I/O ou panic no caminho quente.
//!
//! **Síntese sonora não vive aqui.** Gerar som é do instrumento, não da análise:
//! nada disso é necessário para reproduzir a comparação entre execuções nem para
//! implementar o FARG.

pub mod clock;
pub mod intensity;

pub use clock::GrooveClock;
pub use intensity::IntensityAnalyzer;
