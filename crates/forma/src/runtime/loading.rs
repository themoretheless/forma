//! Building a runtime from sources, project files or a scene artifact.

use super::*;

impl Runtime {
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
        rt.install_clips(&controls);
        rt.ui_binding_scene = controls;
        rt.ui_binding_states = doc.states.clone();
        rt.ui_document = Some(doc);
        Ok(rt)
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
        next.install_clips(&controls);
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
        super::scroll_areas::validate_areas(artifact.controls.iter().map(|c| c.scroll.as_slice()))?;
        runtime.install_clips(&artifact.controls);
        Ok(runtime)
    }

    pub fn from_sources(source: &str, component: &str) -> Result<Self, String> {
        let mut scene = markup::parse(source).map_err(|error| {
            // The layout places a control that does not fit its container at a negative
            // position, which the scene format refuses; say what that means.
            if error.contains("must be non-negative") {
                format!("A control does not fit inside its container: {error}")
            } else {
                error
            }
        })?;
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
            modal: None,
        });
        let parent = if scene.scroll {
            nodes[0].children.push(1);
            nodes.push(Node {
                id: 1,
                parent: Some(0),
                children: Vec::with_capacity(base),
                kind: NodeKind::Scroll,
                control: None,
                modal: None,
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
                    while modal_nodes.len() <= m {
                        let cur = modal_nodes.len();
                        let id = nodes.len();
                        nodes[0].children.push(id);
                        nodes.push(Node {
                            id,
                            parent: Some(0),
                            children: Vec::with_capacity(scene.modals[cur].controls.len()),
                            kind: NodeKind::Modal,
                            control: None,
                            modal: Some(cur),
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
                modal: None,
            });
        }
        // Empty layers still exist as nodes so hosts can open and close them.
        for cur in modal_nodes.len()..scene.modals.len() {
            let id = nodes.len();
            nodes[0].children.push(id);
            nodes.push(Node {
                id,
                parent: Some(0),
                children: vec![],
                kind: NodeKind::Modal,
                control: None,
                modal: Some(cur),
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
            text_links: Default::default(),
            popups: Default::default(),
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
            clips: Vec::new(),
            scroll_areas: Vec::new(),
            scroll_chains: Vec::new(),
            pointer_at: None,
            groups: Vec::new(),
            area_nodes: Vec::new(),
            control_homes: Vec::new(),
            ui_binding_states: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            svet_document: None,
        };
        runtime.refresh_editors();
        runtime.acknowledge_binding_widgets();
        Ok(runtime)
    }

    pub(super) fn apply_logical_content_end(&mut self, end: Option<[f64; 2]>) {
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
}

impl Runtime {
    /// Take each control's clip rectangle from the scene mapping. A clipped control is drawn by
    /// the multi-control path, which owns the root frame's clipping and scrolling, so a lone
    /// control hands those back to the runtime like it does beside siblings.
    pub(super) fn install_clips(&mut self, controls: &[crate::form_document::SceneControl]) {
        (self.scroll_areas, self.scroll_chains) =
            super::scroll_areas::areas_of(controls.iter().map(|c| c.scroll.as_slice()));
        self.attach_area_nodes();
        self.groups = controls.iter().map(|c| c.group.clone()).collect();
        self.set_clips(
            controls
                .iter()
                .map(|control| control.clip.map(|rect| rect.map(|v| v as f32)))
                .collect(),
        );
    }

    /// Replace every control's clip rectangle and drop what was painted under the old ones.
    pub(super) fn set_clips(&mut self, clips: Vec<Option<[f32; 4]>>) {
        self.clips = clips;
        if self.clips.iter().any(Option::is_some) {
            for control in &mut self.controls {
                control.scene.clip = false;
                control.scene.scroll = false;
            }
        }
        self.sync_control_scroll();
        *self.geometry.borrow_mut() = None;
        self.revision = self.revision.wrapping_add(1);
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
    }
}
