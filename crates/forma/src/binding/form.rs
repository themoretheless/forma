//! The form: its controls, document and runtime.

use super::*;

pub(super) struct DocumentState {
    pub(super) program: crate::form_document::Document,
    pub(super) inputs: Vec<(usize, String, Box<dyn Input>)>,
    pub(super) nodes: BTreeMap<usize, usize>,
    pub(super) node_controls: BTreeMap<usize, usize>,
    pub(super) scene: Vec<crate::form_document::SceneControl>,
    pub(super) event_types: BTreeMap<(usize, String), Vec<String>>,
    pub(super) base_disabled: Vec<bool>,
}

/// Owns one renderer and its binding connections. Drive input through `update`,
/// and call `sync` before painting after model changes outside an input callback.
/// No mutable renderer reference escapes, so source writes cannot be missed.
pub struct Form {
    pub(super) document: Option<DocumentState>,
    pub(super) runtime: Runtime,
    pub(super) controls: Vec<Rc<ControlState>>,
    pub(super) wake: Rc<Wake>,
}

impl Form {
    pub fn new(source: &str, template: &str) -> Result<Self, String> {
        Ok(Self {
            document: None,
            runtime: Runtime::from_sources(source, template)?,
            controls: Vec::new(),
            wake: Rc::new(Wake::default()),
        })
    }
    pub fn from_document(program: crate::form_document::Document) -> Result<Self, String> {
        let mut form = Self::new(
            "component Empty { Frame { width:1; height:1; } }",
            "component Button { Rectangle {} }",
        )?;
        form.document = Some(DocumentState {
            program,
            inputs: Vec::new(),
            nodes: BTreeMap::new(),
            node_controls: BTreeMap::new(),
            scene: Vec::new(),
            event_types: BTreeMap::new(),
            base_disabled: Vec::new(),
        });
        form.wake.notify();
        Ok(form)
    }
    pub fn observe<C: 'static, T: ModelValue>(
        &mut self,
        node: usize,
        path: &str,
        context: &Context<C>,
        select: impl Fn(&C) -> Property<T> + 'static,
    ) {
        let field = Field::path(context, select, self.wake.clone());
        self.document
            .as_mut()
            .expect("generated document")
            .inputs
            .push((node, path.into(), Box::new(field)));
    }
    pub fn observe_projected<C: 'static, T: Clone + PartialEq + Default + 'static>(
        &mut self,
        node: usize,
        path: &str,
        context: &Context<C>,
        select: impl Fn(&C) -> Property<T> + 'static,
        project: impl Fn(&T) -> crate::form_document::Value + 'static,
    ) {
        let field = Field::path(context, select, self.wake.clone());
        self.document
            .as_mut()
            .expect("generated document")
            .inputs
            .push((
                node,
                path.into(),
                Box::new(ProjectedInput {
                    field,
                    project: Rc::new(project),
                }),
            ));
    }
    pub fn declare_event(&mut self, node: usize, name: &str, types: &[&str]) {
        self.document
            .as_mut()
            .expect("generated document")
            .event_types
            .insert(
                (node, name.into()),
                types.iter().map(|v| (*v).into()).collect(),
            );
    }
    pub fn observe_number<C: 'static, T: ModelNumber>(
        &mut self,
        node: usize,
        path: &str,
        context: &Context<C>,
        select: impl Fn(&C) -> Property<T> + 'static,
    ) {
        self.observe(node, path, context, select);
    }
    pub fn observe_bool<C: 'static>(
        &mut self,
        node: usize,
        path: &str,
        context: &Context<C>,
        select: impl Fn(&C) -> Property<bool> + 'static,
    ) {
        self.observe(node, path, context, select);
    }
    pub fn observe_text<C: 'static>(
        &mut self,
        node: usize,
        path: &str,
        context: &Context<C>,
        select: impl Fn(&C) -> Property<String> + 'static,
    ) {
        self.observe(node, path, context, select);
    }
    pub fn bind_node(&mut self, index: usize, node: usize) {
        let doc = self.document.as_mut().expect("generated document");
        doc.nodes.insert(index, node);
        doc.node_controls.insert(node, index);
    }
    pub fn add_control<C: 'static>(
        &mut self,
        index: usize,
        context: Context<C>,
    ) -> Result<Control<C>, String> {
        if (self.document.is_none() && index >= self.runtime.control_count())
            || self.controls.iter().any(|c| c.index == index)
        {
            return Err(format!("Invalid or duplicate bound control {index}"));
        }
        let present = context.clone();
        let context_changed = Rc::new(Cell::new(false));
        let changed = context_changed.clone();
        let context_epoch = Rc::new(Cell::new(0u64));
        let epoch = context_epoch.clone();
        let wake = self.wake.clone();
        let subscription = context.subscribe(move |_| {
            epoch.set(epoch.get().wrapping_add(1));
            changed.set(true);
            wake.notify();
        });
        let state = Rc::new(ControlState {
            index,
            resolved: Cell::new(Some(index)),
            dispatch_index: Cell::new(index),
            dispatch_arguments: RefCell::new(Vec::new()),
            collection_bindings: RefCell::new(Vec::new()),
            bindings: RefCell::new(BTreeMap::new()),
            properties: RefCell::new(BTreeMap::new()),
            handler: RefCell::new(None),
            changed_handler: RefCell::new(None),
            submitted_handler: RefCell::new(None),
            present: Rc::new(move || present.get().is_some()),
            base_disabled: Cell::new(self.runtime.control_disabled(index)),
            context_changed,
            context_epoch,
            context_subscription: RefCell::new(Some(subscription)),
            wake: self.wake.clone(),
        });
        self.controls.push(state.clone());
        Ok(Control { context, state })
    }
    /// Schedule a redraw when data changes. The callback must schedule work, not
    /// synchronously render; multiple notifications are coalesced until sync.
    pub fn on_dirty(&self, callback: impl Fn() + 'static) {
        let callback: Rc<dyn Fn()> = Rc::new(callback);
        *self.wake.callback.borrow_mut() = Some(callback.clone());
        if self.is_dirty() {
            callback();
        }
    }
    pub fn is_dirty(&self) -> bool {
        self.wake.pending.get()
            || self
                .document
                .as_ref()
                .is_some_and(|d| d.inputs.iter().any(|(_, _, i)| i.dirty()))
            || self.controls.iter().any(|control| {
                control.context_changed.get()
                    || control.bindings.borrow().values().any(|b| b.dirty())
                    || control.properties.borrow().values().any(|b| b.dirty())
                    || (self.document.is_none()
                        && self.runtime.control_disabled(control.index) != self.disabled(control))
            })
    }
    pub(super) fn disabled(&self, control: &ControlState) -> bool {
        if !(control.present)() {
            return true;
        }
        if control
            .bindings
            .borrow()
            .values()
            .any(|binding| match binding.as_ref() {
                Binding::String(field, _) => field.0.property.borrow().is_none(),
                Binding::Bool(field) => field.0.property.borrow().is_none(),
                Binding::Number(field, _) => !field.present(),
            })
        {
            return true;
        }
        match control
            .bindings
            .borrow()
            .get(&Target::Disabled)
            .map(Rc::as_ref)
        {
            Some(Binding::Bool(field)) => field.get().unwrap_or(true),
            _ => control.base_disabled.get(),
        }
    }
    /// Use an owned Svet graph to schedule this form's property bindings.
    /// This does not select the Studio renderer backend.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_svet_bindings(&mut self, enabled: bool) {
        self.wake.native.borrow_mut().enable(enabled);
        self.wake.notify();
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn svet_binding_telemetry(&self) -> Option<svet_graph::context::Telemetry> {
        self.wake
            .native
            .borrow()
            .graph
            .as_ref()
            .map(|graph| graph.telemetry())
    }
    pub fn sync(&mut self) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        self.wake.native.borrow_mut().drain();
        if self.is_dirty() {
            self.rebuild_document_scene()?;
        }
        self.push_bindings_to_runtime()?;
        self.clean_bindings();
        self.wake.pending.set(false);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pending = self.wake.native.borrow_mut().drain() > 0;
            if pending {
                self.wake.notify();
            }
        }
        Ok(())
    }

    /// Re-renders the document from its inputs and control overrides and replaces the runtime
    /// with the result. On any error the document keeps its previous state and scene.
    pub(super) fn rebuild_document_scene(&mut self) -> Result<(), String> {
        let Some(document) = &mut self.document else {
            return Ok(());
        };
        let has_control_overrides = self.controls.iter().any(|c| {
            document.nodes.contains_key(&c.index)
                && (!c.properties.borrow().is_empty() || !c.bindings.borrow().is_empty())
        });
        let saved_root = has_control_overrides.then(|| document.program.root.clone());
        let saved_states = document.program.states.clone();
        write_inputs_into_states(document);
        if has_control_overrides {
            write_control_overrides(document, &self.controls);
        }
        let rendered = document.program.render_scene_artifact();
        if let Some(original_root) = saved_root {
            document.program.root = original_root;
        }
        let built = rendered.and_then(|artifact| {
            check_event_arguments(document, &artifact.controls)?;
            let runtime = Runtime::from_scene_artifact(&artifact)?;
            Ok((artifact, runtime))
        });
        let (artifact, mut next) = match built {
            Ok(built) => built,
            Err(error) => {
                document.program.states = saved_states;
                return Err(error);
            }
        };
        next.preserve_binding_interaction(&self.runtime);
        document.base_disabled = (0..next.control_count())
            .map(|i| next.control_disabled(i))
            .collect();
        let scene = &artifact.controls;
        for control in &self.controls {
            let index = document
                .nodes
                .get(&control.index)
                .and_then(|node| scene.iter().position(|item| item.node == *node));
            control.resolved.set(index);
            if let Some(index) = index {
                control.base_disabled.set(next.control_disabled(index));
            }
        }
        document.scene = artifact.controls;
        self.runtime = next;
        for (_, _, input) in &document.inputs {
            input.clean();
        }
        for control in &self.controls {
            for input in control.properties.borrow().values() {
                input.clean();
            }
        }
        Ok(())
    }

    /// Pushes binding values and the disabled state of every resolved control into the runtime.
    pub(super) fn push_bindings_to_runtime(&mut self) -> Result<(), String> {
        let sync_instances = self.sync_instances();
        for (control, index) in &sync_instances {
            if control.context_changed.get() {
                self.runtime.cancel_binding_interaction(*index);
            }
            for (target, binding) in control.bindings.borrow().iter() {
                if self.document.is_none() {
                    binding.sync(&mut self.runtime, *index, *target)?;
                } else if *target == Target::Value {
                    if let Binding::String(field, _) = binding.as_ref() {
                        if field.0.reset.get() {
                            self.runtime.set_binding_value(
                                *index,
                                &field.get().unwrap_or_default(),
                                true,
                            )?;
                        }
                    }
                }
            }
            let mut disabled = self.disabled(control);
            if let Some(doc) = &self.document {
                if (control.present)() && !control.bindings.borrow().contains_key(&Target::Disabled)
                {
                    disabled = doc.base_disabled.get(*index).copied().unwrap_or(true);
                }
            }
            self.runtime.set_binding_disabled(*index, disabled)?;
        }
        Ok(())
    }

    /// Marks every binding as delivered.
    pub(super) fn clean_bindings(&self) {
        for control in &self.controls {
            control.context_changed.set(false);
            for binding in control.bindings.borrow().values() {
                match binding.as_ref() {
                    Binding::String(f, _) => {
                        f.0.dirty.set(false);
                        f.0.reset.set(false);
                    }
                    Binding::Number(f, _) => f.clean(),
                    Binding::Bool(f) => {
                        f.0.dirty.set(false);
                        f.0.reset.set(false);
                    }
                }
            }
        }
    }
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
    pub(super) fn sync_instances(&self) -> Vec<(Rc<ControlState>, usize)> {
        if let Some(document) = &self.document {
            document
                .scene
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    let control_index = document.node_controls.get(&item.node)?;
                    let control = self
                        .controls
                        .get(*control_index)
                        .filter(|c| c.index == *control_index)
                        .or_else(|| self.controls.iter().find(|c| c.index == *control_index))?;
                    Some((control.clone(), index))
                })
                .collect()
        } else {
            self.controls.iter().map(|c| (c.clone(), c.index)).collect()
        }
    }
    pub(super) fn instances(
        &self,
    ) -> Vec<(
        Rc<ControlState>,
        usize,
        crate::form_document::Properties,
        String,
    )> {
        if let Some(document) = &self.document {
            document
                .scene
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    let control_index = document.node_controls.get(&item.node)?;
                    let control = self
                        .controls
                        .get(*control_index)
                        .filter(|c| c.index == *control_index)
                        .or_else(|| self.controls.iter().find(|c| c.index == *control_index))?;
                    Some((control.clone(), index, item.state.clone(), item.key.clone()))
                })
                .collect()
        } else {
            self.controls
                .iter()
                .map(|c| {
                    (
                        c.clone(),
                        c.index,
                        Default::default(),
                        self.runtime.control_key(c.index),
                    )
                })
                .collect()
        }
    }
    pub fn update<R>(&mut self, input: impl FnOnce(&mut Runtime) -> R) -> Result<R, String> {
        self.sync()?;
        let instances = self.instances();
        let before: Vec<_> = instances
            .iter()
            .map(|(control, index, _, _)| ControlSnapshot::take(&self.runtime, control, *index))
            .collect();
        let result = input(&mut self.runtime);
        let mut writes = PendingWrites::default();
        let mut events = Vec::new();
        for ((control, index, locals, key), snapshot) in instances.into_iter().zip(before) {
            if control.context_epoch.get() != snapshot.context_epoch {
                continue;
            }
            let change = ControlChange {
                next_text: self.runtime.text_value(index),
                next_range: self.runtime.range_value(index),
                next_scaled: self.runtime.range_scaled_value(index),
                clicks: self
                    .runtime
                    .control_clicks(index)
                    .saturating_sub(snapshot.clicks),
                text: snapshot.text,
                range: snapshot.range,
            };
            let submitted = self
                .runtime
                .control_submissions(index)
                .saturating_sub(snapshot.submissions);
            let mut toggled = writes.collect_bindings(&control, snapshot.bindings, &change)?;
            toggled |= writes.collect_collections(snapshot.collections, &locals, &change)?;
            let mut queue = |event: &'static str, handler: Handler| {
                events.push(QueuedEvent {
                    control: control.clone(),
                    key: key.clone(),
                    context_epoch: snapshot.context_epoch,
                    event,
                    handler,
                });
            };
            if change.value_changed() || toggled {
                if let Some(handler) = control.changed_handler.borrow().clone() {
                    queue("changed", handler);
                }
            }
            if let Some(handler) = control.submitted_handler.borrow().clone() {
                for _ in 0..submitted {
                    queue("submitted", handler.clone());
                }
            }
            if let Some(handler) = control.handler.borrow().clone() {
                for _ in 0..change.clicks {
                    queue("clicked", handler.clone());
                }
            }
        }
        // Validate and stage all fallible numeric and collection writes before
        // committing any observable Property mutation in this batch.
        for commit in writes.stage()? {
            commit();
        }
        self.sync()?;
        for event in events {
            self.dispatch(event)?;
        }
        self.sync()?;
        Ok(result)
    }

    /// Runs a queued handler against the control's current place in the scene, if it still has one.
    pub(super) fn dispatch(&mut self, event: QueuedEvent) -> Result<(), String> {
        let QueuedEvent {
            control,
            key,
            context_epoch,
            event,
            handler,
        } = event;
        if control.context_epoch.get() != context_epoch {
            return Ok(());
        }
        let current = if let Some(document) = &self.document {
            document
                .scene
                .iter()
                .enumerate()
                .find(|(_, item)| item.key == key)
                .map(|(index, item)| {
                    (
                        index,
                        item.event_args.get(event).cloned().unwrap_or_default(),
                    )
                })
        } else {
            Some((control.index, Vec::new()))
        };
        if let Some((index, args)) = current {
            control.dispatch_index.set(index);
            *control.dispatch_arguments.borrow_mut() = args;
            handler();
            self.sync()?;
        }
        Ok(())
    }
}

impl Drop for Form {
    fn drop(&mut self) {
        self.wake.callback.borrow_mut().take();
        #[cfg(not(target_arch = "wasm32"))]
        self.wake.native.borrow_mut().enable(false);
        // Detached handles do not leave model listeners or event callbacks alive.
        for control in &self.controls {
            control.bindings.borrow_mut().clear();
            control.collection_bindings.borrow_mut().clear();
            control.properties.borrow_mut().clear();
            control.handler.borrow_mut().take();
            control.changed_handler.borrow_mut().take();
            control.submitted_handler.borrow_mut().take();
            control.context_subscription.borrow_mut().take();
        }
    }
}
