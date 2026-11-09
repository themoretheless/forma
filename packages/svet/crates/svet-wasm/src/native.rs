//! Owned native graph contexts for window/binding adapters. Values and rendering
//! remain with the host; dependency discovery and scheduling use ReactiveGraph.
use crate::{KIND_COMPUTED, KIND_EFFECT, KIND_SIGNAL, ReactiveGraph};
use std::sync::{Arc, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Signal,
    Computed,
    Effect,
}
impl Kind {
    fn raw(self) -> u8 {
        match self {
            Self::Signal => KIND_SIGNAL,
            Self::Computed => KIND_COMPUTED,
            Self::Effect => KIND_EFFECT,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    owner: Weak<()>,
    id: u32,
    serial: u64,
    kind: Kind,
}
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.owner.ptr_eq(&other.owner)
            && self.id == other.id
            && self.serial == other.serial
            && self.kind == other.kind
    }
}
impl Eq for Node {}
impl std::hash::Hash for Node {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.owner.as_ptr(), state);
        std::hash::Hash::hash(&self.id, state);
        std::hash::Hash::hash(&self.serial, state);
    }
}
impl Node {
    /// Numeric slot for diagnostics. Use the whole handle as an identity: slots
    /// are reused after disposal/reset.
    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    /// Context ownership only, not node liveness. Operations additionally check
    /// the allocation serial, including after disposal/reset.
    pub fn context_alive(&self) -> bool {
        self.owner.strong_count() != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    ForeignContext,
    StaleNode,
    WrongKind,
    ScopeActive,
    IdentityExhausted,
    Cycle,
}
impl std::fmt::Display for Error {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "Svet native graph: {self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Telemetry {
    pub nodes: u32,
    pub edges: usize,
    pub slots: usize,
    pub batch_depth: u32,
    pub tracking_depth: usize,
}

/// An independent graph, with no dependency on the thread-local C ABI. A context
/// can move between threads; access remains exclusive through Rust borrowing.
pub struct Context {
    graph: ReactiveGraph,
    _owner: Arc<()>,
    identity: Weak<()>,
    serials: Vec<u64>,
    next_serial: u64,
}
impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
impl Context {
    pub fn new() -> Self {
        let owner = Arc::new(());
        Self {
            graph: ReactiveGraph::new(),
            identity: Arc::downgrade(&owner),
            _owner: owner,
            serials: Vec::new(),
            next_serial: 1,
        }
    }
    pub fn create(&mut self, kind: Kind) -> Result<Node, Error> {
        let serial = self.next_serial;
        let next = serial.checked_add(1).ok_or(Error::IdentityExhausted)?;
        let id = self.graph.create_node(kind.raw());
        if self.serials.len() <= id as usize {
            self.serials.resize(id as usize + 1, 0);
        }
        self.serials[id as usize] = serial;
        self.next_serial = next;
        Ok(Node {
            owner: self.identity.clone(),
            id,
            serial,
            kind,
        })
    }
    fn validate(&self, node: &Node) -> Result<(), Error> {
        if !node.owner.ptr_eq(&self.identity) {
            return Err(Error::ForeignContext);
        }
        if self.serials.get(node.id as usize) != Some(&node.serial)
            || !self
                .graph
                .nodes
                .get(node.id as usize)
                .is_some_and(|slot| slot.alive && slot.kind == node.kind.raw())
        {
            return Err(Error::StaleNode);
        }
        Ok(())
    }
    pub fn dispose(&mut self, node: &Node) -> Result<(), Error> {
        self.validate(node)?;
        self.serials[node.id as usize] = 0;
        self.graph.dispose_node(node.id);
        Ok(())
    }
    /// Publish a changed signal/computed value held by the native host.
    pub fn notify(&mut self, node: &Node) -> Result<(), Error> {
        self.validate(node)?;
        if node.kind == Kind::Effect {
            return Err(Error::WrongKind);
        }
        self.graph.trigger_write(node.id);
        Ok(())
    }
    pub fn state(&mut self, node: &Node) -> Result<u8, Error> {
        self.validate(node)?;
        Ok(self.graph.get_state(node.id))
    }
    /// Discover this observer's current dependencies. Unwinding still ends the
    /// discovery frame; partial reads are committed, so hosts own retry policy.
    pub fn track<R>(
        &mut self,
        observer: &Node,
        body: impl FnOnce(&mut Tracking<'_>) -> R,
    ) -> Result<R, Error> {
        self.validate(observer)?;
        if observer.kind == Kind::Signal {
            return Err(Error::WrongKind);
        }
        if self.graph.nodes[observer.id as usize].tracking {
            return Err(Error::Cycle);
        }
        self.graph.begin_tracking(observer.id);
        let mut frame = Tracking {
            context: self,
            observer: observer.id,
        };
        Ok(body(&mut frame))
    }
    /// Delay native effect delivery. Pending jobs survive both normal return and
    /// unwind; the host explicitly takes them after the outer batch ends.
    pub fn batch<R>(&mut self, body: impl FnOnce(&mut Context) -> R) -> Result<R, Error> {
        self.graph.batch_depth = self
            .graph
            .batch_depth
            .checked_add(1)
            .ok_or(Error::IdentityExhausted)?;
        let frame = Batch { context: self };
        Ok(body(frame.context))
    }
    /// Snapshot ready jobs as checked handles, not borrowed native buffer IDs.
    /// A job retired after this call is refused when the host later tracks it.
    pub fn take_effects(&mut self) -> Vec<Node> {
        let count = self.graph.prepare_flush();
        self.graph.flush_buffer[..count]
            .iter()
            .map(|&id| Node {
                owner: self.identity.clone(),
                id,
                serial: self.serials[id as usize],
                kind: Kind::Effect,
            })
            .collect()
    }
    pub fn reset(&mut self) -> Result<(), Error> {
        if self.graph.batch_depth != 0 || !self.graph.tracking_stack.is_empty() {
            return Err(Error::ScopeActive);
        }
        self.graph.reset();
        self.serials.clear();
        Ok(())
    }
    pub fn telemetry(&self) -> Telemetry {
        Telemetry {
            nodes: self.graph.active_count,
            edges: self.graph.sources.iter().map(Vec::len).sum(),
            slots: self.graph.nodes.len(),
            batch_depth: self.graph.batch_depth,
            tracking_depth: self.graph.tracking_stack.len(),
        }
    }
}

pub struct Tracking<'a> {
    context: &'a mut Context,
    observer: u32,
}
impl Tracking<'_> {
    /// Evaluate a native-host computed value, then register it in the parent
    /// frame. Hosts retain/cache the returned value; the graph owns dependencies.
    pub fn evaluate<R>(
        &mut self,
        computed: &Node,
        body: impl FnOnce(&mut Tracking<'_>) -> R,
    ) -> Result<R, Error> {
        self.context.validate(computed)?;
        if computed.kind != Kind::Computed {
            return Err(Error::WrongKind);
        }
        let value = self.context.track(computed, body)?;
        self.read(computed)?;
        Ok(value)
    }
    pub fn state(&mut self, node: &Node) -> Result<u8, Error> {
        self.context.state(node)
    }
    pub fn read(&mut self, source: &Node) -> Result<(), Error> {
        self.context.validate(source)?;
        if source.kind == Kind::Effect {
            return Err(Error::WrongKind);
        }
        if self.context.graph.nodes[source.id as usize].tracking
            || source.id == self.observer
            || self.context.graph.depends_on(source.id, self.observer)
        {
            return Err(Error::Cycle);
        }
        self.context.graph.track_read(source.id);
        Ok(())
    }
}
impl Drop for Tracking<'_> {
    fn drop(&mut self) {
        self.context.graph.end_tracking();
    }
}
struct Batch<'a> {
    context: &'a mut Context,
}
impl Drop for Batch<'_> {
    fn drop(&mut self) {
        self.context.graph.batch_depth -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn contexts_reject_foreign_equal_numeric_ids() {
        let mut a = Context::new();
        let mut b = Context::new();
        let x = a.create(Kind::Signal).unwrap();
        let y = b.create(Kind::Signal).unwrap();
        assert_eq!(x.id(), y.id());
        assert_ne!(x, y);
        let keys: std::collections::HashSet<_> =
            [x.clone(), x.clone(), y.clone()].into_iter().collect();
        assert_eq!(keys.len(), 2);
        assert_eq!(b.notify(&x), Err(Error::ForeignContext));
        assert_eq!(a.dispose(&y), Err(Error::ForeignContext));
        assert_eq!(a.telemetry().nodes, 1);
        assert_eq!(b.telemetry().nodes, 1);
    }
    #[test]
    fn stale_handles_cannot_touch_recycled_or_reset_slots() {
        let mut graph = Context::new();
        let old = graph.create(Kind::Signal).unwrap();
        graph.dispose(&old).unwrap();
        let next = graph.create(Kind::Signal).unwrap();
        assert_eq!(old.id(), next.id());
        assert_eq!(graph.notify(&old), Err(Error::StaleNode));
        graph.reset().unwrap();
        let fresh = graph.create(Kind::Signal).unwrap();
        assert_eq!(next.id(), fresh.id());
        assert_eq!(graph.dispose(&next), Err(Error::StaleNode));
        assert_eq!(graph.telemetry().nodes, 1);
        graph.dispose(&fresh).unwrap();
        assert_eq!(graph.telemetry().nodes, 0);
    }
    #[test]
    fn dynamic_binding_reads_replace_dependencies_and_batches_deduplicate() {
        let mut graph = Context::new();
        let left = graph.create(Kind::Signal).unwrap();
        let right = graph.create(Kind::Signal).unwrap();
        let view = graph.create(Kind::Effect).unwrap();
        graph
            .track(&view, |frame| frame.read(&left).unwrap())
            .unwrap();
        graph
            .batch(|graph| {
                graph.notify(&left).unwrap();
                graph.notify(&left).unwrap();
                assert!(graph.take_effects().is_empty());
            })
            .unwrap();
        let jobs = graph.take_effects();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id(), view.id());
        graph
            .track(&jobs[0], |frame| frame.read(&right).unwrap())
            .unwrap();
        graph.notify(&left).unwrap();
        assert!(graph.take_effects().is_empty());
        graph.notify(&right).unwrap();
        assert_eq!(graph.take_effects().len(), 1);
        for node in [&view, &left, &right] {
            graph.dispose(node).unwrap();
        }
        assert_eq!(graph.telemetry().nodes, 0);
        assert_eq!(graph.telemetry().edges, 0);
    }
    #[test]
    fn unwind_restores_tracking_and_batch_scopes() {
        let mut graph = Context::new();
        let source = graph.create(Kind::Signal).unwrap();
        let view = graph.create(Kind::Effect).unwrap();
        assert!(
            catch_unwind(AssertUnwindSafe(|| graph.track(&view, |frame| {
                frame.read(&source).unwrap();
                panic!("render");
            })))
            .is_err()
        );
        assert_eq!(graph.telemetry().tracking_depth, 0);
        assert!(
            catch_unwind(AssertUnwindSafe(|| graph.batch(|graph| {
                graph.notify(&source).unwrap();
                assert_eq!(graph.reset(), Err(Error::ScopeActive));
                panic!("batch");
            })))
            .is_err()
        );
        assert_eq!(graph.telemetry().batch_depth, 0);
        assert_eq!(graph.take_effects().len(), 1);
        graph.reset().unwrap();
        assert_eq!(graph.telemetry().nodes, 0);
    }
    #[test]
    fn disposed_queued_effect_does_not_schedule_reused_slot() {
        let mut graph = Context::new();
        let source = graph.create(Kind::Signal).unwrap();
        let old = graph.create(Kind::Effect).unwrap();
        graph
            .track(&old, |frame| frame.read(&source).unwrap())
            .unwrap();
        graph.notify(&source).unwrap();
        graph.dispose(&old).unwrap();
        let next = graph.create(Kind::Effect).unwrap();
        assert_eq!(old.id(), next.id());
        assert!(graph.take_effects().is_empty());
        assert_eq!(graph.track(&old, |_| ()), Err(Error::StaleNode));
        graph
            .track(&next, |frame| frame.read(&source).unwrap())
            .unwrap();
        graph.notify(&source).unwrap();
        assert_eq!(graph.take_effects().len(), 1);
    }
    #[test]
    fn context_drop_expires_weak_handles_and_owned_graph_can_move_threads() {
        let mut graph = Context::new();
        let source = graph.create(Kind::Signal).unwrap();
        let copy = source.clone();
        std::thread::spawn(move || {
            graph.notify(&source).unwrap();
            graph.dispose(&source).unwrap();
        })
        .join()
        .unwrap();
        assert!(!copy.context_alive());
    }
    #[test]
    fn identity_exhaustion_refuses_before_allocating() {
        let mut graph = Context::new();
        graph.next_serial = u64::MAX;
        assert!(matches!(
            graph.create(Kind::Signal),
            Err(Error::IdentityExhausted)
        ));
        assert_eq!(graph.telemetry().nodes, 0);
    }

    #[test]
    fn tracking_rejects_cycles_without_leaving_frames() {
        let mut graph = Context::new();
        let a = graph.create(Kind::Computed).unwrap();
        let b = graph.create(Kind::Computed).unwrap();
        graph.track(&b, |frame| frame.read(&a).unwrap()).unwrap();
        assert_eq!(
            graph.track(&a, |frame| frame.read(&b)),
            Ok(Err(Error::Cycle))
        );
        assert_eq!(
            graph.track(&a, |frame| frame.read(&a)),
            Ok(Err(Error::Cycle))
        );
        assert_eq!(graph.telemetry().tracking_depth, 0);
        let effect = graph.create(Kind::Effect).unwrap();
        assert_eq!(graph.notify(&effect), Err(Error::WrongKind));
    }

    #[test]
    fn native_diamond_evaluation_publishes_one_coherent_result() {
        let mut graph = Context::new();
        let source = graph.create(Kind::Signal).unwrap();
        let left = graph.create(Kind::Computed).unwrap();
        let right = graph.create(Kind::Computed).unwrap();
        let view = graph.create(Kind::Effect).unwrap();
        let render = |graph: &mut Context, value: i32| {
            graph
                .track(&view, |frame| {
                    let a = frame
                        .evaluate(&left, |reads| {
                            reads.read(&source).unwrap();
                            value * 2
                        })
                        .unwrap();
                    let b = frame
                        .evaluate(&right, |reads| {
                            reads.read(&source).unwrap();
                            value * 3
                        })
                        .unwrap();
                    a + b
                })
                .unwrap()
        };
        assert_eq!(render(&mut graph, 1), 5);
        graph
            .batch(|graph| {
                graph.notify(&source).unwrap();
                graph.notify(&source).unwrap();
            })
            .unwrap();
        assert_eq!(graph.take_effects().len(), 1);
        assert_eq!(render(&mut graph, 3), 15);
        assert_eq!(graph.telemetry().edges, 4);
        assert_eq!(graph.telemetry().tracking_depth, 0);
        graph.reset().unwrap();
        assert_eq!(graph.telemetry().nodes, 0);
    }
    #[test]
    fn recursive_computed_evaluation_is_refused_and_outer_frame_restored() {
        let mut graph = Context::new();
        let computed = graph.create(Kind::Computed).unwrap();
        graph
            .track(&computed, |frame| {
                assert_eq!(frame.evaluate(&computed, |_| ()), Err(Error::Cycle));
            })
            .unwrap();
        assert_eq!(graph.telemetry().tracking_depth, 0);
    }

    #[test]
    fn drained_frame_job_cannot_render_a_replacement_lifetime() {
        let mut graph = Context::new();
        let source = graph.create(Kind::Signal).unwrap();
        let view = graph.create(Kind::Effect).unwrap();
        graph
            .track(&view, |reads| reads.read(&source).unwrap())
            .unwrap();
        graph.notify(&source).unwrap();
        let jobs = graph.take_effects();
        graph.dispose(&view).unwrap();
        let replacement = graph.create(Kind::Effect).unwrap();
        assert_eq!(jobs[0].id(), replacement.id());
        let mut rendered = false;
        assert_eq!(
            graph.track(&jobs[0], |_| rendered = true),
            Err(Error::StaleNode)
        );
        assert!(!rendered);
        graph
            .track(&replacement, |reads| reads.read(&source).unwrap())
            .unwrap();
        graph.notify(&source).unwrap();
        let next = graph.take_effects();
        assert_eq!(next, vec![replacement]);
    }
}
