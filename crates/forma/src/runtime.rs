//! Shared native/WASM scene runtime. Node IDs are local to one loaded snapshot.
//! Controls own interaction; the renderer sees only geometry and paint slots.
use crate::canvas::CanvasEventKind;
use crate::display_list::{DisplayList, ListHints, RenderScene, STRIDE};
use crate::{Button, geometry::round_inside, markup};
use std::{cell::RefCell, sync::Arc};
use wasm_bindgen::prelude::*;

mod a11y;
mod bindings;
mod canvas;
mod links;
mod loading;
mod modal;
mod popup;
mod raster;
mod scroll_areas;
mod ui_state;
mod vector;
mod wasm_api;

pub use a11y::{A11yNode, A11yRole, A11yTree};
pub use canvas::MAX_CANVAS_EVENTS;
use canvas::{CanvasEntry, CanvasInput};
pub use links::{MAX_TEXT_LINK_EVENTS, TextLinkEvent};
pub use popup::{Placement, PopupKind, PopupSpec};
use raster::RasterCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Frame,
    Scroll,
    Modal,
    Control,
}

#[derive(Debug)]
pub struct Node {
    pub id: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: NodeKind,
    pub control: Option<usize>,
    pub modal: Option<usize>,
}
struct GeometryCache {
    scale: f32,
    background: bool,
    list: Arc<DisplayList>,
}

#[derive(Clone, PartialEq)]
enum BindingWidgetValue {
    Text(String),
    Range(f32),
}

const MAX_UI_ACTIVATIONS: usize = 4096;
#[derive(Default)]
struct UiActivations {
    events: Vec<usize>,
    overflow: bool,
}
impl UiActivations {
    fn record(&mut self, index: usize) {
        if self.events.len() >= MAX_UI_ACTIVATIONS {
            self.overflow = true;
        } else if !self.overflow {
            self.events.push(index);
        }
    }
    fn clear(&mut self) {
        self.events.clear();
        self.overflow = false;
    }
}

#[wasm_bindgen]
pub struct Runtime {
    scene: markup::Scene,
    controls: Vec<Button>,
    editors: Vec<Option<crate::text_input::Editor>>,
    range_values: Vec<Option<f32>>,
    binding_widget_baseline: RefCell<std::collections::BTreeMap<usize, BindingWidgetValue>>,
    nodes: Vec<Node>,
    focused: Option<usize>,
    captured: Option<usize>,
    last_event: Option<usize>,
    submissions: Vec<u32>,
    last_submitted: Option<usize>,
    ui_activations: UiActivations,
    // Modal layers: each control belongs to the base scene or to one layer.
    // Only the topmost open layer receives input; lower layers stay visible
    // under its backdrop and the base scene keeps its scroll offset.
    modal_of: Vec<Option<usize>>,
    modal_open: Vec<bool>,
    modal_restore: Vec<Option<usize>>,
    dismissals: u32,
    last_dismissed: Option<usize>,
    scroll: [f32; 2],
    canvases: std::collections::BTreeMap<String, CanvasEntry>,
    canvas_input: CanvasInput,
    text_links: links::TextLinkInput,
    popups: popup::Popups,
    revision: u32,
    geometry_revision: u32,
    layout_revision: u32,
    geometry: RefCell<Option<GeometryCache>>,
    raster_cache: RefCell<Option<RasterCache>>,
    ui_document: Option<crate::form_document::Document>,
    ui_render_pending: bool,
    ui_binding_idle_ready: bool,
    // Binding indices for the last committed scene, reused while model state
    // is unchanged. Recovery resolves current control identities separately.
    ui_binding_scene: Vec<crate::form_document::SceneControl>,
    // The scene rectangle each control stays inside, set by clipping containers below the root;
    // empty when the scene came without control mappings.
    clips: Vec<Option<[f32; 4]>>,
    // `Scroll` containers below the root, and per control the ones around it, outermost first.
    scroll_areas: Vec<scroll_areas::ScrollArea>,
    scroll_chains: Vec<Vec<usize>>,
    // Per control, the state path of its `group <->` binding: one radio group per path.
    groups: Vec<Option<String>>,
    // The tree node of each scroll area, and the node each control sat under before areas.
    area_nodes: Vec<usize>,
    control_homes: Vec<usize>,
    // Where the pointer last was, so a wheel reaches the scroll area under it.
    pointer_at: Option<[f32; 2]>,
    // State of the last successfully displayed retained scene. Documents are
    // private to Runtime; project/source replacement installs a new Runtime.
    ui_binding_states: Vec<crate::form_document::Properties>,
    #[cfg(not(target_arch = "wasm32"))]
    svet_document: Option<crate::native_document_graph::DocumentGraph>,
    #[cfg(not(target_arch = "wasm32"))]
    native_instance_id: u64,
    #[cfg(not(target_arch = "wasm32"))]
    native_accessibility_id: Option<u64>,
}

fn templates(source: &str, count: usize) -> Result<Vec<&str>, String> {
    let Some(mut rest) = source.strip_prefix("FORMA-TEMPLATES-1\n") else {
        return Ok(vec![source; count]);
    };
    let mut result = Vec::with_capacity(count);
    while !rest.is_empty() {
        let (length, body) = rest
            .split_once('\n')
            .ok_or("Missing template byte length")?;
        let length = length
            .parse::<usize>()
            .map_err(|_| "Invalid template byte length")?;
        if length == 0 || length > body.len() || !body.is_char_boundary(length) {
            return Err("Invalid template boundary".into());
        }
        result.push(&body[..length]);
        rest = &body[length..];
        if result.len() > count {
            return Err("Too many control templates".into());
        }
    }
    if result.len() != count {
        return Err("Control/template count mismatch".into());
    }
    Ok(result)
}
impl Runtime {
    /// Identity of the loaded scene, including noneditable accessibility nodes.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn native_scene_identity(&self) -> u64 {
        self.native_accessibility_id
            .unwrap_or(self.native_instance_id)
    }

    /// Scene-local editor identity for native composition ownership. Rebuilding
    /// a Runtime retires its editor session even if key and index are reused.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn native_editor_session(&self) -> Option<(u64, i32)> {
        (self.text_editing() && !self.control_readonly(self.focused.unwrap()))
            .then(|| (self.native_instance_id, self.focused_index()))
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    fn refresh_editors(&mut self) {
        for (i, editor) in self.editors.iter_mut().enumerate() {
            let content = if let Some(editor) = editor {
                let focused = self.focused == Some(i) && !self.controls[i].disabled();
                if !focused {
                    editor.set_preedit("");
                }
                if !editor.needs_content(focused) {
                    continue;
                }
                editor.content(focused)
            } else if let Some(range) = &self.controls[i].template.range {
                if self.range_values[i] == Some(range.value) {
                    continue;
                }
                self.range_values[i] = Some(range.value);
                range.content()
            } else {
                continue;
            };
            let control = &mut self.controls[i];
            if content != control.template.content {
                Arc::make_mut(&mut control.template).content = content;
                *control.raster_cache.borrow_mut() = None;
                *control.display_cache.borrow_mut() = None;
                *self.geometry.borrow_mut() = None;
                self.geometry_revision = self.geometry_revision.wrapping_add(1);
                control.visual_revision = control.visual_revision.wrapping_add(1);
                self.revision = self.revision.wrapping_add(1);
            }
        }
    }

    fn active(&self) -> usize {
        self.last_event.or(self.focused).unwrap_or(0)
    }

    fn viewport_rect(&self) -> [f32; 4] {
        let p = self.scene.padding;
        if self.scene.scroll {
            [
                p[3],
                p[0],
                (self.width() - p[1] - p[3]).max(0.),
                (self.height() - p[0] - p[2]).max(0.),
            ]
        } else {
            [0., 0., self.width(), self.height()]
        }
    }

    fn set_focus(&mut self, index: Option<usize>) {
        self.focused = index.filter(|i| self.enabled(*i));
        for (i, c) in self.controls.iter_mut().enumerate() {
            c.focus(Some(i) == self.focused);
        }
        if let Some(i) = self.focused {
            self.reveal_in_scroll_areas(i);
        }
        self.refresh_editors();
    }

    fn update_revision(&mut self, previous: u32) {
        if self
            .controls
            .iter()
            .map(Button::visual_revision)
            .fold(0, u32::wrapping_add)
            != previous
        {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn child_revision(&self) -> u32 {
        self.controls
            .iter()
            .map(Button::visual_revision)
            .fold(0, u32::wrapping_add)
    }

    /// One control without layers or canvas geometry uses the control's own
    /// caches directly.
    pub(super) fn single_leaf(&self) -> bool {
        self.controls.len() == 1
            && self.scene.modals.is_empty()
            && self.canvas_scene_rect(0).is_none()
            && self.clip(0).is_none()
    }

    fn content_size(&self) -> [f32; 2] {
        crate::scroll_content_size(&self.scene)
    }
}
impl crate::form_document::WidgetState for Runtime {
    fn control_count(&self) -> usize {
        Runtime::control_count(self)
    }
    fn control_key(&self, index: usize) -> String {
        Runtime::control_key(self, index)
    }
    fn binding_widget_changed(&self, index: usize) -> bool {
        Runtime::binding_widget_changed(self, index)
    }
    fn control_editable(&self, index: usize) -> bool {
        Runtime::control_editable(self, index)
    }
    fn text_value(&self, index: usize) -> String {
        Runtime::text_value(self, index)
    }
    fn range_value(&self, index: usize) -> f32 {
        Runtime::range_value(self, index)
    }
    fn acknowledge_binding_widgets(&self) {
        Runtime::acknowledge_binding_widgets(self);
    }
}
impl RenderScene for Runtime {
    fn release_cpu_cache(&self) {
        Runtime::release_cpu_cache(self);
    }
    fn gpu_scroll(&self) -> [f32; 2] {
        self.scroll
    }
    fn vector_snapshot(&self, s: f32, b: bool) -> Arc<DisplayList> {
        Runtime::vector_snapshot(self, s, b)
    }
    fn gpu_params(&self, w: u32, h: u32, s: f32, o: bool) -> Vec<f32> {
        Runtime::gpu_params(self, w, h, s, o)
    }
    fn gpu_paints(&self) -> Vec<f32> {
        Runtime::gpu_paints(self)
    }
    fn gpu_paints_into(&self, out: &mut Vec<f32>) {
        out.clear();
        let reveal_count = self
            .controls
            .iter()
            .filter(|c| c.template.reveal.is_some())
            .count();
        out.reserve(self.controls.len() * 8 + reveal_count * 8);
        for c in &self.controls {
            out.extend(
                c.fill
                    .color()
                    .into_iter()
                    .chain(c.border.color())
                    .map(|v| v as f32 / 255.),
            );
        }
        for c in &self.controls {
            if c.template.reveal.is_some() {
                out.extend(c.reveal_paint().gpu());
            }
        }
    }
    fn gpu_params_into(&self, w: u32, h: u32, s: f32, o: bool, out: &mut Vec<f32>) {
        out.clear();
        out.extend_from_slice(&[
            w as f32,
            h as f32,
            s,
            if o { 1. } else { 0. },
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            self.scroll[0],
            self.scroll[1],
            0.,
            0.,
        ]);
    }
}

#[cfg(test)]
mod control_role_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod raster_tests;
