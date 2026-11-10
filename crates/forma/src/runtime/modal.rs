//! Modal layer stack isolation, reachability, focus restoration and dismissal tracking.

use super::{Node, Runtime};

impl Runtime {
    /// Resolves the `scene.modals` index corresponding to a `NodeKind::Modal` tree node.
    pub(super) fn modal_index_for_node(&self, node: &Node) -> usize {
        self.scene
            .modals
            .iter()
            .position(|m| {
                (!m.controls.is_empty()).then_some(m.controls.start)
                    == node
                        .children
                        .first()
                        .and_then(|&cid| self.nodes[cid].control)
            })
            .unwrap_or(0)
    }

    /// Topmost open blocking layer, if any. It alone receives pointer and keyboard input,
    /// together with popups opened above it. Popup layers never block (see `popup.rs`).
    pub(super) fn active_modal_index(&self) -> Option<usize> {
        (0..self.modal_open.len())
            .rev()
            .find(|&m| self.modal_open[m] && !self.is_popup(m))
    }

    pub(super) fn visible(&self, index: usize) -> bool {
        self.modal_of[index].is_none_or(|m| self.modal_open[m])
    }

    /// Reachable controls belong to the active layer; everything else is
    /// blocked by the backdrop and neither hit, hovered nor focusable.
    pub(super) fn reachable(&self, index: usize) -> bool {
        match self.modal_of[index] {
            Some(m) if self.is_popup(m) => self.popup_reachable(m),
            owner => owner == self.active_modal_index(),
        }
    }

    pub(super) fn enabled(&self, index: usize) -> bool {
        self.reachable(index)
            && !self.controls[index].disabled()
            && self.controls[index].template.clickable
    }

    pub(super) fn invalidate_layers(&mut self) {
        *self.geometry.borrow_mut() = None;
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.layout_revision = self.layout_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn cancel_gestures(&mut self) {
        let before = self.child_revision();
        self.captured = None;
        let active = self.active_modal_index();
        for (i, c) in self.controls.iter_mut().enumerate() {
            let blocked = self.modal_of[i] != active;
            c.keyboard = None;
            if blocked {
                c.pointer_interaction(-1., -1., 3);
                c.reveal_pointer(0., 0., false);
            } else {
                c.down = false;
                c.update_colors();
            }
        }
        self.update_revision(before);
    }
}
