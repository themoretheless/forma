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
                Binding::Group(group) => !group.present(),
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
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
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
