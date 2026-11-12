//! A control of a form and the bindings on it.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Target {
    Value,
    Text,
    Disabled,
    Checked,
    Selected,
}

pub(super) enum Binding {
    String(Field<String>, bool),
    Bool(Field<bool>),
    Number(Rc<dyn NumericInput>, bool),
}

impl Target {
    /// The document property a binding on this target writes.
    pub(super) fn property_name(self) -> &'static str {
        match self {
            Target::Value => "value",
            Target::Text => "text",
            Target::Disabled => "disabled",
            Target::Checked => "checked",
            Target::Selected => "selected",
        }
    }
}

impl Binding {
    /// The bound value as a document property; a slider takes its number as a percentage.
    pub(super) fn node_value(&self, slider: bool) -> crate::form_document::Value {
        match self {
            Binding::String(f, _) => f.get().unwrap_or_default().to_value(),
            Binding::Bool(f) => f.get().unwrap_or(false).to_value(),
            Binding::Number(f, _) if slider => {
                crate::form_document::Value::Expr(format!("{}%", f.number() * 100.))
            }
            Binding::Number(f, _) => crate::form_document::Value::Number(f.number()),
        }
    }

    pub(super) fn dirty(&self) -> bool {
        match self {
            Self::String(f, _) => f.0.dirty.get(),
            Self::Bool(f) => f.0.dirty.get(),
            Self::Number(f, _) => f.dirty(),
        }
    }
    pub(super) fn sync(
        &self,
        runtime: &mut Runtime,
        index: usize,
        target: Target,
    ) -> Result<(), String> {
        match self {
            Self::String(field, _) => {
                if !field.0.dirty.get() {
                    return Ok(());
                }
                let value = field.get().unwrap_or_default();
                if target == Target::Value {
                    runtime.set_binding_value(index, &value, field.0.reset.get())?;
                } else {
                    runtime.set_binding_text(index, &value)?;
                }
                field.0.dirty.set(false);
                field.0.reset.set(false);
            }
            Self::Number(field, _) => {
                field.clean();
            }
            Self::Bool(field) => {
                if !field.0.dirty.get() {
                    return Ok(());
                }
                if target == Target::Disabled {
                    runtime.set_binding_disabled(index, field.get().unwrap_or(true))?;
                }
                field.0.dirty.set(false);
                field.0.reset.set(false);
            }
        }
        Ok(())
    }
}

pub(super) type Handler = Rc<dyn Fn()>;

pub(super) struct ControlState {
    pub(super) index: usize,
    pub(super) resolved: Cell<Option<usize>>,
    pub(super) dispatch_index: Cell<usize>,
    pub(super) dispatch_arguments: RefCell<Vec<crate::form_document::Value>>,
    pub(super) collection_bindings: RefCell<Vec<CollectionBinding>>,
    pub(super) bindings: RefCell<BTreeMap<Target, Rc<Binding>>>,
    pub(super) properties: RefCell<BTreeMap<String, Box<dyn Input>>>,
    pub(super) handler: RefCell<Option<Handler>>,
    pub(super) changed_handler: RefCell<Option<Handler>>,
    pub(super) submitted_handler: RefCell<Option<Handler>>,
    pub(super) present: Rc<dyn Fn() -> bool>,
    pub(super) base_disabled: Cell<bool>,
    pub(super) context_changed: Rc<Cell<bool>>,
    pub(super) context_epoch: Rc<Cell<u64>>,
    pub(super) context_subscription: RefCell<Option<Subscription>>,
    pub(super) wake: Rc<Wake>,
}

/// Typed handle emitted for an explicitly keyed control in a generated form.
pub struct Control<C: 'static> {
    pub(super) context: Context<C>,
    pub(super) state: Rc<ControlState>,
}

impl<C> Clone for Control<C> {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            state: self.state.clone(),
        }
    }
}

pub struct Event<C> {
    pub context: Rc<C>,
    pub control_index: usize,
    pub arguments: Vec<crate::form_document::Value>,
}

impl<C: 'static> Control<C> {
    pub fn index(&self) -> usize {
        self.state.resolved.get().unwrap_or(usize::MAX)
    }
    pub fn context(&self) -> Context<C> {
        self.context.clone()
    }
    pub fn set_context(&self, value: Rc<C>) {
        self.context.set(value);
    }
    pub fn clear_context(&self) {
        self.context.clear();
    }
    pub fn inherit_context(&self) {
        self.context.inherit();
    }
    pub(super) fn insert(&self, target: Target, binding: Binding) {
        self.state
            .bindings
            .borrow_mut()
            .insert(target, Rc::new(binding));
    }
    /// Override a component property while retaining its dependent expressions.
    pub fn bind_property<T: ModelValue>(&self, name: &str, property: Property<T>) {
        self.state.properties.borrow_mut().insert(
            name.into(),
            Box::new(Field::explicit(property, self.state.wake.clone())),
        );
    }
    pub fn bind_property_path<T: ModelValue>(
        &self,
        name: &str,
        select: impl Fn(&C) -> Property<T> + 'static,
    ) {
        self.state.properties.borrow_mut().insert(
            name.into(),
            Box::new(Field::path(&self.context, select, self.state.wake.clone())),
        );
    }
    pub fn bind_value(&self, property: Property<String>) {
        self.insert(
            Target::Value,
            Binding::String(Field::explicit(property, self.state.wake.clone()), true),
        );
    }
    pub fn bind_value_path(
        &self,
        select: impl Fn(&C) -> Property<String> + 'static,
        two_way: bool,
    ) {
        self.insert(
            Target::Value,
            Binding::String(
                Field::path(&self.context, select, self.state.wake.clone()),
                two_way,
            ),
        );
    }
    pub fn bind_value_context<P: 'static>(
        &self,
        context: &Context<P>,
        select: impl Fn(&P) -> Property<String> + 'static,
        two_way: bool,
    ) {
        self.insert(
            Target::Value,
            Binding::String(
                Field::path(context, select, self.state.wake.clone()),
                two_way,
            ),
        );
    }
    pub fn bind_text(&self, property: Property<String>) {
        self.insert(
            Target::Text,
            Binding::String(Field::explicit(property, self.state.wake.clone()), false),
        );
    }
    pub fn bind_text_path(&self, select: impl Fn(&C) -> Property<String> + 'static) {
        self.insert(
            Target::Text,
            Binding::String(
                Field::path(&self.context, select, self.state.wake.clone()),
                false,
            ),
        );
    }
    pub fn bind_text_context<P: 'static>(
        &self,
        context: &Context<P>,
        select: impl Fn(&P) -> Property<String> + 'static,
    ) {
        self.insert(
            Target::Text,
            Binding::String(Field::path(context, select, self.state.wake.clone()), false),
        );
    }
    pub fn bind_disabled(&self, property: Property<bool>) {
        self.insert(
            Target::Disabled,
            Binding::Bool(Field::explicit(property, self.state.wake.clone())),
        );
    }
    pub fn bind_disabled_path(&self, select: impl Fn(&C) -> Property<bool> + 'static) {
        self.insert(
            Target::Disabled,
            Binding::Bool(Field::path(&self.context, select, self.state.wake.clone())),
        );
    }
    pub fn bind_disabled_context<P: 'static>(
        &self,
        context: &Context<P>,
        select: impl Fn(&P) -> Property<bool> + 'static,
    ) {
        self.insert(
            Target::Disabled,
            Binding::Bool(Field::path(context, select, self.state.wake.clone())),
        );
    }
    pub fn bind_range<T: ModelNumber>(&self, property: Property<T>) {
        self.insert(
            Target::Value,
            Binding::Number(
                Rc::new(Field::explicit(property, self.state.wake.clone())),
                true,
            ),
        );
    }
    pub fn bind_range_path<T: ModelNumber>(
        &self,
        select: impl Fn(&C) -> Property<T> + 'static,
        two_way: bool,
    ) {
        self.bind_range_context(&self.context, select, two_way);
    }
    pub fn bind_range_context<P: 'static, T: ModelNumber>(
        &self,
        context: &Context<P>,
        select: impl Fn(&P) -> Property<T> + 'static,
        two_way: bool,
    ) {
        self.insert(
            Target::Value,
            Binding::Number(
                Rc::new(Field::path(context, select, self.state.wake.clone())),
                two_way,
            ),
        );
    }
    pub fn bind_checked(&self, property: Property<bool>) {
        self.insert(
            Target::Checked,
            Binding::Bool(Field::explicit(property, self.state.wake.clone())),
        );
    }
    pub fn bind_checked_path(&self, select: impl Fn(&C) -> Property<bool> + 'static) {
        self.bind_checked_context(&self.context, select);
    }
    pub fn bind_checked_context<P: 'static>(
        &self,
        context: &Context<P>,
        select: impl Fn(&P) -> Property<bool> + 'static,
    ) {
        self.insert(
            Target::Checked,
            Binding::Bool(Field::path(context, select, self.state.wake.clone())),
        );
    }
    pub fn bind_selected_path(&self, select: impl Fn(&C) -> Property<bool> + 'static) {
        self.bind_selected_context(&self.context, select);
    }
    pub fn bind_selected_context<P: 'static>(
        &self,
        context: &Context<P>,
        select: impl Fn(&P) -> Property<bool> + 'static,
    ) {
        self.insert(
            Target::Selected,
            Binding::Bool(Field::path(context, select, self.state.wake.clone())),
        );
    }
    pub fn bind_collection_context<P: 'static, T: Clone + PartialEq + 'static>(
        &self,
        target: &str,
        indices: &[&str],
        value_path: &str,
        context: &Context<P>,
        select: impl Fn(&P) -> Property<Vec<T>> + 'static,
        set: impl Fn(&mut T, &[usize], &crate::form_document::Value) -> Result<(), String> + 'static,
    ) {
        let field = Field::path(context, select, self.state.wake.clone());
        self.state
            .collection_bindings
            .borrow_mut()
            .push(CollectionBinding {
                target: target.into(),
                indices: indices.iter().map(|s| (*s).into()).collect(),
                value_path: value_path.into(),
                field: Rc::new(CollectionField {
                    field,
                    set: Rc::new(set),
                }),
            });
    }
    pub fn on_changed(&self, handler: impl Fn(Event<C>) + 'static) {
        let context = self.context.clone();
        let state = self.state.clone();
        *self.state.changed_handler.borrow_mut() = Some(Rc::new(move || {
            if let Some(context) = context.get() {
                handler(Event {
                    context,
                    control_index: state.dispatch_index.get(),
                    arguments: state.dispatch_arguments.borrow().clone(),
                });
            }
        }));
    }
    pub fn on_submitted(&self, handler: impl Fn(Event<C>) + 'static) {
        let context = self.context.clone();
        let state = self.state.clone();
        *self.state.submitted_handler.borrow_mut() = Some(Rc::new(move || {
            if let Some(context) = context.get() {
                handler(Event {
                    context,
                    control_index: state.dispatch_index.get(),
                    arguments: state.dispatch_arguments.borrow().clone(),
                });
            }
        }));
    }
    pub fn on_clicked(&self, handler: impl Fn(Event<C>) + 'static) {
        let context = self.context.clone();
        let state = self.state.clone();
        *self.state.handler.borrow_mut() = Some(Rc::new(move || {
            if let Some(context) = context.get() {
                handler(Event {
                    context,
                    control_index: state.dispatch_index.get(),
                    arguments: state.dispatch_arguments.borrow().clone(),
                });
            }
        }));
    }
    pub fn disconnect_clicked(&self) {
        self.state.handler.borrow_mut().take();
    }
}

#[derive(Clone)]
pub(super) struct CollectionBinding {
    pub(super) target: String,
    pub(super) indices: Vec<String>,
    pub(super) value_path: String,
    pub(super) field: Rc<dyn CollectionWrite>,
}
