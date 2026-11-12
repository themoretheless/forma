//! A bound model field and the wake-up it triggers.

use super::*;

pub(super) struct FieldInner<T> {
    pub(super) property: RefCell<Option<Property<T>>>,
    pub(super) dirty: Cell<bool>,
    pub(super) reset: Cell<bool>,
    pub(super) epoch: Cell<u64>,
    pub(super) revision: Cell<u64>,
    pub(super) property_subscription: RefCell<Option<Subscription>>,
    pub(super) context_subscription: RefCell<Option<Subscription>>,
    pub(super) context: RefCell<Option<Box<dyn Any>>>,
    pub(super) wake: Rc<Wake>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) native_key: Cell<u64>,
}

#[derive(Default)]
pub(super) struct Wake {
    pub(super) pending: Cell<bool>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) native: RefCell<NativeBindings>,
    pub(super) callback: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Wake {
    pub(super) fn notify(&self) {
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
pub(super) struct NativeBindings {
    pub(super) next: u64,
    pub(super) fields: BTreeMap<u64, Rc<dyn Fn()>>,
    pub(super) graph: Option<svet_graph::context::Context>,
    pub(super) nodes: BTreeMap<u64, (svet_graph::context::Node, svet_graph::context::Node)>,
    pub(super) jobs: std::collections::HashMap<svet_graph::context::Node, u64>,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeBindings {
    pub(super) fn add_nodes(&mut self, key: u64) {
        use svet_graph::context::Kind;
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
    pub(super) fn register(&mut self, dirty: Rc<dyn Fn()>) -> u64 {
        self.next = self
            .next
            .checked_add(1)
            .expect("native binding identity exhausted");
        let key = self.next;
        self.fields.insert(key, dirty);
        self.add_nodes(key);
        key
    }
    pub(super) fn remove(&mut self, key: u64) {
        self.fields.remove(&key);
        if let Some((source, effect)) = self.nodes.remove(&key) {
            self.jobs.remove(&effect);
            if let Some(graph) = &mut self.graph {
                graph.dispose(&effect).unwrap();
                graph.dispose(&source).unwrap();
            }
        }
    }
    pub(super) fn notify(&mut self, key: u64) -> bool {
        if let (Some(graph), Some((source, _))) = (&mut self.graph, self.nodes.get(&key)) {
            graph.notify(source).unwrap();
            true
        } else {
            false
        }
    }
    pub(super) fn drain(&mut self) -> usize {
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
    pub(super) fn enable(&mut self, enabled: bool) {
        self.drain();
        if enabled == self.graph.is_some() {
            return;
        }
        self.nodes.clear();
        self.jobs.clear();
        self.graph = enabled.then(svet_graph::context::Context::new);
        for key in self.fields.keys().copied().collect::<Vec<_>>() {
            self.add_nodes(key);
        }
    }
}

impl<T> FieldInner<T> {
    pub(super) fn invalidate(&self) {
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

pub(super) struct Field<T>(pub(super) Rc<FieldInner<T>>);

impl<T> Clone for Field<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Clone + PartialEq + 'static> Field<T> {
    pub(super) fn empty(wake: Rc<Wake>) -> Self {
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
    pub(super) fn attach(&self, property: Option<Property<T>>) {
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
    pub(super) fn explicit(property: Property<T>, wake: Rc<Wake>) -> Self {
        let field = Self::empty(wake);
        field.attach(Some(property));
        field
    }
    pub(super) fn path<C: 'static>(
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
    pub(super) fn get(&self) -> Option<T> {
        self.0.property.borrow().as_ref().map(Property::get)
    }
    pub(super) fn set(&self, value: T) {
        let property = self.0.property.borrow().clone();
        if let Some(property) = property {
            property.set(value);
        }
    }
}
