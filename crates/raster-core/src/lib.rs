//! Rasterizer library: the pixel-render counterpart of the compute kernels.
//!
//! The WGSL sources in `shaders/` are the single source of truth for the
//! raster shaders. The same text runs in the browser through WebGPU (the
//! TypeScript renderer embeds generated copies) and natively through wgpu
//! (Metal on macOS, Vulkan/DX12 elsewhere — including NVIDIA hardware).
//!
//! - [`shaders`]: WGSL sources via `include_str!`.
//! - [`uniform`]: CPU-side mirror of the `Scene`/`Obj` uniform structs.
//! - [`variants`]: textual `instanced`/`immediate` shader variants.
//! - [`pipeline`]: cached render pipelines for every shader.
//! - [`rasterizer`]: headless offscreen rasterizer used by tests and
//!   available for native embedding.

pub use gpu_compute;
pub use wgpu;

pub mod chunks;
pub mod codegen;
pub mod generated_layouts;
pub mod layout;
pub mod pipeline;
pub mod postprocess;
pub mod rasterizer;
pub mod readback;
pub mod resources;
pub mod shaders;
pub mod uniform;
pub mod variants;

pub use postprocess::{
    CullingStats, apply_chromatic_aberration_and_vignette, bilateral_depth_filter,
    bloom_bright_pass_and_blur, build_hiz_mip_level_max, compute_normal_map_from_depth,
    compute_ssao_ao, evaluate_culling_stats, fxaa_luma_antialias, sobel_edge_outline,
    tone_map_aces_filmic,
};
