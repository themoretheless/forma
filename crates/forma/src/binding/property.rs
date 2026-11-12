//! Observable properties and subscriptions.

use super::*;

/// Disconnects a listener on drop. Keeping a subscription never keeps its source alive.
pub struct Subscription(pub(super) Option<Box<dyn FnOnce()>>);

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(disconnect) = self.0.take() {
            disconnect();
        }
    }
}

pub(super) type Listener<T> = Rc<dyn Fn(T)>;

pub(super) struct PropertyInner<T> {
    pub(super) value: RefCell<T>,
    pub(super) next: Cell<u64>,
    pub(super) listeners: RefCell<Vec<(u64, Listener<T>)>>,
}

/// An observable field. Clones share a field; creating another Property creates
/// independent state. Mutate through `set` so every attached form is notified.
pub struct Property<T>(pub(super) Rc<PropertyInner<T>>);

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
