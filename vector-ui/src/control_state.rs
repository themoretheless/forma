//! Renderer-independent control state shared by native and WASM hosts.
//!
//! Hosts gate disabled/read-only input, map platform events, and invalidate paint.
//! These models never execute application actions or own bindings. Editing uses
//! Unicode scalar boundaries, not grapheme clusters, shaping, or an IME session.

use std::collections::{BTreeSet, HashMap, HashSet};
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Column sort direction for [`DataTableModel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    None,
    Ascending,
    Descending,
}

/// Column descriptor for [`DataTableModel`].
#[derive(Clone, Debug, PartialEq)]
pub struct TableColumn {
    pub id: String,
    pub width: f64,
    pub min_width: f64,
    pub max_width: f64,
}

/// Interactive state model for `DataTable`: multi-column sorting, column width resizing,
/// text filtering, single/multi row selection, keyboard navigation, and row virtualization.
#[derive(Clone, Debug, PartialEq)]
pub struct DataTableModel {
    columns: Vec<TableColumn>,
    rows: Vec<Vec<String>>,
    sort_column: Option<usize>,
    sort_direction: SortDirection,
    filter: String,
    visible_rows: Vec<usize>,
    selected_rows: BTreeSet<usize>,
    active_row: Option<usize>,
    scroll_y: f64,
    row_height: f64,
    viewport_height: f64,
}

impl DataTableModel {
    pub fn new(
        columns: Vec<TableColumn>,
        rows: Vec<Vec<String>>,
        row_height: f64,
        viewport_height: f64,
    ) -> Result<Self, ModelError> {
        if !row_height.is_finite() || row_height <= 0.0 || !viewport_height.is_finite() || viewport_height < 0.0 {
            return Err(ModelError::NonFiniteValue);
        }
        let mut model = Self {
            columns,
            rows,
            sort_column: None,
            sort_direction: SortDirection::None,
            filter: String::new(),
            visible_rows: Vec::new(),
            selected_rows: BTreeSet::new(),
            active_row: None,
            scroll_y: 0.0,
            row_height,
            viewport_height,
        };
        model.recompute_view();
        Ok(model)
    }

    fn recompute_view(&mut self) {
        let query = self.filter.to_lowercase();
        let mut indices: Vec<usize> = (0..self.rows.len())
            .filter(|&r| {
                query.is_empty()
                    || self.rows[r]
                        .iter()
                        .any(|cell| cell.to_lowercase().contains(&query))
            })
            .collect();
        if let Some(col) = self.sort_column {
            match self.sort_direction {
                SortDirection::Ascending => {
                    indices.sort_by(|&a, &b| {
                        let ca = self.rows[a].get(col).map(String::as_str).unwrap_or("");
                        let cb = self.rows[b].get(col).map(String::as_str).unwrap_or("");
                        match (ca.parse::<f64>(), cb.parse::<f64>()) {
                            (Ok(na), Ok(nb)) => na.total_cmp(&nb),
                            _ => ca.cmp(cb),
                        }
                    });
                }
                SortDirection::Descending => {
                    indices.sort_by(|&a, &b| {
                        let ca = self.rows[a].get(col).map(String::as_str).unwrap_or("");
                        let cb = self.rows[b].get(col).map(String::as_str).unwrap_or("");
                        match (ca.parse::<f64>(), cb.parse::<f64>()) {
                            (Ok(na), Ok(nb)) => nb.total_cmp(&na),
                            _ => cb.cmp(ca),
                        }
                    });
                }
                SortDirection::None => {}
            }
        }
        self.visible_rows = indices;
        if let Some(active) = self.active_row {
            if !self.visible_rows.contains(&active) {
                self.active_row = self.visible_rows.first().copied();
            }
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let max_scroll = ((self.visible_rows.len() as f64) * self.row_height - self.viewport_height).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max_scroll);
    }

    pub fn visible_rows(&self) -> &[usize] {
        &self.visible_rows
    }

    pub fn active_row(&self) -> Option<usize> {
        self.active_row
    }

    pub fn sort_column(&self) -> Option<usize> {
        self.sort_column
    }

    pub fn sort_direction(&self) -> SortDirection {
        self.sort_direction
    }

    pub fn toggle_sort(&mut self, column: usize) -> Result<SortDirection, ModelError> {
        if column >= self.columns.len() {
            return Err(ModelError::UnknownItem(column));
        }
        if self.sort_column == Some(column) {
            self.sort_direction = match self.sort_direction {
                SortDirection::None => SortDirection::Ascending,
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::None,
            };
            if self.sort_direction == SortDirection::None {
                self.sort_column = None;
            }
        } else {
            self.sort_column = Some(column);
            self.sort_direction = SortDirection::Ascending;
        }
        self.recompute_view();
        Ok(self.sort_direction)
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.recompute_view();
    }

    pub fn resize_column(&mut self, column: usize, width: f64) -> Result<f64, ModelError> {
        if !width.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        let col = self
            .columns
            .get_mut(column)
            .ok_or(ModelError::UnknownItem(column))?;
        col.width = width.clamp(col.min_width, col.max_width);
        Ok(col.width)
    }

    pub fn column_width(&self, column: usize) -> Option<f64> {
        self.columns.get(column).map(|c| c.width)
    }

    pub fn select_row(&mut self, row_id: usize, additive: bool) -> Result<(), ModelError> {
        if row_id >= self.rows.len() {
            return Err(ModelError::UnknownItem(row_id));
        }
        if !additive {
            self.selected_rows.clear();
        }
        self.selected_rows.insert(row_id);
        self.active_row = Some(row_id);
        self.ensure_row_visible(row_id);
        Ok(())
    }

    pub fn selected_rows(&self) -> Vec<usize> {
        self.selected_rows.iter().copied().collect()
    }

    pub fn move_selection(&mut self, delta: isize) -> Option<usize> {
        if self.visible_rows.is_empty() {
            return None;
        }
        let pos = match self.active_row.and_then(|r| self.visible_rows.iter().position(|&v| v == r)) {
            Some(idx) => idx.saturating_add_signed(delta).min(self.visible_rows.len() - 1),
            None => {
                if delta >= 0 {
                    0
                } else {
                    self.visible_rows.len() - 1
                }
            }
        };
        let row_id = self.visible_rows[pos];
        self.selected_rows.clear();
        self.selected_rows.insert(row_id);
        self.active_row = Some(row_id);
        self.ensure_row_visible(row_id);
        Some(row_id)
    }

    pub fn ensure_row_visible(&mut self, row_id: usize) {
        if let Some(pos) = self.visible_rows.iter().position(|&r| r == row_id) {
            let top = (pos as f64) * self.row_height;
            let bottom = top + self.row_height;
            if top < self.scroll_y {
                self.scroll_y = top;
            } else if bottom > self.scroll_y + self.viewport_height {
                self.scroll_y = (bottom - self.viewport_height).max(0.0);
            }
            self.clamp_scroll();
        }
    }

    pub fn set_scroll_y(&mut self, scroll_y: f64) -> Result<f64, ModelError> {
        if !scroll_y.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        self.scroll_y = scroll_y;
        self.clamp_scroll();
        Ok(self.scroll_y)
    }

    pub fn scroll_y(&self) -> f64 {
        self.scroll_y
    }

    /// Returns the `[start_visible_index, end_visible_index)` slice of `visible_rows` that intersect the viewport.
    pub fn virtual_window(&self, overscan: usize) -> Range<usize> {
        if self.visible_rows.is_empty() || self.viewport_height <= 0.0 {
            return 0..0;
        }
        let first = (self.scroll_y / self.row_height).floor().max(0.0) as usize;
        let count = (self.viewport_height / self.row_height).ceil().max(1.0) as usize;
        let start = first.saturating_sub(overscan).min(self.visible_rows.len());
        let end = (first + count + overscan).min(self.visible_rows.len());
        start..end
    }
}

/// Interactive state model for `VirtualList`: virtualized viewport windowing with overscan,
/// scroll clamping, and keyboard item selection.
#[derive(Clone, Debug, PartialEq)]
pub struct VirtualListModel {
    item_count: usize,
    item_height: f64,
    viewport_height: f64,
    overscan: usize,
    scroll_y: f64,
    selected: Option<usize>,
}

impl VirtualListModel {
    pub fn new(
        item_count: usize,
        item_height: f64,
        viewport_height: f64,
        overscan: usize,
    ) -> Result<Self, ModelError> {
        if !item_height.is_finite() || item_height <= 0.0 || !viewport_height.is_finite() || viewport_height < 0.0 {
            return Err(ModelError::NonFiniteValue);
        }
        Ok(Self {
            item_count,
            item_height,
            viewport_height,
            overscan,
            scroll_y: 0.0,
            selected: None,
        })
    }

    pub fn total_height(&self) -> f64 {
        (self.item_count as f64) * self.item_height
    }

    pub fn scroll_y(&self) -> f64 {
        self.scroll_y
    }

    pub fn set_scroll_y(&mut self, scroll_y: f64) -> Result<f64, ModelError> {
        if !scroll_y.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        let max_scroll = (self.total_height() - self.viewport_height).max(0.0);
        self.scroll_y = scroll_y.clamp(0.0, max_scroll);
        Ok(self.scroll_y)
    }

    pub fn visible_range(&self) -> Range<usize> {
        if self.item_count == 0 || self.viewport_height <= 0.0 {
            return 0..0;
        }
        let first = (self.scroll_y / self.item_height).floor().max(0.0) as usize;
        let visible = (self.viewport_height / self.item_height).ceil().max(1.0) as usize;
        let start = first.saturating_sub(self.overscan).min(self.item_count);
        let end = (first + visible + self.overscan).min(self.item_count);
        start..end
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, index: usize) -> Result<usize, ModelError> {
        if index >= self.item_count {
            return Err(ModelError::UnknownItem(index));
        }
        self.selected = Some(index);
        self.ensure_visible(index);
        Ok(index)
    }

    pub fn move_by(&mut self, delta: isize) -> Option<usize> {
        if self.item_count == 0 {
            return None;
        }
        let next = match self.selected {
            Some(cur) => cur.saturating_add_signed(delta).min(self.item_count - 1),
            None => {
                if delta >= 0 {
                    0
                } else {
                    self.item_count - 1
                }
            }
        };
        let _ = self.select(next);
        self.selected
    }

    pub fn ensure_visible(&mut self, index: usize) {
        if index >= self.item_count {
            return;
        }
        let top = (index as f64) * self.item_height;
        let bottom = top + self.item_height;
        if top < self.scroll_y {
            let _ = self.set_scroll_y(top);
        } else if bottom > self.scroll_y + self.viewport_height {
            let _ = self.set_scroll_y((bottom - self.viewport_height).max(0.0));
        }
    }
}

/// Inline text style flags for [`RichTextModel`] and [`MarkdownModel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InlineStyle {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Code,
}

/// Styled span over a UTF-8 byte range in [`RichTextModel`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichSpan {
    pub range: Range<usize>,
    pub style: InlineStyle,
}

/// Interactive state model for `RichTextDocument` and `MarkdownDocument`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichTextModel {
    editor: TextEdit,
    spans: Vec<RichSpan>,
    heading_level: u8,
}

impl RichTextModel {
    pub fn new(initial: String) -> Result<Self, ModelError> {
        Ok(Self {
            editor: TextEdit::with_multiline(initial, true)?,
            spans: Vec::new(),
            heading_level: 0,
        })
    }

    pub fn from_markdown(md: &str) -> Result<Self, ModelError> {
        let trimmed = md.trim_start();
        let (heading_level, body) = if let Some(rest) = trimmed.strip_prefix("### ") {
            (3, rest)
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            (2, rest)
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            (1, rest)
        } else {
            (0, trimmed)
        };
        let mut plain = String::new();
        let mut spans = Vec::new();
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if body[i..].starts_with("**") {
                if let Some(end) = body[i + 2..].find("**") {
                    let start_out = plain.len();
                    plain.push_str(&body[i + 2..i + 2 + end]);
                    spans.push(RichSpan {
                        range: start_out..plain.len(),
                        style: InlineStyle::Bold,
                    });
                    i += 4 + end;
                    continue;
                }
            }
            if body[i..].starts_with('`') {
                if let Some(end) = body[i + 1..].find('`') {
                    let start_out = plain.len();
                    plain.push_str(&body[i + 1..i + 1 + end]);
                    spans.push(RichSpan {
                        range: start_out..plain.len(),
                        style: InlineStyle::Code,
                    });
                    i += 2 + end;
                    continue;
                }
            }
            if body[i..].starts_with('*') {
                if let Some(end) = body[i + 1..].find('*') {
                    let start_out = plain.len();
                    plain.push_str(&body[i + 1..i + 1 + end]);
                    spans.push(RichSpan {
                        range: start_out..plain.len(),
                        style: InlineStyle::Italic,
                    });
                    i += 2 + end;
                    continue;
                }
            }
            let ch = body[i..].chars().next().unwrap();
            plain.push(ch);
            i += ch.len_utf8();
        }
        Ok(Self {
            editor: TextEdit::with_multiline(plain, true)?,
            spans,
            heading_level,
        })
    }

    pub fn to_markdown(&self) -> String {
        let prefix = match self.heading_level {
            1 => "# ",
            2 => "## ",
            3 => "### ",
            _ => "",
        };
        format!("{prefix}{}", self.editor.value())
    }

    pub fn text(&self) -> &str {
        self.editor.value()
    }

    pub fn editor_mut(&mut self) -> &mut TextEdit {
        &mut self.editor
    }

    pub fn spans(&self) -> &[RichSpan] {
        &self.spans
    }

    pub fn heading_level(&self) -> u8 {
        self.heading_level
    }

    pub fn set_heading_level(&mut self, level: u8) {
        self.heading_level = level.min(3);
    }

    pub fn toggle_style(&mut self, style: InlineStyle) -> bool {
        let sel = self.editor.selection();
        if sel.is_empty() {
            return false;
        }
        if let Some(pos) = self
            .spans
            .iter()
            .position(|s| s.range == sel && s.style == style)
        {
            self.spans.remove(pos);
            false
        } else {
            self.spans.push(RichSpan { range: sel, style });
            true
        }
    }

    pub fn clear_formatting(&mut self) {
        self.spans.clear();
        self.heading_level = 0;
    }
}

/// Syntax token span emitted by [`CodeEditorModel`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeTokenSpan {
    pub kind: String,
    pub value: String,
    pub line: usize,
    pub column: usize,
}

/// Interactive state model for `CodeEditor`: multiline editing, line/column metrics,
/// indentation/dedentation, auto-closing brackets, and `.ui` syntax tokenization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeEditorModel {
    editor: TextEdit,
    tab_size: usize,
}

impl CodeEditorModel {
    pub fn new(initial: String, tab_size: usize) -> Result<Self, ModelError> {
        Ok(Self {
            editor: TextEdit::with_multiline(initial, true)?,
            tab_size: tab_size.clamp(1, 8),
        })
    }

    pub fn value(&self) -> &str {
        self.editor.value()
    }

    pub fn editor_mut(&mut self) -> &mut TextEdit {
        &mut self.editor
    }

    pub fn line_count(&self) -> usize {
        self.editor.value().split('\n').count().max(1)
    }

    /// Returns 1-based `(line, column)` of the current caret.
    pub fn cursor_line_column(&self) -> (usize, usize) {
        let prefix = &self.editor.value()[..self.editor.caret()];
        let mut line = 1usize;
        let mut last_nl = 0usize;
        for (i, ch) in prefix.char_indices() {
            if ch == '\n' {
                line += 1;
                last_nl = i + 1;
            }
        }
        let col = prefix[last_nl..].chars().count() + 1;
        (line, col)
    }

    pub fn indent(&mut self) -> bool {
        let spaces = " ".repeat(self.tab_size);
        self.editor.insert(&spaces).unwrap_or(false)
    }

    pub fn insert_pair(&mut self, open: char) -> Result<bool, ModelError> {
        let close = match open {
            '{' => Some('}'),
            '(' => Some(')'),
            '[' => Some(']'),
            '"' => Some('"'),
            '\'' => Some('\''),
            _ => None,
        };
        if let Some(closing) = close {
            let pair = format!("{open}{closing}");
            let changed = self.editor.insert(&pair)?;
            let _ = self.editor.left(false);
            Ok(changed)
        } else {
            self.editor.insert(&open.to_string())
        }
    }

    /// Tokenizes the current buffer using `themoretheless_tokenizer_forma::tokenize`.
    pub fn syntax_tokens(&self) -> Result<Vec<CodeTokenSpan>, String> {
        let src = self.editor.value();
        let tokens =
            themoretheless_tokenizer_forma::tokenize(src).map_err(|e| e.to_string())?;
        Ok(tokens
            .into_iter()
            .filter(|t| !t.text.is_empty())
            .map(|t| {
                let prefix = &src[..t.start.min(src.len())];
                let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
                let col_start = prefix.rfind('\n').map_or(0, |idx| idx + 1);
                let column = prefix[col_start..].chars().count() + 1;
                let kind = match t.text.as_str() {
                    "component" | "base" | "use" | "prop" | "state" | "event" | "enum"
                    | "struct" | "where" | "tokens" | "i18n" | "style" | "let" | "slot"
                    | "override" | "match" | "shortcut" | "true" | "false" => "keyword",
                    "{" | "}" | "(" | ")" | "[" | "]" | ":" | ";" | "," | "->" | "=>" => "punct",
                    s if s.starts_with('\'') || s.starts_with('"') => "string",
                    s if s.starts_with('#') => "color",
                    s if s.chars().next().is_some_and(|c| c.is_ascii_digit()) => "number",
                    _ => "ident",
                };
                CodeTokenSpan {
                    kind: kind.to_string(),
                    value: t.text,
                    line,
                    column,
                }
            })
            .collect())
    }
}

/// Interactive state model for `DateTimeInput`: leap-year aware calendar math,
/// 42-cell month grid generation, day/month/time stepping, and ISO formatting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateTimeModel {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    open: bool,
}

impl DateTimeModel {
    pub fn new(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> Result<Self, ModelError> {
        if !(1..=12).contains(&month)
            || day < 1
            || day > Self::days_in_month(year, month)
            || hour > 23
            || minute > 59
        {
            return Err(ModelError::InvalidRange);
        }
        Ok(Self {
            year,
            month,
            day,
            hour,
            minute,
            open: false,
        })
    }

    pub fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    pub fn days_in_month(year: i32, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if Self::is_leap_year(year) => 29,
            2 => 28,
            _ => 30,
        }
    }

    /// Day of week (`0 = Monday .. 6 = Sunday`) via Sakamoto's algorithm.
    pub fn weekday(year: i32, month: u8, day: u8) -> u8 {
        let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let y = if month < 3 { year - 1 } else { year };
        let sun0 = (y + y / 4 - y / 100 + y / 400 + t[(month - 1) as usize] + i32::from(day))
            .rem_euclid(7) as u8;
        (sun0 + 6) % 7
    }

    pub fn format_iso(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }

    pub fn format_date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn year(&self) -> i32 {
        self.year
    }
    pub fn month(&self) -> u8 {
        self.month
    }
    pub fn day(&self) -> u8 {
        self.day
    }
    pub fn hour(&self) -> u8 {
        self.hour
    }
    pub fn minute(&self) -> u8 {
        self.minute
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    pub fn step_months(&mut self, delta: i32) {
        let total = self.year * 12 + (i32::from(self.month) - 1) + delta;
        self.year = total.div_euclid(12);
        self.month = (total.rem_euclid(12) + 1) as u8;
        let max_day = Self::days_in_month(self.year, self.month);
        self.day = self.day.min(max_day);
    }

    pub fn step_days(&mut self, delta: i32) {
        let mut remaining = delta;
        while remaining > 0 {
            let max_d = Self::days_in_month(self.year, self.month);
            if self.day < max_d {
                self.day += 1;
            } else {
                self.day = 1;
                self.step_months(1);
            }
            remaining -= 1;
        }
        while remaining < 0 {
            if self.day > 1 {
                self.day -= 1;
            } else {
                self.step_months(-1);
                self.day = Self::days_in_month(self.year, self.month);
            }
            remaining += 1;
        }
    }

    /// Generates a 42-cell `(6 weeks × 7 days)` calendar grid for the current month.
    /// Cells outside the current month have `0`, cells in the current month have `1..=days_in_month`.
    pub fn calendar_grid(&self) -> [u8; 42] {
        let mut grid = [0u8; 42];
        let first_wd = Self::weekday(self.year, self.month, 1) as usize;
        let days = Self::days_in_month(self.year, self.month);
        for d in 1..=days {
            grid[first_wd + (d as usize - 1)] = d;
        }
        grid
    }
}

/// Command entry registered in [`CommandPaletteModel`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandItem {
    pub id: String,
    pub title: String,
    pub category: String,
    pub shortcut: String,
}

/// Interactive state model for `CommandPalette`: fuzzy/substring filtering,
/// ranked matching, keyboard navigation, and command execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPaletteModel {
    commands: Vec<CommandItem>,
    query: String,
    filtered: Vec<usize>,
    selected: usize,
    open: bool,
}

impl CommandPaletteModel {
    pub fn new(commands: Vec<CommandItem>) -> Self {
        let filtered = (0..commands.len()).collect();
        Self {
            commands,
            query: String::new(),
            filtered,
            selected: 0,
            open: false,
        }
    }

    fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
        if query.is_empty() {
            return Some(0);
        }
        let q = query.to_lowercase();
        let c = candidate.to_lowercase();
        if let Some(pos) = c.find(&q) {
            return Some(1000 - (pos as i32) * 5 - (c.len() as i32));
        }
        let mut score = 0i32;
        let mut c_chars = c.chars();
        for qc in q.chars() {
            loop {
                match c_chars.next() {
                    Some(cc) if cc == qc => {
                        score += 10;
                        break;
                    }
                    Some(_) => score -= 1,
                    None => return None,
                }
            }
        }
        Some(score)
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        let mut scored: Vec<(usize, i32)> = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(idx, cmd)| {
                let hay = format!("{} {} {}", cmd.category, cmd.title, cmd.id);
                Self::fuzzy_score(&self.query, &hay).map(|s| (idx, s))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        self.selected = 0;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered
    }

    pub fn move_selection(&mut self, delta: isize) -> Option<&CommandItem> {
        if self.filtered.is_empty() {
            return None;
        }
        let len = self.filtered.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        self.active_command()
    }

    pub fn active_command(&self) -> Option<&CommandItem> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.commands.get(idx))
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

    #[test]
    fn data_table_and_virtual_list_models_sort_filter_resize_and_virtualize_rows() {
        let cols = vec![
            TableColumn {
                id: "name".into(),
                width: 120.,
                min_width: 60.,
                max_width: 240.,
            },
            TableColumn {
                id: "score".into(),
                width: 80.,
                min_width: 40.,
                max_width: 160.,
            },
        ];
        let rows = vec![
            vec!["Beta".into(), "20".into()],
            vec!["Alpha".into(), "100".into()],
            vec!["Gamma".into(), "5".into()],
            vec!["Delta".into(), "40".into()],
        ];
        let mut table = DataTableModel::new(cols, rows, 24., 48.).unwrap();
        assert_eq!(table.visible_rows(), &[0, 1, 2, 3]);
        assert_eq!(table.virtual_window(0), 0..2);
        assert_eq!(table.toggle_sort(1).unwrap(), SortDirection::Ascending);
        assert_eq!(table.visible_rows(), &[2, 0, 3, 1]);
        assert_eq!(table.toggle_sort(1).unwrap(), SortDirection::Descending);
        assert_eq!(table.visible_rows(), &[1, 3, 0, 2]);
        assert_eq!(table.resize_column(0, 500.).unwrap(), 240.);
        assert_eq!(table.resize_column(0, 20.).unwrap(), 60.);
        table.set_filter("a".into());
        assert_eq!(table.visible_rows(), &[1, 3, 0, 2]);
        table.set_filter("gam".into());
        assert_eq!(table.visible_rows(), &[2]);

        let mut vlist = VirtualListModel::new(100, 20., 60., 1).unwrap();
        assert_eq!(vlist.visible_range(), 0..4);
        assert_eq!(vlist.select(10).unwrap(), 10);
        assert!(vlist.scroll_y() >= 160.);
        assert!(vlist.visible_range().contains(&10));
    }

    #[test]
    fn rich_text_code_editor_datetime_and_command_palette_models_preserve_invariants() {
        let rich = RichTextModel::from_markdown("## **Forma** *UI* `code`").unwrap();
        assert_eq!(rich.heading_level(), 2);
        assert_eq!(rich.text(), "Forma UI code");
        assert_eq!(rich.spans().len(), 3);
        assert_eq!(rich.to_markdown(), "## Forma UI code");

        let mut code = CodeEditorModel::new("component Demo {\n}".into(), 2).unwrap();
        assert_eq!(code.line_count(), 2);
        assert_eq!(code.cursor_line_column(), (2, 2));
        let tokens = code.syntax_tokens().unwrap();
        assert!(tokens.iter().any(|t| t.value == "component"));
        assert!(code.indent());

        let mut dt = DateTimeModel::new(2024, 2, 28, 14, 30).unwrap();
        assert_eq!(dt.format_iso(), "2024-02-28 14:30");
        dt.step_days(1);
        assert_eq!(dt.format_date(), "2024-02-29");
        dt.step_days(1);
        assert_eq!(dt.format_date(), "2024-03-01");
        assert!(dt.calendar_grid().iter().any(|&d| d == 31));

        let mut palette = CommandPaletteModel::new(vec![
            CommandItem {
                id: "file.save".into(),
                title: "Save Document".into(),
                category: "File".into(),
                shortcut: "Cmd+S".into(),
            },
            CommandItem {
                id: "view.zoom".into(),
                title: "Zoom In".into(),
                category: "View".into(),
                shortcut: "Cmd++".into(),
            },
        ]);
        palette.set_query("zoom".into());
        assert_eq!(palette.active_command().unwrap().id, "view.zoom");
    }
}
