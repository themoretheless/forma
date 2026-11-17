//! Owned graph contexts for window/binding adapters. Values and rendering
//! remain with the host; dependency discovery and scheduling use [`ReactiveGraph`].
use crate::{KIND_COMPUTED, KIND_EFFECT, KIND_SIGNAL, ReactiveGraph};
use std::sync::{Arc, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Signal,
    Computed,
    Effect,
}

impl Kind {
    #[inline]
    pub const fn raw(self) -> u8 {
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

/// An independent graph, with no dependency on any thread-local C ABI. A context
/// can move between threads; access remains exclusive through Rust borrowing.
#[derive(Debug)]
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

    /// Publish a changed signal/computed value held by the host.
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

    /// Delay effect delivery. Pending jobs survive both normal return and
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

    /// Snapshot ready jobs as checked handles, not borrowed buffer IDs.
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
            edges: self.graph.edge_count(),
            slots: self.graph.nodes.len(),
            batch_depth: self.graph.batch_depth,
            tracking_depth: self.graph.tracking_stack.len(),
        }
    }
}

#[derive(Debug)]
pub struct Tracking<'a> {
    context: &'a mut Context,
    observer: u32,
}

impl Tracking<'_> {
    /// Evaluate a host computed value, then register it in the parent
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
        let source_idx = source.id as usize;
        if self.context.graph.nodes[source_idx].tracking
            || source.id == self.observer
            || (source.kind != Kind::Signal
                && !self.context.graph.sources[source_idx].is_empty()
                && self.context.graph.depends_on(source.id, self.observer))
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
mod tests;
