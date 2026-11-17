//! Per-control and single-control queries.

use super::*;

#[wasm_bindgen]
impl Runtime {
    pub fn control_count(&self) -> usize {
        self.controls.len()
    }

    /// Put control `i` in the radio group of the state path `group`, as a `group <->` binding does;
    /// an empty path takes it out. Arrow keys move through a group. A scene loaded from markup
    /// alone carries no bindings to say this. Returns whether `i` exists.
    pub fn set_control_group(&mut self, i: usize, group: &str) -> bool {
        if i >= self.controls.len() {
            return false;
        }
        self.groups.resize(self.controls.len(), None);
        self.groups[i] = (!group.is_empty()).then(|| group.to_owned());
        true
    }

    /// Put the controls inside scroll areas, as `Scroll` containers below the root do: a JSON array
    /// with, per control, the areas around it outermost first, each
    /// `{ "area", "viewport": [x, y, w, h], "content": [w, h] }`. A scene loaded from markup alone
    /// has no containers to say this. Refuses an array of another length or a malformed area.
    pub fn set_control_scroll_areas(&mut self, json: &str) -> Result<(), String> {
        let chains: Vec<Vec<crate::form_document::SceneScroll>> =
            serde_json::from_str(json).map_err(|error| format!("Invalid scroll areas: {error}"))?;
        if chains.len() != self.controls.len() {
            return Err("Scroll areas must list every control".into());
        }
        self.set_scroll_areas(chains.iter().map(Vec::as_slice));
        Ok(())
    }

    /// Clip control `i` to the scene rectangle `x, y, width, height`, as a clipping container
    /// around it does; a negative width removes the clip. A scene loaded from markup alone has no
    /// container to say this, so the caller that laid it out does. Returns whether `i` exists.
    pub fn set_control_clip(&mut self, i: usize, x: f32, y: f32, width: f32, height: f32) -> bool {
        if i >= self.controls.len() {
            return false;
        }
        let mut clips = std::mem::take(&mut self.clips);
        clips.resize(self.controls.len(), None);
        clips[i] = (width >= 0. && [x, y, width, height].iter().all(|v| v.is_finite()))
            .then_some([x, y, width, height.max(0.)]);
        self.set_clips(clips);
        true
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

    pub fn event_index(&self) -> i32 {
        self.last_event.map_or(-1, |i| i as i32)
    }

    pub fn focused_index(&self) -> i32 {
        self.focused.map_or(-1, |i| i as i32)
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
}
