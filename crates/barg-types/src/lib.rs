//! Casa 13 — Tipos fundamentais compartilhados por todos os crates.
//! `no_std`-compatible para uso no audio thread.
#![no_std]

/// Vozes do kit estendido afro-brasileiro.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice {
    Agogo = 0,
    Tamborim = 1,
    Ganza = 2,
    SurdoLow = 3,
    SurdoResp = 4,
    Repique = 5,
}

pub const VOICE_COUNT: usize = 6;

impl Voice {
    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(Voice::Agogo),
            1 => Some(Voice::Tamborim),
            2 => Some(Voice::Ganza),
            3 => Some(Voice::SurdoLow),
            4 => Some(Voice::SurdoResp),
            5 => Some(Voice::Repique),
            _ => None,
        }
    }
}

/// Evento musical gerado pelo engine.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NoteEvent {
    pub voice: Voice,
    pub frame_offset: i32,
    pub microtiming_ms: f32,
    pub gain: f32,
    pub is_paradinha: bool,
}

/// Padrão de 16 steps como bitmask (bit 0 = step 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepPattern {
    pub mask: u16,
}

impl StepPattern {
    /// Cria pattern a partir de string de 16 chars onde 'x'/'X' = hit.
    pub const fn from_str(s: &[u8]) -> Self {
        let mut mask: u16 = 0;
        let mut i = 0;
        while i < 16 && i < s.len() {
            if s[i] == b'x' || s[i] == b'X' {
                mask |= 1u16 << i;
            }
            i += 1;
        }
        Self { mask }
    }

    #[inline]
    pub const fn hit(self, step: u8) -> bool {
        (self.mask >> step) & 1 != 0
    }

    /// Conta o número de hits no padrão.
    pub const fn count(self) -> u32 {
        self.mask.count_ones()
    }
}

/// Seções do kit (controladas por sliders do MiniLab).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Marcacao = 0,
    Timeline = 1,
    Subdivisao = 2,
    Cortes = 3,
}

pub const SECTION_COUNT: usize = 4;

/// Identificador de preset rítmico.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetId {
    Ijexa = 0,
    Samba = 1,
    Baiao = 2,
}

/// Cue de transição (control → generation thread).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub enum Cue {
    SetPreset(PresetId),
    Paradinha,
    TriggerVoice { voice: Voice, gain: f32 },
}

/// Evento emitido pelo GrooveClock a cada fronteira de step.
#[derive(Clone, Copy, Debug)]
pub struct StepEvent {
    pub step_in_bar: u8,
    pub bar: i64,
    pub frame_offset: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_pattern_from_str_ijexa_agogo() {
        let p = StepPattern::from_str(b"x.xx..x.x.xx..x.");
        assert!(p.hit(0));
        assert!(!p.hit(1));
        assert!(p.hit(2));
        assert!(p.hit(3));
        assert!(!p.hit(4));
        assert!(!p.hit(5));
        assert!(p.hit(6));
        assert!(!p.hit(7));
        assert!(p.hit(8));
        assert!(!p.hit(9));
        assert!(p.hit(10));
        assert!(p.hit(11));
        assert!(!p.hit(12));
        assert!(!p.hit(13));
        assert!(p.hit(14));
        assert!(!p.hit(15));
    }

    #[test]
    fn step_pattern_all_hits() {
        let p = StepPattern::from_str(b"xxxxxxxxxxxxxxxx");
        assert_eq!(p.count(), 16);
        for i in 0..16 {
            assert!(p.hit(i));
        }
    }

    #[test]
    fn step_pattern_empty() {
        let p = StepPattern::from_str(b"................");
        assert_eq!(p.count(), 0);
        for i in 0..16 {
            assert!(!p.hit(i));
        }
    }

    #[test]
    fn voice_from_index() {
        assert_eq!(Voice::from_index(0), Some(Voice::Agogo));
        assert_eq!(Voice::from_index(5), Some(Voice::Repique));
        assert_eq!(Voice::from_index(6), None);
    }
}
