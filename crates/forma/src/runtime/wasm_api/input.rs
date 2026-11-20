//! Pointer, keyboard, focus and scrolling.

use super::*;

#[wasm_bindgen]
impl Runtime {
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
        if self.active_modal_index().is_none()
            && dx.is_finite()
            && dy.is_finite()
            && self.scroll_area_under_pointer(dx, dy)
        {
            let pointer_at = self.pointer_at;
            self.replace_open_popups();
            self.pointer(-1., -1., 3);
            self.pointer_at = pointer_at;
            return;
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
            self.composition_revision = self.composition_revision.wrapping_add(1);
            self.scene_dirty
                .insert(super::DirtyFlags::COMPOSITION | super::DirtyFlags::PAINT);
        }
        self.sync_control_scroll();
        if old != self.scroll {
            self.replace_open_popups();
        }
        let pointer_at = self.pointer_at;
        self.pointer(-1., -1., 3);
        self.pointer_at = pointer_at;
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
                    && self.inside_clip(*i, x, y)
                    && c.hit(x, y)
            })
            .map_or(-1, |(i, _)| i as i32)
    }

    /// The topmost enabled control at `(x, y)`, looking past the ones above it that take no press
    /// at all (a Text). A disabled pressable control still blocks what lies under it.
    fn enabled_hit_below(&self, x: f32, y: f32) -> Option<usize> {
        let v = self.viewport_rect();
        let outside_viewport =
            self.scene.scroll && (x < v[0] || y < v[1] || x >= v[0] + v[2] || y >= v[1] + v[3]);
        let at = (0..self.controls.len()).rev().find(|&i| {
            self.reachable(i)
                && !(outside_viewport && self.modal_of[i].is_none())
                && self.inside_clip(i, x, y)
                && self.controls[i].hit(x, y)
                && self.controls[i].template.clickable
        })?;
        self.enabled(at).then_some(at)
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
        self.pointer_at = (kind != 3 && x.is_finite() && y.is_finite()).then_some([x, y]);
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
        // A point over what takes no press (a Text) belongs to the pressable control below it,
        // such as the area of a container with a `clicked` handler.
        let pressed = hit
            .filter(|i| self.enabled(*i))
            .or_else(|| hit.and_then(|_| self.enabled_hit_below(x, y)));
        self.popup_hover(hit, kind != 3);
        if kind == 1 {
            for c in &mut self.controls {
                c.keyboard = None;
            }
            self.captured = pressed;
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
            let (px, py) = if hit == Some(i) || pressed == Some(i) {
                (x, y)
            } else {
                (-1., -1.)
            };
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
                if self.controls[i].template.range.is_some() {
                    if let Some(range) =
                        Arc::make_mut(&mut self.controls[i].template).range.as_mut()
                    {
                        range.value = range.snapped((x - b[0] - 8.) / (b[2] - 16.).max(1.));
                    }
                }
            }
            self.refresh_editors();
        }
        if kind == 2 || kind == 3 {
            self.captured = None;
        }
        self.route_canvas_pointer(x, y, kind, hit);
        self.route_link_pointer(x, y, kind, hit);
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

    /// Proximity reaches sibling controls even when the pointer is outside all hits.
    pub fn reveal_pointer(&mut self, x: f32, y: f32, present: bool) {
        let before = self.child_revision();
        for i in 0..self.controls.len() {
            let reachable = self.reachable(i);
            self.controls[i].reveal_pointer(x, y, present && reachable);
        }
        self.update_revision(before);
    }
}
