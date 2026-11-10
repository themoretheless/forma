//! Boolean toggle shared by switches and selectable chips.

/// Boolean interaction shared by a switch and a selectable chip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToggleModel {
    value: bool,
}
impl ToggleModel {
    pub fn new(value: bool) -> Self {
        Self { value }
    }
    pub fn value(&self) -> bool {
        self.value
    }
    pub fn set_value(&mut self, value: bool) -> bool {
        let changed = self.value != value;
        self.value = value;
        changed
    }
    pub fn activate(&mut self) -> bool {
        self.set_value(!self.value)
    }
}
pub type SwitchModel = ToggleModel;
pub type SelectableChipModel = ToggleModel;
