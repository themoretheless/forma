//! The JavaScript-facing API of [`Runtime`]: every method exported through wasm-bindgen.
//!
//! Rust callers use the same methods; the inherent Rust-only API lives in `runtime.rs`.

use super::*;

mod accessibility;
mod controls;
mod editing;
mod input;
mod loading;
mod modals;

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Runtime {
    pub fn geometry_revision(&self) -> u32 {
        self.geometry_revision
    }

    /// Control bounds change on scroll or source replacement. Paint and
    /// text-content changes do not invalidate these bounds.
    pub fn layout_revision(&self) -> u32 {
        self.layout_revision
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
