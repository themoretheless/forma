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
        let ir = doc.render_scene_ir()?;
        let mut rt = Self::from_scene_ir(&ir)?;
        rt.ui_binding_scene = ir.controls;
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
        next.composition_revision = self.composition_revision.wrapping_add(1);
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
        let ir = doc.render_scene_ir()?;
        let mut next = Self::from_scene_ir(&ir)?;
        next.reuse_caches_from_runtime(self);
        next.ui_binding_scene = ir.controls;
        next.ui_binding_states = doc.states.clone();
        next.ui_document = Some(doc);
        #[cfg(not(target_arch = "wasm32"))]
        next.set_svet_document_scheduling(self.svet_document.is_some());
        next.revision = self.revision.wrapping_add(1);
        next.geometry_revision = self.geometry_revision.wrapping_add(1);
        next.layout_revision = self.layout_revision.wrapping_add(1);
        next.composition_revision = self.composition_revision.wrapping_add(1);
        *self = next;
        Ok(())
    }

    /// Reconciles an already-running runtime in place against typed [`crate::form_document::SceneIr`].
    /// Preserves persistent UI tree nodes, interaction state, and cached geometry/raster buffers
    /// whenever only paint slots or localized properties change; falls back to atomic replacement
    /// on structural changes. Refusing an invalid `ir` leaves `self` untouched.
    pub fn reconcile_scene_ir(
        &mut self,
        ir: &crate::form_document::SceneIr,
    ) -> Result<DirtyFlags, String> {
        if ir
            .logical_content_end
            .is_some_and(|end| end.iter().any(|v| !v.is_finite() || *v < 0.))
        {
            return Err("Invalid logical content extent".into());
        }
        if ir.templates.len() != ir.scene.buttons.len() {
            return Err("Control/template count mismatch".into());
        }
        if ir.controls.len() != ir.scene.buttons.len() {
            return Err("Scene control mapping count does not match rendered controls".into());
        }
        if ir.logical_content_end.is_some() && !ir.scene.scroll {
            return Err("Logical content extent requires a Scroll scene".into());
        }
        for (mapping, tmpl) in ir.controls.iter().zip(&ir.templates) {
            if mapping.key != tmpl.props.key {
                return Err(
                    "Scene control mapping key/order does not match rendered controls".into(),
                );
            }
        }
        super::scroll_areas::validate_areas(ir.controls.iter().map(|c| c.scroll.as_slice()))?;
        self.reconcile_validated(
            ir.scene.clone(),
            &ir.templates,
            ir.logical_content_end,
            Some(&ir.controls),
        )
    }

    /// Reconciles an already-running runtime in place against scene and component markup.
    /// Paint-only edits keep layout nodes (`DirtyFlags::LAYOUT` unset) and unchanged controls'
    /// retained caches intact. Refusing invalid markup leaves `self` untouched.
    pub fn reconcile_sources(
        &mut self,
        source: &str,
        component: &str,
    ) -> Result<DirtyFlags, String> {
        let scene = markup::parse(source).map_err(|error| {
            if error.contains("must be non-negative") {
                format!("A control does not fit inside its container: {error}")
            } else {
                error
            }
        })?;
        let template_sources = templates(component, scene.buttons.len())?;
        let parsed = scene
            .buttons
            .iter()
            .zip(template_sources)
            .map(|(spec, src)| crate::template::parse_cached(src, spec))
            .collect::<Result<Vec<_>, _>>()?;
        let dirty = self.reconcile_validated(scene, &parsed, None, None)?;
        self.ui_document = None;
        self.ui_binding_scene.clear();
        self.ui_binding_states.clear();
        self.clips.clear();
        self.scroll_areas.clear();
        self.scroll_chains.clear();
        Ok(dirty)
    }

    /// Copies matching controls' retained display-list and raster coverage caches from `previous`
    /// into `self` so an atomic reload skips rebuilding unchanged controls.
    pub(crate) fn reuse_caches_from_runtime(&mut self, previous: &Runtime) {
        use std::collections::HashMap;
        if previous.controls.is_empty() || self.controls.is_empty() {
            return;
        }
        let same_count = previous.controls.len() == self.controls.len();
        let mut by_key: HashMap<&str, Option<usize>> =
            HashMap::with_capacity(previous.controls.len());
        for (i, c) in previous.controls.iter().enumerate() {
            let key = c.scene.button.key.as_str();
            if !key.is_empty() {
                by_key
                    .entry(key)
                    .and_modify(|slot| *slot = None)
                    .or_insert(Some(i));
            }
        }
        for (i, control) in self.controls.iter_mut().enumerate() {
            let key = control.scene.button.key.as_str();
            let old_idx = by_key
                .get(key)
                .copied()
                .flatten()
                .or_else(|| (same_count && i < previous.controls.len()).then_some(i));
            let Some(old_idx) = old_idx else {
                continue;
            };
            let prev = &previous.controls[old_idx];
            if prev.scene.clip != control.scene.clip
                || prev.scene.scroll != control.scene.scroll
                || prev.scene.width != control.scene.width
                || prev.scene.height != control.scene.height
                || prev.scene.radius != control.scene.radius
                || prev.scene.padding != control.scene.padding
                || prev.scroll_x != control.scroll_x
                || prev.scroll_y != control.scroll_y
            {
                continue;
            }
            let same_box = prev.scene.button.x == control.scene.button.x
                && prev.scene.button.y == control.scene.button.y
                && prev.scene.button.width == control.scene.button.width
                && prev.scene.button.height == control.scene.button.height
                && prev.scene.button.radius == control.scene.button.radius;
            if !same_box {
                continue;
            }
            if Arc::ptr_eq(&prev.template, &control.template)
                || prev.template == control.template
                || prev
                    .template
                    .same_geometry_except_paint_slots(&control.template)
            {
                if let Some(display) = prev.display_cache.borrow().as_ref() {
                    *control.display_cache.borrow_mut() = Some(crate::display_list::CachedList {
                        scale: display.scale,
                        background: display.background,
                        list: Arc::clone(&display.list),
                    });
                }
            }
        }
    }

    /// Atomically replace the scene from versioned JSON; a refusal preserves this Runtime.
    pub fn load_scene_artifact_native(&mut self, json: &str) -> Result<(), String> {
        let artifact = crate::form_document::SceneArtifact::from_json(json)?;
        let mut next = Self::from_scene_artifact(&artifact)?;
        next.revision = self.revision.wrapping_add(1);
        next.geometry_revision = self.geometry_revision.wrapping_add(1);
        next.layout_revision = self.layout_revision.wrapping_add(1);
        next.composition_revision = self.composition_revision.wrapping_add(1);
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

    /// Instantiates a [`Runtime`] directly from a validated typed [`crate::form_document::SceneIr`]
    /// without formatting or parsing intermediate source or template text.
    pub fn from_scene_ir(ir: &crate::form_document::SceneIr) -> Result<Self, String> {
        if ir
            .logical_content_end
            .is_some_and(|end| end.iter().any(|v| !v.is_finite() || *v < 0.))
        {
            return Err("Invalid logical content extent".into());
        }
        if ir.templates.len() != ir.scene.buttons.len() {
            return Err("Control/template count mismatch".into());
        }
        if ir.controls.len() != ir.scene.buttons.len() {
            return Err("Scene control mapping count does not match rendered controls".into());
        }
        let mut runtime = Self::from_validated_scene(ir.scene.clone(), &ir.templates)?;
        for (mapping, control) in ir.controls.iter().zip(&runtime.controls) {
            if mapping.key != control.scene.button.key {
                return Err(
                    "Scene control mapping key/order does not match rendered controls".into(),
                );
            }
        }
        if ir.logical_content_end.is_some() && !runtime.scene.scroll {
            return Err("Logical content extent requires a Scroll scene".into());
        }
        runtime.apply_logical_content_end(ir.logical_content_end);
        super::scroll_areas::validate_areas(ir.controls.iter().map(|c| c.scroll.as_slice()))?;
        runtime.install_clips(&ir.controls);
        Ok(runtime)
    }

    pub fn from_sources(source: &str, component: &str) -> Result<Self, String> {
        let scene = markup::parse(source).map_err(|error| {
            // The layout places a control that does not fit its container at a negative
            // position, which the scene format refuses; say what that means.
            if error.contains("must be non-negative") {
                format!("A control does not fit inside its container: {error}")
            } else {
                error
            }
        })?;
        let template_sources = templates(component, scene.buttons.len())?;
        let parsed = scene
            .buttons
            .iter()
            .zip(template_sources)
            .map(|(spec, src)| crate::template::parse_cached(src, spec))
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_validated_scene(scene, &parsed)
    }

    fn from_validated_scene(
        mut scene: markup::Scene,
        parsed_templates: &[Arc<crate::template::Template>],
    ) -> Result<Self, String> {
        let mut controls = Vec::with_capacity(scene.buttons.len());
        let modal_of: Vec<Option<usize>> = (0..scene.buttons.len())
            .map(|i| scene.modals.iter().position(|m| m.controls.contains(&i)))
            .collect();
        let mut y = scene.padding[0];
        for (index, (spec, parsed_template)) in
            scene.buttons.iter().zip(parsed_templates).enumerate()
        {
            if let Some(m) = modal_of[index] {
                if scene.modals[m].controls.start == index {
                    y = scene.modals[m].y;
                }
            }
            let mut button = parsed_template.props.clone();
            if !spec.specified.contains("y") {
                button.y = y;
            }
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
                button,
                buttons: Vec::new(),
                modals: Vec::new(),
            };
            let control = Button::from_leaf_scene(leaf, Arc::clone(parsed_template));
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
            dirty: DirtyFlags::ALL,
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
                dirty: DirtyFlags::ALL,
            });
            1
        } else {
            0
        };
        let mut control_nodes = Vec::with_capacity(controls.len());
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
                            dirty: DirtyFlags::ALL,
                        });
                        modal_nodes.push(id);
                    }
                    modal_nodes[m]
                }
            };
            let id = nodes.len();
            nodes[parent].children.push(id);
            control_nodes.push(id);
            nodes.push(Node {
                id,
                parent: Some(parent),
                children: vec![],
                kind: NodeKind::Control,
                control: Some(index),
                modal: None,
                dirty: DirtyFlags::ALL,
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
                dirty: DirtyFlags::ALL,
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
        let control_visual_revisions = controls.iter().map(Button::visual_revision).collect();
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
            native_instance_id: Self::next_native_instance_id(),
            revision: 0,
            geometry_revision: 0,
            layout_revision: 0,
            composition_revision: 0,
            scene_dirty: DirtyFlags::ALL,
            control_nodes,
            control_visual_revisions,
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
            control_origins: Vec::new(),
            area_nodes: Vec::new(),
            control_homes: Vec::new(),
            ui_binding_states: Vec::new(),
            announcement: None,
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

    fn can_reconcile_in_place(
        &self,
        scene: &markup::Scene,
        parsed_templates: &[Arc<crate::template::Template>],
        mapping: Option<&[crate::form_document::SceneControl]>,
    ) -> bool {
        if scene.buttons.len() != self.controls.len()
            || scene.scroll != self.scene.scroll
            || scene.modals != self.scene.modals
        {
            return false;
        }
        for (i, tmpl) in parsed_templates.iter().enumerate() {
            if tmpl.props.key != self.controls[i].scene.button.key
                || tmpl.inputs.is_empty() != self.editors[i].is_none()
                || tmpl.range.is_some() != self.controls[i].template.range.is_some()
            {
                return false;
            }
            if let Some(controls) = mapping {
                let orig = controls[i].origin.as_deref().unwrap_or("");
                let prev = self.control_origins.get(i).map_or("", String::as_str);
                if prev != orig {
                    return false;
                }
            }
        }
        if let Some(controls) = mapping {
            if !self.scroll_areas.is_empty() || controls.iter().any(|c| !c.scroll.is_empty()) {
                let (areas, chains) =
                    super::scroll_areas::areas_of(controls.iter().map(|c| c.scroll.as_slice()));
                if areas.len() != self.scroll_areas.len()
                    || chains != self.scroll_chains
                    || !areas
                        .iter()
                        .zip(&self.scroll_areas)
                        .all(|(a, b)| a.viewport == b.viewport && a.parent == b.parent)
                {
                    return false;
                }
            }
        }
        true
    }

    fn reconcile_validated(
        &mut self,
        scene: markup::Scene,
        parsed_templates: &[Arc<crate::template::Template>],
        logical_content_end: Option<[f64; 2]>,
        mapping: Option<&[crate::form_document::SceneControl]>,
    ) -> Result<DirtyFlags, String> {
        if !self.can_reconcile_in_place(&scene, parsed_templates, mapping) {
            return self.reconcile_full_rebuild(
                scene,
                parsed_templates,
                logical_content_end,
                mapping,
            );
        }
        self.reconcile_in_place(scene, parsed_templates, logical_content_end, mapping)
    }

    fn reconcile_full_rebuild(
        &mut self,
        scene: markup::Scene,
        parsed_templates: &[Arc<crate::template::Template>],
        logical_content_end: Option<[f64; 2]>,
        mapping: Option<&[crate::form_document::SceneControl]>,
    ) -> Result<DirtyFlags, String> {
        let mut next = Self::from_validated_scene(scene, parsed_templates)?;
        next.apply_logical_content_end(logical_content_end);
        if let Some(controls) = mapping {
            next.install_clips(controls);
        }
        next.reuse_caches_from_runtime(self);
        next.preserve_binding_interaction(self);
        next.keep_area_scroll(self);
        next.announcement = self.announcement.clone();
        next.composition_revision = self.composition_revision.wrapping_add(1);
        next.scene_dirty = DirtyFlags::ALL;
        #[cfg(not(target_arch = "wasm32"))]
        {
            next.svet_document = self.svet_document.take();
        }
        *self = next;
        Ok(DirtyFlags::ALL)
    }

    fn reconcile_in_place(
        &mut self,
        mut scene: markup::Scene,
        parsed_templates: &[Arc<crate::template::Template>],
        logical_content_end: Option<[f64; 2]>,
        mapping: Option<&[crate::form_document::SceneControl]>,
    ) -> Result<DirtyFlags, String> {
        enum EditorPlan {
            KeepUnchanged,
            UpdateKeepModel {
                spec: crate::template::Input,
                base: Vec<crate::template::Content>,
            },
            Rebuild(crate::text_input::Editor),
        }

        // Prepare any editor updates first so failures keep `self` atomic and untouched.
        let mut editor_plans = Vec::with_capacity(self.controls.len());
        for (i, tmpl) in parsed_templates.iter().enumerate() {
            if let (Some(prev), Some(next_input)) = (&self.editors[i], tmpl.inputs.first()) {
                let keep_model = (next_input.value == prev.spec.value
                    || next_input.value == prev.model.value())
                    && next_input.multiline == prev.spec.multiline;
                if keep_model {
                    if prev.spec == *next_input && prev.base == tmpl.content {
                        editor_plans.push(EditorPlan::KeepUnchanged);
                    } else {
                        editor_plans.push(EditorPlan::UpdateKeepModel {
                            spec: next_input.clone(),
                            base: tmpl.content.clone(),
                        });
                    }
                } else {
                    let mut ed =
                        crate::text_input::Editor::new(next_input.clone(), tmpl.content.clone())?;
                    ed.reduced_motion(self.controls[i].reduced_motion);
                    editor_plans.push(EditorPlan::Rebuild(ed));
                }
            } else {
                editor_plans.push(EditorPlan::KeepUnchanged);
            }
        }

        // Compute new implicit positions & extent without allocating throwaway Scenes.
        let mut next_buttons = Vec::with_capacity(scene.buttons.len());
        let mut y = scene.padding[0];
        for (index, (spec, tmpl)) in scene.buttons.iter().zip(parsed_templates).enumerate() {
            if let Some(m) = self.modal_of[index] {
                if scene.modals[m].controls.start == index {
                    y = scene.modals[m].y;
                }
            }
            let mut button = tmpl.props.clone();
            if !spec.specified.contains("y") {
                button.y = y;
            }
            y = button.y + button.height + scene.gap;
            next_buttons.push(button);
        }
        let base = self.modal_of.iter().filter(|m| m.is_none()).count();
        scene.content_width = next_buttons[..base]
            .iter()
            .map(|b| b.x + b.width + scene.padding[1])
            .fold(scene.width, f32::max);
        scene.content_height = next_buttons[..base]
            .iter()
            .map(|b| b.y + b.height + scene.padding[2])
            .fold(scene.height, f32::max);
        if let Some([right, bottom]) = logical_content_end {
            scene.content_width = scene
                .content_width
                .max((right + f64::from(scene.padding[1])).min(f64::from(f32::MAX)) as f32);
            scene.content_height = scene
                .content_height
                .max((bottom + f64::from(scene.padding[2])).min(f64::from(f32::MAX)) as f32);
        }

        let frame_layout_changed = self.scene.width != scene.width
            || self.scene.height != scene.height
            || self.scene.clip != scene.clip
            || self.scene.radius != scene.radius
            || self.scene.padding != scene.padding
            || self.scene.content_width != scene.content_width
            || self.scene.content_height != scene.content_height;
        let frame_bg_changed = self.scene.background != scene.background;

        let mut layout_changed = frame_layout_changed;
        let mut geometry_changed = frame_layout_changed || frame_bg_changed;
        let mut paint_changed = frame_layout_changed || frame_bg_changed;
        let mut composition_changed = false;

        let single_leaf_clips = mapping.is_some_and(|controls| {
            self.controls.len() == 1 && controls.first().is_some_and(|c| c.clip.is_some())
        });
        let force_flat_control =
            self.controls.len() > 1 || !scene.modals.is_empty() || single_leaf_clips;

        for (i, ((button, tmpl), plan)) in next_buttons
            .into_iter()
            .zip(parsed_templates)
            .zip(editor_plans)
            .enumerate()
        {
            let range_unchanged = tmpl.range.as_ref().is_some_and(|r| {
                self.range_values[i] == Some(r.value)
                    && self.controls[i].template.range.as_ref() == Some(r)
            });
            let editor_unchanged =
                matches!(plan, EditorPlan::KeepUnchanged) && self.editors[i].is_some();
            let preserve_generated = editor_unchanged || range_unchanged;

            let control = &mut self.controls[i];
            let prev_clip = control.scene.clip;
            let prev_scroll = control.scene.scroll;
            control.scene.name.clear();
            control.scene.width = scene.width;
            control.scene.height = scene.height;
            control.scene.background = scene.background;
            control.scene.clip = if force_flat_control {
                false
            } else {
                scene.clip
            };
            control.scene.radius = scene.radius;
            control.scene.scroll = if force_flat_control {
                false
            } else {
                scene.scroll
            };
            control.scene.padding = scene.padding;
            control.scene.gap = scene.gap;
            if control.scene.clip != prev_clip || control.scene.scroll != prev_scroll {
                composition_changed = true;
                geometry_changed = true;
                layout_changed = true;
            }

            let change =
                control.apply_template_in_place(button, Arc::clone(tmpl), preserve_generated);
            if control.disabled() || !control.template.clickable {
                if self.focused == Some(i) {
                    self.focused = None;
                }
                if self.captured == Some(i) {
                    self.captured = None;
                }
            }
            match plan {
                EditorPlan::KeepUnchanged => {
                    if let Some(editor) = &mut self.editors[i] {
                        editor.set_preedit("");
                    }
                }
                EditorPlan::UpdateKeepModel { spec, base } => {
                    if let Some(editor) = &mut self.editors[i] {
                        editor.spec = spec;
                        editor.base = base;
                        editor.set_preedit("");
                        editor.invalidate_content();
                    }
                }
                EditorPlan::Rebuild(editor) => {
                    self.editors[i] = Some(editor);
                }
            }
            if control.template.range.is_none() {
                self.range_values[i] = None;
            }
            self.control_visual_revisions[i] = control.visual_revision;

            let node_dirty = match change {
                crate::control::TemplateChange::None => DirtyFlags::NONE,
                crate::control::TemplateChange::PaintSlotsOnly => {
                    paint_changed = true;
                    DirtyFlags::PAINT
                }
                crate::control::TemplateChange::Visual => {
                    paint_changed = true;
                    geometry_changed = true;
                    DirtyFlags::PAINT
                }
                crate::control::TemplateChange::Layout => {
                    paint_changed = true;
                    geometry_changed = true;
                    layout_changed = true;
                    DirtyFlags::LAYOUT | DirtyFlags::PAINT
                }
            };
            if !node_dirty.is_empty() {
                self.mark_control_dirty(i, node_dirty);
            }
        }

        // Update frame metadata, clips, scroll areas, and clamp active scroll offsets.
        self.scene.name = scene.name;
        self.scene.width = scene.width;
        self.scene.height = scene.height;
        self.scene.background = scene.background;
        self.scene.clip = scene.clip;
        self.scene.radius = scene.radius;
        self.scene.scroll = scene.scroll;
        self.scene.padding = scene.padding;
        self.scene.content_width = scene.content_width;
        self.scene.content_height = scene.content_height;
        self.scene.gap = scene.gap;
        self.scene.buttons = scene.buttons;

        if let Some(controls) = mapping {
            let new_clips: Vec<Option<[f32; 4]>> = controls
                .iter()
                .map(|c| c.clip.map(|r| r.map(|v| v as f32)))
                .collect();
            if self.clips != new_clips {
                self.clips = new_clips;
                composition_changed = true;
                geometry_changed = true;
                paint_changed = true;
            }
            self.groups = controls.iter().map(|c| c.group.clone()).collect();
            self.control_origins = controls
                .iter()
                .map(|c| c.origin.clone().unwrap_or_default())
                .collect();
            if !self.scroll_areas.is_empty() {
                let (areas, _) =
                    super::scroll_areas::areas_of(controls.iter().map(|c| c.scroll.as_slice()));
                let mut area_changed = false;
                for (cur, next) in self.scroll_areas.iter_mut().zip(areas) {
                    if cur.content != next.content {
                        cur.content = next.content;
                        let limit = cur.limit();
                        cur.offset = [cur.offset[0].min(limit[0]), cur.offset[1].min(limit[1])];
                        area_changed = true;
                    }
                }
                if area_changed {
                    composition_changed = true;
                    layout_changed = true;
                    paint_changed = true;
                }
            }
        }

        if self.scene.scroll {
            let v = self.viewport_rect();
            let size = self.content_size();
            let clamped = [
                self.scroll[0].clamp(0., (size[0] - v[2]).max(0.)),
                self.scroll[1].clamp(0., (size[1] - v[3]).max(0.)),
            ];
            if clamped != self.scroll {
                self.scroll = clamped;
                composition_changed = true;
                layout_changed = true;
                paint_changed = true;
            }
        }
        self.sync_control_scroll();

        let rev_before_refresh = self.revision;
        let geom_before_refresh = self.geometry_revision;
        self.refresh_editors();
        if self.revision != rev_before_refresh {
            paint_changed = true;
        }
        if self.geometry_revision != geom_before_refresh {
            geometry_changed = true;
        }
        self.acknowledge_binding_widgets();

        let mut dirty = DirtyFlags::NONE;
        if geometry_changed && self.geometry_revision == geom_before_refresh {
            *self.geometry.borrow_mut() = None;
            self.geometry_revision = self.geometry_revision.wrapping_add(1);
        }
        if layout_changed {
            self.layout_revision = self.layout_revision.wrapping_add(1);
            dirty.insert(DirtyFlags::LAYOUT);
        }
        if composition_changed {
            self.composition_revision = self.composition_revision.wrapping_add(1);
            dirty.insert(DirtyFlags::COMPOSITION);
        }
        if paint_changed || layout_changed || composition_changed {
            if self.revision == rev_before_refresh {
                self.revision = self.revision.wrapping_add(1);
            }
            dirty.insert(DirtyFlags::PAINT);
        }
        if frame_layout_changed || frame_bg_changed {
            self.mark_node_dirty(0, dirty);
        } else {
            self.scene_dirty.insert(dirty);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !dirty.is_empty() {
            if self.native_accessibility_id.is_none() {
                self.native_accessibility_id = Some(self.native_scene_identity());
            }
            self.native_instance_id = Self::next_native_instance_id();
        }
        Ok(dirty)
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
        self.control_origins = controls
            .iter()
            .map(|c| c.origin.clone().unwrap_or_default())
            .collect();
        self.set_clips(
            controls
                .iter()
                .map(|control| control.clip.map(|rect| rect.map(|v| v as f32)))
                .collect(),
        );
        self.sync_control_scroll();
    }

    /// Replace every control's clip rectangle and drop what was painted under the old ones.
    pub(super) fn set_clips(&mut self, clips: Vec<Option<[f32; 4]>>) {
        self.clips = clips;
        self.clips_changed();
    }

    /// A clip was set or removed: what was painted under the old ones is stale. Constant time,
    /// so setting clips one control at a time stays linear in the controls.
    pub(super) fn clips_changed(&mut self) {
        // Beside siblings a control already leaves the root frame's clipping and scrolling to
        // the runtime; a lone clipped control hands them over here.
        if self.controls.len() == 1 && self.clip(0).is_some() {
            self.controls[0].scene.clip = false;
            self.controls[0].scene.scroll = false;
        }
        *self.geometry.borrow_mut() = None;
        self.revision = self.revision.wrapping_add(1);
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.composition_revision = self.composition_revision.wrapping_add(1);
        self.scene_dirty
            .insert(DirtyFlags::COMPOSITION | DirtyFlags::PAINT);
    }
}
