//! BarBuffer — struct de compasso pré-gerado + wrappers de fila SPSC.

use casa13_types::{BassNote, NoteEvent, PresetId};
use heapless::Vec as HVec;

pub const MAX_EVENTS_PER_BAR: usize = 128;
pub const MAX_BASS_PER_BAR: usize = 32;

/// Buffer de um compasso completo pré-gerado.
/// Stack-allocated via heapless::Vec — clone é memcpy, zero heap.
#[derive(Clone)]
pub struct BarBuffer {
    pub events: HVec<NoteEvent, MAX_EVENTS_PER_BAR>,
    /// Linha de baixo do compasso (vamp de um acorde — Gap C).
    pub bass: HVec<BassNote, MAX_BASS_PER_BAR>,
    pub bar_number: i64,
    pub bpm_at_generation: f64,
    pub preset: PresetId,
}

impl BarBuffer {
    pub fn new() -> Self {
        Self {
            events: HVec::new(),
            bass: HVec::new(),
            bar_number: 0,
            bpm_at_generation: 100.0,
            preset: PresetId::Ijexa,
        }
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.bass.clear();
        self.bar_number = 0;
    }
}

impl Default for BarBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Cria par (Producer, Consumer) para a BarQueue.
/// Capacidade tipicamente = 2 (um compasso de look-ahead).
pub fn bar_queue_new(capacity: usize) -> (rtrb::Producer<BarBuffer>, rtrb::Consumer<BarBuffer>) {
    rtrb::RingBuffer::new(capacity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use casa13_types::Voice;

    #[test]
    fn bar_buffer_push_events() {
        let mut bar = BarBuffer::new();
        for i in 0..MAX_EVENTS_PER_BAR {
            let result = bar.events.push(NoteEvent {
                voice: Voice::Agogo,
                frame_offset: i as i32,
                microtiming_ms: 0.0,
                gain: 0.5,
                is_paradinha: false,
            });
            assert!(result.is_ok());
        }
        assert_eq!(bar.events.len(), MAX_EVENTS_PER_BAR);
        // 129th should fail
        let result = bar.events.push(NoteEvent {
            voice: Voice::Agogo,
            frame_offset: 0,
            microtiming_ms: 0.0,
            gain: 0.5,
            is_paradinha: false,
        });
        assert!(result.is_err());
    }

    #[test]
    fn bar_queue_fifo() {
        let (mut prod, mut cons) = bar_queue_new(2);

        let mut bar1 = BarBuffer::new();
        bar1.bar_number = 1;
        let mut bar2 = BarBuffer::new();
        bar2.bar_number = 2;

        prod.push(bar1).unwrap();
        prod.push(bar2).unwrap();

        let out1 = cons.pop().unwrap();
        assert_eq!(out1.bar_number, 1);
        let out2 = cons.pop().unwrap();
        assert_eq!(out2.bar_number, 2);
    }

    #[test]
    fn bar_queue_full_returns_err() {
        let (mut prod, _cons) = bar_queue_new(2);
        let bar = BarBuffer::new();
        assert!(prod.push(bar.clone()).is_ok());
        assert!(prod.push(bar.clone()).is_ok());
        assert!(prod.push(bar).is_err());
    }

    #[test]
    fn bar_queue_empty_returns_none() {
        let (_prod, mut cons) = bar_queue_new(2);
        assert!(cons.pop().is_err());
    }
}
