//! What assistive technology reads and requests.

use super::*;

#[wasm_bindgen]
impl Runtime {
    /// Exports the AccessKit-compatible semantic accessibility tree as a compact JSON snapshot.
    pub fn accessibility_tree_json(&self) -> String {
        let tree = self.accessibility_tree();
        serde_json::to_string(&tree).expect("semantic tree serializes to JSON")
    }

    /// Replace the entire text value as one undoable semantic action. Invalid,
    /// readonly, disabled, modal-blocked and oversized requests leave focus and
    /// selection unchanged. The host synchronizes bindings after dispatch.
    pub fn accessibility_set_text_value(&mut self, node_id: usize, value: &str) -> bool {
        let Some(index) = self
            .nodes
            .get(node_id)
            .filter(|node| node.id == node_id)
            .and_then(|node| node.control)
        else {
            return false;
        };
        if !self.control_editable(index) || !self.enabled(index) || self.control_readonly(index) {
            return false;
        }
        // CRLF normalization can at most halve the byte length.
        if value.len() > crate::text_input::MAX_VALUE_BYTES * 2 {
            return false;
        }
        let clean = value.replace("\r\n", "\n").replace('\r', "\n");
        let clean = if self.control_multiline(index) {
            clean
        } else {
            clean.replace('\n', " ")
        };
        if clean.len() > crate::text_input::MAX_VALUE_BYTES {
            return false;
        }
        if !self.focus_control(index) {
            return false;
        }
        let editor = self.editors[index].as_mut().unwrap();
        editor.set_preedit("");
        if editor.model.value() != clean {
            editor.select_original_utf16(0, u32::MAX);
            editor.insert(&clean);
        }
        self.refresh_editors();
        true
    }

    /// Set a normalized `[0.0, 1.0]` slider range value via semantic accessibility action.
    /// Accepts values in `[0.0, 1.0]` or percentage `[0.0, 100.0]` (normalized when `> 1.0 && <= 100.0`).
    ///
    /// A slider with a scale of its own takes the value on that scale instead.
    pub fn accessibility_set_range_value(&mut self, node_id: usize, value: f64) -> bool {
        if !value.is_finite() {
            return false;
        }
        let Some(index) = self
            .nodes
            .get(node_id)
            .filter(|node| node.id == node_id)
            .and_then(|node| node.control)
        else {
            return false;
        };
        if !self.enabled(index) {
            return false;
        }
        let Some(range) = self
            .controls
            .get(index)
            .and_then(|c| c.template.range.as_ref())
        else {
            return false;
        };
        let normalized = if range.has_scale() {
            range.fraction_of(value as f32)
        } else if !(0. ..=100.).contains(&value) {
            return false;
        } else if value > 1. {
            (value / 100.).clamp(0., 1.) as f32
        } else {
            value.clamp(0., 1.) as f32
        };
        if !self.focus_control(index) {
            return false;
        }
        if let Some(range) = Arc::make_mut(&mut self.controls[index].template)
            .range
            .as_mut()
        {
            range.value = range.snapped(normalized);
        }
        self.refresh_editors();
        true
    }

    /// Step a slider range control via semantic accessibility increment/decrement action.
    pub fn accessibility_step_range(&mut self, node_id: usize, delta: f32) -> bool {
        if !delta.is_finite() {
            return false;
        }
        let Some(index) = self
            .nodes
            .get(node_id)
            .filter(|node| node.id == node_id)
            .and_then(|node| node.control)
        else {
            return false;
        };
        if !self.enabled(index) {
            return false;
        }
        if self
            .controls
            .get(index)
            .and_then(|c| c.template.range.as_ref())
            .is_none()
        {
            return false;
        }
        if !self.focus_control(index) {
            return false;
        }
        if let Some(range) = Arc::make_mut(&mut self.controls[index].template)
            .range
            .as_mut()
        {
            // On a step grid one increment is one step, whatever distance the caller asked for.
            let distance = if range.step > 0. {
                delta.signum() * range.key_steps().0
            } else {
                delta
            };
            range.value = range.snapped(range.value + distance);
        }
        self.refresh_editors();
        true
    }

    /// Dismiss an open modal dialog node via semantic accessibility action.
    pub fn accessibility_dismiss_modal(&mut self, node_id: usize) -> bool {
        let Some(node) = self
            .nodes
            .get(node_id)
            .filter(|node| node.id == node_id && node.kind == NodeKind::Modal)
        else {
            return false;
        };
        let modal_idx = self.modal_index_for_node(node);
        if self.active_modal_index() != Some(modal_idx) {
            return false;
        }
        self.dismiss_modal(modal_idx)
    }

    /// Apply a semantic scroll action on the scene's ScrollView node.
    pub fn accessibility_scroll(&mut self, node_id: usize, dx: f32, dy: f32) -> bool {
        let Some(node) = self
            .nodes
            .get(node_id)
            .filter(|node| node.id == node_id && node.kind == NodeKind::Scroll)
        else {
            return false;
        };
        let _ = node;
        if !self.scene.scroll
            || self.active_modal_index().is_some()
            || !dx.is_finite()
            || !dy.is_finite()
        {
            return false;
        }
        self.scroll(dx, dy);
        true
    }

    /// Accept a semantic text-selection action using snapshot-local node ID and
    /// UTF-16 offsets in the original value (CRLF retained). Readonly permits
    /// selection; disabled, modal-blocked, noneditor and unknown nodes refuse it.
    /// Returns whether accepted, including an already matching selection.
    pub fn accessibility_set_text_selection(
        &mut self,
        node_id: usize,
        anchor: u32,
        caret: u32,
    ) -> bool {
        let Some(index) = self
            .nodes
            .get(node_id)
            .filter(|node| node.id == node_id)
            .and_then(|node| node.control)
        else {
            return false;
        };
        if !self.control_editable(index) || !self.enabled(index) {
            return false;
        }
        if !self.focus_control(index) {
            return false;
        }
        let editor = self.editors[index].as_mut().unwrap();
        editor.set_preedit("");
        editor.select_original_utf16(anchor, caret);
        self.refresh_editors();
        true
    }
}
