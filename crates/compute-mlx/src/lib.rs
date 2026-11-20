//! Native MLX tensors through the MLX-C shared library, with explicit GPU streams.
//! No Python process, implicit CPU fallback, or link-time MLX dependency.
#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(not(target_arch = "wasm32"))]
mod ffi;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
