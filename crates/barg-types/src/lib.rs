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

/// Seção de arranjo (Gap D) — cena disparável por pad/pedal que muda o estado do
/// acompanhamento (orquestração/energia/vamp). Modelo Fela/JB: navegar a forma
/// verso→solo→break→ponte regendo pela energia, não trocando de instrumento.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneId {
    Intro = 0,
    Verso = 1,
    Solo = 2,
    Break = 3,
    Ponte = 4,
}

pub const SCENE_COUNT: usize = 5;

impl SceneId {
    pub fn from_index(i: u32) -> Option<Self> {
        match i {
            0 => Some(SceneId::Intro),
            1 => Some(SceneId::Verso),
            2 => Some(SceneId::Solo),
            3 => Some(SceneId::Break),
            4 => Some(SceneId::Ponte),
            _ => None,
        }
    }
}

/// Escala modal do vamp de UM acorde (harmonia estática — sem rastrear acordes).
/// Fela puxa para o menor-sétima/dórico; JB para o dominante-sétima/mixolídio
/// (ver `concept.md §3`). É dado para o motor de baixo/temas (Gaps C/E); a máquina
/// de arranjo já o carrega por cena, mesmo que ainda nada o soe.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dorian = 0,     // menor-sétima modal (Fela)
    Mixolydian = 1, // dominante-sétima bluesy (JB)
    Aeolian = 2,    // menor natural
    Ionian = 3,     // maior
}

impl Mode {
    /// Terça do modo (semitons a partir da fundamental): menor (3) nos modos menores,
    /// maior (4) nos maiores. É o que distingue dórico (Fela) de mixolídio (JB) no baixo.
    pub fn third(self) -> i8 {
        match self {
            Mode::Dorian | Mode::Aeolian => 3,
            Mode::Mixolydian | Mode::Ionian => 4,
        }
    }

    /// Sétima do modo: menor (10) nos modais Fela/JB, maior (11) no jônio.
    pub fn seventh(self) -> i8 {
        match self {
            Mode::Ionian => 11,
            _ => 10,
        }
    }

    /// Paleta de tons do acorde para o baixo do vamp, em semitons a partir da
    /// fundamental: [fundamental, terça, quinta, sétima, oitava]. Um acorde só
    /// (harmonia estática) — o baixo ostinatia sobre estes.
    pub fn chord_tones(self) -> [i8; 5] {
        [0, self.third(), 7, self.seventh(), 12]
    }
}

/// Nota de baixo gerada para um compasso (vamp de um acorde). Pitch em MIDI.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BassNote {
    pub midi_note: u8,
    pub frame_offset: i32,
    pub gain: f32,
}

/// Cue de transição (control → generation thread).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub enum Cue {
    SetPreset(PresetId),
    Paradinha,
    TriggerVoice { voice: Voice, gain: f32 },
    /// Lança uma cena de arranjo na próxima fronteira de compasso (Gap D).
    LaunchScene(SceneId),
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
