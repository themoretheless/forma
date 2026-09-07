//! Thin JS access to the same interaction models available to native hosts.
//! Caret and anchor getters/setters use UTF-8 byte offsets, not JS UTF-16 indices.

use crate::control_state::{
    CheckMode, CheckModel, CheckState, ModelError, RangeModel, SingleSelection, TextEdit,
    ToggleModel, TreeItem, TreeModel,
};
use wasm_bindgen::prelude::*;

fn js_error(error: ModelError) -> JsValue {
    JsValue::from_str(&error.to_string())
}

/// Exclusive selection for indexed radio, tab, and navigation items.
#[wasm_bindgen]
pub struct SelectionValue {
    model: SingleSelection,
}
#[wasm_bindgen]
impl SelectionValue {
    #[wasm_bindgen(constructor)]
    pub fn new(length: u32, selected: i32) -> Result<SelectionValue, JsValue> {
        if length > i32::MAX as u32 || selected < -1 {
            return Err(JsValue::from_str(
                "selection needs length <= i32::MAX and selected >= -1",
            ));
        }
        let selected = if selected == -1 {
            None
        } else {
            Some(selected as usize)
        };
        let model =
            SingleSelection::new((0..length as usize).collect(), selected).map_err(js_error)?;
        Ok(Self { model })
    }
    pub fn selected(&self) -> i32 {
        self.model.selected().map_or(-1, |index| index as i32)
    }
    /// -1 clears the selection. Other indices must belong to the group.
    pub fn select(&mut self, index: i32) -> Result<i32, JsValue> {
        if index == -1 {
            self.model.clear();
        } else if index < -1 {
            return Err(JsValue::from_str("selection index must be >= -1"));
        } else {
            self.model.select(index as usize).map_err(js_error)?;
        }
        Ok(self.selected())
    }
    pub fn next(&mut self, delta: i32, wrap: bool) -> i32 {
        self.model.move_by(delta as isize, wrap);
        self.selected()
    }
}

fn tree_from_rows(parents: Vec<i32>, branches: Vec<u32>) -> Result<TreeModel, String> {
    if parents.len() != branches.len() || parents.len() > i32::MAX as usize {
        return Err("tree parents and branches need equal lengths <= i32::MAX".into());
    }
    let mut items = Vec::with_capacity(parents.len());
    for (id, parent) in parents.into_iter().enumerate() {
        if parent < -1 {
            return Err(format!(
                "tree parent for row {id} must be -1 or an earlier row index"
            ));
        }
        items.push(TreeItem {
            id,
            parent: if parent == -1 {
                None
            } else {
                Some(parent as usize)
            },
        });
    }
    let model = TreeModel::new(items).map_err(|error| error.to_string())?;
    for (id, flag) in branches.into_iter().enumerate() {
        if flag > 1 || (flag == 1) != model.is_branch(id) {
            return Err(format!(
                "tree branch flag for row {id} must be 0/1 and match its children"
            ));
        }
    }
    Ok(model)
}

/// Preorder tree whose stable IDs are row indices. Branch flags describe actual
/// children; empty folders are not expandable in the current pure tree model.
#[wasm_bindgen]
pub struct TreeValue {
    model: TreeModel,
}
#[wasm_bindgen]
impl TreeValue {
    #[wasm_bindgen(constructor)]
    pub fn new(parents: Vec<i32>, branches: Vec<u32>) -> Result<TreeValue, JsValue> {
        let model = tree_from_rows(parents, branches).map_err(|error| JsValue::from_str(&error))?;
        Ok(Self { model })
    }
    pub fn visible(&self) -> Vec<u32> {
        self.model
            .visible_ids()
            .into_iter()
            .map(|index| index as u32)
            .collect()
    }
    pub fn selected(&self) -> i32 {
        self.model.selected().map_or(-1, |index| index as i32)
    }
    pub fn select(&mut self, index: u32) -> Result<i32, JsValue> {
        self.model.select(index as usize).map_err(js_error)?;
        Ok(self.selected())
    }
    pub fn toggle(&mut self, index: u32) -> Result<bool, JsValue> {
        self.model.toggle_expanded(index as usize).map_err(js_error)
    }
    pub fn expanded(&self, index: u32) -> bool {
        self.model.is_expanded(index as usize)
    }
    pub fn left(&mut self) -> bool {
        self.model.left()
    }
    pub fn right(&mut self) -> bool {
        self.model.right()
    }
    pub fn up(&mut self) -> bool {
        self.model.move_by(-1)
    }
    pub fn down(&mut self) -> bool {
        self.model.move_by(1)
    }
}

#[wasm_bindgen]
pub struct CheckValue {
    model: CheckModel,
}
#[wasm_bindgen]
impl CheckValue {
    /// State: 0 = unchecked, 1 = checked, 2 = mixed.
    #[wasm_bindgen(constructor)]
    pub fn new(initial: u8, tristate: bool) -> Result<CheckValue, JsValue> {
        let state = match initial {
            0 => CheckState::Unchecked,
            1 => CheckState::Checked,
            2 => CheckState::Mixed,
            _ => return Err(JsValue::from_str("check state must be 0, 1 or 2")),
        };
        Ok(Self {
            model: CheckModel::new(
                state,
                if tristate {
                    CheckMode::TriState
                } else {
                    CheckMode::Binary
                },
            ),
        })
    }
    pub fn state(&self) -> u8 {
        match self.model.state() {
            CheckState::Unchecked => 0,
            CheckState::Checked => 1,
            CheckState::Mixed => 2,
        }
    }
    pub fn activate(&mut self) -> u8 {
        self.model.activate();
        self.state()
    }
}

#[wasm_bindgen]
pub struct ToggleValue {
    model: ToggleModel,
}
#[wasm_bindgen]
impl ToggleValue {
    #[wasm_bindgen(constructor)]
    pub fn new(initial: bool) -> Self {
        Self {
            model: ToggleModel::new(initial),
        }
    }
    pub fn value(&self) -> bool {
        self.model.value()
    }
    pub fn toggle(&mut self) -> bool {
        self.model.activate();
        self.value()
    }
}

#[wasm_bindgen]
pub struct RangeValue {
    model: RangeModel,
}
#[wasm_bindgen]
impl RangeValue {
    #[wasm_bindgen(constructor)]
    pub fn new(min: f64, max: f64, step: f64, value: f64) -> Result<RangeValue, JsValue> {
        Ok(Self {
            model: RangeModel::new(min, max, step, value).map_err(js_error)?,
        })
    }
    pub fn value(&self) -> f64 {
        self.model.value()
    }
    pub fn fraction(&self) -> f64 {
        self.model.fraction()
    }
    pub fn set(&mut self, value: f64) -> Result<f64, JsValue> {
        self.model.set_value(value).map_err(js_error)?;
        Ok(self.value())
    }
    pub fn set_fraction(&mut self, fraction: f64) -> Result<f64, JsValue> {
        self.model.set_fraction(fraction).map_err(js_error)?;
        Ok(self.value())
    }
    pub fn step_by(&mut self, steps: i32) -> f64 {
        self.model.step_by(i64::from(steps));
        self.value()
    }
}

#[wasm_bindgen]
pub struct TextValue {
    model: TextEdit,
}
#[wasm_bindgen]
impl TextValue {
    #[wasm_bindgen(constructor)]
    pub fn new(initial: String) -> Result<TextValue, JsValue> {
        Ok(Self {
            model: TextEdit::new(initial).map_err(js_error)?,
        })
    }
    pub fn value(&self) -> String {
        self.model.value().to_owned()
    }
    pub fn caret(&self) -> usize {
        self.model.caret()
    }
    pub fn anchor(&self) -> usize {
        self.model.anchor()
    }
    pub fn selected_text(&self) -> String {
        self.model.selected_text().to_owned()
    }
    pub fn can_undo(&self) -> bool {
        self.model.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.model.can_redo()
    }
    pub fn insert(&mut self, text: &str) -> Result<bool, JsValue> {
        self.model.insert(text).map_err(js_error)
    }
    pub fn backspace(&mut self) -> bool {
        self.model.backspace()
    }
    pub fn delete(&mut self) -> bool {
        self.model.delete()
    }
    #[wasm_bindgen(js_name = "move")]
    pub fn move_caret(&mut self, delta: i32, extend: bool) -> bool {
        self.model.move_chars(delta as isize, extend)
    }
    pub fn home(&mut self, extend: bool) -> bool {
        self.model.home(extend)
    }
    pub fn end(&mut self, extend: bool) -> bool {
        self.model.end(extend)
    }
    pub fn select_all(&mut self) -> bool {
        self.model.select_all()
    }
    pub fn undo(&mut self) -> bool {
        self.model.undo()
    }
    pub fn redo(&mut self) -> bool {
        self.model.redo()
    }
    pub fn set_selection(&mut self, anchor: usize, caret: usize) -> Result<bool, JsValue> {
        self.model.set_selection(anchor, caret).map_err(js_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_selection_wrapper_preserves_exclusive_and_empty_selection() {
        let mut selection = SelectionValue::new(3, 1).unwrap();
        assert_eq!(selection.selected(), 1);
        assert_eq!(selection.select(2).unwrap(), 2);
        assert_eq!(selection.next(1, true), 0);
        assert_eq!(selection.next(-1, false), 0);
        assert_eq!(selection.select(-1).unwrap(), -1);
        assert_eq!(selection.next(-1, false), 2);
        let mut empty = SelectionValue::new(0, -1).unwrap();
        assert_eq!(empty.next(1, true), -1);
    }

    #[test]
    fn tree_wrapper_navigation_and_collapse_use_shared_model_state() {
        let mut tree = TreeValue::new(vec![-1, 0, 1, 0, -1], vec![1, 1, 0, 0, 0]).unwrap();
        assert_eq!(tree.visible(), vec![0, 4]);
        assert_eq!(tree.selected(), -1);
        tree.right();
        assert_eq!(tree.selected(), 0);
        tree.right();
        assert!(tree.expanded(0));
        assert_eq!(tree.visible(), vec![0, 1, 3, 4]);
        tree.right();
        assert_eq!(tree.selected(), 1);
        tree.right();
        assert!(tree.expanded(1));
        tree.down();
        assert_eq!(tree.selected(), 2);
        tree.left();
        assert_eq!(tree.selected(), 1);
        tree.left();
        assert!(!tree.expanded(1));
        tree.up();
        assert_eq!(tree.selected(), 0);
        tree.toggle(0).unwrap();
        assert_eq!(tree.visible(), vec![0, 4]);
        tree.down();
        assert_eq!(tree.selected(), 4);
        assert!(!tree.down());
        tree.up();
        assert_eq!(tree.selected(), 0);
    }

    #[test]
    fn tree_adapter_rejects_invalid_topology_or_branch_flags() {
        assert!(tree_from_rows(vec![-1], vec![]).is_err());
        assert!(tree_from_rows(vec![-2], vec![0]).is_err());
        assert!(tree_from_rows(vec![0], vec![1]).is_err());
        assert!(tree_from_rows(vec![-1, 0], vec![0, 0]).is_err());
        assert!(tree_from_rows(vec![-1], vec![1]).is_err());
        assert!(tree_from_rows(vec![-1], vec![2]).is_err());
        assert!(tree_from_rows(vec![-1, -1, 0], vec![1, 0, 0]).is_err());
        assert!(tree_from_rows(vec![], vec![]).is_ok());
    }
}
