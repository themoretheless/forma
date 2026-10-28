#![recursion_limit = "256"]

//! Compute-core: the runtime library for WGSL compute kernels — the compute
//! counterpart of raster-core.
//!
//! Domain kernels (math-core, photogrammetry-core, geometry-bridge) own their
//! WGSL sources behind `include_str!` with naga validation in their own cargo
//! tests. What they share is the dispatch plumbing, and that lives here:
//!
//! - [`Kernel`]: a validated, cached compute pipeline with a declared binding
//!   layout and per-backend workgroup-size tuning (the `WG` anchor convention).
//! - Buffer helpers: typed storage/uniform uploads and readbacks on top of
//!   [`gpu_compute`] primitives.
//! - [`shaders`]: generic building-block kernels (elementwise maps, block
//!   reductions) reusable across domains.
//!
//! GPU-backed tests skip with a notice on machines without an adapter,
//! matching the `gpu-compute` convention.

mod array;
mod batch;
mod buffer;
mod comparison;
mod error;
mod fusion;
mod kernel;
mod kernel_cache;
mod matrix;
mod ops;
mod program;
mod readback;
mod reduction;
mod runtime;
mod scan;
mod scratch;
mod selection;
pub mod shaders;
mod tensor;

pub use array::{GpuArray, GpuElement};
pub use batch::ComputeBatch;
pub use buffer::{
    read_f32, read_u32, storage_f32, storage_f32_zeroed, storage_u32, try_read_f32, try_read_u32,
    uniform_f32,
};
pub use error::ComputeError;
pub use gpu_compute;
pub use gpu_compute::wgpu;
pub use kernel::{Binding, Kernel, KernelError};
pub use ops::{BinaryOp, CompareOp, ReduceOp, ScatterOp, UnaryOp};
pub use program::ComputeProgram;
pub use readback::Readback;
pub use reduction::{Reduction, reduce_f32};
pub use runtime::ComputeRuntime;
pub use scratch::{ScratchArray, ScratchPool};
pub use tensor::{GpuLowTensor, GpuTensor, TensorComputeError};
pub use tensor_core;

pub use selection::CompactedArray;

pub use kernel::{BindingInfo, KernelBindingError};
pub use kernel_cache::{KernelCache, KernelCacheStats};

pub use matrix::{GpuMatrix, MatrixView};

pub use fusion::{Expression, FusedKernel, FusedSumKernel, FusionError, FusionGraph, Predicate};

#[cfg(not(target_arch = "wasm32"))]
mod tensor_backend;
#[cfg(not(target_arch = "wasm32"))]
mod tensor_low_backend;
