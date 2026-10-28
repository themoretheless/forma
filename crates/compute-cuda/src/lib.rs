//! Resident f32/f64/u32 and low-precision tensors on an explicit CUDA stream.
//!
//! Custom kernels use NVRTC at initialization (or caller-supplied compatible
//! PTX). Matrix multiplication uses cuBLAS with an explicit precision policy.
//! No operation falls back to the CPU. CUDA execution must be qualified on
//! NVIDIA hardware; successful compilation on another platform is not GPU proof.
#![doc = include_str!("../README.md")]
mod attention;
mod attention_dispatch;
mod convolution;
mod dispatch;
mod error;
mod evaluation;
mod float64;
mod indexing;
mod libraries;
mod low_attention;
mod low_dispatch;
mod low_indexing;
mod low_ops;
mod low_precision;
mod low_scatter;
mod low_statistics;
mod matmul;
mod policy;
mod program;
mod reduction;
mod runtime;
mod statistics;
mod statistics_dispatch;

pub use error::CudaError;
pub use low_precision::CudaLowTensor;
pub use policy::CudaMatmulPolicy;
pub use runtime::{CUDA_KERNEL_SOURCE, CudaCapabilities, CudaRuntime, CudaTensor};
pub use tensor_core;

pub use program::{
    CudaPrepareOptions, CudaPreparedProgram, CudaProgramBuilder, CudaProgramStats, CudaValue,
};

pub use program::{CudaDtype, CudaProgramInput, CudaProgramOutput, CudaProgramOutputMut};

pub use gpu_compute::cuda::CudaStreamMode;
pub use program::{CudaCaptureOptions, CudaGraphProgram, CudaGraphStats};
