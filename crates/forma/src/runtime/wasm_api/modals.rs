//! Modal layers.

use super::*;

#[wasm_bindgen]
impl Runtime {
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
}
