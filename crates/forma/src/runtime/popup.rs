//! Anchored, non-modal popup layers: popups, menus, selects and tooltips.
//!
//! A popup reuses a scene `Modal` layer (surface, controls, open state) and turns it into a
//! layer anchored to a control: [`Runtime::set_popup`] names the layer by key and the anchor by
//! control key. Unlike a modal it draws no backdrop and blocks nothing: the base scene keeps
//! hover, hits, focus and scrolling while it is open. A press outside the surface and the anchor
//! closes it (the press still reaches the control under it); Escape closes the topmost one.
//! Menus and selects take focus, move it with the arrow keys, close after an item is activated
//! and return focus to the anchor. A tooltip opens after the pointer rests on its anchor for
//! `delay_ms`; the delay runs in [`Runtime::tick`], and [`Runtime::is_animating`] stays true
//! while it is pending, so hosts keep their usual redraw loop running.

use super::Runtime;

/// What the layer is for; decides focus and keyboard behaviour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PopupKind {
    /// Free content (a card, a picker). Opening does not move focus.
    #[default]
    Popup,
    /// Takes focus; Up/Down move it, activation closes the menu, focus returns to the anchor.
    Menu,
    /// A menu whose items are choices; behaves like [`PopupKind::Menu`].
    Select,
    /// Opens on hover after a delay, closes when the pointer leaves the anchor. Never focused
    /// and never hit: the pointer passes through to the scene.
    Tooltip,
}

/// Preferred side of the anchor. The side flips when the popup does not fit there but fits on
/// the opposite side; the cross axis is aligned to the anchor's start and clamped to the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    #[default]
    Below,
    Above,
    Right,
    Left,
}

impl Placement {
    fn opposite(self) -> Self {
        match self {
            Self::Below => Self::Above,
            Self::Above => Self::Below,
            Self::Right => Self::Left,
            Self::Left => Self::Right,
        }
    }
}

/// Configuration of one popup layer.
#[derive(Clone, Debug, PartialEq)]
pub struct PopupSpec {
    /// Key of the control the popup is anchored to.
    pub anchor: String,
    pub kind: PopupKind,
    pub placement: Placement,
    /// Gap between the anchor and the popup, in logical pixels.
    pub offset: f32,
    /// Hover time before a tooltip opens, in milliseconds.
    pub delay_ms: f32,
}

impl PopupSpec {
    pub fn new(anchor: impl Into<String>, kind: PopupKind) -> Self {
        Self {
            anchor: anchor.into(),
            kind,
            placement: Placement::Below,
            offset: 4.,
            delay_ms: if kind == PopupKind::Tooltip { 500. } else { 0. },
        }
    }
    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }
    pub fn delay_ms(mut self, delay_ms: f32) -> Self {
        self.delay_ms = delay_ms;
        self
    }
}

#[derive(Clone, Debug)]
pub(super) struct Popup {
    spec: PopupSpec,
    anchor: Option<usize>,
    placement: Placement,
}

/// Popup layers of one runtime, indexed like `scene.modals`.
#[derive(Clone, Debug, Default)]
pub(super) struct Popups {
    layers: Vec<Option<Popup>>,
    /// Tooltip layer waiting for its delay and the time left, in milliseconds.
    pending: Option<(usize, f32)>,
}

/// Place a `size` rectangle next to `anchor` inside `[0, 0, frame]`.
pub(super) fn place(
    anchor: [f32; 4],
    size: [f32; 2],
    frame: [f32; 2],
    placement: Placement,
    offset: f32,
) -> ([f32; 2], Placement) {
    let fits = |p: Placement| match p {
        Placement::Below => anchor[1] + anchor[3] + offset + size[1] <= frame[1],
        Placement::Above => anchor[1] - offset - size[1] >= 0.,
        Placement::Right => anchor[0] + anchor[2] + offset + size[0] <= frame[0],
        Placement::Left => anchor[0] - offset - size[0] >= 0.,
    };
    let side = if !fits(placement) && fits(placement.opposite()) {
        placement.opposite()
    } else {
        placement
    };
    let clamp = |v: f32, len: f32, max: f32| v.min(max - len).max(0.);
    let origin = match side {
        Placement::Below => [anchor[0], anchor[1] + anchor[3] + offset],
        Placement::Above => [anchor[0], anchor[1] - offset - size[1]],
        Placement::Right => [anchor[0] + anchor[2] + offset, anchor[1]],
        Placement::Left => [anchor[0] - offset - size[0], anchor[1]],
    };
    (
        [
            clamp(origin[0], size[0], frame[0]),
            clamp(origin[1], size[1], frame[1]),
        ],
        side,
    )
}

impl Runtime {
    pub(super) fn popup(&self, m: usize) -> Option<&Popup> {
        self.popups.layers.get(m).and_then(Option::as_ref)
    }
    pub(super) fn is_popup(&self, m: usize) -> bool {
        self.popup(m).is_some()
    }
    fn popup_kind(&self, m: usize) -> Option<PopupKind> {
        self.popup(m).map(|p| p.spec.kind)
    }
    fn modal_by_key(&self, key: &str) -> Option<usize> {
        if key.is_empty() {
            return None;
        }
        self.scene.modals.iter().position(|m| m.key == key)
    }
    /// Whether a control of popup layer `m` can be hit and focused.
    pub(super) fn popup_reachable(&self, m: usize) -> bool {
        self.modal_open[m]
            && self.popup_kind(m) != Some(PopupKind::Tooltip)
            && self.active_modal_index().is_none_or(|active| active < m)
    }

    /// Turn the scene `Modal` layer `layer` into a popup anchored to the control keyed
    /// `spec.anchor`. The layer stops drawing its backdrop and blocking the scene. An open layer
    /// is placed at once; otherwise it is placed every time it opens.
    pub fn set_popup(&mut self, layer: &str, spec: PopupSpec) -> Result<(), String> {
        let m = self
            .modal_by_key(layer)
            .ok_or_else(|| format!("No Modal layer with key '{layer}'"))?;
        let anchor = self
            .controls
            .iter()
            .position(|c| !spec.anchor.is_empty() && c.scene.button.key == spec.anchor)
            .ok_or_else(|| format!("No control with key '{}'", spec.anchor))?;
        if self.modal_of[anchor] == Some(m) {
            return Err("A popup cannot be anchored to its own content".into());
        }
        if !(spec.offset.is_finite() && spec.delay_ms.is_finite() && spec.delay_ms >= 0.) {
            return Err("Popup offset and delay must be finite".into());
        }
        self.popups.layers.resize(self.scene.modals.len(), None);
        self.scene.modals[m].backdrop = [0; 4];
        self.popups.layers[m] = Some(Popup {
            placement: spec.placement,
            spec,
            anchor: Some(anchor),
        });
        if self.modal_open[m] {
            self.place_popup(m);
            if self.popup_kind(m) == Some(PopupKind::Tooltip) {
                // A tooltip is never focused; it opens only from hover.
                self.modal_open[m] = false;
            }
        }
        self.cancel_gestures();
        self.invalidate_layers();
        Ok(())
    }

    /// Move layer `m` next to its anchor, translating its controls with the surface.
    fn place_popup(&mut self, m: usize) {
        let Some(popup) = self.popup(m) else {
            return;
        };
        let Some(anchor) = popup.anchor.filter(|&a| a < self.controls.len()) else {
            return;
        };
        let (spec, rect) = (popup.spec.clone(), self.controls[anchor].bounds_rect());
        let surface = &self.scene.modals[m];
        let (origin, side) = place(
            rect,
            [surface.width, surface.height],
            [self.scene.width, self.scene.height],
            spec.placement,
            spec.offset,
        );
        let (dx, dy) = (origin[0] - surface.x, origin[1] - surface.y);
        if let Some(Some(p)) = self.popups.layers.get_mut(m) {
            p.placement = side;
        }
        if dx == 0. && dy == 0. {
            return;
        }
        self.scene.modals[m].x = origin[0];
        self.scene.modals[m].y = origin[1];
        for i in self.scene.modals[m].controls.clone() {
            let control = &mut self.controls[i];
            control.scene.button.x += dx;
            control.scene.button.y += dy;
            *control.raster_cache.borrow_mut() = None;
            *control.display_cache.borrow_mut() = None;
            control.visual_revision = control.visual_revision.wrapping_add(1);
        }
        self.invalidate_layers();
    }

    /// Open popup layer `m` next to its anchor.
    pub(super) fn open_popup_index(&mut self, m: usize) -> bool {
        if m >= self.modal_open.len() || self.modal_open[m] || !self.is_popup(m) {
            return false;
        }
        if self.popups.pending.is_some_and(|(p, _)| p == m) {
            self.popups.pending = None;
        }
        self.place_popup(m);
        self.modal_open[m] = true;
        self.modal_restore[m] = self.focused;
        if matches!(
            self.popup_kind(m),
            Some(PopupKind::Menu | PopupKind::Select)
        ) {
            let first = self.scene.modals[m]
                .controls
                .clone()
                .find(|&i| self.enabled(i));
            if first.is_some() {
                self.set_focus(first);
            }
        }
        self.invalidate_layers();
        true
    }

    /// Close popup layer `m`. Focus inside it returns to the anchor (menus and selects) or to
    /// the control focused before it opened.
    pub(super) fn close_popup_index(&mut self, m: usize) -> bool {
        if m >= self.modal_open.len() || !self.modal_open[m] || !self.is_popup(m) {
            return false;
        }
        self.modal_open[m] = false;
        let restore = self.modal_restore[m].take();
        let inside = self.focused.is_some_and(|i| self.modal_of[i] == Some(m));
        if inside {
            let anchor = self.popup(m).and_then(|p| p.anchor);
            let target = match self.popup_kind(m) {
                Some(PopupKind::Menu | PopupKind::Select) => anchor.or(restore),
                _ => restore,
            };
            self.set_focus(target.filter(|&i| self.enabled(i)));
        }
        for i in self.scene.modals[m].controls.clone() {
            self.controls[i].pointer_interaction(-1., -1., 3);
            self.controls[i].keyboard = None;
        }
        self.invalidate_layers();
        true
    }

    fn dismiss_popup_index(&mut self, m: usize) -> bool {
        if !self.close_popup_index(m) {
            return false;
        }
        self.dismissals = self.dismissals.wrapping_add(1);
        self.last_dismissed = Some(m);
        true
    }

    /// Open the popup layer keyed `layer`.
    pub fn open_popup(&mut self, layer: &str) -> bool {
        let before = self.child_revision();
        let opened = self
            .modal_by_key(layer)
            .is_some_and(|m| self.open_popup_index(m));
        self.update_revision(before);
        opened
    }
    /// Close the popup layer keyed `layer` without recording a dismissal.
    pub fn close_popup(&mut self, layer: &str) -> bool {
        let before = self.child_revision();
        let closed = self
            .modal_by_key(layer)
            .is_some_and(|m| self.close_popup_index(m));
        self.update_revision(before);
        closed
    }
    /// Open it when closed, close it when open.
    pub fn toggle_popup(&mut self, layer: &str) -> bool {
        if self.popup_open(layer) {
            self.close_popup(layer)
        } else {
            self.open_popup(layer)
        }
    }
    pub fn popup_open(&self, layer: &str) -> bool {
        self.modal_by_key(layer)
            .is_some_and(|m| self.is_popup(m) && self.modal_open[m])
    }
    /// Surface of the popup keyed `layer` as `[x, y, width, height]` in frame coordinates.
    pub fn popup_bounds(&self, layer: &str) -> Option<[f32; 4]> {
        let m = self.modal_by_key(layer).filter(|&m| self.is_popup(m))?;
        let s = &self.scene.modals[m];
        Some([s.x, s.y, s.width, s.height])
    }
    /// Side the popup was last placed on, after flipping.
    pub fn popup_placement(&self, layer: &str) -> Option<Placement> {
        self.modal_by_key(layer)
            .and_then(|m| self.popup(m))
            .map(|p| p.placement)
    }

    /// Topmost open popup above any open modal, ignoring tooltips.
    fn top_popup(&self) -> Option<usize> {
        (0..self.modal_open.len())
            .rev()
            .find(|&m| self.popup_reachable(m))
    }

    /// Press handling before hit testing: a press outside an open popup and outside its anchor
    /// dismisses it, and any press closes tooltips. The press itself is not consumed.
    pub(super) fn popup_press(&mut self, x: f32, y: f32) {
        self.popups.pending = None;
        for m in (0..self.modal_open.len()).rev() {
            if !self.modal_open[m] {
                continue;
            }
            let Some(popup) = self.popup(m) else {
                continue;
            };
            if popup.spec.kind == PopupKind::Tooltip {
                self.close_popup_index(m);
                continue;
            }
            if !self.popup_reachable(m) {
                continue;
            }
            let s = &self.scene.modals[m];
            let inside = x >= s.x && y >= s.y && x < s.x + s.width && y < s.y + s.height;
            let on_anchor = popup
                .anchor
                .is_some_and(|a| self.reachable(a) && self.controls[a].hit(x, y));
            if !inside && !on_anchor && self.scene.modals[m].dismiss {
                self.dismiss_popup_index(m);
            }
        }
    }

    /// Hover handling for tooltips: start the delay over an anchor, close when it is left.
    pub(super) fn popup_hover(&mut self, hit: Option<usize>, present: bool) {
        for m in 0..self.popups.layers.len() {
            let Some(popup) = self.popup(m) else {
                continue;
            };
            if popup.spec.kind != PopupKind::Tooltip {
                continue;
            }
            let over = present && popup.anchor.is_some() && popup.anchor == hit;
            let delay = popup.spec.delay_ms;
            if !over {
                if self.popups.pending.is_some_and(|(p, _)| p == m) {
                    self.popups.pending = None;
                }
                self.close_popup_index(m);
            } else if !self.modal_open[m] && self.popups.pending.is_none_or(|(p, _)| p != m) {
                if delay <= 0. {
                    self.open_popup_index(m);
                } else {
                    self.popups.pending = Some((m, delay));
                }
            }
        }
    }

    /// Advance the tooltip delay by `dt` milliseconds.
    pub(super) fn popup_tick(&mut self, dt: f32) {
        if let Some((m, left)) = self.popups.pending {
            let left = left - dt.max(0.);
            if left <= 0. {
                self.popups.pending = None;
                self.open_popup_index(m);
            } else {
                self.popups.pending = Some((m, left));
            }
        }
    }
    pub(super) fn popup_pending(&self) -> bool {
        self.popups.pending.is_some()
    }

    /// Escape: closes the topmost popup (or an open tooltip). Returns whether it did.
    pub(super) fn popup_escape(&mut self) -> bool {
        if let Some(m) = self.top_popup() {
            if self.scene.modals[m].dismiss {
                return self.dismiss_popup_index(m);
            }
        }
        let tooltips: Vec<usize> = (0..self.modal_open.len())
            .filter(|&m| self.modal_open[m] && self.popup_kind(m) == Some(PopupKind::Tooltip))
            .collect();
        self.popups.pending = None;
        let mut closed = false;
        for m in tooltips {
            closed |= self.close_popup_index(m);
        }
        closed
    }

    /// Up (`forward == false`) or Down inside an open menu or select: move focus to the
    /// previous or next enabled item, wrapping. On a focused menu anchor, Down opens the menu.
    pub(super) fn popup_arrow(&mut self, forward: bool) -> bool {
        let menu = self.top_popup().filter(|&m| {
            matches!(
                self.popup_kind(m),
                Some(PopupKind::Menu | PopupKind::Select)
            )
        });
        let Some(m) = menu else {
            let anchored = (0..self.popups.layers.len()).find(|&m| {
                matches!(
                    self.popup_kind(m),
                    Some(PopupKind::Menu | PopupKind::Select)
                ) && self.focused.is_some()
                    && self.popup(m).and_then(|p| p.anchor) == self.focused
            });
            return forward && anchored.is_some_and(|m| self.open_popup_index(m));
        };
        let items: Vec<usize> = self.scene.modals[m]
            .controls
            .clone()
            .filter(|&i| self.enabled(i))
            .collect();
        if items.is_empty() {
            return false;
        }
        let current = self
            .focused
            .and_then(|f| items.iter().position(|&i| i == f));
        let next = match (current, forward) {
            (None, true) => 0,
            (None, false) => items.len() - 1,
            (Some(c), true) => (c + 1) % items.len(),
            (Some(c), false) => (c + items.len() - 1) % items.len(),
        };
        self.set_focus(Some(items[next]));
        true
    }

    /// After control `index` was activated: an item of a menu or select closes its layer.
    pub(super) fn popup_activated(&mut self, index: usize) {
        if let Some(m) = self.modal_of.get(index).copied().flatten() {
            if matches!(
                self.popup_kind(m),
                Some(PopupKind::Menu | PopupKind::Select)
            ) {
                self.close_popup_index(m);
            }
        }
    }

    /// Re-place open popups after their anchors moved (scrolling).
    pub(super) fn replace_open_popups(&mut self) {
        for m in 0..self.popups.layers.len() {
            if self.modal_open[m] && self.is_popup(m) {
                self.place_popup(m);
            }
        }
    }

    /// Keep popup configuration across a scene rebuild by layer and anchor key.
    pub(super) fn preserve_popups(&mut self, previous: &Runtime) {
        for (old, layer) in previous.popups.layers.iter().enumerate() {
            let Some(layer) = layer else {
                continue;
            };
            let key = previous.scene.modals[old].key.clone();
            let was_open = previous.modal_open[old];
            if self.set_popup(&key, layer.spec.clone()).is_ok() {
                if let Some(m) = self.modal_by_key(&key) {
                    if was_open && !self.modal_open[m] {
                        self.open_popup_index(m);
                    } else if self.modal_open[m] {
                        self.place_popup(m);
                    }
                }
            }
        }
    }
}
