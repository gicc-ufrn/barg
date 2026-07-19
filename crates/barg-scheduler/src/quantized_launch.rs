//! QuantizedLaunch — atrasa cues até a próxima fronteira de quantum.

use casa13_types::Cue;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchQuantum {
    Bar,  // próximo step 0
    Beat, // próximo step múltiplo de 4
}

pub struct QuantizedLaunch {
    pending_cue: Option<Cue>,
    quantum: LaunchQuantum,
}

impl QuantizedLaunch {
    pub fn new(quantum: LaunchQuantum) -> Self {
        Self {
            pending_cue: None,
            quantum,
        }
    }

    /// Enfileira cue (sobrescreve pendente anterior).
    pub fn enqueue(&mut self, cue: Cue) {
        self.pending_cue = Some(cue);
    }

    /// Chamado a cada step. Retorna Some(cue) se é fronteira de quantum.
    pub fn poll(&mut self, step: u8) -> Option<Cue> {
        self.pending_cue?; // nada pendente → None (Cue é Copy)
        let is_boundary = match self.quantum {
            LaunchQuantum::Bar => step == 0,
            LaunchQuantum::Beat => step.is_multiple_of(4),
        };
        if is_boundary {
            self.pending_cue.take()
        } else {
            None
        }
    }

    pub fn set_quantum(&mut self, q: LaunchQuantum) {
        self.quantum = q;
    }

    pub fn has_pending(&self) -> bool {
        self.pending_cue.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casa13_types::PresetId;

    #[test]
    fn bar_quantum_waits_for_step_0() {
        let mut ql = QuantizedLaunch::new(LaunchQuantum::Bar);
        ql.enqueue(Cue::SetPreset(PresetId::Samba));

        // Steps 1..15 should not fire
        for step in 1..16u8 {
            assert!(ql.poll(step).is_none());
        }
        // Step 0 fires
        let cue = ql.poll(0);
        assert!(cue.is_some());
        // After firing, no more pending
        assert!(ql.poll(0).is_none());
    }

    #[test]
    fn beat_quantum_fires_at_multiples_of_4() {
        let mut ql = QuantizedLaunch::new(LaunchQuantum::Beat);
        ql.enqueue(Cue::SetPreset(PresetId::Baiao));

        // Step 1, 2, 3 should not fire
        assert!(ql.poll(1).is_none());
        assert!(ql.poll(2).is_none());
        assert!(ql.poll(3).is_none());
        // Step 4 fires
        assert!(ql.poll(4).is_some());
    }

    #[test]
    fn last_cue_overwrites() {
        let mut ql = QuantizedLaunch::new(LaunchQuantum::Bar);
        ql.enqueue(Cue::SetPreset(PresetId::Ijexa));
        ql.enqueue(Cue::SetPreset(PresetId::Samba));

        let cue = ql.poll(0).unwrap();
        match cue {
            Cue::SetPreset(id) => assert_eq!(id, PresetId::Samba),
            _ => panic!("wrong cue type"),
        }
    }
}
