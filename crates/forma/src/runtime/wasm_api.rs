//! The JavaScript-facing API of [`Runtime`]: every method exported through wasm-bindgen.
//!
//! Rust callers use the same methods; the inherent Rust-only API lives in `runtime.rs`.

use super::*;

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Runtime {
    /// Exports the AccessKit-compatible semantic accessibility tree as a compact JSON snapshot.
    pub fn accessibility_tree_json(&self) -> String {
        let tree = self.accessibility_tree();
        serde_json::to_string(&tree).expect("semantic tree serializes to JSON")
    }

    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::from_source(crate::EXAMPLE).expect("valid built-in scene")
    }
    pub fn load_component(&mut self, source: &str, component: &str) -> Result<(), JsValue> {
        self.load_component_native(source, component)
            .map_err(|e| JsValue::from_str(&e))
    }
    /// Load a complete artifact atomically through the Rust parser/runtime.
    pub fn load_scene_artifact(&mut self, json: &str) -> Result<(), JsValue> {
        self.load_scene_artifact_native(json)
            .map_err(|e| JsValue::from_str(&e))
    }
    /// Compile a UI project to a versioned transport in Rust for a later runtime load.
    pub fn compile_ui_scene_artifact(
        files_json: &str,
        entry_file: &str,
        state_json: &str,
    ) -> Result<String, JsValue> {
        Self::compile_ui_scene_artifact_native(files_json, entry_file, state_json)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn load(&mut self, source: &str) -> Result<(), JsValue> {
        self.load_component(source, crate::BUTTON_COMPONENT)
    }
    /// Compiles a JSON map of `.ui`/`.svg` project files directly in Rust and loads the rendered scene.
    pub fn load_ui_files_json(
        &mut self,
        files_json: &str,
        entry_file: &str,
        state_json: &str,
    ) -> Result<(), JsValue> {
        self.load_ui_files_json_native(files_json, entry_file, state_json)
            .map_err(|e| JsValue::from_str(&e))
    }
    /// Synchronizes two-way `<->` bindings (`value`, `checked`, `selected`) from the live runtime
    /// back into the retained Rust `.ui` `Document`, re-rendering the scene while preserving focus
    /// and active text/slider interaction. Returns `true` after a successful scene rebuild.
    pub fn sync_ui_bindings(&mut self) -> Result<bool, JsValue> {
        self.sync_ui_bindings_native()
            .map_err(|e| JsValue::from_str(&e))
    }
    /// Replaces the named root state fields of the retained `.ui` document from a JSON object and
    /// synchronizes once, keeping focus, text and scroll. Returns `true` after a scene rebuild.
    pub fn set_ui_state_json(&mut self, state_json: &str) -> Result<bool, JsValue> {
        self.set_ui_state_json_native(state_json)
            .map_err(|e| JsValue::from_str(&e))
    }
    /// Sets individual state paths (`{"user.name":"Ada","items.2.done":true}`) of the retained
    /// `.ui` document and synchronizes once. Returns `true` after a scene rebuild.
    pub fn patch_ui_state_json(&mut self, patch_json: &str) -> Result<bool, JsValue> {
        self.patch_ui_state_json_native(patch_json)
            .map_err(|e| JsValue::from_str(&e))
    }
    /// Number of accepted activations waiting for a successful binding commit.
    pub fn pending_ui_binding_activations(&self) -> usize {
        self.ui_activations.events.len()
    }
    /// Explicitly discard buffered activations, clearing an overflow refusal.
    /// Widget values remain available for the next binding synchronization.
    pub fn cancel_ui_binding_activations(&mut self) -> usize {
        let count = self.ui_activations.events.len();
        self.ui_activations.clear();
        self.last_event = None;
        count
    }
    /// Exports the current state of the retained Rust `.ui` `Document` as a JSON string.
    pub fn ui_state_json(&self) -> String {
        self.ui_document
            .as_ref()
            .and_then(|doc| doc.state_json().ok())
            .unwrap_or_else(|| "{}".to_string())
    }
    pub fn geometry_revision(&self) -> u32 {
        self.geometry_revision
    }
    /// Control bounds change on scroll or source replacement. Paint and
    /// text-content changes do not invalidate these bounds.
    pub fn layout_revision(&self) -> u32 {
        self.layout_revision
    }
    pub fn range_value(&self, i: usize) -> f32 {
        self.controls
            .get(i)
            .and_then(|c| c.template.range.as_ref())
            .map_or(f32::NAN, |r| r.value)
    }
    pub fn range_key(&mut self, key: &str) -> bool {
        let Some(i) = self.focused else { return false };
        if self.controls[i].disabled() {
            return false;
        }
        let Some(range) = Arc::make_mut(&mut self.controls[i].template).range.as_mut() else {
            return false;
        };
        range.value = match key {
            "Home" => 0.,
            "End" => 1.,
            "ArrowLeft" | "ArrowDown" => (range.value - 0.01).max(0.),
            "ArrowRight" | "ArrowUp" => (range.value + 0.01).min(1.),
            "PageDown" => (range.value - 0.1).max(0.),
            "PageUp" => (range.value + 0.1).min(1.),
            _ => return false,
        };
        self.refresh_editors();
        true
    }
    pub fn text_editing(&self) -> bool {
        self.focused
            .is_some_and(|i| self.editors[i].is_some() && self.enabled(i))
    }
    /// Whether this control accepts multiline editing; false for noneditors.
    pub fn control_readonly(&self, i: usize) -> bool {
        self.editors
            .get(i)
            .and_then(Option::as_ref)
            .is_some_and(|editor| editor.spec.read_only)
    }
    pub fn control_multiline(&self, i: usize) -> bool {
        self.editors
            .get(i)
            .and_then(Option::as_ref)
            .is_some_and(|editor| editor.spec.multiline)
    }
    pub fn control_editable(&self, i: usize) -> bool {
        self.editors.get(i).is_some_and(Option::is_some)
    }
    /// Semantic editor epoch; visual caret animation does not change it.
    /// Zero means unavailable/exhausted: consumers must read the model without caching.
    pub fn text_model_revision(&self) -> u32 {
        self.focused
            .and_then(|i| self.editors[i].as_ref())
            .map_or(0, |editor| editor.semantic_revision)
    }
    pub fn text_value(&self, i: usize) -> String {
        self.editors
            .get(i)
            .and_then(Option::as_ref)
            .map_or_else(String::new, |e| e.model.value().into())
    }
    /// Focused anchor/caret in textarea UTF-16 units, with CRLF normalized to LF.
    /// The model keeps its original bytes; empty without a focused editor.
    pub fn text_selection_utf16(&self) -> Vec<u32> {
        self.focused
            .and_then(|i| self.editors[i].as_ref())
            .map_or_else(Vec::new, |editor| {
                [editor.model.anchor(), editor.model.caret()]
                    .into_iter()
                    .map(|at| {
                        let prefix = &editor.model.value()[..at];
                        (prefix.encode_utf16().count()
                            - prefix
                                .as_bytes()
                                .windows(2)
                                .filter(|pair| *pair == b"\r\n")
                                .count()) as u32
                    })
                    .collect()
            })
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
    pub fn accessibility_set_range_value(&mut self, node_id: usize, value: f64) -> bool {
        if !value.is_finite() || !(0. ..=100.).contains(&value) {
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
        let normalized = if value > 1. {
            (value / 100.).clamp(0., 1.) as f32
        } else {
            value.clamp(0., 1.) as f32
        };
        if let Some(range) = Arc::make_mut(&mut self.controls[index].template)
            .range
            .as_mut()
        {
            range.value = normalized;
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
            range.value = (range.value + delta).clamp(0., 1.);
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
    /// Apply normalized textarea UTF-16 selection, preserving grapheme boundaries.
    pub fn text_set_selection_utf16(&mut self, anchor: u32, caret: u32) -> bool {
        if !self.text_editing() {
            return false;
        }
        let changed = self.editors[self.focused.unwrap()]
            .as_mut()
            .unwrap()
            .select_utf16(anchor, caret);
        self.refresh_editors();
        changed
    }
    pub fn text_selected(&self) -> String {
        self.focused
            .and_then(|i| self.editors[i].as_ref())
            .map_or_else(String::new, |e| e.model.selected_text().into())
    }
    pub fn text_insert(&mut self, text: &str) -> bool {
        if !self.text_editing() || self.control_readonly(self.focused.unwrap()) {
            return false;
        }
        self.editors[self.focused.unwrap()]
            .as_mut()
            .unwrap()
            .insert(text);
        self.refresh_editors();
        true
    }
    pub fn text_preedit(&mut self, text: &str) {
        if self.text_editing() {
            self.editors[self.focused.unwrap()]
                .as_mut()
                .unwrap()
                .set_preedit(text);
            self.refresh_editors();
        }
    }
    /// Host-normalized word modifier (Option on macOS, Ctrl on Windows/Linux).
    pub fn text_word_key(&mut self, key: &str, shift: bool) -> bool {
        if !self.text_editing() {
            return false;
        }
        let handled = self.editors[self.focused.unwrap()]
            .as_mut()
            .unwrap()
            .word_key(key, shift);
        self.refresh_editors();
        handled
    }
    pub fn text_key(&mut self, key: &str, shift: bool, command: bool) -> bool {
        if !self.text_editing() {
            return false;
        }
        let index = self.focused.unwrap();
        let multiline = self.control_multiline(index);
        let handled = self.editors[index]
            .as_mut()
            .unwrap()
            .key(key, shift, command);
        self.refresh_editors();
        if handled && key == "Enter" && !multiline {
            self.submissions[index] = self.submissions[index].saturating_add(1);
            self.last_submitted = Some(index);
        }
        handled
    }
    pub fn text_caret_bounds(&self) -> Vec<f32> {
        let Some(i) = self.focused else { return vec![] };
        let Some(e) = &self.editors[i] else {
            return vec![];
        };
        let mut rect = e.caret_rect();
        let b = self.controls[i].bounds_rect();
        rect[0] += b[0];
        rect[1] += b[1];
        rect.to_vec()
    }
    /// Number of visual lines produced by soft-wrapping the control's text editor.
    pub fn text_soft_line_count(&self, i: usize) -> usize {
        self.editors
            .get(i)
            .and_then(Option::as_ref)
            .map_or(0, |e| e.soft_wrapped_lines().len())
    }
    /// Hit-test a point inside the focused editor using visual soft-wrapping.
    pub fn text_hit_soft_wrapped(&mut self, x: f32, y: f32, extend: bool) -> bool {
        let Some(i) = self.focused else {
            return false;
        };
        if !self.text_editing() {
            return false;
        }
        let b = self.controls[i].bounds_rect();
        self.editors[i]
            .as_mut()
            .unwrap()
            .hit_soft_wrapped(x - b[0], y - b[1], extend);
        self.refresh_editors();
        true
    }
    pub fn control_count(&self) -> usize {
        self.controls.len()
    }
    pub fn control_key(&self, i: usize) -> String {
        self.controls.get(i).map(Button::key).unwrap_or_default()
    }
    /// Stable editor name, separate from current value (label, placeholder, key).
    pub fn control_input_label(&self, i: usize) -> String {
        self.editors
            .get(i)
            .and_then(Option::as_ref)
            .map_or_else(String::new, |editor| {
                if !editor.spec.label.is_empty() {
                    editor.spec.label.clone()
                } else if !editor.spec.placeholder.is_empty() {
                    editor.spec.placeholder.clone()
                } else {
                    self.control_key(i)
                }
            })
    }
    pub fn control_label(&self, i: usize) -> String {
        self.controls.get(i).map(Button::label).unwrap_or_default()
    }
    pub fn control_action(&self, i: usize) -> String {
        self.controls.get(i).map(Button::action).unwrap_or_default()
    }
    pub fn control_disabled(&self, i: usize) -> bool {
        self.controls.get(i).is_none_or(Button::disabled)
    }
    pub fn control_interactive(&self, i: usize) -> bool {
        self.controls.get(i).is_some_and(|c| c.template.clickable)
    }
    pub fn control_clicks(&self, i: usize) -> u32 {
        self.controls.get(i).map_or(0, Button::clicks)
    }
    pub fn control_submissions(&self, i: usize) -> u32 {
        self.submissions.get(i).copied().unwrap_or(0)
    }
    pub fn submissions(&self) -> u32 {
        self.submissions
            .iter()
            .copied()
            .fold(0, u32::saturating_add)
    }
    pub fn submitted_index(&self) -> i32 {
        self.last_submitted.map_or(-1, |i| i as i32)
    }
    pub fn control_bounds(&self, i: usize) -> Vec<f32> {
        self.controls.get(i).map(Button::bounds).unwrap_or_default()
    }
    pub fn control_hovered(&self, i: usize) -> bool {
        self.controls.get(i).is_some_and(|b| b.hover)
    }
    pub fn control_focused(&self, i: usize) -> bool {
        self.focused == Some(i)
    }
    /// Layer index of a control, -1 for the base scene.
    pub fn control_modal(&self, i: usize) -> i32 {
        self.modal_of
            .get(i)
            .copied()
            .flatten()
            .map_or(-1, |m| m as i32)
    }
    /// Hidden by a closed layer or covered by another layer's backdrop.
    pub fn control_blocked(&self, i: usize) -> bool {
        i >= self.controls.len() || !self.visible(i) || !self.reachable(i)
    }
    pub fn modal_key(&self, m: usize) -> String {
        self.scene
            .modals
            .get(m)
            .map(|s| s.key.clone())
            .unwrap_or_default()
    }
    pub fn modal_is_open(&self, m: usize) -> bool {
        self.modal_open.get(m).copied().unwrap_or(false)
    }
    pub fn modal_dismissible(&self, m: usize) -> bool {
        self.scene.modals.get(m).is_some_and(|s| s.dismiss)
    }
    pub fn modal_action(&self, m: usize) -> String {
        self.scene
            .modals
            .get(m)
            .and_then(|s| s.action.clone())
            .unwrap_or_default()
    }
    pub fn modal_bounds(&self, m: usize) -> Vec<f32> {
        self.scene
            .modals
            .get(m)
            .map(|s| vec![s.x, s.y, s.width, s.height])
            .unwrap_or_default()
    }
    pub fn active_modal(&self) -> i32 {
        self.active_modal_index().map_or(-1, |m| m as i32)
    }
    pub fn modal_count(&self) -> usize {
        self.scene.modals.len()
    }
    pub fn open_modal(&mut self, m: usize) -> bool {
        if self.is_popup(m) {
            return self.open_popup_index(m);
        }
        if m >= self.modal_open.len() || self.modal_open[m] {
            return false;
        }
        self.modal_open[m] = true;
        self.modal_restore[m] = self.focused;
        self.cancel_gestures();
        let first = self.scene.modals[m]
            .controls
            .clone()
            .find(|&i| self.enabled(i));
        self.set_focus(first);
        self.invalidate_layers();
        true
    }
    pub fn close_modal(&mut self, m: usize) -> bool {
        if self.is_popup(m) {
            return self.close_popup_index(m);
        }
        if m >= self.modal_open.len() || !self.modal_open[m] {
            return false;
        }
        self.modal_open[m] = false;
        let restore = self.modal_restore[m].take();
        self.cancel_gestures();
        let focused = self.focused.filter(|&i| self.enabled(i));
        self.set_focus(focused.or(restore));
        self.invalidate_layers();
        true
    }
    /// Escape or a press on the backdrop: closes the layer and records a
    /// dismissal so hosts can run the layer's `dismissed` handler.
    pub fn dismiss_modal(&mut self, m: usize) -> bool {
        if m >= self.scene.modals.len() || !self.scene.modals[m].dismiss || !self.close_modal(m) {
            return false;
        }
        self.dismissals = self.dismissals.wrapping_add(1);
        self.last_dismissed = Some(m);
        true
    }
    pub fn dismiss_count(&self) -> u32 {
        self.dismissals
    }
    pub fn dismissed_index(&self) -> i32 {
        self.last_dismissed.map_or(-1, |m| m as i32)
    }
    pub fn event_index(&self) -> i32 {
        self.last_event.map_or(-1, |i| i as i32)
    }
    pub fn focused_index(&self) -> i32 {
        self.focused.map_or(-1, |i| i as i32)
    }
    pub fn width(&self) -> f32 {
        self.scene.width
    }
    pub fn height(&self) -> f32 {
        self.scene.height
    }
    pub fn clipped(&self) -> bool {
        self.scene.clip
    }
    pub fn frame_radius(&self) -> f32 {
        self.scene.radius
    }
    pub fn overflow(&self) -> String {
        if self.clipped() { "hidden" } else { "visible" }.into()
    }
    pub fn background_color(&self) -> String {
        {
            let c = self.scene.background;
            format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
        }
    }
    pub fn render_width(&self) -> f32 {
        if self.scene.clip || self.scene.scroll {
            self.width()
        } else {
            self.scene.content_width
        }
    }
    pub fn render_height(&self) -> f32 {
        if self.scene.clip || self.scene.scroll {
            self.height()
        } else {
            self.scene.content_height
        }
    }
    pub fn scrollable(&self) -> bool {
        self.scene.scroll
    }
    pub fn viewport(&self) -> Vec<f32> {
        self.viewport_rect().to_vec()
    }
    pub fn scroll_offset(&self) -> Vec<f32> {
        self.scroll.to_vec()
    }
    pub fn scroll(&mut self, dx: f32, dy: f32) {
        // A wheel over a canvas belongs to the application (pan/zoom), not to
        // the scene's scroll container.
        if let (Some(i), Some((last, local))) = (self.canvas_input.hovered, self.canvas_input.last)
        {
            if i == last && dx.is_finite() && dy.is_finite() && self.reachable(i) {
                self.push_canvas_event(i, CanvasEventKind::Wheel { dx, dy }, local);
                return;
            }
        }
        // Modal input is isolated from the base scroll container, just as
        // pointer hit testing and keyboard focus are isolated from the base.
        // A refused scroll must also preserve the active modal's press.
        if !self.scene.scroll
            || self.active_modal_index().is_some()
            || !dx.is_finite()
            || !dy.is_finite()
        {
            return;
        }
        let v = self.viewport_rect();
        let size = self.content_size();
        let old = self.scroll;
        self.scroll = [
            (old[0] + dx).clamp(0., (size[0] - v[2]).max(0.)),
            (old[1] + dy).clamp(0., (size[1] - v[3]).max(0.)),
        ];
        if old != self.scroll {
            self.revision = self.revision.wrapping_add(1);
            self.layout_revision = self.layout_revision.wrapping_add(1);
        }
        for (i, c) in self.controls.iter_mut().enumerate() {
            if self.modal_of[i].is_some() {
                continue;
            }
            c.scroll_x = self.scroll[0];
            c.scroll_y = self.scroll[1];
        }
        if old != self.scroll {
            self.replace_open_popups();
        }
        self.pointer(-1., -1., 3);
    }
    pub fn hit_index(&self, x: f32, y: f32) -> i32 {
        if !x.is_finite() || !y.is_finite() {
            return -1;
        }
        if self.scene.clip
            && !round_inside(
                x,
                y,
                [0., 0., self.width(), self.height()],
                self.scene.radius,
            )
        {
            return -1;
        }
        let v = self.viewport_rect();
        let outside_viewport =
            self.scene.scroll && (x < v[0] || y < v[1] || x >= v[0] + v[2] || y >= v[1] + v[3]);
        self.controls
            .iter()
            .enumerate()
            .rev()
            .find(|(i, c)| {
                self.reachable(*i)
                    && !(outside_viewport && self.modal_of[*i].is_none())
                    && c.hit(x, y)
            })
            .map_or(-1, |(i, _)| i as i32)
    }
    pub fn hit(&self, x: f32, y: f32) -> bool {
        self.hit_index(x, y) >= 0
    }
    pub fn focus(&mut self, value: bool) {
        let before = self.child_revision();
        self.set_focus(if value {
            self.focused
                .or_else(|| (0..self.controls.len()).find(|i| self.enabled(*i)))
        } else {
            None
        });
        if !value {
            self.captured = None;
        }
        self.update_revision(before);
    }
    pub fn focus_control(&mut self, index: usize) -> bool {
        if index >= self.controls.len() || !self.enabled(index) {
            return false;
        }
        let before = self.child_revision();
        self.captured = None;
        for c in &mut self.controls {
            c.down = false;
            c.keyboard = None;
        }
        self.set_focus(Some(index));
        self.update_revision(before);
        true
    }
    /// Move within the scene; false lets the browser transfer focus outside it.
    pub fn focus_next(&mut self, reverse: bool) -> bool {
        let mut next = if reverse {
            (0..self.focused.unwrap_or(self.controls.len()))
                .rev()
                .find(|i| self.enabled(*i))
        } else {
            (self.focused.map_or(0, |i| i + 1)..self.controls.len()).find(|i| self.enabled(*i))
        };
        // An open layer traps focus: Tab wraps inside it instead of leaving
        // the scene, so the host never moves focus behind the backdrop.
        if next.is_none() && self.active_modal_index().is_some() {
            next = if reverse {
                (0..self.controls.len()).rev().find(|i| self.enabled(*i))
            } else {
                (0..self.controls.len()).find(|i| self.enabled(*i))
            };
        }
        if next.is_none() {
            return false;
        }
        let before = self.child_revision();
        self.captured = None;
        self.set_focus(next);
        self.update_revision(before);
        true
    }
    /// Keys: 1 = Space, 2 = Enter, 3 = Escape, 4 = ArrowUp, 5 = ArrowDown (menus and selects).
    /// Escape closes the topmost popup, otherwise dismisses the active
    /// layer even while its text field is being edited.
    pub fn key_event(&mut self, key: u8, pressed: bool, repeat: bool) {
        if key == 3 {
            if pressed && !repeat {
                let before = self.child_revision();
                if !self.popup_escape() {
                    if let Some(m) = self.active_modal_index() {
                        self.dismiss_modal(m);
                    }
                }
                self.update_revision(before);
            }
            return;
        }
        // 4 = ArrowUp, 5 = ArrowDown: move inside an open menu or select.
        if matches!(key, 4 | 5) && !self.text_editing() {
            if pressed {
                let before = self.child_revision();
                self.popup_arrow(key == 5);
                self.update_revision(before);
            }
            return;
        }
        if self.text_editing() {
            return;
        }
        if let Some(i) = self.focused {
            let before = self.child_revision();
            let clicks = self.controls[i].clicks();
            self.controls[i].key_event(key, pressed, repeat);
            if self.controls[i].clicks() != clicks {
                self.last_event = Some(i);
                if self.ui_document.is_some() {
                    self.ui_activations.record(i);
                }
                self.popup_activated(i);
            }
            self.update_revision(before);
        }
    }
    pub fn pointer(&mut self, x: f32, y: f32, kind: u8) {
        let before = self.child_revision();
        if kind == 1 {
            self.popup_press(x, y);
        }
        let active = self.active_modal_index();
        for i in 0..self.controls.len() {
            let reachable = self.reachable(i);
            self.controls[i].reveal_pointer(x, y, kind != 3 && reachable);
        }
        let hit = usize::try_from(self.hit_index(x, y)).ok();
        self.popup_hover(hit, kind != 3);
        if kind == 1 {
            for c in &mut self.controls {
                c.keyboard = None;
            }
            self.captured = hit.filter(|i| self.enabled(*i));
            self.set_focus(self.captured);
            if hit.is_none() && x.is_finite() && y.is_finite() {
                if let Some(m) = active {
                    let s = &self.scene.modals[m];
                    let inside = x >= s.x && y >= s.y && x < s.x + s.width && y < s.y + s.height;
                    if !inside {
                        self.dismiss_modal(m);
                    }
                }
            }
        }
        let mut activated = None;
        for (i, c) in self.controls.iter_mut().enumerate() {
            let clicks = c.clicks();
            let (px, py) = if hit == Some(i) { (x, y) } else { (-1., -1.) };
            // Only the captured control receives release. Others cannot inherit it.
            let event = if kind == 2 && self.captured != Some(i) {
                0
            } else {
                kind
            };
            c.pointer_interaction(px, py, event);
            if c.clicks() != clicks {
                self.last_event = Some(i);
                if self.ui_document.is_some() {
                    self.ui_activations.record(i);
                }
                activated = Some(i);
            }
        }
        if let Some(i) = activated {
            self.popup_activated(i);
        }
        if kind == 1 || (kind == 0 && self.captured.is_some()) {
            if let Some(i) = self.captured {
                let b = self.controls[i].bounds_rect();
                if let Some(editor) = &mut self.editors[i] {
                    editor.hit(x - b[0], y - b[1], kind == 0);
                }
                if let Some(range) = Arc::make_mut(&mut self.controls[i].template).range.as_mut() {
                    range.value = ((x - b[0] - 8.) / (b[2] - 16.).max(1.)).clamp(0., 1.);
                }
            }
            self.refresh_editors();
        }
        if kind == 2 || kind == 3 {
            self.captured = None;
        }
        self.route_canvas_pointer(x, y, kind, hit);
        self.update_revision(before);
    }
    pub fn activate(&mut self) {
        if let Some(i) = self
            .focused
            .or_else(|| (0..self.controls.len()).find(|i| self.enabled(*i)))
        {
            self.activate_control(i);
        }
    }
    pub fn activate_control(&mut self, i: usize) {
        if i < self.controls.len() && !self.reachable(i) {
            return;
        }
        if let Some(c) = self.controls.get_mut(i) {
            let count = c.clicks();
            c.activate();
            if c.clicks() != count {
                self.last_event = Some(i);
                if self.ui_document.is_some() {
                    self.ui_activations.record(i);
                }
                self.popup_activated(i);
            }
        }
    }
    pub fn clicks(&self) -> u32 {
        self.controls
            .iter()
            .map(Button::clicks)
            .fold(0, u32::saturating_add)
    }
    pub fn key(&self) -> String {
        self.controls
            .get(self.active())
            .map(Button::key)
            .unwrap_or_default()
    }
    pub fn label(&self) -> String {
        self.controls
            .get(self.active())
            .map(Button::label)
            .unwrap_or_default()
    }
    pub fn action(&self) -> String {
        self.controls
            .get(self.active())
            .map(Button::action)
            .unwrap_or_default()
    }
    pub fn disabled(&self) -> bool {
        self.controls.iter().all(Button::disabled)
    }
    pub fn bounds(&self) -> Vec<f32> {
        self.controls
            .get(self.focused.unwrap_or(0))
            .map(Button::bounds)
            .unwrap_or_else(|| vec![0., 0., self.width(), self.height()])
    }
    pub fn is_focused(&self) -> bool {
        self.focused.is_some()
    }
    pub fn visual_revision(&self) -> u32 {
        self.revision
    }
    pub fn is_animating(&self) -> bool {
        self.popup_pending()
            || self.controls.iter().any(Button::is_animating)
            || self
                .editors
                .iter()
                .flatten()
                .any(crate::text_input::Editor::is_animating)
    }
    /// Proximity reaches sibling controls even when the pointer is outside all hits.
    pub fn reveal_pointer(&mut self, x: f32, y: f32, present: bool) {
        let before = self.child_revision();
        for i in 0..self.controls.len() {
            let reachable = self.reachable(i);
            self.controls[i].reveal_pointer(x, y, present && reachable);
        }
        self.update_revision(before);
    }
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        let before = self.child_revision();
        for c in &mut self.controls {
            c.set_reduced_motion(reduced);
        }
        for editor in self.editors.iter_mut().flatten() {
            editor.reduced_motion(reduced);
        }
        self.refresh_editors();
        self.update_revision(before);
    }
    pub fn tick(&mut self, dt: f32) -> bool {
        let before = self.child_revision();
        self.popup_tick(dt);
        for c in &mut self.controls {
            c.tick(dt);
        }
        for editor in self.editors.iter_mut().flatten() {
            editor.tick(dt);
        }
        self.refresh_editors();
        self.update_revision(before);
        self.is_animating()
    }
    pub fn preserve_interaction(&mut self, previous: &Runtime) {
        self.preserve_interaction_matches(previous);
    }
    pub(super) fn preserve_interaction_matches(
        &mut self,
        previous: &Runtime,
    ) -> Vec<Option<usize>> {
        use std::collections::HashMap;
        // Reconcile only unambiguous identities; never transfer a gesture by index.
        let mut previous_keys: HashMap<&str, Option<usize>> =
            HashMap::with_capacity(previous.controls.len());
        for (i, c) in previous.controls.iter().enumerate() {
            let key = c.scene.button.key.as_str();
            if !key.is_empty() {
                previous_keys
                    .entry(key)
                    .and_modify(|i| *i = None)
                    .or_insert(Some(i));
            }
        }
        let matches: Vec<Option<usize>> = {
            let mut current_counts: HashMap<&str, usize> =
                HashMap::with_capacity(self.controls.len());
            for c in &self.controls {
                let key = c.scene.button.key.as_str();
                if !key.is_empty() {
                    *current_counts.entry(key).or_default() += 1;
                }
            }
            self.controls
                .iter()
                .map(|c| {
                    let key = c.scene.button.key.as_str();
                    if current_counts.get(key) == Some(&1) {
                        previous_keys.get(key).copied().flatten()
                    } else {
                        None
                    }
                })
                .collect()
        };
        self.scroll(
            previous.scroll[0] - self.scroll[0],
            previous.scroll[1] - self.scroll[1],
        );
        // A layer keeps its runtime open/closed state and focus-restore target
        // while its source still declares the same initial state under one key.
        let mut layers_changed = false;
        for (m, spec) in self.scene.modals.iter().enumerate() {
            if spec.key.is_empty() {
                continue;
            }
            let Some(old) = previous
                .scene
                .modals
                .iter()
                .position(|p| p.key == spec.key && p.open == spec.open)
            else {
                continue;
            };
            if self.modal_open[m] != previous.modal_open[old] {
                self.modal_open[m] = previous.modal_open[old];
                layers_changed = true;
            }
            self.modal_restore[m] = previous.modal_restore[old]
                .and_then(|target| matches.iter().position(|x| *x == Some(target)));
        }
        self.dismissals = previous.dismissals;
        self.preserve_popups(previous);
        // Application geometry is keyed by canvas key, not by scene identity.
        self.canvases = previous.canvases.clone();
        if !self.canvases.is_empty() {
            *self.geometry.borrow_mut() = None;
        }
        if layers_changed {
            self.invalidate_layers();
        }
        let before = self.child_revision();
        let mut focused = None;
        for (i, c) in self.controls.iter_mut().enumerate() {
            if let Some(old) = matches[i] {
                c.preserve_interaction(&previous.controls[old]);
                self.submissions[i] = previous.submissions[old];
                if previous.focused == Some(old) && !c.disabled() && c.template.clickable {
                    focused = Some(i);
                }
            }
        }
        for (editor, matched) in self.editors.iter_mut().zip(&matches) {
            if let Some(old) = *matched {
                if let (Some(next), Some(prev)) = (editor, &previous.editors[old]) {
                    if (next.spec.value == prev.spec.value || next.spec.value == prev.model.value())
                        && next.spec.multiline == prev.spec.multiline
                    {
                        next.model = prev.model.clone();
                        next.invalidate_content();
                    }
                }
            }
        }
        self.captured = None;
        self.last_event = None;
        self.last_submitted = None;
        self.ui_activations.clear();
        self.set_focus(focused);
        self.update_revision(before);
        matches
    }
    pub fn pixels(&self, w: u32, h: u32, s: f32) -> Vec<u8> {
        self.raster(w, h, s, true)
    }
    pub fn content_pixels(&self, w: u32, h: u32, s: f32) -> Vec<u8> {
        self.raster(w, h, s, false)
    }
    pub fn raster_stats(&self) -> Vec<u32> {
        let mut stats = vec![0u32; 2];
        for c in &self.controls {
            let s = [c.raster_builds.get(), c.raster_paints.get()];
            for k in 0..2 {
                stats[k] = stats[k].saturating_add(s[k]);
            }
        }
        stats
    }
    /// Discard reconstructible CPU pixels after the GPU takes over. For multiple
    /// controls, leaf rasters are created only with the root raster, so its
    /// presence also gates this traversal on subsequent GPU frames.
    pub fn release_cpu_cache(&self) {
        let had_root = self.raster_cache.borrow_mut().take().is_some();
        if had_root || self.controls.len() == 1 {
            for control in &self.controls {
                control.release_cpu_cache();
            }
        }
    }
    pub fn gpu_commands(&self, s: f32, b: bool) -> Vec<f32> {
        self.vector_snapshot(s, b).commands.clone()
    }
    pub fn gpu_edges(&self, s: f32, b: bool) -> Vec<f32> {
        self.vector_snapshot(s, b).edges.clone()
    }
    pub fn gpu_tiles(&self, w: u32, h: u32, s: f32, b: bool) -> Vec<u32> {
        if w == 0 || h == 0 || w > 8192 || h > 8192 || !s.is_finite() || s <= 0. {
            return vec![];
        }
        self.vector_snapshot(s, b).tiles(w, h, s, self.scroll)
    }
    pub fn gpu_paints(&self) -> Vec<f32> {
        let mut paints = Vec::new();
        self.gpu_paints_into(&mut paints);
        paints
    }
    pub fn gpu_params(&self, w: u32, h: u32, s: f32, o: bool) -> Vec<f32> {
        let mut params = Vec::new();
        self.gpu_params_into(w, h, s, o, &mut params);
        params
    }
}
