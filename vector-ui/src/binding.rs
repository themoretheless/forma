//! Typed, UI-thread bindings used by generated forms. Model notifications only
//! mark bindings dirty; `Form::update` / `Form::sync` apply them before painting.
use crate::Runtime;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

/// Disconnects a listener on drop. Keeping a subscription never keeps its source alive.
pub struct Subscription(Option<Box<dyn FnOnce()>>);
impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(disconnect) = self.0.take() {
            disconnect();
        }
    }
}

type Listener<T> = Rc<dyn Fn(T)>;
struct PropertyInner<T> {
    value: RefCell<T>,
    next: Cell<u64>,
    listeners: RefCell<Vec<(u64, Listener<T>)>>,
}
/// An observable field. Clones share a field; creating another Property creates
/// independent state. Mutate through `set` so every attached form is notified.
pub struct Property<T>(Rc<PropertyInner<T>>);
impl<T> Clone for Property<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: Clone + PartialEq + 'static> Property<T> {
    pub fn new(value: T) -> Self {
        Self(Rc::new(PropertyInner {
            value: RefCell::new(value),
            next: Cell::new(0),
            listeners: RefCell::new(Vec::new()),
        }))
    }
    pub fn get(&self) -> T {
        self.0.value.borrow().clone()
    }
    pub fn set(&self, value: T) -> bool {
        if *self.0.value.borrow() == value {
            return false;
        }
        *self.0.value.borrow_mut() = value;
        // Fast-path for 0 or 1 listener; snapshot IDs when multiple listeners exist
        // so a listener disconnected during notification cannot receive a stale call.
        let len = self.0.listeners.borrow().len();
        if len == 0 {
            return true;
        }
        if len == 1 {
            let callback = self.0.listeners.borrow()[0].1.clone();
            callback(self.get());
            return true;
        }
        let ids: Vec<u64> = self
            .0
            .listeners
            .borrow()
            .iter()
            .map(|&(id, _)| id)
            .collect();
        for id in ids {
            let callback = self
                .0
                .listeners
                .borrow()
                .iter()
                .find_map(|(k, cb)| (*k == id).then(|| cb.clone()));
            if let Some(callback) = callback {
                callback(self.get());
            }
        }
        true
    }
    pub fn subscribe(&self, callback: impl Fn(T) + 'static) -> Subscription {
        let id = self.0.next.get();
        self.0.next.set(id + 1);
        self.0.listeners.borrow_mut().push((id, Rc::new(callback)));
        let source = Rc::downgrade(&self.0);
        Subscription(Some(Box::new(move || {
            if let Some(source) = source.upgrade() {
                source.listeners.borrow_mut().retain(|(k, _)| *k != id);
            }
        })))
    }
}

struct Identity<C>(Option<Rc<C>>);
impl<C> Clone for Identity<C> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<C> PartialEq for Identity<C> {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}
struct ContextInner<C: 'static> {
    value: Property<Identity<C>>,
    inherited: RefCell<Option<Rc<C>>>,
    overridden: Cell<bool>,
    parent: RefCell<Option<Box<dyn Any>>>,
    parent_subscription: RefCell<Option<Subscription>>,
    selected: RefCell<Option<Context<C>>>,
    selected_subscription: RefCell<Option<Subscription>>,
}
/// A replaceable context. Child contexts follow their parent until explicitly
/// overridden. `clear` is an explicit empty context; `inherit` restores following.
pub struct Context<C: 'static>(Rc<ContextInner<C>>);
impl<C> Clone for Context<C> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<C: 'static> Context<C> {
    pub fn empty() -> Self {
        Self(Rc::new(ContextInner {
            value: Property::new(Identity(None)),
            inherited: RefCell::new(None),
            overridden: Cell::new(false),
            parent: RefCell::new(None),
            parent_subscription: RefCell::new(None),
            selected: RefCell::new(None),
            selected_subscription: RefCell::new(None),
        }))
    }
    pub fn new(value: Rc<C>) -> Self {
        let context = Self::empty();
        context.set(value);
        context
    }
    pub fn get(&self) -> Option<Rc<C>> {
        self.0.value.get().0
    }
    pub fn set(&self, value: Rc<C>) {
        self.0.overridden.set(true);
        self.0.value.set(Identity(Some(value)));
    }
    pub fn clear(&self) {
        self.0.overridden.set(true);
        self.0.value.set(Identity(None));
    }
    pub fn inherit(&self) {
        self.0.overridden.set(false);
        let value = self.0.inherited.borrow().clone();
        self.0.value.set(Identity(value));
    }
    fn receive(&self, value: Option<Rc<C>>) {
        *self.0.inherited.borrow_mut() = value.clone();
        if !self.0.overridden.get() {
            self.0.value.set(Identity(value));
        }
    }
    pub fn subscribe(&self, callback: impl Fn(Option<Rc<C>>) + 'static) -> Subscription {
        self.0.value.subscribe(move |value| callback(value.0))
    }
    pub fn child(parent: &Self) -> Self {
        let context = Self::empty();
        context.receive(parent.get());
        let weak = Rc::downgrade(&context.0);
        *context.0.parent_subscription.borrow_mut() = Some(parent.subscribe(move |value| {
            if let Some(context) = weak.upgrade() {
                Context(context).receive(value);
            }
        }));
        *context.0.parent.borrow_mut() = Some(Box::new(parent.clone()));
        context
    }
    /// `select` resolves a nested context in the *parent* scope. Both replacement
    /// of the parent and replacement of its selected object are observed.
    pub fn project<P: 'static>(parent: &Context<P>, select: impl Fn(&P) -> Self + 'static) -> Self {
        let context = Self::empty();
        let weak = Rc::downgrade(&context.0);
        let refresh = move |value: Option<Rc<P>>| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.selected_subscription.borrow_mut().take();
            inner.selected.borrow_mut().take();
            let context = Context(inner);
            if let Some(value) = value {
                let selected = select(&value);
                context.receive(selected.get());
                let weak = Rc::downgrade(&context.0);
                *context.0.selected_subscription.borrow_mut() =
                    Some(selected.subscribe(move |value| {
                        if let Some(inner) = weak.upgrade() {
                            Context(inner).receive(value);
                        }
                    }));
                *context.0.selected.borrow_mut() = Some(selected);
            } else {
                context.receive(None);
            }
        };
        refresh(parent.get());
        *context.0.parent_subscription.borrow_mut() = Some(parent.subscribe(refresh));
        *context.0.parent.borrow_mut() = Some(Box::new(parent.clone()));
        context
    }
}

struct FieldInner<T> {
    property: RefCell<Option<Property<T>>>,
    dirty: Cell<bool>,
    reset: Cell<bool>,
    epoch: Cell<u64>,
    revision: Cell<u64>,
    property_subscription: RefCell<Option<Subscription>>,
    context_subscription: RefCell<Option<Subscription>>,
    context: RefCell<Option<Box<dyn Any>>>,
    wake: Rc<Wake>,
    #[cfg(not(target_arch = "wasm32"))]
    native_key: Cell<u64>,
}
#[derive(Default)]
struct Wake {
    pending: Cell<bool>,
    #[cfg(not(target_arch = "wasm32"))]
    native: RefCell<NativeBindings>,
    callback: RefCell<Option<Rc<dyn Fn()>>>,
}
impl Wake {
    fn notify(&self) {
        if !self.pending.replace(true) {
            let callback = self.callback.borrow().clone();
            if let Some(callback) = callback {
                callback();
            }
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct NativeBindings {
    next: u64,
    fields: BTreeMap<u64, Rc<dyn Fn()>>,
    graph: Option<svet_wasm::native::Context>,
    nodes: BTreeMap<u64, (svet_wasm::native::Node, svet_wasm::native::Node)>,
    jobs: std::collections::HashMap<svet_wasm::native::Node, u64>,
}
#[cfg(not(target_arch = "wasm32"))]
impl NativeBindings {
    fn add_nodes(&mut self, key: u64) {
        use svet_wasm::native::Kind;
        if let Some(graph) = &mut self.graph {
            let source = graph.create(Kind::Signal).expect("native binding source");
            let effect = graph.create(Kind::Effect).expect("native binding effect");
            graph
                .track(&effect, |scope| scope.read(&source))
                .unwrap()
                .unwrap();
            self.jobs.insert(effect.clone(), key);
            self.nodes.insert(key, (source, effect));
        }
    }
    fn register(&mut self, dirty: Rc<dyn Fn()>) -> u64 {
        self.next = self
            .next
            .checked_add(1)
            .expect("native binding identity exhausted");
        let key = self.next;
        self.fields.insert(key, dirty);
        self.add_nodes(key);
        key
    }
    fn remove(&mut self, key: u64) {
        self.fields.remove(&key);
        if let Some((source, effect)) = self.nodes.remove(&key) {
            self.jobs.remove(&effect);
            if let Some(graph) = &mut self.graph {
                graph.dispose(&effect).unwrap();
                graph.dispose(&source).unwrap();
            }
        }
    }
    fn notify(&mut self, key: u64) -> bool {
        if let (Some(graph), Some((source, _))) = (&mut self.graph, self.nodes.get(&key)) {
            graph.notify(source).unwrap();
            true
        } else {
            false
        }
    }
    fn drain(&mut self) -> usize {
        let Some(graph) = &mut self.graph else {
            return 0;
        };
        let jobs = graph.take_effects();
        let count = jobs.len();
        for job in jobs {
            if let Some(key) = self.jobs.get(&job) {
                let (source, effect) = self.nodes.get(key).unwrap();
                graph
                    .track(effect, |scope| scope.read(source))
                    .unwrap()
                    .unwrap();
                if let Some(dirty) = self.fields.get(key) {
                    dirty();
                }
            }
        }
        count
    }
    fn enable(&mut self, enabled: bool) {
        self.drain();
        if enabled == self.graph.is_some() {
            return;
        }
        self.nodes.clear();
        self.jobs.clear();
        self.graph = enabled.then(svet_wasm::native::Context::new);
        for key in self.fields.keys().copied().collect::<Vec<_>>() {
            self.add_nodes(key);
        }
    }
}
impl<T> FieldInner<T> {
    fn invalidate(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        let scheduled = self.wake.native.borrow_mut().notify(self.native_key.get());
        #[cfg(target_arch = "wasm32")]
        let scheduled = false;
        if !scheduled {
            self.dirty.set(true);
        }
        self.wake.notify();
    }
}
impl<T> Drop for FieldInner<T> {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.wake.native.borrow_mut().remove(self.native_key.get());
    }
}
struct Field<T>(Rc<FieldInner<T>>);
impl<T> Clone for Field<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: Clone + PartialEq + 'static> Field<T> {
    fn empty(wake: Rc<Wake>) -> Self {
        let field = Rc::new(FieldInner {
            property: RefCell::new(None),
            dirty: Cell::new(true),
            reset: Cell::new(true),
            epoch: Cell::new(0),
            revision: Cell::new(0),
            property_subscription: RefCell::new(None),
            context_subscription: RefCell::new(None),
            context: RefCell::new(None),
            wake,
            #[cfg(not(target_arch = "wasm32"))]
            native_key: Cell::new(0),
        });
        #[cfg(not(target_arch = "wasm32"))]
        {
            let weak = Rc::downgrade(&field);
            let key = field.wake.native.borrow_mut().register(Rc::new(move || {
                if let Some(field) = weak.upgrade() {
                    field.dirty.set(true);
                }
            }));
            field.native_key.set(key);
        }
        Self(field)
    }
    fn attach(&self, property: Option<Property<T>>) {
        self.0.epoch.set(self.0.epoch.get().wrapping_add(1));
        self.0.revision.set(self.0.revision.get().wrapping_add(1));
        self.0.property_subscription.borrow_mut().take();
        if let Some(property) = &property {
            let weak = Rc::downgrade(&self.0);
            *self.0.property_subscription.borrow_mut() = Some(property.subscribe(move |_| {
                if let Some(field) = weak.upgrade() {
                    field.revision.set(field.revision.get().wrapping_add(1));
                    field.invalidate();
                }
            }));
        }
        *self.0.property.borrow_mut() = property;
        self.0.reset.set(true);
        self.0.invalidate();
    }
    fn explicit(property: Property<T>, wake: Rc<Wake>) -> Self {
        let field = Self::empty(wake);
        field.attach(Some(property));
        field
    }
    fn path<C: 'static>(
        context: &Context<C>,
        select: impl Fn(&C) -> Property<T> + 'static,
        wake: Rc<Wake>,
    ) -> Self {
        let field = Self::empty(wake);
        let weak = Rc::downgrade(&field.0);
        let refresh = move |value: Option<Rc<C>>| {
            if let Some(field) = weak.upgrade() {
                Field(field).attach(value.map(|value| select(&value)));
            }
        };
        refresh(context.get());
        *field.0.context_subscription.borrow_mut() = Some(context.subscribe(refresh));
        *field.0.context.borrow_mut() = Some(Box::new(context.clone()));
        field
    }
    fn get(&self) -> Option<T> {
        self.0.property.borrow().as_ref().map(Property::get)
    }
    fn set(&self, value: T) {
        let property = self.0.property.borrow().clone();
        if let Some(property) = property {
            property.set(value);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Value,
    Text,
    Disabled,
    Checked,
    Selected,
}
enum Binding {
    String(Field<String>, bool),
    Bool(Field<bool>),
    Number(Rc<dyn NumericInput>, bool),
}
impl Binding {
    fn dirty(&self) -> bool {
        match self {
            Self::String(f, _) => f.0.dirty.get(),
            Self::Bool(f) => f.0.dirty.get(),
            Self::Number(f, _) => f.dirty(),
        }
    }
    fn sync(&self, runtime: &mut Runtime, index: usize, target: Target) -> Result<(), String> {
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
type Handler = Rc<dyn Fn()>;
struct ControlState {
    index: usize,
    resolved: Cell<Option<usize>>,
    dispatch_index: Cell<usize>,
    dispatch_arguments: RefCell<Vec<crate::form_document::Value>>,
    collection_bindings: RefCell<Vec<CollectionBinding>>,
    bindings: RefCell<BTreeMap<Target, Rc<Binding>>>,
    properties: RefCell<BTreeMap<String, Box<dyn Input>>>,
    handler: RefCell<Option<Handler>>,
    changed_handler: RefCell<Option<Handler>>,
    submitted_handler: RefCell<Option<Handler>>,
    present: Rc<dyn Fn() -> bool>,
    base_disabled: Cell<bool>,
    context_changed: Rc<Cell<bool>>,
    context_epoch: Rc<Cell<u64>>,
    context_subscription: RefCell<Option<Subscription>>,
    wake: Rc<Wake>,
}
/// Typed handle emitted for an explicitly keyed control in a generated form.
pub struct Control<C: 'static> {
    context: Context<C>,
    state: Rc<ControlState>,
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
    fn insert(&self, target: Target, binding: Binding) {
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

/// Values available to generated expressions. Numeric fields remain typed in Rust.
pub trait ModelValue: Clone + PartialEq + Default + 'static {
    fn to_value(&self) -> crate::form_document::Value;
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        Err(format!(
            "Cannot decode {value:?} as {}",
            std::any::type_name::<Self>()
        ))
    }
}
pub fn decode<T: ModelValue>(value: &crate::form_document::Value) -> Result<T, String> {
    T::from_value(value)
}

impl ModelValue for String {
    fn to_value(&self) -> crate::form_document::Value {
        crate::form_document::Value::Text(self.clone())
    }
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        if let crate::form_document::Value::Text(value) = value {
            Ok(value.clone())
        } else {
            Err("Expected String event argument".into())
        }
    }
}
impl<T: ModelValue> ModelValue for Option<T> {
    fn to_value(&self) -> crate::form_document::Value {
        self.as_ref()
            .map(ModelValue::to_value)
            .unwrap_or(crate::form_document::Value::Null)
    }
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        if matches!(value, crate::form_document::Value::Null) {
            Ok(None)
        } else {
            T::from_value(value).map(Some)
        }
    }
}
impl<T: ModelValue> ModelValue for BTreeMap<String, T> {
    fn to_value(&self) -> crate::form_document::Value {
        crate::form_document::Value::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), v.to_value()))
                .collect(),
        )
    }
}
impl<T: ModelValue> ModelValue for Vec<T> {
    fn to_value(&self) -> crate::form_document::Value {
        crate::form_document::Value::List(self.iter().map(ModelValue::to_value).collect())
    }
}
impl ModelValue for bool {
    fn to_value(&self) -> crate::form_document::Value {
        crate::form_document::Value::Bool(*self)
    }
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        value.boolean()
    }
}
macro_rules! numbers { ($($t:ty),*) => {$ (impl ModelValue for $t {
    fn to_value(&self)->crate::form_document::Value { crate::form_document::Value::Number(*self as f64) }
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        let n=value.number()?; let result=n as $t;
        if result as f64 != n { return Err(format!("Number {n} cannot be represented as {}", stringify!($t))); }
        Ok(result)
    }
})*}; }
numbers!(f64, f32, i32, u32, i64, u64, usize);
pub trait ModelNumber: ModelValue {}
macro_rules! numeric {($($t:ty),*)=>{$(impl ModelNumber for $t {})*};}
numeric!(f64, f32, i32, u32, i64, u64, usize);
trait Input {
    fn present(&self) -> bool;
    fn dirty(&self) -> bool;
    fn value(&self) -> crate::form_document::Value;
    fn clean(&self);
}
impl<T: ModelValue> Input for Field<T> {
    fn present(&self) -> bool {
        self.get().is_some()
    }
    fn dirty(&self) -> bool {
        self.0.dirty.get()
    }
    fn value(&self) -> crate::form_document::Value {
        self.get().unwrap_or_default().to_value()
    }
    fn clean(&self) {
        self.0.dirty.set(false);
    }
}
trait NumericInput {
    fn present(&self) -> bool;
    fn dirty(&self) -> bool;
    fn epoch(&self) -> u64;
    fn number(&self) -> f64;
    fn clean(&self);
    fn set_number(&self, value: f64) -> Result<(), String>;
}
impl<T: ModelNumber> NumericInput for Field<T> {
    fn present(&self) -> bool {
        self.get().is_some()
    }
    fn dirty(&self) -> bool {
        self.0.dirty.get()
    }
    fn epoch(&self) -> u64 {
        self.0.epoch.get()
    }
    fn number(&self) -> f64 {
        self.get()
            .unwrap_or_default()
            .to_value()
            .number()
            .unwrap_or_default()
    }
    fn clean(&self) {
        self.0.dirty.set(false);
        self.0.reset.set(false);
    }
    fn set_number(&self, value: f64) -> Result<(), String> {
        self.set(T::from_value(&crate::form_document::Value::Number(value))?);
        Ok(())
    }
}
struct ProjectedInput<T> {
    field: Field<T>,
    project: Rc<dyn Fn(&T) -> crate::form_document::Value>,
}
impl<T: Clone + PartialEq + Default + 'static> Input for ProjectedInput<T> {
    fn present(&self) -> bool {
        self.field.get().is_some()
    }
    fn dirty(&self) -> bool {
        self.field.0.dirty.get()
    }
    fn value(&self) -> crate::form_document::Value {
        (self.project)(&self.field.get().unwrap_or_default())
    }
    fn clean(&self) {
        self.field.0.dirty.set(false);
    }
}
trait CollectionWrite {
    fn epoch(&self) -> u64;
    fn write(&self, indices: &[usize], value: &crate::form_document::Value) -> Result<(), String>;
}
struct CollectionField<T> {
    field: Field<Vec<T>>,
    set: Rc<dyn Fn(&mut T, &[usize], &crate::form_document::Value) -> Result<(), String>>,
}
impl<T: Clone + PartialEq + 'static> CollectionWrite for CollectionField<T> {
    fn epoch(&self) -> u64 {
        self.field.0.revision.get()
    }
    fn write(&self, indices: &[usize], value: &crate::form_document::Value) -> Result<(), String> {
        let Some(mut items) = self.field.get() else {
            return Ok(());
        };
        let Some(index) = indices.first() else {
            return Err("Collection binding requires an item index".into());
        };
        let Some(item) = items.get_mut(*index) else {
            return Err("Repeated control no longer has a collection item".into());
        };
        (self.set)(item, indices, value)?;
        self.field.set(items);
        Ok(())
    }
}
#[derive(Clone)]
struct CollectionBinding {
    target: String,
    indices: Vec<String>,
    value_path: String,
    field: Rc<dyn CollectionWrite>,
}
struct DocumentState {
    program: crate::form_document::Document,
    inputs: Vec<(usize, String, Box<dyn Input>)>,
    nodes: BTreeMap<usize, usize>,
    node_controls: BTreeMap<usize, usize>,
    scene: Vec<crate::form_document::SceneControl>,
    event_types: BTreeMap<(usize, String), Vec<String>>,
    base_disabled: Vec<bool>,
}

/// Owns one renderer and its binding connections. Drive input through `update`,
/// and call `sync` before painting after model changes outside an input callback.
/// No mutable renderer reference escapes, so source writes cannot be missed.
pub struct Form {
    document: Option<DocumentState>,
    runtime: Runtime,
    controls: Vec<Rc<ControlState>>,
    wake: Rc<Wake>,
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
    fn disabled(&self, control: &ControlState) -> bool {
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
    pub fn svet_binding_telemetry(&self) -> Option<svet_wasm::native::Telemetry> {
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
            if let Some(document) = &mut self.document {
                let has_control_overrides = self.controls.iter().any(|c| {
                    document.nodes.contains_key(&c.index)
                        && (!c.properties.borrow().is_empty() || !c.bindings.borrow().is_empty())
                });
                let mut saved_root = has_control_overrides.then(|| document.program.root.clone());
                let saved_states = document.program.states.clone();
                for (node, path, input) in &document.inputs {
                    while document.program.states.len() <= *node {
                        document.program.states.push(Default::default());
                    }
                    if input.present() {
                        document.program.states[*node].insert(path.clone(), input.value());
                    } else {
                        let val = input.value();
                        if matches!(val, crate::form_document::Value::List(_)) {
                            document.program.states[*node].insert(path.clone(), val);
                        }
                    }
                }
                if has_control_overrides {
                    fn collect_nodes_mut<'a>(
                        node: &'a mut crate::form_document::Node,
                        out: &mut std::collections::HashMap<usize, &'a mut crate::form_document::Node>,
                        count: &mut usize,
                    ) {
                        let id = node
                            .props
                            .get("__node")
                            .and_then(|v| v.number().ok())
                            .map_or(*count, |v| v as usize);
                        *count += 1;
                        let ptr = node as *mut crate::form_document::Node;
                        for child in &mut node.children {
                            collect_nodes_mut(child, out, count);
                        }
                        // SAFETY: `ptr` points to `node` while `children` points to disjoint heap allocations.
                        out.insert(id, unsafe { &mut *ptr });
                    }
                    let mut node_map = std::collections::HashMap::new();
                    collect_nodes_mut(&mut document.program.root, &mut node_map, &mut 0);
                    for control in &self.controls {
                        if let Some(index) = document.nodes.get(&control.index) {
                            if let Some(node) = node_map.get_mut(index) {
                                for (name, input) in control.properties.borrow().iter() {
                                    node.props.insert(name.clone(), input.value());
                                }
                                for (target, binding) in control.bindings.borrow().iter() {
                                    let name = match target {
                                        Target::Value => "value",
                                        Target::Text => "text",
                                        Target::Disabled => "disabled",
                                        Target::Checked => "checked",
                                        Target::Selected => "selected",
                                    };
                                    let value = match binding.as_ref() {
                                        Binding::String(f, _) => {
                                            f.get().unwrap_or_default().to_value()
                                        }
                                        Binding::Bool(f) => f.get().unwrap_or(false).to_value(),
                                        Binding::Number(f, _) if node.kind == "Slider" => {
                                            crate::form_document::Value::Expr(format!(
                                                "{}%",
                                                f.number() * 100.
                                            ))
                                        }
                                        Binding::Number(f, _) => {
                                            crate::form_document::Value::Number(f.number())
                                        }
                                    };
                                    node.props.insert(name.into(), value);
                                }
                            }
                        }
                    }
                }
                let render_res = document.program.render_scene_artifact();
                if let Some(orig_root) = saved_root.take() {
                    document.program.root = orig_root;
                }
                let artifact = match render_res {
                    Ok(a) => a,
                    Err(e) => {
                        document.program.states = saved_states;
                        return Err(e);
                    }
                };
                let scene = &artifact.controls;
                for item in scene {
                    for ((node, event), types) in &document.event_types {
                        if *node != item.node {
                            continue;
                        }
                        let empty_args = Vec::new();
                        let arguments = item.event_args.get(event).unwrap_or(&empty_args);
                        if arguments.len() != types.len() {
                            document.program.states = saved_states;
                            return Err(format!("Event {event} expects {} arguments", types.len()));
                        }
                        for (value, kind) in arguments.iter().zip(types) {
                            let valid = if kind.ends_with('?')
                                && matches!(value, crate::form_document::Value::Null)
                            {
                                true
                            } else {
                                match kind.trim_end_matches('?') {
                                    "String" => {
                                        matches!(value, crate::form_document::Value::Text(_))
                                    }
                                    "Bool" | "Boolean" => {
                                        matches!(value, crate::form_document::Value::Bool(_))
                                    }
                                    "Number" | "Float" => value.number().is_ok(),
                                    "Int" => i64::from_value(value).is_ok(),
                                    _ => false,
                                }
                            };
                            if !valid {
                                document.program.states = saved_states;
                                return Err(format!("Event {event} expects {kind}, got {value:?}"));
                            }
                        }
                    }
                }
                let mut next = match Runtime::from_scene_artifact(&artifact) {
                    Ok(r) => r,
                    Err(e) => {
                        document.program.states = saved_states;
                        return Err(e);
                    }
                };
                next.preserve_binding_interaction(&self.runtime);
                document.base_disabled = (0..next.control_count())
                    .map(|i| next.control_disabled(i))
                    .collect();
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
            }
        }
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
            if self.document.is_some()
                && (control.present)()
                && !control.bindings.borrow().contains_key(&Target::Disabled)
            {
                disabled = self
                    .document
                    .as_ref()
                    .unwrap()
                    .base_disabled
                    .get(*index)
                    .copied()
                    .unwrap_or(true);
            }
            self.runtime.set_binding_disabled(*index, disabled)?;
        }
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
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
    fn sync_instances(&self) -> Vec<(Rc<ControlState>, usize)> {
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
            self.controls
                .iter()
                .map(|c| (c.clone(), c.index))
                .collect()
        }
    }
    fn instances(
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
            .map(|(c, index, _, _)| {
                (
                    self.runtime.text_value(*index),
                    self.runtime.range_value(*index),
                    self.runtime.control_clicks(*index),
                    self.runtime.control_submissions(*index),
                    c.context_epoch.get(),
                    c.bindings
                        .borrow()
                        .iter()
                        .map(|(target, binding)| {
                            (
                                *target,
                                binding.clone(),
                                match binding.as_ref() {
                                    Binding::String(f, _) => f.0.epoch.get(),
                                    Binding::Number(f, _) => f.epoch(),
                                    Binding::Bool(f) => f.0.epoch.get(),
                                },
                            )
                        })
                        .collect::<Vec<_>>(),
                    c.collection_bindings
                        .borrow()
                        .iter()
                        .map(|b| (b.clone(), b.field.epoch()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let result = input(&mut self.runtime);
        let mut string_writes = Vec::new();
        let mut number_writes = Vec::new();
        let mut bool_writes = Vec::new();
        let mut collection_writes = Vec::new();
        let mut events = Vec::new();
        for (
            (control, index, locals, key),
            (text, range, clicks, submissions, context_epoch, bindings, collections),
        ) in instances.into_iter().zip(before)
        {
            if control.context_epoch.get() != context_epoch {
                continue;
            }
            let next_text = self.runtime.text_value(index);
            let next_range = self.runtime.range_value(index);
            let count = self.runtime.control_clicks(index).saturating_sub(clicks);
            let submitted = self.runtime.control_submissions(index).saturating_sub(submissions);
            let changed = text != next_text || range.is_finite() && range != next_range;
            let mut toggled = false;
            for (target, binding, epoch) in bindings {
                if !control
                    .bindings
                    .borrow()
                    .get(&target)
                    .is_some_and(|current| Rc::ptr_eq(current, &binding))
                {
                    continue;
                }
                match binding.as_ref() {
                    Binding::String(field, true)
                        if target == Target::Value && text != next_text =>
                    {
                        string_writes.push((field.clone(), next_text.clone(), epoch))
                    }
                    Binding::Number(field, true)
                        if target == Target::Value && range.is_finite() && range != next_range =>
                    {
                        number_writes.push((field.clone(), next_range as f64, epoch))
                    }
                    Binding::Number(field, true)
                        if target == Target::Value && text != next_text =>
                    {
                        let value = next_text
                            .parse::<f64>()
                            .map_err(|_| "Numeric input expects a finite number")?;
                        number_writes.push((field.clone(), value, epoch));
                    }
                    Binding::Bool(field)
                        if (target == Target::Checked || target == Target::Selected)
                            && count % 2 == 1 =>
                    {
                        bool_writes.push((
                            field.clone(),
                            if target == Target::Selected {
                                true
                            } else {
                                !field.get().unwrap_or(false)
                            },
                            epoch,
                        ));
                        toggled = true;
                    }
                    _ => {}
                }
            }
            for (binding, epoch) in collections {
                let item_indices = binding
                    .indices
                    .iter()
                    .map(|key| {
                        locals
                            .get(key)
                            .and_then(|v| v.number().ok())
                            .map(|v| v as usize)
                    })
                    .collect::<Option<Vec<_>>>();
                let Some(item_indices) = item_indices else {
                    continue;
                };
                let value = match binding.target.as_str() {
                    "value" if range.is_finite() && range != next_range => {
                        Some(crate::form_document::Value::Number(next_range as f64))
                    }
                    "value" if text != next_text => {
                        Some(crate::form_document::Value::Text(next_text.clone()))
                    }
                    "checked" if count % 2 == 1 => {
                        toggled = true;
                        Some(crate::form_document::Value::Bool(
                            !crate::form_document::Value::Expr(binding.value_path.clone())
                                .evaluate(&Default::default(), &locals)?
                                .boolean()?,
                        ))
                    }
                    "selected" if count > 0 => {
                        toggled = true;
                        Some(crate::form_document::Value::Bool(true))
                    }
                    _ => None,
                };
                if let Some(value) = value {
                    collection_writes.push((binding.field, item_indices, value, epoch));
                }
            }
            if changed || toggled {
                if let Some(handler) = control.changed_handler.borrow().clone() {
                    events.push((
                        control.clone(),
                        key.clone(),
                        context_epoch,
                        "changed",
                        handler,
                    ));
                }
            }
            if let Some(handler) = control.submitted_handler.borrow().clone() {
                for _ in 0..submitted {
                    events.push((
                        control.clone(),
                        key.clone(),
                        context_epoch,
                        "submitted",
                        handler.clone(),
                    ));
                }
            }
            if let Some(handler) = control.handler.borrow().clone() {
                for _ in 0..count {
                    events.push((
                        control.clone(),
                        key.clone(),
                        context_epoch,
                        "clicked",
                        handler.clone(),
                    ));
                }
            }
        }
        for (field, value, epoch) in string_writes {
            if field.0.epoch.get() == epoch {
                field.set(value);
            }
        }
        for (field, value, epoch) in number_writes {
            if field.epoch() == epoch {
                field.set_number(value)?;
            }
        }
        for (field, value, epoch) in bool_writes {
            if field.0.epoch.get() == epoch {
                field.set(value);
            }
        }
        for (field, index, value, epoch) in collection_writes {
            if field.epoch() == epoch {
                field.write(&index, &value)?;
            }
        }
        self.sync()?;
        for (control, key, epoch, event, handler) in events {
            if control.context_epoch.get() != epoch {
                continue;
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
        }
        self.sync()?;
        Ok(result)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn svet_bindings_drive_runtime_and_release_retained_control() {
        let mut form = Form::new(
            "component Demo { Frame { width:100; height:60; Button { width:80; height:40; } } }",
            "component Button { Rectangle { ContentInput { width:80; height:30; value:'seed'; } PointerArea { clicked -> events.clicked(); } } }",
        ).unwrap();
        let control = form.add_control(0, Context::new(Rc::new(()))).unwrap();
        let property = Property::new("first".to_string());
        control.bind_value(property.clone());
        form.set_svet_bindings(true);
        form.sync().unwrap();
        assert_eq!(form.runtime().text_value(0), "first");
        assert_eq!(form.svet_binding_telemetry().unwrap().nodes, 2);
        property.set("second".into());
        property.set("third".into());
        let binding = control
            .state
            .bindings
            .borrow()
            .get(&Target::Value)
            .unwrap()
            .clone();
        assert!(!binding.dirty(), "only graph drain may dirty this binding");
        form.sync().unwrap();
        assert_eq!(form.runtime().text_value(0), "third");
        form.update(|runtime| {
            runtime.focus_control(0);
            runtime.text_insert("!");
        })
        .unwrap();
        assert_eq!(property.get(), form.runtime().text_value(0));
        assert!(property.get().contains('!'));
        property.set("pending".into());
        form.set_svet_bindings(false);
        form.sync().unwrap();
        assert_eq!(form.runtime().text_value(0), "pending");
        form.set_svet_bindings(true);
        drop(binding);
        property.set("stale".into());
        let replacement = Property::new("replacement".to_string());
        control.bind_value(replacement.clone());
        assert_eq!(form.svet_binding_telemetry().unwrap().nodes, 2);
        form.sync().unwrap();
        assert_eq!(form.runtime().text_value(0), "replacement");
        property.set("retired".into());
        assert!(!form.is_dirty());
        let wake = form.wake.clone();
        drop(form);
        assert!(wake.native.borrow().graph.is_none());
        assert!(wake.native.borrow().fields.is_empty());
        assert!(replacement.0.listeners.borrow().is_empty());
        drop(control);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn svet_scheduler_deduplicates_and_isolates_forms() {
        let wake = Rc::new(Wake::default());
        let property = Property::new(0);
        let field = Field::explicit(property.clone(), wake.clone());
        wake.native.borrow_mut().enable(true);
        field.0.dirty.set(false);
        for value in 1..100 {
            property.set(value);
        }
        assert!(!field.0.dirty.get());
        assert_eq!(wake.native.borrow_mut().drain(), 1);
        assert!(field.0.dirty.get());
        field.0.dirty.set(false);
        property.set(100);
        assert_eq!(wake.native.borrow_mut().drain(), 1);
        let other = Rc::new(Wake::default());
        let other_field = Field::explicit(Property::new(0), other.clone());
        other.native.borrow_mut().enable(true);
        other_field.0.dirty.set(false);
        property.set(101);
        assert_eq!(other.native.borrow_mut().drain(), 0);
        drop(field);
        assert_eq!(wake.native.borrow_mut().drain(), 0);
        assert_eq!(
            wake.native
                .borrow()
                .graph
                .as_ref()
                .unwrap()
                .telemetry()
                .nodes,
            0
        );
        assert!(!other_field.0.dirty.get());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn svet_context_replacement_and_reentrant_wake_preserve_latest_value() {
        struct Model {
            value: Property<i32>,
        }
        let first = Rc::new(Model {
            value: Property::new(1),
        });
        let second = Rc::new(Model {
            value: Property::new(2),
        });
        let context = Context::new(first.clone());
        let wake = Rc::new(Wake::default());
        let field = Field::path(&context, |model| model.value.clone(), wake.clone());
        wake.native.borrow_mut().enable(true);
        field.0.dirty.set(false);
        wake.pending.set(false);
        let weak = Rc::downgrade(&wake);
        let property = first.value.clone();
        *wake.callback.borrow_mut() = Some(Rc::new(move || {
            // Arbitrary host notification must run outside the graph RefCell borrow.
            assert_eq!(
                weak.upgrade()
                    .unwrap()
                    .native
                    .borrow()
                    .graph
                    .as_ref()
                    .unwrap()
                    .telemetry()
                    .nodes,
                2
            );
            property.set(3);
        }));
        first.value.set(4);
        assert_eq!(wake.native.borrow_mut().drain(), 1);
        assert_eq!(field.get(), Some(3));
        context.set(second.clone());
        assert_eq!(wake.native.borrow_mut().drain(), 1);
        assert_eq!(field.get(), Some(2));
        field.0.dirty.set(false);
        first.value.set(10);
        assert_eq!(wake.native.borrow_mut().drain(), 0);
        assert!(!field.0.dirty.get());
        second.value.set(20);
        assert_eq!(wake.native.borrow_mut().drain(), 1);
        assert_eq!(field.get(), Some(20));
    }

    #[test]
    fn property_notifications_allow_reentrant_writes_and_disconnect_on_drop() {
        let property = Property::new(0);
        let observed = Rc::new(RefCell::new(Vec::new()));
        let values = observed.clone();
        let subscription = property.subscribe(move |value| values.borrow_mut().push(value));
        assert!(!property.set(0));
        assert!(property.set(1));
        drop(subscription);
        property.set(2);
        assert_eq!(*observed.borrow(), vec![1]);

        let weak = Rc::downgrade(&property.0);
        let listener = property.subscribe(move |value| {
            if value == 3 {
                Property(weak.upgrade().unwrap()).set(4);
            }
        });
        property.set(3);
        assert_eq!(property.get(), 4);
        drop(listener);
    }

    #[test]
    fn inherited_context_tracks_replacements_while_overridden_and_releases_sources() {
        struct Model;
        let first = Rc::new(Model);
        let second = Rc::new(Model);
        let local = Rc::new(Model);
        let first_weak = Rc::downgrade(&first);
        let second_weak = Rc::downgrade(&second);
        let root = Context::new(first.clone());
        let child = Context::child(&root);
        child.set(local.clone());
        root.set(second.clone());
        assert!(Rc::ptr_eq(&child.get().unwrap(), &local));
        child.inherit();
        assert!(Rc::ptr_eq(&child.get().unwrap(), &second));
        drop(first);
        assert!(first_weak.upgrade().is_none());
        drop(second);
        drop(root);
        drop(child);
        assert!(second_weak.upgrade().is_none());
    }
}
