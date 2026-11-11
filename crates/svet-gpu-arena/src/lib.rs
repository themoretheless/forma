//! UMA/DUMA `f32` linear-memory arena, 128-byte cache-line dirty-span tracking,
//! SIMD128/scalar `mat4` & vector kernels, homogeneous WebGPU frustum clipping,
//! and Z-aware / triangle-occluded 3D vertex picking.
//!
//! Extracted from `svet-wasm` so `svet-graph` and native hosts do not carry
//! GPU buffers or 3D scene-graph math, and live vs. snapshot picking share a
//! single slice-based implementation without dummy graph allocations.

mod arena;
mod geometry;
mod math;

pub use arena::{GpuArena, MAX_GPU_ARENA_FLOATS, grow_gpu_arena};
pub use geometry::{
    ViewportPickQuery, clip_primitive_3d, pick_nearest_vertex_3d, pick_visible_vertex_3d,
};
pub use math::{compose_trs_matrix, perspective_matrix};

#[cfg(test)]
mod tests;
