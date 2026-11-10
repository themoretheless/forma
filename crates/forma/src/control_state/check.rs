//! Checkbox state: two- and three-state modes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Unchecked,
    Checked,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckMode {
    /// An initially mixed checkbox becomes checked on activation.
    Binary,
    /// Unchecked -> Checked -> Mixed -> Unchecked.
    TriState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckModel {
    state: CheckState,
    mode: CheckMode,
}

impl CheckModel {
    pub fn new(state: CheckState, mode: CheckMode) -> Self {
        Self { state, mode }
    }
    pub fn state(&self) -> CheckState {
        self.state
    }
    pub fn set_state(&mut self, state: CheckState) -> bool {
        let changed = self.state != state;
        self.state = state;
        changed
    }
    pub fn activate(&mut self) -> bool {
        use CheckState::*;
        self.set_state(match (self.mode, self.state) {
            (_, Unchecked) | (CheckMode::Binary, Mixed) => Checked,
            (CheckMode::TriState, Checked) => Mixed,
            _ => Unchecked,
        })
    }
}
