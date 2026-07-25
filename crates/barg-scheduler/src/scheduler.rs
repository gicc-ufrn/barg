//! SchedulerAntecipado — consome BarBuffers na fronteira de compasso.
//! RT-safe: pop de rtrb é wait-free, clone de BarBuffer é memcpy (heapless).

use crate::bar_buffer::BarBuffer;
use casa13_types::NoteEvent;

pub struct SchedulerAntecipado {
    current_bar: Option<BarBuffer>,
    fallback_bar: Option<BarBuffer>,
    overrun_count: u32,
    cold_start: bool,
}

impl SchedulerAntecipado {
    pub fn new() -> Self {
        Self {
            current_bar: None,
            fallback_bar: None,
            overrun_count: 0,
            cold_start: true,
        }
    }

    /// Chamado na fronteira de compasso (step 0). Tenta pop da BarQueue.
    /// RT-safe. Retorna `true` se consumiu um compasso fresco da fila (uma vaga foi
    /// liberada) — o caller usa isso para sinalizar a gen thread (§5.1).
    pub fn on_bar_boundary(&mut self, consumer: &mut rtrb::Consumer<BarBuffer>) -> bool {
        // Salva current como fallback
        if let Some(ref current) = self.current_bar {
            self.fallback_bar = Some(current.clone());
        }

        match consumer.pop() {
            Ok(bar) => {
                self.current_bar = Some(bar);
                self.cold_start = false;
                true
            }
            Err(_) => {
                if self.cold_start {
                    self.current_bar = None;
                } else {
                    // Overrun: repete último compasso
                    self.current_bar = self.fallback_bar.clone();
                    self.overrun_count += 1;
                }
                false
            }
        }
    }

    /// Filtra eventos cujo frame_offset cai no range [step_start, step_end)
    /// e calcula o offset relativo adicionando step_offset.
    pub fn dispatch_events(
        &self,
        step_start: i32,
        step_end: i32,
        step_offset: i32,
    ) -> impl Iterator<Item = (&NoteEvent, i32)> {
        self.current_events().iter().filter_map(move |ev| {
            if ev.frame_offset >= step_start && ev.frame_offset < step_end {
                Some((ev, ev.frame_offset - step_start + step_offset))
            } else {
                None
            }
        })
    }

    pub fn dispatch_bass(
        &self,
        step_start: i32,
        step_end: i32,
        step_offset: i32,
    ) -> impl Iterator<Item = (&casa13_types::BassNote, i32)> {
        self.current_bass().iter().filter_map(move |n| {
            if n.frame_offset >= step_start && n.frame_offset < step_end {
                Some((n, n.frame_offset - step_start + step_offset))
            } else {
                None
            }
        })
    }

    pub fn dispatch_theme(
        &self,
        step_start: i32,
        step_end: i32,
        step_offset: i32,
    ) -> impl Iterator<Item = (&casa13_types::ThemeNote, i32)> {
        match &self.current_bar {
            Some(bar) => bar.theme.as_slice(),
            None => &[],
        }
        .iter()
        .filter_map(move |n| {
            if n.frame_offset >= step_start && n.frame_offset < step_end {
                Some((n, n.frame_offset - step_start + step_offset))
            } else {
                None
            }
        })
    }

    /// Retorna todos os eventos do compasso atual (para dispatch simplificado).
    pub fn current_events(&self) -> &[NoteEvent] {
        match &self.current_bar {
            Some(bar) => bar.events.as_slice(),
            None => &[],
        }
    }

    /// Retorna as notas de baixo do compasso atual (Gap C).
    pub fn current_bass(&self) -> &[casa13_types::BassNote] {
        match &self.current_bar {
            Some(bar) => bar.bass.as_slice(),
            None => &[],
        }
    }

    pub fn overrun_count(&self) -> u32 {
        self.overrun_count
    }

    pub fn current_bar(&self) -> Option<&BarBuffer> {
        self.current_bar.as_ref()
    }

    pub fn reset(&mut self) {
        self.current_bar = None;
        self.fallback_bar = None;
        self.overrun_count = 0;
        self.cold_start = true;
    }
}

impl Default for SchedulerAntecipado {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bar_buffer::bar_queue_new;
    use casa13_types::Voice;

    #[test]
    fn cold_start_silence() {
        let (_prod, mut cons) = bar_queue_new(2);
        let mut sched = SchedulerAntecipado::new();
        sched.on_bar_boundary(&mut cons);
        assert!(sched.current_events().is_empty());
        assert_eq!(sched.overrun_count(), 0);
    }

    #[test]
    fn normal_consumption() {
        let (mut prod, mut cons) = bar_queue_new(2);
        let mut sched = SchedulerAntecipado::new();

        let mut bar = BarBuffer::new();
        bar.bar_number = 42;
        bar.events
            .push(NoteEvent {
                voice: Voice::Agogo,
                frame_offset: 0,
                microtiming_ms: 0.0,
                gain: 0.8,
                is_paradinha: false,
            })
            .unwrap();
        prod.push(bar).unwrap();

        sched.on_bar_boundary(&mut cons);
        assert_eq!(sched.current_events().len(), 1);
        assert_eq!(sched.current_events()[0].voice, Voice::Agogo);
    }

    #[test]
    fn overrun_uses_fallback() {
        let (mut prod, mut cons) = bar_queue_new(2);
        let mut sched = SchedulerAntecipado::new();

        // Produce and consume one bar
        let mut bar = BarBuffer::new();
        bar.events
            .push(NoteEvent {
                voice: Voice::SurdoLow,
                frame_offset: 100,
                microtiming_ms: 7.0,
                gain: 0.9,
                is_paradinha: false,
            })
            .unwrap();
        prod.push(bar).unwrap();
        sched.on_bar_boundary(&mut cons);
        assert_eq!(sched.overrun_count(), 0);

        // Now queue is empty → overrun
        sched.on_bar_boundary(&mut cons);
        assert_eq!(sched.overrun_count(), 1);
        // Fallback should have the previous bar's events
        assert_eq!(sched.current_events().len(), 1);
        assert_eq!(sched.current_events()[0].voice, Voice::SurdoLow);
    }
}
