//! Application-drawn canvas slots, view transforms, placement cache and pointer routing.
use super::Runtime;
use crate::canvas::{
    self, CanvasContent, CanvasError, CanvasEvent, CanvasEventKind, CanvasStats, CanvasView,
};
use crate::display_list::DisplayList;
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};

/// Pending canvas pointer events kept until `take_canvas_events`; later events
/// are dropped and counted.
pub const MAX_CANVAS_EVENTS: usize = 4096;

/// Application geometry for one canvas key. Lives outside the parsed scene, so
/// scene rebuilds carry it over by key.
#[derive(Clone, Default)]
pub(super) struct CanvasEntry {
    content: Arc<CanvasContent>,
    view: CanvasView,
    content_sets: u32,
    view_sets: u32,
    placements: Cell<u32>,
    placed: RefCell<Option<canvas::Placed>>,
}

#[derive(Default)]
pub(super) struct CanvasInput {
    events: Vec<CanvasEvent>,
    dropped: u32,
    captured: Option<usize>,
    pub(super) hovered: Option<usize>,
    pub(super) last: Option<(usize, [f32; 2])>,
}

impl Runtime {
    fn canvas_entry(&self, index: usize) -> Option<&CanvasEntry> {
        let control = self.controls.get(index)?;
        control.template.canvas?;
        let key = control.scene.button.key.as_str();
        if key.is_empty() {
            return None;
        }
        self.canvases.get(key)
    }
    /// Canvas rectangle in scene content coordinates (before scrolling), only
    /// for a control that declares `Canvas {}` and has content.
    pub(super) fn canvas_scene_rect(&self, index: usize) -> Option<[f32; 4]> {
        let entry = self.canvas_entry(index)?;
        if entry.content.is_empty() {
            return None;
        }
        Some(self.canvas_slot_rect(index)?.1)
    }
    /// (screen rectangle, content rectangle) of a control's canvas slot.
    fn canvas_slot_rect(&self, index: usize) -> Option<([f32; 4], [f32; 4])> {
        let control = self.controls.get(index)?;
        let slot = control.template.canvas?;
        let b = &control.scene.button;
        let r = slot.resolve([b.width, b.height]);
        let screen = control.bounds_rect();
        Some((
            [screen[0] + r[0], screen[1] + r[1], r[2], r[3]],
            [b.x + r[0], b.y + r[1], r[2], r[3]],
        ))
    }
    pub(super) fn canvas_placed(&self, index: usize) -> Option<Arc<DisplayList>> {
        let entry = self.canvas_entry(index)?;
        let rect = self.canvas_scene_rect(index)?;
        let origin = [rect[0], rect[1]];
        let mut placed = entry.placed.borrow_mut();
        if let Some(p) = placed.as_ref() {
            if p.origin == origin && p.view == entry.view {
                return Some(Arc::clone(&p.list));
            }
        }
        let list = Arc::new(canvas::place(&entry.content, origin, entry.view));
        *placed = Some(canvas::Placed {
            origin,
            view: entry.view,
            list: Arc::clone(&list),
        });
        entry.placements.set(entry.placements.get().wrapping_add(1));
        Some(list)
    }
    pub(super) fn raster_canvas(&self, index: usize, pixels: &mut [u8], w: u32, h: u32, s: f32) {
        let Some(list) = self.canvas_placed(index) else {
            return;
        };
        let Some((screen, content)) = self.canvas_slot_rect(index) else {
            return;
        };
        let offset = [screen[0] - content[0], screen[1] - content[1]];
        canvas::rasterize(&list, pixels, w, h, s, offset, screen);
    }
    fn canvas_changed(&mut self) {
        *self.geometry.borrow_mut() = None;
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
    }
    fn canvas_index(&self, key: &str) -> Option<usize> {
        if key.is_empty() {
            return None;
        }
        self.controls
            .iter()
            .position(|c| c.template.canvas.is_some() && c.scene.button.key == key)
    }
    /// Keys of controls whose template declares `Canvas {}`, in scene order.
    pub fn canvas_keys(&self) -> Vec<String> {
        self.controls
            .iter()
            .filter(|c| c.template.canvas.is_some() && !c.scene.button.key.is_empty())
            .map(|c| c.scene.button.key.clone())
            .collect()
    }
    /// Screen rectangle `[x, y, width, height]` of the canvas with this key.
    pub fn canvas_bounds(&self, key: &str) -> Option<[f32; 4]> {
        Some(self.canvas_slot_rect(self.canvas_index(key)?)?.0)
    }
    /// Replace the geometry of canvas `key`. Content may be set before the
    /// scene declares the key; it is drawn once a control with that key and a
    /// `Canvas {}` template appears. Content survives scene rebuilds.
    pub fn set_canvas_content(&mut self, key: &str, content: impl Into<Arc<CanvasContent>>) {
        let entry = self.canvases.entry(key.to_owned()).or_default();
        entry.content = content.into();
        entry.content_sets = entry.content_sets.wrapping_add(1);
        entry.placed.borrow_mut().take();
        self.canvas_changed();
    }
    /// Change pan/zoom of canvas `key` without rebuilding its content: the
    /// finished geometry is only re-transformed.
    pub fn set_canvas_view(&mut self, key: &str, view: CanvasView) -> Result<(), CanvasError> {
        let view = CanvasView::new(view.pan, view.zoom)?;
        let entry = self.canvases.entry(key.to_owned()).or_default();
        if entry.view == view {
            return Ok(());
        }
        entry.view = view;
        entry.view_sets = entry.view_sets.wrapping_add(1);
        self.canvas_changed();
        Ok(())
    }
    pub fn canvas_view(&self, key: &str) -> Option<CanvasView> {
        self.canvases.get(key).map(|e| e.view)
    }
    pub fn canvas_content(&self, key: &str) -> Option<Arc<CanvasContent>> {
        self.canvases.get(key).map(|e| Arc::clone(&e.content))
    }
    pub fn canvas_stats(&self, key: &str) -> Option<CanvasStats> {
        self.canvases.get(key).map(|e| CanvasStats {
            content_sets: e.content_sets,
            view_sets: e.view_sets,
            placements: e.placements.get(),
        })
    }
    /// Forget content and view of canvas `key`.
    pub fn clear_canvas(&mut self, key: &str) -> bool {
        let removed = self.canvases.remove(key).is_some();
        if removed {
            self.canvas_changed();
        }
        removed
    }
    /// Scene point to canvas content units through the current view.
    pub fn canvas_point_to_content(&self, key: &str, x: f32, y: f32) -> Option<[f32; 2]> {
        let rect = self.canvas_bounds(key)?;
        let view = self.canvas_view(key).unwrap_or_default();
        Some(view.to_content([x - rect[0], y - rect[1]]))
    }
    /// Pointer events over canvases since the last call, oldest first.
    pub fn take_canvas_events(&mut self) -> Vec<CanvasEvent> {
        std::mem::take(&mut self.canvas_input.events)
    }
    /// Events dropped because more than [`MAX_CANVAS_EVENTS`] were pending.
    pub fn canvas_events_dropped(&self) -> u32 {
        self.canvas_input.dropped
    }
    pub(super) fn push_canvas_event(
        &mut self,
        index: usize,
        kind: CanvasEventKind,
        local: [f32; 2],
    ) {
        if self.canvas_input.events.len() >= MAX_CANVAS_EVENTS {
            self.canvas_input.dropped = self.canvas_input.dropped.saturating_add(1);
            return;
        }
        let key = self.controls[index].scene.button.key.clone();
        let view = self.canvases.get(&key).map(|e| e.view).unwrap_or_default();
        self.canvas_input.events.push(CanvasEvent {
            key,
            kind,
            local,
            content: view.to_content(local),
        });
    }
    /// Pointer kinds as in [`Runtime::pointer`]: 0 move, 1 press, 2 release,
    /// 3 cancel/leave. A press on a canvas captures later moves and the release.
    pub(super) fn route_canvas_pointer(&mut self, x: f32, y: f32, kind: u8, hit: Option<usize>) {
        let canvas_hit = hit.filter(|&i| {
            !self.controls[i].scene.button.key.is_empty()
                && self.canvas_slot_rect(i).is_some_and(|(r, _)| {
                    x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3]
                })
        });
        let local_of = |rt: &Self, i: usize| {
            let r = rt.canvas_slot_rect(i).map_or([0.; 4], |(r, _)| r);
            [x - r[0], y - r[1]]
        };
        let target = self.canvas_input.captured.or(canvas_hit);
        if let Some(previous) = self.canvas_input.hovered {
            if kind == 3 || (Some(previous) != target && self.canvas_input.captured.is_none()) {
                let local = local_of(self, previous);
                self.push_canvas_event(previous, CanvasEventKind::Leave, local);
                self.canvas_input.hovered = None;
            }
        }
        if kind == 3 {
            self.canvas_input.captured = None;
            self.canvas_input.last = None;
            return;
        }
        let Some(i) = target else {
            return;
        };
        let event = match kind {
            1 if canvas_hit == Some(i) => {
                self.canvas_input.captured = Some(i);
                CanvasEventKind::Press
            }
            2 if self.canvas_input.captured == Some(i) => {
                self.canvas_input.captured = None;
                CanvasEventKind::Release
            }
            0 => CanvasEventKind::Move,
            _ => return,
        };
        let local = local_of(self, i);
        self.push_canvas_event(i, event, local);
        self.canvas_input.last = Some((i, local));
        let hovered = canvas_hit.or(self.canvas_input.captured);
        if hovered.is_none() {
            // Released outside the canvas that held the capture.
            self.push_canvas_event(i, CanvasEventKind::Leave, local);
            self.canvas_input.last = None;
        }
        self.canvas_input.hovered = hovered;
    }
}
