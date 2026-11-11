//! # Svet Engine — C-ABI / WebAssembly Adapter (`svet-wasm`)
//!
//! Thin zero-GC linear-memory adapter composing the decoupled workspace crates:
//! - [`svet_graph`] — glitch-free push-pull reactive DAG & safe host `Context`
//! - [`svet_reconcile`] — $O(n \log n)$ Longest Increasing Subsequence reconciler
//! - [`svet_gpu_arena`] — UMA/DUMA `f32` arena, dirty-span tracking, SIMD128 math & 3D picking

mod core;
mod ffi;

pub use core::Core;
pub use ffi::*;
pub use svet_gpu_arena::{
    GpuArena, MAX_GPU_ARENA_FLOATS, ViewportPickQuery, clip_primitive_3d, compose_trs_matrix,
    grow_gpu_arena, perspective_matrix, pick_nearest_vertex_3d, pick_visible_vertex_3d,
};
pub use svet_graph::{
    EdgeHashBuilder, EdgeHasher, KIND_COMPUTED, KIND_EFFECT, KIND_SIGNAL, Node, ReactiveGraph,
    STATE_CHECK, STATE_CLEAN, STATE_DIRTY, STATE_DISPOSED, ScheduledEffect,
};
pub use svet_reconcile::LisScratch;

pub mod native {
    pub use svet_graph::context::*;
}

#[cfg(test)]
mod tests;
