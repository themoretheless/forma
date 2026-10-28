//! Shared GPU platform services for rasterization and computation.
//! Device creation, backend reports and byte transport live here; render and
//! compute pipeline policy belongs to their respective crates.

#![recursion_limit = "256"]

mod backend;
mod buffer;
mod context;
mod executor;
mod memory;
mod profiling;
mod readback;

pub use backend::{
    ApiEnvironment, AsdPageBuffer, BackendKind, BackendReport, BackendShaderIr,
    ComputeShaderTarget, HardwareExecutionTopology, MemoryArchitecture, MemoryArchitectureProfile,
    ModernMemoryTech, MultiTargetShaderArtifact, StorageDirectCapability, SubgroupReport,
    TargetPlatform, TransferStrategy, UnifiedTransferPlan, available_backend_report,
    available_execution_topology, available_subgroup_report, backend_label,
    compile_shader_multi_target, compile_wgsl_for_target, generate_wgsl_cad_vertex_transform_shader,
    generate_wgsl_parallel_reduction_shader, generate_wgsl_rtx_raytracing_shader,
    generate_wgsl_tiled_matmul_shader, recommend_workgroup_shape_2d, tuned_workgroup_size,
};
pub use buffer::{
    f32_le_bytes, pack_f32, pack_u32, read_buffer, storage_entry, u32_le_bytes, uniform_entry,
    write_buffer_words, write_small_parts,
};
pub use context::{GpuContext, GpuContextError};
pub use executor::block_on;
pub use gpu_shader;
pub use memory::{
    BufferError, GpuBuffer, GpuBufferView, create_buffer_initialized, device_memory_architecture,
    shared_memory_uploads,
};
pub use profiling::{GpuTimer, GpuTimestamp, TimestampError, TimestampReadback};
pub use readback::{ByteReadback, ReadbackError, try_read_buffer, try_read_buffer_with};
pub use wgpu as gpu_runtime;
pub use wgpu;

#[cfg(feature = "cuda")]
pub mod cuda;
