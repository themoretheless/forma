//! Shared native/WASM scene runtime. Node IDs are local to one loaded snapshot.
//! Controls own interaction; the renderer sees only geometry and paint slots.
use crate::display_list::{DisplayList, ListHints, RenderScene, STRIDE};
use crate::{
    Button,
    geometry::{round_coverage, round_inside},
    markup,
};
use std::{cell::RefCell, sync::Arc};
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Frame,
    Scroll,
    Modal,
    Control,
}

/// AccessKit-compatible semantic role for a node in the accessibility tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum A11yRole {
    Window,
    ScrollView,
    Dialog,
    Button,
    TextInput,
    Slider,
    StaticText,
}

impl A11yRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Window => "Window",
            Self::ScrollView => "ScrollView",
            Self::Dialog => "Dialog",
            Self::Button => "Button",
            Self::TextInput => "TextInput",
            Self::Slider => "Slider",
            Self::StaticText => "StaticText",
        }
    }
}

/// AccessKit-compatible semantic accessibility node exported by [`Runtime::accessibility_tree`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A11yNode {
    pub id: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub role: A11yRole,
    pub key: String,
    pub label: String,
    pub value: Option<String>,
    #[serde(skip)]
    pub numeric_value: Option<f32>,
    #[serde(serialize_with = "serialize_a11y_bounds")]
    pub bounds: [f32; 4],
    pub focused: bool,
    pub disabled: bool,
    pub read_only: bool,
    pub multiline: bool,
    /// Anchor/caret UTF-16 offsets in this node's original value (CRLF retained).
    pub selection_utf16: Option<[u32; 2]>,
    pub modal_blocked: bool,
    pub actions: Vec<&'static str>,
}

/// Snapshot of the scene's semantic accessibility tree and live region state.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A11yTree {
    pub root_id: usize,
    pub focused_id: Option<usize>,
    pub active_modal_id: Option<usize>,
    pub nodes: Vec<A11yNode>,
}
// Preserve the existing JSON numeric values, formerly materialized through Value.
fn serialize_a11y_bounds<S: serde::Serializer>(
    bounds: &[f32; 4],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&bounds.map(f64::from), serializer)
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

#[derive(Clone, Copy, PartialEq)]
struct RasterKey {
    width: u32,
    height: u32,
    scale: f32,
    scroll: [f32; 2],
}
struct RasterCache {
    key: RasterKey,
    // Low five bits contain 0..16 frame coverage; high bits contain the
    // scroll viewport/scrollbar classification, shared by all controls.
    frame: Vec<u8>,
    pixels: Vec<u8>,
    native_pixels: Vec<u32>,
    paint: Option<(u32, bool)>,
}

#[derive(PartialEq)]
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

fn templates<'a>(source: &'a str, count: usize) -> Result<Vec<&'a str>, String> {
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

    /// Returns the retained `.ui` `Document` state map (if loaded via `from_ui_files`), or empty map.
    pub fn ui_state_properties(&self) -> crate::form_document::Properties {
        self.ui_document
            .as_ref()
            .and_then(|d| d.states.first().cloned())
            .unwrap_or_default()
    }

    /// Builds an AccessKit-compatible semantic accessibility tree for native and Web assistive technologies.
    pub fn accessibility_tree(&self) -> A11yTree {
        let mut a11y_nodes = Vec::with_capacity(self.nodes.len());
        let mut focused_node_id = None;
        let active_modal = self.active_modal_index();
        let mut active_modal_node_id = None;

        for node in &self.nodes {
            match node.kind {
                NodeKind::Frame => {
                    a11y_nodes.push(A11yNode {
                        id: node.id,
                        parent: node.parent,
                        children: node.children.clone(),
                        role: A11yRole::Window,
                        key: self.scene.name.clone(),
                        label: self.scene.name.clone(),
                        value: None,
                        numeric_value: None,
                        bounds: [0.0, 0.0, self.render_width(), self.render_height()],
                        focused: false,
                        disabled: false,
                        multiline: false,
                        selection_utf16: None,
                        read_only: false,
                        modal_blocked: false,
                        actions: Vec::new(),
                    });
                }
                NodeKind::Scroll => {
                    a11y_nodes.push(A11yNode {
                        id: node.id,
                        parent: node.parent,
                        children: node.children.clone(),
                        role: A11yRole::ScrollView,
                        key: "scroll".into(),
                        label: "Scrollable Region".into(),
                        value: Some(format!("{:.0},{:.0}", self.scroll[0], self.scroll[1])),
                        numeric_value: Some(self.scroll[1]),
                        bounds: self.viewport_rect(),
                        focused: false,
                        disabled: false,
                        multiline: false,
                        selection_utf16: None,
                        read_only: false,
                        modal_blocked: active_modal.is_some(),
                        actions: vec!["scroll"],
                    });
                }
                NodeKind::Modal => {
                    let modal_idx = self
                        .scene
                        .modals
                        .iter()
                        .position(|m| {
                            (!m.controls.is_empty()).then_some(m.controls.start)
                                == node
                                    .children
                                    .first()
                                    .and_then(|&cid| self.nodes[cid].control)
                        })
                        .unwrap_or(0);
                    let open = self.modal_open.get(modal_idx).copied().unwrap_or(false);
                    if active_modal == Some(modal_idx) {
                        active_modal_node_id = Some(node.id);
                    }
                    let m_spec = &self.scene.modals[modal_idx];
                    a11y_nodes.push(A11yNode {
                        id: node.id,
                        parent: node.parent,
                        children: if open {
                            node.children.clone()
                        } else {
                            Vec::new()
                        },
                        role: A11yRole::Dialog,
                        key: m_spec.key.clone(),
                        label: m_spec.key.clone(),
                        value: Some(if open { "open" } else { "closed" }.into()),
                        numeric_value: None,
                        bounds: [m_spec.x, m_spec.y, m_spec.width, m_spec.height],
                        focused: false,
                        disabled: !open,
                        multiline: false,
                        selection_utf16: None,
                        read_only: false,
                        modal_blocked: active_modal.is_some() && active_modal != Some(modal_idx),
                        actions: if open && m_spec.dismiss {
                            vec!["dismiss"]
                        } else {
                            Vec::new()
                        },
                    });
                }
                NodeKind::Control => {
                    let idx = node.control.unwrap_or(0);
                    let b = self.control_bounds(idx);
                    let bounds = [b[0], b[1], b[2], b[3]];
                    let disabled = self.control_disabled(idx);
                    let blocked = self.control_blocked(idx);
                    let focused = self.focused == Some(idx);
                    if focused {
                        focused_node_id = Some(node.id);
                    }
                    let is_ranged = self
                        .controls
                        .get(idx)
                        .is_some_and(|c| c.template.range.is_some());
                    let role = if self.control_editable(idx) {
                        A11yRole::TextInput
                    } else if is_ranged {
                        A11yRole::Slider
                    } else if self.control_interactive(idx) {
                        A11yRole::Button
                    } else {
                        A11yRole::StaticText
                    };
                    let mut label = self.controls[idx].scene.button.text.clone();
                    if label.is_empty() {
                        for item in &self.controls[idx].template.content {
                            if let crate::template::Content::Text { text, .. } = item {
                                if !text.text.is_empty() {
                                    label = text.text.clone();
                                    break;
                                }
                            }
                        }
                    }
                    if self.control_editable(idx) {
                        label = self.control_input_label(idx);
                    }
                    let value = if self.control_editable(idx) {
                        Some(self.text_value(idx))
                    } else if is_ranged {
                        Some(format!("{:.0}%", self.range_value(idx) * 100.0))
                    } else {
                        None
                    };
                    let numeric_value = if is_ranged {
                        Some(self.range_value(idx))
                    } else {
                        None
                    };
                    let mut actions = Vec::new();
                    if !disabled && !blocked {
                        match role {
                            A11yRole::Button => actions.extend(["focus", "click"]),
                            A11yRole::TextInput => {
                                actions.extend(["focus", "set_text_selection"]);
                                if !self.control_readonly(idx) {
                                    actions.push("set_value");
                                }
                            }
                            A11yRole::Slider => {
                                actions.extend(["focus", "increment", "decrement", "set_value"])
                            }
                            _ => {}
                        }
                    }
                    a11y_nodes.push(A11yNode {
                        id: node.id,
                        parent: node.parent,
                        children: Vec::new(),
                        role,
                        key: self.controls[idx].key(),
                        label,
                        value,
                        numeric_value,
                        bounds,
                        focused,
                        disabled,
                        multiline: self.control_multiline(idx),
                        selection_utf16: self.editors[idx].as_ref().map(|editor| {
                            [editor.model.anchor(), editor.model.caret()]
                                .map(|at| editor.model.value()[..at].encode_utf16().count() as u32)
                        }),
                        read_only: self.control_readonly(idx),
                        modal_blocked: blocked,
                        actions,
                    });
                }
            }
        }

        A11yTree {
            root_id: 0,
            focused_id: focused_node_id,
            active_modal_id: active_modal_node_id,
            nodes: a11y_nodes,
        }
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
    /// Topmost open layer, if any. It alone receives pointer and keyboard input.
    fn active_modal_index(&self) -> Option<usize> {
        (0..self.modal_open.len())
            .rev()
            .find(|&m| self.modal_open[m])
    }
    fn visible(&self, index: usize) -> bool {
        self.modal_of[index].is_none_or(|m| self.modal_open[m])
    }
    /// Reachable controls belong to the active layer; everything else is
    /// blocked by the backdrop and neither hit, hovered nor focusable.
    fn reachable(&self, index: usize) -> bool {
        self.modal_of[index] == self.active_modal_index()
    }
    fn enabled(&self, index: usize) -> bool {
        self.reachable(index)
            && !self.controls[index].disabled()
            && self.controls[index].template.clickable
    }
    fn invalidate_layers(&mut self) {
        *self.geometry.borrow_mut() = None;
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.layout_revision = self.layout_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
    }
    fn cancel_gestures(&mut self) {
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
    pub fn vector_snapshot(&self, scale: f32, background: bool) -> Arc<DisplayList> {
        if self.controls.len() == 1 && self.scene.modals.is_empty() {
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
    fn raster_cached(
        &self,
        w: u32,
        h: u32,
        s: f32,
        background: bool,
    ) -> Option<std::cell::RefMut<'_, RasterCache>> {
        if w == 0
            || h == 0
            || w > 4096
            || h > 4096
            || w as u64 * h as u64 > 8_388_608
            || !s.is_finite()
            || s <= 0.
        {
            return None;
        }
        let key = RasterKey {
            width: w,
            height: h,
            scale: s,
            scroll: self.scroll,
        };
        let len = (w * h) as usize;
        let mut stored = self.raster_cache.borrow_mut();
        let cache = stored.get_or_insert_with(|| RasterCache {
            key,
            frame: Vec::new(),
            pixels: Vec::new(),
            native_pixels: Vec::new(),
            paint: None,
        });
        if cache.key != key || cache.frame.len() != len {
            cache.key = key;
            cache.frame.resize(len, 0);
            cache.pixels.resize(len * 4, 0);
            cache.paint = None;
            let v = self.viewport_rect();
            let [cw, ch] = self.content_size();
            let inv_s = 1. / s;
            let sw = self.width();
            let sh = self.height();
            let r = self.scene.radius.min(sw * 0.5).min(sh * 0.5).max(0.);
            let flat_frame = r <= 0. && (sw * s) >= w as f32 && (sh * s) >= h as f32;
            let vert_x0 = (v[0] + v[2] - 5.).max(v[0]);
            let vert_y0 = v[1] + self.scroll[1] / ch * v[3];
            let vert_y1 = v[1] + (self.scroll[1] + v[3]) / ch * v[3];
            let horz_y0 = (v[1] + v[3] - 5.).max(v[1]);
            let horz_x0 = v[0] + self.scroll[0] / cw * v[2];
            let horz_x1 = v[0] + (self.scroll[0] + v[2]) / cw * v[2];
            if flat_frame && !self.scene.scroll {
                cache.frame.fill(16 | (1 << 5));
            } else {
                for y in 0..h {
                    let py = (y as f32 + 0.5) * inv_s;
                    let row = (y * w) as usize;
                    for x in 0..w {
                        let px = (x as f32 + 0.5) * inv_s;
                        let mut viewport = 1;
                        if self.scene.scroll {
                            if px < v[0] || py < v[1] || px >= v[0] + v[2] || py >= v[1] + v[3] {
                                viewport = 0;
                            } else {
                                let vertical =
                                    ch > v[3] && px >= vert_x0 && py >= vert_y0 && py < vert_y1;
                                let horizontal =
                                    cw > v[2] && py >= horz_y0 && px >= horz_x0 && px < horz_x1;
                                if vertical || horizontal {
                                    viewport = 2;
                                }
                            }
                        }
                        let coverage = if flat_frame {
                            1.
                        } else {
                            round_coverage(x, y, s, [0., 0., sw, sh], self.scene.radius)
                        };
                        cache.frame[row + x as usize] = (coverage * 16.) as u8 | viewport << 5;
                    }
                }
            }
        }
        let paint = (self.revision, background);
        if cache.paint != Some(paint) {
            cache.pixels.fill(0);
            for (i, control) in self.controls.iter().enumerate() {
                if self.modal_of[i].is_none() {
                    control.composite_content(&mut cache.pixels, w, h, s);
                }
            }
            if self.scene.scroll {
                for (p, &frame) in cache.pixels.chunks_exact_mut(4).zip(&cache.frame) {
                    match frame >> 5 {
                        0 => p.fill(0),
                        2 => p.copy_from_slice(&[110, 130, 170, 255]),
                        _ => {}
                    }
                }
            }
            // Layers sit above the scroll viewport and its bars; the frame's
            // rounded coverage still trims every layer below.
            for (m, modal) in self.scene.modals.iter().enumerate() {
                if !self.modal_open[m] {
                    continue;
                }
                if modal.backdrop[3] > 0 {
                    for p in cache.pixels.chunks_exact_mut(4) {
                        crate::raster_cache::over(p, &modal.backdrop);
                    }
                }
                for index in modal.controls.clone() {
                    self.controls[index].composite_content(&mut cache.pixels, w, h, s);
                }
            }
            if background || self.scene.clip {
                for (p, &frame) in cache.pixels.chunks_exact_mut(4).zip(&cache.frame) {
                    let cov_int = frame & 31;
                    if background {
                        if cov_int == 16 && p[3] == 255 {
                            // Fully opaque pixel inside unclipped interior: background is completely occluded.
                        } else if cov_int == 16 && p[3] == 0 {
                            p.copy_from_slice(&self.scene.background);
                        } else {
                            let mut bg = self.scene.background;
                            if !self.scene.clip && cov_int < 16 {
                                bg[3] = (bg[3] as f32 * (cov_int as f32 / 16.)).round() as u8;
                            }
                            crate::raster_cache::over(&mut bg, p);
                            p.copy_from_slice(&bg);
                        }
                    }
                    if self.scene.clip && cov_int < 16 {
                        if cov_int == 0 {
                            p.fill(0);
                        } else {
                            p[3] = (p[3] as f32 * (cov_int as f32 / 16.)).round() as u8;
                            if p[3] == 0 {
                                p.fill(0);
                            }
                        }
                    }
                }
            }
            cache.paint = Some(paint);
            cache.native_pixels.clear();
        }
        Some(std::cell::RefMut::map(stored, |cache| {
            cache.as_mut().unwrap()
        }))
    }
    fn raster(&self, w: u32, h: u32, s: f32, background: bool) -> Vec<u8> {
        if self.controls.len() == 1 && self.scene.modals.is_empty() {
            return if background {
                self.controls[0].pixels(w, h, s)
            } else {
                self.controls[0].content_pixels(w, h, s)
            };
        }
        self.raster_cached(w, h, s, background)
            .map_or_else(Vec::new, |cache| cache.pixels.clone())
    }
    /// Paint RGBA into caller-owned storage without allocating an output Vec.
    /// Invalid dimensions/DPI leave the destination unchanged.
    pub fn paint_rgba(
        &self,
        out: &mut [u8],
        w: u32,
        h: u32,
        s: f32,
        background: bool,
    ) -> Result<(), &'static str> {
        if self.controls.len() == 1 && self.scene.modals.is_empty() {
            return self.controls[0].paint_rgba(out, w, h, s, background);
        }
        if (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(out.len())
        {
            return Err("Invalid RGBA buffer size");
        }
        let cache = self
            .raster_cached(w, h, s, background)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        out.copy_from_slice(&cache.pixels);
        Ok(())
    }

    pub fn paint_native(
        &self,
        out: &mut [u32],
        w: u32,
        h: u32,
        s: f32,
    ) -> Result<(), &'static str> {
        if self.controls.len() == 1 && self.scene.modals.is_empty() {
            return self.controls[0].paint_native(out, w, h, s);
        }
        if (w as usize).checked_mul(h as usize) != Some(out.len()) {
            return Err("Invalid native buffer size");
        }
        let mut cache = self
            .raster_cached(w, h, s, true)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        if cache.native_pixels.is_empty() {
            let RasterCache {
                native_pixels,
                pixels,
                ..
            } = &mut *cache;
            native_pixels.reserve(pixels.len() / 4);
            native_pixels.extend(pixels.chunks_exact(4).map(|src| {
                let a = src[3] as u32;
                if a == 255 {
                    ((src[0] as u32) << 16) | ((src[1] as u32) << 8) | (src[2] as u32)
                } else if a == 0 {
                    0x111319
                } else {
                    let rgb = [17u32, 19, 25];
                    let c = |k: usize| (src[k] as u32 * a + rgb[k] * (255 - a) + 127) / 255;
                    c(0) << 16 | c(1) << 8 | c(2)
                }
            }));
        }
        out.copy_from_slice(&cache.native_pixels);
        Ok(())
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
        if !value.is_finite() || value < 0. || value > 100. {
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
        let modal_idx = self
            .scene
            .modals
            .iter()
            .position(|m| {
                (!m.controls.is_empty()).then_some(m.controls.start)
                    == node
                        .children
                        .first()
                        .and_then(|&cid| self.nodes[cid].control)
            })
            .unwrap_or(0);
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
    /// Keys: 1 = Space, 2 = Enter, 3 = Escape. Escape dismisses the active
    /// layer even while its text field is being edited.
    pub fn key_event(&mut self, key: u8, pressed: bool, repeat: bool) {
        if key == 3 {
            if pressed && !repeat {
                if let Some(m) = self.active_modal_index() {
                    self.dismiss_modal(m);
                }
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
            }
            self.update_revision(before);
        }
    }
    pub fn pointer(&mut self, x: f32, y: f32, kind: u8) {
        let before = self.child_revision();
        let active = self.active_modal_index();
        for (i, c) in self.controls.iter_mut().enumerate() {
            c.reveal_pointer(x, y, kind != 3 && self.modal_of[i] == active);
        }
        let hit = usize::try_from(self.hit_index(x, y)).ok();
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
            }
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
        self.controls.iter().any(Button::is_animating)
            || self
                .editors
                .iter()
                .flatten()
                .any(crate::text_input::Editor::is_animating)
    }
    /// Proximity reaches sibling controls even when the pointer is outside all hits.
    pub fn reveal_pointer(&mut self, x: f32, y: f32, present: bool) {
        let before = self.child_revision();
        let active = self.active_modal_index();
        for (i, c) in self.controls.iter_mut().enumerate() {
            c.reveal_pointer(x, y, present && self.modal_of[i] == active);
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
    fn preserve_interaction_matches(&mut self, previous: &Runtime) -> Vec<Option<usize>> {
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
        for i in 0..self.controls.len() {
            if let Some(old) = matches[i] {
                if let (Some(next), Some(prev)) = (&mut self.editors[i], &previous.editors[old]) {
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
