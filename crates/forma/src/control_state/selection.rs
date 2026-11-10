//! Exclusive selection by host-owned id, for radio groups and tabs.

use super::*;

/// Exclusive selection by host-owned ID, shared by radio groups and tabs.
/// Supply enabled/selectable IDs in navigation order; disabled policy stays in
/// the host. Changing the item set preserves an existing ID or clears selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleSelection {
    items: Vec<usize>,
    selected: Option<usize>,
}
impl SingleSelection {
    pub fn new(items: Vec<usize>, selected: Option<usize>) -> Result<Self, ModelError> {
        validate_ids(&items)?;
        if let Some(id) = selected {
            if !items.contains(&id) {
                return Err(ModelError::UnknownItem(id));
            }
        }
        Ok(Self { items, selected })
    }
    pub fn items(&self) -> &[usize] {
        &self.items
    }
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
    pub fn select(&mut self, id: usize) -> Result<bool, ModelError> {
        if !self.items.contains(&id) {
            return Err(ModelError::UnknownItem(id));
        }
        let changed = self.selected != Some(id);
        self.selected = Some(id);
        Ok(changed)
    }
    pub fn clear(&mut self) -> bool {
        self.selected.take().is_some()
    }
    pub fn replace_items(&mut self, items: Vec<usize>) -> Result<bool, ModelError> {
        validate_ids(&items)?;
        let selected = self.selected.filter(|id| items.contains(id));
        let changed = self.items != items || self.selected != selected;
        self.items = items;
        self.selected = selected;
        Ok(changed)
    }
    /// With no selection, a positive move selects first and a negative move last.
    pub fn move_by(&mut self, delta: isize, wrap: bool) -> bool {
        if self.items.is_empty() || delta == 0 {
            return false;
        }
        let last = self.items.len() - 1;
        let index = match self
            .selected
            .and_then(|id| self.items.iter().position(|item| *item == id))
        {
            None => {
                if delta > 0 {
                    0
                } else {
                    last
                }
            }
            Some(index) => {
                let next = index as i128 + delta as i128;
                if wrap {
                    next.rem_euclid(self.items.len() as i128) as usize
                } else {
                    next.clamp(0, last as i128) as usize
                }
            }
        };
        self.select(self.items[index])
            .expect("index belongs to selection")
    }
    pub fn first(&mut self) -> bool {
        self.items
            .first()
            .copied()
            .is_some_and(|id| self.select(id).unwrap())
    }
    pub fn last(&mut self) -> bool {
        self.items
            .last()
            .copied()
            .is_some_and(|id| self.select(id).unwrap())
    }
}
pub type RadioGroup = SingleSelection;
pub type TabSelection = SingleSelection;

pub(super) fn validate_ids(items: &[usize]) -> Result<(), ModelError> {
    let mut seen = HashSet::new();
    for id in items {
        if !seen.insert(*id) {
            return Err(ModelError::DuplicateItem(*id));
        }
    }
    Ok(())
}
