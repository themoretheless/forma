//! # Svet Engine — WASM Reactive Core (`svet-wasm`)
//!
//! Solves fundamental flaws of React, Vue, Svelte, and Blazor:
//! 1. **Zero GC Pressure + Slot Recycling (`free_list`)**: Graph edges, node states, and
//!    topological dirty queues live in linear WASM memory. Disposed nodes return their ID
//!    to `free_list` for O(1) memory reuse without unbounded vector growth in long-running SPAs.
//! 2. **Glitch-Free Push-Pull Hybrid DAG**:
//!    - *Push phase*: Marks downstream computed & effect nodes as `DIRTY` / `CHECK` in linear time.
//!    - *Pull phase*: Evaluates computed nodes lazily and executes effects only once per batch,
//!      eliminating diamond dependency glitches.
//! 3. **Zero JS<->WASM Marshaling Bottleneck (unlike Blazor)**:
//!    Instead of single-call interop per node, flushed effect IDs and LIS reconciliation patches
//!    are written directly to linear WASM memory buffers read as `Uint32Array` views by TypeScript.
//! 4. **O(n log n) LIS Keyed List Reconciler**:
//!    Computes the Longest Increasing Subsequence in WASM to produce the mathematically minimal
//!    number of DOM moves for keyed lists.

mod ffi;
mod gpu_math;
mod graph;
mod lis;

pub use ffi::*;
pub use gpu_math::MAX_GPU_ARENA_FLOATS;
pub use graph::{
    EdgeHashBuilder, EdgeHasher, KIND_COMPUTED, KIND_EFFECT, KIND_SIGNAL, Node, ReactiveGraph,
    STATE_CHECK, STATE_CLEAN, STATE_DIRTY, STATE_DISPOSED, ScheduledEffect,
};

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(test)]
mod tests;
