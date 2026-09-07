//! Renderer-independent control state shared by native and WASM hosts.
//!
//! Hosts gate disabled/read-only input, map platform events, and invalidate paint.
//! These models never execute application actions or own bindings. Editing uses
//! Unicode scalar boundaries, not grapheme clusters, shaping, or an IME session.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    InvalidRange,
    NonFiniteValue,
    DuplicateItem(usize),
    UnknownItem(usize),
    InvalidTreeOrder(usize),
    InvalidTextBoundary(usize),
    MultilineText,
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRange => {
                write!(f, "range needs finite min <= max and positive finite step")
            }
            Self::NonFiniteValue => write!(f, "value must be finite"),
            Self::DuplicateItem(id) => write!(f, "duplicate item {id}"),
            Self::UnknownItem(id) => write!(f, "unknown item {id}"),
            Self::InvalidTreeOrder(id) => {
                write!(f, "tree item {id} must follow its parent in preorder")
            }
            Self::InvalidTextBoundary(offset) => write!(f, "invalid UTF-8 text boundary {offset}"),
            Self::MultilineText => write!(f, "single-line text cannot contain CR or LF"),
        }
    }
}
impl std::error::Error for ModelError {}

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

/// Slider/stepper value. Steps are anchored at min; both endpoints are reachable
/// even when max is not on the step grid. Mutations return whether value changed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeModel {
    min: f64,
    max: f64,
    step: f64,
    value: f64,
}
impl RangeModel {
    pub fn new(min: f64, max: f64, step: f64, value: f64) -> Result<Self, ModelError> {
        if !min.is_finite()
            || !max.is_finite()
            || min > max
            || !step.is_finite()
            || step <= 0.0
            || !(max - min).is_finite()
            || !((max - min) / step).is_finite()
        {
            return Err(ModelError::InvalidRange);
        }
        let mut model = Self {
            min,
            max,
            step,
            value: min,
        };
        model.set_value(value)?;
        Ok(model)
    }
    pub fn min(&self) -> f64 {
        self.min
    }
    pub fn max(&self) -> f64 {
        self.max
    }
    pub fn step(&self) -> f64 {
        self.step
    }
    pub fn value(&self) -> f64 {
        self.value
    }
    pub fn fraction(&self) -> f64 {
        if self.min == self.max {
            0.0
        } else {
            (self.value - self.min) / (self.max - self.min)
        }
    }
    pub fn set_value(&mut self, value: f64) -> Result<bool, ModelError> {
        if !value.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        let next = if value <= self.min {
            self.min
        } else if value >= self.max {
            self.max
        } else {
            (self.min + ((value - self.min) / self.step).round() * self.step)
                .clamp(self.min, self.max)
        };
        let changed = self.value != next;
        self.value = next;
        Ok(changed)
    }
    pub fn set_fraction(&mut self, fraction: f64) -> Result<bool, ModelError> {
        if !fraction.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        self.set_value(self.min + (self.max - self.min) * fraction.clamp(0.0, 1.0))
    }
    pub fn step_by(&mut self, steps: i64) -> bool {
        if steps == 0 || self.min == self.max {
            return false;
        }
        let position = (self.value - self.min) / self.step;
        // Avoid skipping a grid value because 0.3 / 0.1 is just below 3.
        let nearest = position.round();
        let position = if (position - nearest).abs() <= 8.0 * f64::EPSILON * position.abs().max(1.0)
        {
            nearest
        } else {
            position
        };
        let origin = if steps > 0 {
            position.floor()
        } else {
            position.ceil()
        };
        let index = origin + steps as f64;
        let target = if index <= 0.0 {
            self.min
        } else if index >= (self.max - self.min) / self.step {
            self.max
        } else {
            self.min + index * self.step
        };
        self.set_value(target)
            .expect("validated range produces finite values")
    }
}

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

fn validate_ids(items: &[usize]) -> Result<(), ModelError> {
    let mut seen = HashSet::new();
    for id in items {
        if !seen.insert(*id) {
            return Err(ModelError::DuplicateItem(*id));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeItem {
    pub id: usize,
    pub parent: Option<usize>,
}

/// A preorder forest. Collapsing an ancestor moves a hidden selection to that
/// ancestor; keyboard movement visits visible rows only.
#[derive(Debug, Clone)]
pub struct TreeModel {
    items: Vec<TreeItem>,
    parents: HashMap<usize, Option<usize>>,
    expanded: HashSet<usize>,
    selected: Option<usize>,
}
impl TreeModel {
    pub fn new(items: Vec<TreeItem>) -> Result<Self, ModelError> {
        validate_ids(&items.iter().map(|item| item.id).collect::<Vec<_>>())?;
        let mut ancestors = Vec::new();
        for item in &items {
            match item.parent {
                None => ancestors.clear(),
                Some(parent) => {
                    let Some(index) = ancestors.iter().position(|id| *id == parent) else {
                        return Err(ModelError::InvalidTreeOrder(item.id));
                    };
                    ancestors.truncate(index + 1);
                }
            }
            ancestors.push(item.id);
        }
        let parents = items.iter().map(|item| (item.id, item.parent)).collect();
        Ok(Self {
            items,
            parents,
            expanded: HashSet::new(),
            selected: None,
        })
    }
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
    pub fn is_expanded(&self, id: usize) -> bool {
        self.expanded.contains(&id)
    }
    pub fn is_branch(&self, id: usize) -> bool {
        self.items.iter().any(|item| item.parent == Some(id))
    }
    pub fn visible_ids(&self) -> Vec<usize> {
        let mut visible = HashSet::new();
        let mut ids = Vec::new();
        for item in &self.items {
            if item
                .parent
                .is_none_or(|parent| visible.contains(&parent) && self.expanded.contains(&parent))
            {
                visible.insert(item.id);
                ids.push(item.id);
            }
        }
        ids
    }
    pub fn select(&mut self, id: usize) -> Result<bool, ModelError> {
        if !self.visible_ids().contains(&id) {
            return Err(ModelError::UnknownItem(id));
        }
        let changed = self.selected != Some(id);
        self.selected = Some(id);
        Ok(changed)
    }
    pub fn set_expanded(&mut self, id: usize, expanded: bool) -> Result<bool, ModelError> {
        if !self.parents.contains_key(&id) {
            return Err(ModelError::UnknownItem(id));
        }
        if !self.is_branch(id) {
            return Ok(false);
        }
        let changed = if expanded {
            self.expanded.insert(id)
        } else {
            self.expanded.remove(&id)
        };
        if !expanded {
            let mut ancestor = self.selected;
            while let Some(node) = ancestor {
                if node == id {
                    self.selected = Some(id);
                    break;
                }
                ancestor = self.parents[&node];
            }
        }
        Ok(changed)
    }
    pub fn toggle_expanded(&mut self, id: usize) -> Result<bool, ModelError> {
        self.set_expanded(id, !self.is_expanded(id))
    }
    pub fn move_by(&mut self, delta: isize) -> bool {
        let mut selection = SingleSelection::new(self.visible_ids(), self.selected).unwrap();
        let changed = selection.move_by(delta, false);
        self.selected = selection.selected();
        changed
    }
    pub fn first(&mut self) -> bool {
        self.visible_ids()
            .first()
            .copied()
            .is_some_and(|id| self.select(id).unwrap())
    }
    pub fn last(&mut self) -> bool {
        self.visible_ids()
            .last()
            .copied()
            .is_some_and(|id| self.select(id).unwrap())
    }
    pub fn right(&mut self) -> bool {
        let Some(id) = self.selected else {
            return self.first();
        };
        if !self.is_branch(id) {
            return false;
        }
        if !self.is_expanded(id) {
            return self.set_expanded(id, true).unwrap();
        }
        let child = self
            .items
            .iter()
            .find(|item| item.parent == Some(id))
            .unwrap()
            .id;
        self.select(child).unwrap()
    }
    pub fn left(&mut self) -> bool {
        let Some(id) = self.selected else {
            return false;
        };
        if self.is_expanded(id) {
            return self.set_expanded(id, false).unwrap();
        }
        self.parents[&id].is_some_and(|parent| self.select(parent).unwrap())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextSnapshot {
    value: String,
    caret: usize,
    anchor: usize,
}

/// Single-line editor. Caret/anchor are UTF-8 byte offsets, always char boundaries.
/// Each edit is one undo step; selection movement alone does not create history.
#[derive(Debug, Clone)]
pub struct TextEdit {
    multiline: bool,
    current: TextSnapshot,
    undo: Vec<TextSnapshot>,
    redo: Vec<TextSnapshot>,
}
impl TextEdit {
    pub fn new(value: String) -> Result<Self, ModelError> { Self::with_multiline(value, false) }
    pub fn with_multiline(value: String, multiline: bool) -> Result<Self, ModelError> {
        if !multiline {validate_line(&value)?;}
        let caret = value.len();
        Ok(Self {
            multiline,
            current: TextSnapshot {
                value,
                caret,
                anchor: caret,
            },
            undo: vec![],
            redo: vec![],
        })
    }
    pub fn value(&self) -> &str {
        &self.current.value
    }
    pub fn caret(&self) -> usize {
        self.current.caret
    }
    pub fn anchor(&self) -> usize {
        self.current.anchor
    }
    pub fn selection(&self) -> Range<usize> {
        self.caret().min(self.anchor())..self.caret().max(self.anchor())
    }
    pub fn selected_text(&self) -> &str {
        &self.value()[self.selection()]
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn set_selection(&mut self, anchor: usize, caret: usize) -> Result<bool, ModelError> {
        for offset in [anchor, caret] {
            if !self.value().is_char_boundary(offset) {
                return Err(ModelError::InvalidTextBoundary(offset));
            }
        }
        let changed = self.anchor() != anchor || self.caret() != caret;
        self.current.anchor = anchor;
        self.current.caret = caret;
        Ok(changed)
    }
    pub fn select_all(&mut self) -> bool {
        self.set_selection(0, self.value().len()).unwrap()
    }
    fn move_to(&mut self, caret: usize, extend: bool) -> bool {
        self.set_selection(if extend { self.anchor() } else { caret }, caret)
            .unwrap()
    }
    pub fn home(&mut self, extend: bool) -> bool {
        self.move_to(0, extend)
    }
    pub fn end(&mut self, extend: bool) -> bool {
        self.move_to(self.value().len(), extend)
    }
    pub fn left(&mut self, extend: bool) -> bool {
        self.move_chars(-1, extend)
    }
    pub fn right(&mut self, extend: bool) -> bool {
        self.move_chars(1, extend)
    }
    /// Move by Unicode scalars; collapsing a selection consumes the first step.
    pub fn move_chars(&mut self, delta: isize, extend: bool) -> bool {
        if delta == 0 {
            return false;
        }
        let mut count = delta.unsigned_abs();
        let start = if !extend && !self.selection().is_empty() {
            count -= 1;
            if delta < 0 {
                self.selection().start
            } else {
                self.selection().end
            }
        } else {
            self.caret()
        };
        let target = if count == 0 {
            start
        } else if delta < 0 {
            self.value()[..start]
                .char_indices()
                .rev()
                .nth(count - 1)
                .map_or(0, |(i, _)| i)
        } else {
            self.value()[start..]
                .char_indices()
                .nth(count)
                .map_or(self.value().len(), |(i, _)| start + i)
        };
        self.move_to(target, extend)
    }
    fn replace(&mut self, range: Range<usize>, text: &str) -> bool {
        let caret = range.start + text.len();
        if &self.value()[range.clone()] == text {
            return self.set_selection(caret, caret).unwrap();
        }
        if self.undo.len() == 100 {
            self.undo.remove(0);
        }
        self.undo.push(self.current.clone());
        self.redo.clear();
        self.current.value.replace_range(range, text);
        self.current.caret = caret;
        self.current.anchor = caret;
        true
    }
    pub fn insert(&mut self, text: &str) -> Result<bool, ModelError> {
        if !self.multiline {validate_line(text)?;}
        Ok(self.replace(self.selection(), text))
    }
    pub fn backspace(&mut self) -> bool {
        let range = if self.selection().is_empty() {
            self.value()[..self.caret()]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)..self.caret()
        } else {
            self.selection()
        };
        self.replace(range, "")
    }
    pub fn delete(&mut self) -> bool {
        let range = if self.selection().is_empty() {
            self.caret()
                ..self.caret()
                    + self.value()[self.caret()..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
        } else {
            self.selection()
        };
        self.replace(range, "")
    }
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.current, previous));
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.current, next));
        true
    }
    /// Replace externally supplied text and reset local edit history.
    pub fn reset(&mut self, value: String) -> Result<bool, ModelError> {
        if !self.multiline {validate_line(&value)?;}
        let changed =
            self.value() != value || self.caret() != value.len() || self.anchor() != value.len();
        *self = Self::new(value)?;
        Ok(changed)
    }
}
fn validate_line(value: &str) -> Result<(), ModelError> {
    if value.contains(['\r', '\n']) {
        Err(ModelError::MultilineText)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_and_tristate_checks_have_explicit_cycles() {
        let mut binary = CheckModel::new(CheckState::Mixed, CheckMode::Binary);
        binary.activate();
        assert_eq!(binary.state(), CheckState::Checked);
        binary.activate();
        assert_eq!(binary.state(), CheckState::Unchecked);
        let mut tri = CheckModel::new(CheckState::Unchecked, CheckMode::TriState);
        for expected in [
            CheckState::Checked,
            CheckState::Mixed,
            CheckState::Unchecked,
        ] {
            assert!(tri.activate());
            assert_eq!(tri.state(), expected);
        }
        let mut toggle = ToggleModel::default();
        assert!(!toggle.set_value(false));
        assert!(toggle.activate());
        assert!(toggle.value());
    }

    #[test]
    fn range_snaps_clamps_and_reaches_non_grid_endpoints() {
        let mut range = RangeModel::new(0.0, 10.0, 3.0, 4.0).unwrap();
        assert_eq!(range.value(), 3.0);
        range.step_by(2);
        assert_eq!(range.value(), 9.0);
        range.step_by(1);
        assert_eq!(range.value(), 10.0);
        assert!(!range.step_by(1));
        range.step_by(-1);
        assert_eq!(range.value(), 9.0);
        range.step_by(i64::MIN);
        assert_eq!(range.value(), 0.0);
        range.step_by(i64::MAX);
        assert_eq!(range.value(), 10.0);
        range.set_fraction(-2.0).unwrap();
        assert_eq!(range.fraction(), 0.0);
        range.set_fraction(2.0).unwrap();
        assert_eq!(range.fraction(), 1.0);
    }

    #[test]
    fn decimal_steps_do_not_skip_and_invalid_updates_are_atomic() {
        let mut range = RangeModel::new(-1.0, 1.0, 0.1, -0.7).unwrap();
        range.step_by(1);
        assert!((range.value() + 0.6).abs() < 1e-12);
        range.step_by(-1);
        assert!((range.value() + 0.7).abs() < 1e-12);
        let before = range;
        assert!(range.set_value(f64::NAN).is_err());
        assert_eq!(range, before);
        assert!(range.set_fraction(f64::INFINITY).is_err());
        assert_eq!(range, before);
        for (min, max, step) in [
            (1.0, 0.0, 1.0),
            (0.0, 1.0, 0.0),
            (0.0, 1.0, -1.0),
            (0.0, f64::INFINITY, 1.0),
            (-f64::MAX, f64::MAX, 1.0),
        ] {
            assert!(RangeModel::new(min, max, step, 0.0).is_err());
        }
        let mut fixed = RangeModel::new(4.0, 4.0, 1.0, 9.0).unwrap();
        assert_eq!(fixed.value(), 4.0);
        assert_eq!(fixed.fraction(), 0.0);
        assert!(!fixed.step_by(1));
    }

    #[test]
    fn exclusive_selection_uses_ids_and_wraps_or_clamps() {
        let mut radio = RadioGroup::new(vec![10, 30, 50], None).unwrap();
        assert!(radio.move_by(-1, false));
        assert_eq!(radio.selected(), Some(50));
        assert!(!radio.move_by(1, false));
        assert!(radio.move_by(1, true));
        assert_eq!(radio.selected(), Some(10));
        radio.select(30).unwrap();
        assert!(!radio.select(30).unwrap());
        assert!(radio.select(999).is_err());
        assert_eq!(radio.selected(), Some(30));
        radio.replace_items(vec![50, 30]).unwrap();
        assert_eq!(radio.selected(), Some(30));
        assert!(radio.replace_items(vec![1, 1]).is_err());
        assert_eq!(radio.items(), &[50, 30]);
        radio.replace_items(vec![50]).unwrap();
        assert_eq!(radio.selected(), None);
        let mut empty = TabSelection::new(vec![], None).unwrap();
        assert!(!empty.first());
        assert!(!empty.last());
        assert!(!empty.move_by(isize::MAX, true));
    }

    fn tree() -> TreeModel {
        TreeModel::new(vec![
            TreeItem {
                id: 1,
                parent: None,
            },
            TreeItem {
                id: 2,
                parent: Some(1),
            },
            TreeItem {
                id: 3,
                parent: Some(2),
            },
            TreeItem {
                id: 4,
                parent: Some(1),
            },
            TreeItem {
                id: 5,
                parent: None,
            },
        ])
        .unwrap()
    }
    #[test]
    fn tree_navigation_visits_visible_rows_and_collapse_preserves_visible_selection() {
        let mut tree = tree();
        assert_eq!(tree.visible_ids(), vec![1, 5]);
        assert!(tree.select(3).is_err());
        tree.right();
        assert_eq!(tree.selected(), Some(1));
        tree.right();
        assert!(tree.is_expanded(1));
        tree.right();
        assert_eq!(tree.selected(), Some(2));
        tree.right();
        tree.right();
        assert_eq!(tree.selected(), Some(3));
        assert_eq!(tree.visible_ids(), vec![1, 2, 3, 4, 5]);
        tree.set_expanded(1, false).unwrap();
        assert_eq!(tree.selected(), Some(1));
        tree.move_by(1);
        assert_eq!(tree.selected(), Some(5));
        assert!(!tree.left());
        tree.select(1).unwrap();
        tree.right();
        tree.right();
        assert_eq!(tree.selected(), Some(2));
        tree.left();
        assert!(!tree.is_expanded(2));
        tree.left();
        assert_eq!(tree.selected(), Some(1));
    }
    #[test]
    fn malformed_tree_order_and_duplicate_ids_are_rejected() {
        assert!(TreeModel::new(vec![TreeItem {
            id: 1,
            parent: Some(1)
        }])
        .is_err());
        assert!(TreeModel::new(vec![
            TreeItem {
                id: 1,
                parent: None
            },
            TreeItem {
                id: 1,
                parent: None
            }
        ])
        .is_err());
        assert!(TreeModel::new(vec![
            TreeItem {
                id: 1,
                parent: None
            },
            TreeItem {
                id: 2,
                parent: None
            },
            TreeItem {
                id: 3,
                parent: Some(1)
            }
        ])
        .is_err());
    }

    #[test]
    fn text_caret_selection_and_deletion_obey_utf8_boundaries() {
        let mut edit = TextEdit::new("Aя🦀e\u{301}".into()).unwrap();
        edit.home(false);
        edit.right(false);
        assert_eq!(edit.caret(), 1);
        edit.right(false);
        assert_eq!(edit.caret(), 3);
        edit.right(true);
        assert_eq!(edit.selected_text(), "🦀");
        edit.insert("中").unwrap();
        assert_eq!(edit.value(), "Aя中e\u{301}");
        assert_eq!(edit.caret(), 6);
        edit.backspace();
        assert_eq!(edit.value(), "Aяe\u{301}");
        edit.delete();
        assert_eq!(edit.value(), "Aя\u{301}");
        let before = edit.current.clone();
        assert!(edit.set_selection(2, 3).is_err());
        assert_eq!(edit.current, before);
        assert!(edit.set_selection(0, 999).is_err());
        assert_eq!(edit.current, before);
        assert!(edit.insert("bad\nline").is_err());
        assert_eq!(edit.current, before);
    }

    #[test]
    fn text_selection_collapses_and_home_end_extend_from_anchor() {
        let mut edit = TextEdit::new("абв".into()).unwrap();
        edit.set_selection(2, 6).unwrap();
        edit.left(false);
        assert_eq!(edit.selection(), 2..2);
        edit.end(true);
        assert_eq!(edit.selected_text(), "бв");
        edit.home(true);
        assert_eq!(edit.selected_text(), "а");
        assert_eq!(edit.anchor(), 2);
        edit.right(false);
        assert_eq!(edit.selection(), 2..2);
        edit.select_all();
        edit.delete();
        assert_eq!(edit.value(), "");
        assert!(!edit.backspace());
        assert!(!edit.delete());
        assert!(!edit.left(false));
        assert!(!edit.right(false));
    }

    #[test]
    fn text_moves_multiple_unicode_scalars_and_clamps_large_deltas() {
        let mut edit = TextEdit::new("Aя🦀中".into()).unwrap();
        edit.home(false);
        edit.move_chars(3, true);
        assert_eq!(edit.selected_text(), "Aя🦀");
        edit.move_chars(-2, false);
        assert_eq!(edit.caret(), 0);
        edit.move_chars(isize::MAX, false);
        assert_eq!(edit.caret(), edit.value().len());
        edit.move_chars(isize::MIN, true);
        assert_eq!(edit.selected_text(), "Aя🦀中");
    }

    #[test]
    fn undo_restores_text_and_selection_and_new_edits_drop_redo() {
        let mut edit = TextEdit::new("one".into()).unwrap();
        edit.select_all();
        edit.insert("два").unwrap();
        assert!(edit.undo());
        assert_eq!(edit.value(), "one");
        assert_eq!(edit.selected_text(), "one");
        assert!(edit.redo());
        assert_eq!(edit.value(), "два");
        assert_eq!(edit.caret(), 6);
        edit.undo();
        edit.insert("three").unwrap();
        assert!(!edit.can_redo());
        edit.home(false);
        edit.end(false);
        edit.undo();
        assert_eq!(edit.value(), "one");
        assert!(edit.reset("fresh".into()).unwrap());
        assert!(!edit.can_undo());
        assert!(!edit.can_redo());
    }

    #[test]
    fn edit_history_is_bounded_and_noop_edits_do_not_add_undo() {
        let mut edit = TextEdit::new(String::new()).unwrap();
        assert!(!edit.insert("").unwrap());
        assert!(!edit.can_undo());
        for _ in 0..110 {
            edit.insert("x").unwrap();
        }
        let mut steps = 0;
        while edit.undo() {
            steps += 1;
        }
        assert_eq!(steps, 100);
        assert_eq!(edit.value(), "xxxxxxxxxx");
        assert!(TextEdit::new("one\rtwo".into()).is_err());
    }
}
