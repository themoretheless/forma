//! The model context bindings resolve against.

use super::*;

pub(super) struct Identity<C>(pub(super) Option<Rc<C>>);

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

pub(super) struct ContextInner<C: 'static> {
    pub(super) value: Property<Identity<C>>,
    pub(super) inherited: RefCell<Option<Rc<C>>>,
    pub(super) overridden: Cell<bool>,
    pub(super) parent: RefCell<Option<Box<dyn Any>>>,
    pub(super) parent_subscription: RefCell<Option<Subscription>>,
    pub(super) selected: RefCell<Option<Context<C>>>,
    pub(super) selected_subscription: RefCell<Option<Subscription>>,
}

/// A replaceable context. Child contexts follow their parent until explicitly
/// overridden. `clear` is an explicit empty context; `inherit` restores following.
pub struct Context<C: 'static>(pub(super) Rc<ContextInner<C>>);

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
    pub(super) fn receive(&self, value: Option<Rc<C>>) {
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
