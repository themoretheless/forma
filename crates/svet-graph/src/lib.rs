//! Pure zero-GC push-pull reactive DAG (`ReactiveGraph`) and safe owned host
//! graph handles (`Context`, `Node`, `Kind`, `Tracking`, `Telemetry`, `Error`).
//!
//! Separated from DOM reconciliation (`svet-reconcile`), UMA/DUMA GPU math
//! (`svet-gpu-arena`), and the C-ABI WebAssembly adapter (`svet-wasm`) so
//! native and WASM hosts carry zero unrelated arena or reconciler state.

pub mod context;
mod graph;

pub use context::{Context, Error, Kind, Node as ContextNode, Telemetry, Tracking};
pub use graph::{
    EdgeHashBuilder, EdgeHasher, KIND_COMPUTED, KIND_EFFECT, KIND_SIGNAL, Node, ReactiveGraph,
    STATE_CHECK, STATE_CLEAN, STATE_DIRTY, STATE_DISPOSED, ScheduledEffect, edge_key,
};

#[cfg(test)]
mod tests;
