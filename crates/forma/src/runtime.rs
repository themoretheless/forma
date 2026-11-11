//! Shared native/WASM scene runtime. Node IDs are local to one loaded snapshot.
//! Controls own interaction; the renderer sees only geometry and paint slots.
use crate::canvas::CanvasEventKind;
use crate::display_list::{DisplayList, ListHints, RenderScene, STRIDE};
use crate::{Button, geometry::round_inside, markup};
use std::{cell::RefCell, sync::Arc};
use wasm_bindgen::prelude::*;

mod a11y;
mod canvas;
mod modal;
mod raster;
mod wasm_api;

pub use a11y::{A11yNode, A11yRole, A11yTree};
pub use canvas::MAX_CANVAS_EVENTS;
use canvas::{CanvasEntry, CanvasInput};
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

    pub fn from_source(source: &str) -> Result<Self, String> {
        Self::from_sources(source, crate::BUTTON_COMPONENT)
    }

    /// Compiles `.ui` and `.svg` files directly in Rust via [`crate::form_document::Document::from_ui_files`]
    /// and instantiates an interactive [`Runtime`].
    pub fn from_ui_files(
        files: &std::collections::BTreeMap<String, String>,
        entry_file: &str,
        initial_state: &crate::form_document::Properties,
    ) -> Result<Self, String> {
        let doc = crate::form_document::Document::from_ui_files(files, entry_file, initial_state)?;
        let (scene_src, templates, controls, logical_end) = doc.render_scene_with_extents()?;
        let mut rt = Self::from_sources(&scene_src, &templates)?;
        rt.apply_logical_content_end(logical_end);
        rt.ui_binding_scene = controls;
        rt.ui_binding_states = doc.states.clone();
        rt.ui_document = Some(doc);
        Ok(rt)
    }

    /// Select owned Svet scheduling for retained native document updates.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_svet_document_scheduling(&mut self, enabled: bool) {
        if enabled && self.svet_document.is_none() {
            let states = self
                .ui_document
                .as_ref()
                .map_or(&[][..], |doc| doc.states.as_slice());
            self.svet_document = Some(crate::native_document_graph::DocumentGraph::new(states));
        } else if !enabled {
            self.svet_document = None;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn svet_document_telemetry(&self) -> Option<svet_wasm::native::Telemetry> {
        self.svet_document.as_ref().map(|graph| graph.telemetry())
    }

    /// Synchronizes two-way `<->` bindings (`value`, `checked`, `selected`) from the live runtime
    /// back into the retained Rust `.ui` `Document`, re-rendering the scene while preserving focus
    /// and active text/slider interaction. Returns `Ok(true)` when the scene is rebuilt,
    /// including a successful retry of an earlier failed render.
    pub fn sync_ui_bindings_native(&mut self) -> Result<bool, String> {
        if self.ui_activations.overflow {
            return Err("Pending UI activation limit exceeded; cancel pending activations or reload the document".into());
        }
        let Some(mut doc) = self.ui_document.take() else {
            return Ok(false);
        };
        let activated = self.last_event.take();
        let model_changed = doc.states != self.ui_binding_states;
        if self.ui_binding_idle_ready
            && !model_changed
            && !self.ui_render_pending
            && activated.is_none()
            && self.ui_activations.events.is_empty()
            && (0..self.control_count()).all(|index| !self.binding_widget_changed(index))
        {
            self.ui_document = Some(doc);
            return Ok(false);
        }
        // First synchronization, rebuilds and failed attempts must still validate
        // binding targets and initialize missing model fields from live widgets.
        self.ui_binding_idle_ready = false;
        let controls = if !model_changed {
            std::borrow::Cow::Borrowed(self.ui_binding_scene.as_slice())
        } else {
            match doc.binding_scene_controls() {
                Ok(controls) => std::borrow::Cow::Owned(controls),
                Err(error) => {
                    self.ui_render_pending = true;
                    self.last_event = activated;
                    self.ui_document = Some(doc);
                    return Err(error);
                }
            }
        };
        let changed =
            match doc.sync_runtime_binding_events(self, &self.ui_activations.events, &controls) {
                Ok(c) => c,
                Err(e) => {
                    // Binding writes are atomic; an uncommitted activation must
                    // remain available for retry after the metadata is repaired.
                    self.last_event = activated;
                    self.ui_document = Some(doc);
                    return Err(e);
                }
            };
        self.ui_activations.clear();
        #[cfg(not(target_arch = "wasm32"))]
        let needs_render = self
            .svet_document
            .as_mut()
            .map_or(changed > 0, |graph| graph.reconcile(&doc.states))
            || model_changed
            || self.ui_render_pending;
        #[cfg(target_arch = "wasm32")]
        let needs_render = changed > 0 || model_changed || self.ui_render_pending;
        self.ui_render_pending = needs_render;
        if needs_render {
            let (scene_src, templates, controls, logical_end) =
                match doc.render_scene_with_extents() {
                    Ok(res) => res,
                    Err(e) => {
                        self.ui_document = Some(doc);
                        return Err(e);
                    }
                };
            let mut next = match Self::from_sources(&scene_src, &templates) {
                Ok(rt) => rt,
                Err(e) => {
                    self.ui_document = Some(doc);
                    return Err(e);
                }
            };
            next.apply_logical_content_end(logical_end);
            next.preserve_binding_interaction(self);
            next.ui_binding_scene = controls;
            next.ui_binding_states = doc.states.clone();
            next.ui_document = Some(doc);
            #[cfg(not(target_arch = "wasm32"))]
            {
                if let Some(graph) = &mut self.svet_document {
                    graph.complete();
                }
                next.svet_document = self.svet_document.take();
            }
            *self = next;
            Ok(true)
        } else {
            self.ui_binding_idle_ready = true;
            self.ui_document = Some(doc);
            Ok(false)
        }
    }

    /// Replaces root state fields of the retained `.ui` document with application values, then
    /// synchronizes like [`Self::sync_ui_bindings_native`]: one rebuild at most, with focus, text,
    /// undo history and scroll offset kept. Fields that are not named keep their value, so state
    /// owned by the screen itself survives. Text the user typed since the last synchronization
    /// still wins for the field it is bound to. Returns `Ok(false)` when nothing was rebuilt.
    /// A refused push changes nothing: state, pending input and the scene stay as they were.
    pub fn set_ui_state_properties(
        &mut self,
        state: &crate::form_document::Properties,
    ) -> Result<bool, String> {
        self.push_ui_state(crate::form_document::StatePush::fields(state.clone()))
    }

    /// [`Self::set_ui_state_properties`] for a JSON object, converted like the initial state of
    /// [`Self::load_ui_files_json_native`].
    pub fn set_ui_state_json_native(&mut self, state_json: &str) -> Result<bool, String> {
        self.push_ui_state(crate::form_document::StatePush::from_json(
            state_json, false,
        )?)
    }

    /// Sets individual state paths from a JSON object such as
    /// `{"user.name":"Ada","items.2.done":true}`, with the guarantees of
    /// [`Self::set_ui_state_properties`]. A path steps through objects by field name and through
    /// lists by index; missing objects are created, a missing list item is an error.
    pub fn patch_ui_state_json_native(&mut self, patch_json: &str) -> Result<bool, String> {
        self.push_ui_state(crate::form_document::StatePush::from_json(
            patch_json, true,
        )?)
    }

    fn push_ui_state(&mut self, push: crate::form_document::StatePush) -> Result<bool, String> {
        let Some(doc) = self.ui_document.as_mut() else {
            return Err("UI state requires a document loaded from .ui files".into());
        };
        if doc.states.is_empty() {
            doc.states.push(Default::default());
        }
        let mut pushed = doc.states[0].clone();
        push.apply(&mut pushed)?;
        // Synchronization commits input before it renders. Everything it may
        // consume is kept, so a scene that refuses the push loses nothing.
        let state = std::mem::replace(&mut doc.states[0], pushed);
        let baseline = self.binding_widget_baseline.borrow().clone();
        let (events, last_event) = (self.ui_activations.events.clone(), self.last_event);
        let (pending, idle) = (self.ui_render_pending, self.ui_binding_idle_ready);
        let result = self.sync_ui_bindings_native();
        if result.is_err() {
            // A failed synchronization never replaces this Runtime.
            if let Some(doc) = self.ui_document.as_mut() {
                doc.states[0] = state;
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(graph) = &mut self.svet_document {
                    graph.reconcile(&doc.states);
                    if !pending {
                        graph.complete();
                    }
                }
            }
            *self.binding_widget_baseline.borrow_mut() = baseline;
            self.ui_activations.events = events;
            self.last_event = last_event;
            self.ui_render_pending = pending;
            self.ui_binding_idle_ready = idle;
        }
        result
    }

    /// Returns the retained `.ui` `Document` state map (if loaded via `from_ui_files`), or empty map.
    pub fn ui_state_properties(&self) -> crate::form_document::Properties {
        self.ui_document
            .as_ref()
            .and_then(|d| d.states.first().cloned())
            .unwrap_or_default()
    }

    /// Replace component markup atomically without constructing a WASM error.
    pub fn load_component_native(&mut self, source: &str, component: &str) -> Result<(), String> {
        let mut next = Self::from_sources(source, component)?;
        next.revision = self.revision.wrapping_add(1);
        next.geometry_revision = self.geometry_revision.wrapping_add(1);
        next.layout_revision = self.layout_revision.wrapping_add(1);
        *self = next;
        Ok(())
    }
    /// Load markup using the built-in Button template with native Rust errors.
    pub fn load_native(&mut self, source: &str) -> Result<(), String> {
        self.load_component_native(source, crate::BUTTON_COMPONENT)
    }
    /// Compile a UI project to a portable scene artifact with native Rust errors.
    pub fn compile_ui_scene_artifact_native(
        files_json: &str,
        entry_file: &str,
        state_json: &str,
    ) -> Result<String, String> {
        let doc =
            crate::form_document::Document::from_ui_files_json(files_json, entry_file, state_json)?;
        doc.render_scene_artifact()?.to_json()
    }

    /// Reload a UI project atomically with native Rust errors.
    /// A rejected project leaves the live scene, document and scheduler untouched.
    pub fn load_ui_files_json_native(
        &mut self,
        files_json: &str,
        entry_file: &str,
        state_json: &str,
    ) -> Result<(), String> {
        let doc =
            crate::form_document::Document::from_ui_files_json(files_json, entry_file, state_json)?;
        let (source, templates, controls, logical_end) = doc.render_scene_with_extents()?;
        let mut next = Self::from_sources(&source, &templates)?;
        next.apply_logical_content_end(logical_end);
        next.ui_binding_scene = controls;
        next.ui_binding_states = doc.states.clone();
        next.ui_document = Some(doc);
        #[cfg(not(target_arch = "wasm32"))]
        next.set_svet_document_scheduling(self.svet_document.is_some());
        next.revision = self.revision.wrapping_add(1);
        next.geometry_revision = self.geometry_revision.wrapping_add(1);
        next.layout_revision = self.layout_revision.wrapping_add(1);
        *self = next;
        Ok(())
    }

    /// Atomically replace the scene from versioned JSON; a refusal preserves this Runtime.
    pub fn load_scene_artifact_native(&mut self, json: &str) -> Result<(), String> {
        let artifact = crate::form_document::SceneArtifact::from_json(json)?;
        let mut next = Self::from_scene_artifact(&artifact)?;
        next.revision = self.revision.wrapping_add(1);
        next.geometry_revision = self.geometry_revision.wrapping_add(1);
        next.layout_revision = self.layout_revision.wrapping_add(1);
        *self = next;
        Ok(())
    }
    /// Consume the complete Rust Document transport, including non-control layout extents.
    pub fn from_scene_artifact(
        artifact: &crate::form_document::SceneArtifact,
    ) -> Result<Self, String> {
        if artifact
            .logical_content_end
            .is_some_and(|end| end.iter().any(|v| !v.is_finite() || *v < 0.))
        {
            return Err("Invalid logical content extent".into());
        }
        let mut runtime = Self::from_sources(&artifact.source, &artifact.templates)?;
        if artifact.controls.len() != runtime.controls.len() {
            return Err("Scene control mapping count does not match rendered controls".into());
        }
        for (mapping, control) in artifact.controls.iter().zip(&runtime.controls) {
            if mapping.key != control.scene.button.key {
                return Err(
                    "Scene control mapping key/order does not match rendered controls".into(),
                );
            }
        }
        if artifact.logical_content_end.is_some() && !runtime.scene.scroll {
            return Err("Logical content extent requires a Scroll scene".into());
        }
        runtime.apply_logical_content_end(artifact.logical_content_end);
        Ok(runtime)
    }
    pub fn from_sources(source: &str, component: &str) -> Result<Self, String> {
        let mut scene = markup::parse(source)?;
        let templates = templates(component, scene.buttons.len())?;
        let mut controls = Vec::with_capacity(scene.buttons.len());
        let modal_of: Vec<Option<usize>> = (0..scene.buttons.len())
            .map(|i| scene.modals.iter().position(|m| m.controls.contains(&i)))
            .collect();
        let mut y = scene.padding[0];
        for (index, (spec, component)) in scene.buttons.iter().zip(templates).enumerate() {
            if let Some(m) = modal_of[index] {
                if scene.modals[m].controls.start == index {
                    y = scene.modals[m].y;
                }
            }
            // Each leaf needs frame metadata and its own spec, not a temporary
            // deep clone of every sibling (quadratic in the control count).
            let leaf = markup::Scene {
                // The frame name and its legacy spelling of clipping are never read on a
                // control: clipping reaches the renderer as `clip`, so the leaf leaves both
                // strings empty rather than copying the document's.
                name: String::new(),
                width: scene.width,
                height: scene.height,
                background: scene.background,
                overflow: String::new(),
                clip: scene.clip,
                radius: scene.radius,
                scroll: scene.scroll,
                padding: scene.padding,
                content_width: scene.content_width,
                content_height: scene.content_height,
                gap: scene.gap,
                // `from_scene` replaces the spec with the resolved one, so the leaf
                // carries the document spec by reference instead of copying it.
                button: markup::ButtonSpec::default(),
                buttons: Vec::new(),
                modals: Vec::new(),
            };
            let mut control = Button::from_scene(leaf, spec, component)?;
            // Defaults in the component can change height: lay out after linking.
            if !spec.specified.contains("y") {
                control.scene.button.y = y;
            }
            y = control.scene.button.y + control.scene.button.height + scene.gap;
            controls.push(control);
        }
        let base = modal_of.iter().filter(|m| m.is_none()).count();
        scene.content_width = controls[..base]
            .iter()
            .map(|b| b.scene.button.x + b.scene.button.width + scene.padding[1])
            .fold(scene.width, f32::max);
        scene.content_height = controls[..base]
            .iter()
            .map(|b| b.scene.button.y + b.scene.button.height + scene.padding[2])
            .fold(scene.height, f32::max);
        let mut nodes =
            Vec::with_capacity(1 + usize::from(scene.scroll) + scene.modals.len() + controls.len());
        let root_children = if scene.scroll {
            1 + scene.modals.len()
        } else {
            base + scene.modals.len()
        };
        nodes.push(Node {
            id: 0,
            parent: None,
            children: Vec::with_capacity(root_children),
            kind: NodeKind::Frame,
            control: None,
        });
        let parent = if scene.scroll {
            nodes[0].children.push(1);
            nodes.push(Node {
                id: 1,
                parent: Some(0),
                children: Vec::with_capacity(base),
                kind: NodeKind::Scroll,
                control: None,
            });
            1
        } else {
            0
        };
        let mut modal_nodes = Vec::with_capacity(scene.modals.len());
        for (index, b) in controls.iter_mut().enumerate() {
            if scene.buttons.len() > 1 || !scene.modals.is_empty() {
                b.scene.clip = false;
                b.scene.scroll = false;
            }
            let parent = match modal_of[index] {
                None => parent,
                Some(m) => {
                    if modal_nodes.len() <= m {
                        let id = nodes.len();
                        nodes[0].children.push(id);
                        nodes.push(Node {
                            id,
                            parent: Some(0),
                            children: Vec::with_capacity(scene.modals[m].controls.len()),
                            kind: NodeKind::Modal,
                            control: None,
                        });
                        modal_nodes.push(id);
                    }
                    modal_nodes[m]
                }
            };
            let id = nodes.len();
            nodes[parent].children.push(id);
            nodes.push(Node {
                id,
                parent: Some(parent),
                children: vec![],
                kind: NodeKind::Control,
                control: Some(index),
            });
        }
        // Empty layers still exist as nodes so hosts can open and close them.
        for _ in modal_nodes.len()..scene.modals.len() {
            let id = nodes.len();
            nodes[0].children.push(id);
            nodes.push(Node {
                id,
                parent: Some(0),
                children: vec![],
                kind: NodeKind::Modal,
                control: None,
            });
            modal_nodes.push(id);
        }
        let editors = controls
            .iter()
            .map(|c| {
                c.template
                    .inputs
                    .first()
                    .map(|input| {
                        crate::text_input::Editor::new(input.clone(), c.template.content.clone())
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut runtime = Self {
            range_values: vec![None; controls.len()],
            binding_widget_baseline: RefCell::new(Default::default()),
            editors,
            modal_open: scene.modals.iter().map(|m| m.open).collect(),
            modal_restore: vec![None; scene.modals.len()],
            modal_of,
            dismissals: 0,
            last_dismissed: None,
            submissions: vec![0; controls.len()],
            scene,
            controls,
            nodes,
            focused: None,
            captured: None,
            last_event: None,
            last_submitted: None,
            ui_activations: UiActivations::default(),
            scroll: [0.; 2],
            canvases: Default::default(),
            canvas_input: CanvasInput::default(),
            #[cfg(not(target_arch = "wasm32"))]
            native_accessibility_id: None,
            #[cfg(not(target_arch = "wasm32"))]
            native_instance_id: {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            revision: 0,
            geometry_revision: 0,
            layout_revision: 0,
            geometry: RefCell::new(None),
            raster_cache: RefCell::new(None),
            ui_document: None,
            ui_render_pending: false,
            ui_binding_idle_ready: false,
            ui_binding_scene: Vec::new(),
            ui_binding_states: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            svet_document: None,
        };
        runtime.refresh_editors();
        runtime.acknowledge_binding_widgets();
        Ok(runtime)
    }
    pub(crate) fn binding_widget_changed(&self, index: usize) -> bool {
        let baseline = self.binding_widget_baseline.borrow();
        match baseline.get(&index) {
            Some(BindingWidgetValue::Text(value)) => self
                .editors
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|editor| editor.model.value() != value),
            Some(BindingWidgetValue::Range(value)) => self.range_value(index) != *value,
            None => false,
        }
    }
    pub(crate) fn acknowledge_binding_widgets(&self) {
        let mut baseline = self.binding_widget_baseline.borrow_mut();
        for (index, control) in self.controls.iter().enumerate() {
            if let Some(editor) = &self.editors[index] {
                match baseline.get_mut(&index) {
                    Some(BindingWidgetValue::Text(value)) => {
                        if value != editor.model.value() {
                            value.clear();
                            value.push_str(editor.model.value());
                        }
                    }
                    _ => {
                        baseline.insert(
                            index,
                            BindingWidgetValue::Text(editor.model.value().to_owned()),
                        );
                    }
                }
            } else if let Some(range) = &control.template.range {
                match baseline.get_mut(&index) {
                    Some(BindingWidgetValue::Range(value)) => *value = range.value,
                    _ => {
                        baseline.insert(index, BindingWidgetValue::Range(range.value));
                    }
                }
            }
        }
    }
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }
    /// Re-evaluation keeps gestures attached to the same uniquely keyed control.
    /// Context replacement is cancelled separately by the binding owner.
    pub(crate) fn preserve_binding_interaction(&mut self, previous: &Runtime) {
        let matches = self.preserve_interaction_matches(previous);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let same_structure = self.nodes.len() == previous.nodes.len()
                && self.nodes.iter().zip(&previous.nodes).all(|(a, b)| {
                    a.parent == b.parent && a.kind == b.kind && a.control == b.control
                });
            let mut keys = std::collections::BTreeSet::new();
            let same_controls = self.controls.len() == previous.controls.len()
                && matches.iter().enumerate().all(|(i, old)| {
                    let key = self.control_key(i);
                    !key.is_empty() && keys.insert(key) && *old == Some(i)
                });
            if same_structure && same_controls {
                self.native_accessibility_id = Some(previous.native_scene_identity());
            }
        }
        for (i, c) in self.controls.iter_mut().enumerate() {
            let Some(old_index) = matches[i] else {
                continue;
            };
            let old = &previous.controls[old_index];
            if !c.disabled() && c.template.clickable {
                c.down = old.down;
                c.keyboard = old.keyboard;
                c.update_colors();
                if previous.captured == Some(old_index) {
                    self.captured = Some(i);
                }
            }
        }
        let layout_changed = self.width() != previous.width()
            || self.height() != previous.height()
            || self.scene.scroll != previous.scene.scroll
            || self.viewport_rect() != previous.viewport_rect()
            || self.content_size() != previous.content_size()
            || self.controls.len() != previous.controls.len()
            || self
                .controls
                .iter()
                .zip(&previous.controls)
                .any(|(a, b)| a.bounds() != b.bounds());
        self.revision = previous.revision.wrapping_add(1);
        self.geometry_revision = previous.geometry_revision.wrapping_add(1);
        self.layout_revision = previous
            .layout_revision
            .wrapping_add(u32::from(layout_changed));
    }
    /// Apply model text without creating an undo entry. Replacing a binding or
    /// context resets editing history even when the two records contain equal text.
    pub fn set_binding_value(
        &mut self,
        index: usize,
        value: &str,
        reset: bool,
    ) -> Result<(), String> {
        let editor = self
            .editors
            .get_mut(index)
            .and_then(Option::as_mut)
            .ok_or_else(|| format!("Control {index} has no editable value"))?;
        if !reset && editor.model.value() == value {
            return Ok(());
        }
        let mut spec = editor.spec.clone();
        spec.value = value.into();
        let mut next = crate::text_input::Editor::new(spec, editor.base.clone())?;
        next.reduced_motion(self.controls[index].reduced_motion);
        if !reset {
            let mut caret = editor.model.caret().min(value.len());
            while !value.is_char_boundary(caret) {
                caret -= 1;
            }
            next.model
                .set_selection(caret, caret)
                .map_err(|e| e.to_string())?;
            let caret = next.model.grapheme_selection().start;
            next.model
                .set_selection(caret, caret)
                .map_err(|e| e.to_string())?;
        }
        *editor = next;
        self.refresh_editors();
        Ok(())
    }
    /// Text bindings target a single text primitive; ambiguous rich content is
    /// rejected instead of silently replacing captions or icons as well.
    pub fn set_binding_text(&mut self, index: usize, value: &str) -> Result<(), String> {
        let control = self
            .controls
            .get_mut(index)
            .ok_or_else(|| format!("Unknown control {index}"))?;
        let count = usize::from(control.template.text.is_some())
            + control
                .template
                .content
                .iter()
                .filter(|c| matches!(c, crate::template::Content::Text { .. }))
                .count();
        if count != 1 || self.editors[index].is_some() {
            return Err(format!(
                "Control {index}: text binding requires exactly one non-editable text primitive"
            ));
        }
        // The template is shared with the parse cache, so the first write takes a copy.
        // Only the edited control pays, and only once per keystroke it changes.
        let template = Arc::make_mut(&mut control.template);
        let text = if let Some(text) = &mut template.text {
            text
        } else {
            template
                .content
                .iter_mut()
                .find_map(|c| match c {
                    crate::template::Content::Text { text, .. } => Some(text),
                    _ => None,
                })
                .unwrap()
        };
        if text.text == value {
            return Ok(());
        }
        text.text = value.into();
        control.scene.button.text = value.into();
        *control.raster_cache.borrow_mut() = None;
        *control.display_cache.borrow_mut() = None;
        control.visual_revision = control.visual_revision.wrapping_add(1);
        *self.geometry.borrow_mut() = None;
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn set_binding_disabled(&mut self, index: usize, disabled: bool) -> Result<(), String> {
        let control = self
            .controls
            .get_mut(index)
            .ok_or_else(|| format!("Unknown control {index}"))?;
        if control.scene.button.disabled == disabled {
            return Ok(());
        }
        control.scene.button.disabled = disabled;
        Arc::make_mut(&mut control.template).props.disabled = disabled;
        if disabled {
            control.down = false;
            control.keyboard = None;
        }
        control.update_colors();
        if disabled && self.captured == Some(index) {
            self.captured = None;
        }
        if disabled && self.focused == Some(index) {
            self.set_focus(None);
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub(crate) fn cancel_binding_interaction(&mut self, index: usize) {
        let before = self.child_revision();
        if let Some(control) = self.controls.get_mut(index) {
            control.down = false;
            control.keyboard = None;
            control.update_colors();
        }
        if self.captured == Some(index) {
            self.captured = None;
        }
        if let Some(Some(editor)) = self.editors.get_mut(index) {
            editor.set_preedit("");
        }
        self.refresh_editors();
        self.update_revision(before);
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
    }
    pub fn vector_snapshot(&self, scale: f32, background: bool) -> Arc<DisplayList> {
        if self.single_leaf() {
            return self.controls[0].vector_snapshot(scale, background);
        }
        let mut cache = self.geometry.borrow_mut();
        if let Some(c) = cache.as_ref() {
            if c.scale == scale && c.background == background {
                return Arc::clone(&c.list);
            }
        }
        // Every control builds its own list before the merge runs. The scene repeats nearly
        // identical controls, so each one after the first can start at a sibling's final
        // capacity instead of growing into it by doubling, and the merged buffers are exact.
        let mut children = Vec::with_capacity(self.controls.len());
        let mut text_scratch = crate::text::VectorScratch::prewarm();
        let mut hints = None;
        for control in &self.controls {
            let child = control.vector_snapshot_hinted(scale, false, hints, &mut text_scratch);
            hints = Some(ListHints {
                commands: child.commands.len(),
                edges: child.edges.len(),
            });
            children.push(child);
        }
        let mut list = DisplayList::with_hints(Some(ListHints {
            commands: children
                .iter()
                .map(|child| child.commands.len())
                .sum::<usize>()
                + STRIDE * 8,
            edges: children
                .iter()
                .map(|child| child.edges.len())
                .sum::<usize>(),
        }));
        let frame = [0., 0., self.width(), self.height()];
        if self.scene.clip {
            list.push(frame, self.scene.radius);
        }
        if background {
            list.command(
                0.,
                frame,
                self.scene.background,
                if self.scene.clip {
                    0.
                } else {
                    self.scene.radius
                },
                0.,
            );
        }
        let v = self.viewport_rect();
        if self.scene.scroll {
            list.command(2., [v[0], v[1], v[2], v[3]], [0; 4], 0., 1.);
        }
        // Children are built in content coordinates; the scroll offset is a
        // renderer uniform, so this list survives scrolling unchanged.
        let content_start = list.commands.len();
        // Paint slots are indexed by control, not by emission order: reveal
        // slots follow all fill/border pairs in control order.
        let mut reveal_slots = Vec::with_capacity(self.controls.len());
        let mut reveal_slot = self.controls.len() * 2;
        for c in &self.controls {
            reveal_slots.push(reveal_slot);
            if c.template.reveal.is_some() {
                reveal_slot += 2;
            }
        }
        let canvas_lists: Vec<Option<([f32; 4], Arc<DisplayList>)>> = (0..self.controls.len())
            .map(|i| Some((self.canvas_scene_rect(i)?, self.canvas_placed(i)?)))
            .collect();
        let emit = |list: &mut DisplayList, index: usize| {
            let child = &children[index];
            let edge_offset = list.edges.len() / 4;
            for command in child.commands.chunks_exact(STRIDE) {
                let start = list.commands.len();
                list.commands.extend(command);
                let cmd = &mut list.commands[start..];
                if cmd[0] == 1. {
                    cmd[2] += edge_offset as f32;
                }
                if cmd[0] == 4. {
                    cmd[14] = (index * 2) as f32;
                    cmd[15] = (index * 2 + 1) as f32;
                }
                if cmd[0] == 6. {
                    cmd[14] = reveal_slots[index] as f32;
                }
            }
            list.edges.extend(&child.edges);
            // Application geometry follows its control, clipped to the canvas.
            if let Some((rect, placed)) = &canvas_lists[index] {
                let edge_offset = (list.edges.len() / 4) as f32;
                list.push(*rect, 0.);
                let start = list.commands.len();
                list.commands.extend(&placed.commands);
                for cmd in list.commands[start..].chunks_exact_mut(STRIDE) {
                    cmd[2] += edge_offset;
                }
                list.edges.extend(&placed.edges);
                list.pop();
            }
        };
        for index in 0..self.controls.len() {
            if self.modal_of[index].is_none() {
                emit(&mut list, index);
            }
        }
        if self.scene.scroll {
            list.mark_scrolled(content_start);
            list.scrollbars(v, self.content_size());
            list.pop();
        }
        for (m, modal) in self.scene.modals.iter().enumerate() {
            if !self.modal_open[m] {
                continue;
            }
            list.command(
                0.,
                frame,
                modal.backdrop,
                if self.scene.clip {
                    0.
                } else {
                    self.scene.radius
                },
                0.,
            );
            for index in modal.controls.clone() {
                emit(&mut list, index);
            }
        }
        if self.scene.clip {
            list.pop();
        }
        let list = Arc::new(list);
        *cache = Some(GeometryCache {
            scale,
            background,
            list: Arc::clone(&list),
        });
        list
    }
    fn apply_logical_content_end(&mut self, end: Option<[f64; 2]>) {
        if let Some([right, bottom]) = end {
            self.scene.content_width = self
                .scene
                .content_width
                .max((right + f64::from(self.scene.padding[1])).min(f64::from(f32::MAX)) as f32);
            self.scene.content_height = self
                .scene
                .content_height
                .max((bottom + f64::from(self.scene.padding[2])).min(f64::from(f32::MAX)) as f32);
        }
    }
    fn content_size(&self) -> [f32; 2] {
        [
            self.scene.content_width - self.scene.padding[1] - self.scene.padding[3],
            self.scene.content_height - self.scene.padding[0] - self.scene.padding[2],
        ]
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
mod tests;

#[cfg(test)]
mod raster_tests;
