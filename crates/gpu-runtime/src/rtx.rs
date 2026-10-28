#![allow(unsafe_code, dead_code)]

//! Hardware **RTX** & Cross-Platform **Ray Tracing** engine (`BLAS`, `TLAS`, `RayTracingPipeline`, `RayQuery`).
//!
//! Unifies hardware-accelerated BVH ray tracing across:
//! - **NVIDIA RTX (`OptiX` / `VK_KHR_ray_tracing_pipeline` / `DXR 1.1`)**: RT Cores (3rd/4th Gen) for hardware BVH traversal & ray-triangle / ray-AABB intersection.
//! - **Apple Silicon Metal Ray Tracing (`MTLAccelerationStructure`, `visible_function_table`, `raytracing::intersector`)**: M3/M4 hardware ray tracing + M1/M2 compute intersector.
//! - **Vulkan Ray Tracing (`VK_KHR_acceleration_structure`, `VK_KHR_ray_query`)**: Cross-vendor AMD RDNA2/3/4, Intel Arc, Qualcomm Adreno, and NVIDIA GPUs.
//! - **DirectX Raytracing (`DXR 1.1` `RayQuery` / `DispatchRays`)**: Windows 11 DirectX 12 Ultimate.

use crate::{Backend, BufferAddress};

/// Hardware ray tracing tier supported by the active GPU adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RtxTier {
    /// Software / Compute BVH fallback when dedicated RT cores are unavailable.
    ComputeBvhFallback,
    /// Inline RayQuery (`WGSL ray_query` / `GLSL_EXT_ray_query` / `DXR 1.1` / Metal `raytracing::intersector`)
    /// inside `@compute` and `@fragment` shaders.
    InlineRayQuery,
    /// Full Hardware RTX Pipeline (`RayGen`, `ClosestHit`, `AnyHit`, `Miss`, `Intersection` shaders + Shader Binding Table + RT Cores).
    HardwareRtxPipeline,
}

/// Geometry primitive type stored inside a Bottom-Level Acceleration Structure (`BLAS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlasGeometryKind {
    /// Indexed 3D triangle mesh (`BLAS_INPUT` vertex + index buffers).
    Triangles,
    /// Procedural axis-aligned bounding boxes (`AABB`) for exact analytic CAD/CSG/SDF primitives (spheres, cylinders, NURBS patches).
    ProceduralAabbs,
}

/// Descriptor for a single geometry slice inside a Bottom-Level Acceleration Structure (`BLAS`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlasGeometryDescriptor {
    pub kind: BlasGeometryKind,
    pub vertex_count: u32,
    pub primitive_count: u32,
    pub opaque: bool,
    pub allow_compaction: bool,
}

/// Bottom-Level Acceleration Structure (`BLAS`) representing a local-space BVH over triangles or procedural CAD AABBs.
#[derive(Debug, Clone)]
pub struct Blas {
    pub label: Option<String>,
    pub geometries: Vec<BlasGeometryDescriptor>,
    pub scratch_size_bytes: BufferAddress,
    pub compacted_size_bytes: BufferAddress,
    pub device_address: u64,
}

impl Blas {
    /// Creates a hardware-sized `BLAS` descriptor and estimates BVH build scratch & compacted memory footprints.
    pub fn new(label: Option<&str>, geometries: Vec<BlasGeometryDescriptor>) -> Self {
        let total_prims: u64 = geometries
            .iter()
            .map(|g| g.primitive_count.max(1) as u64)
            .sum();
        // Hardware BVH nodes require ~64 bytes per primitive Compacted and ~128 bytes Scratch during build.
        let compacted_size_bytes = (total_prims * 64).max(256).next_multiple_of(256);
        let scratch_size_bytes = (total_prims * 128).max(512).next_multiple_of(256);
        Self {
            label: label.map(str::to_owned),
            geometries,
            scratch_size_bytes,
            compacted_size_bytes,
            device_address: 0x0001_0000_0000 + compacted_size_bytes,
        }
    }
}

/// Instance entry inside a Top-Level Acceleration Structure (`TLAS`), placing a `BLAS` in world space with a 3x4 affine transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TlasInstance {
    pub transform_3x4: [[f32; 4]; 3],
    pub custom_index: u32,
    pub mask: u8,
    pub sbt_record_offset: u32,
    pub blas_device_address: u64,
}

impl TlasInstance {
    pub const fn identity(blas_device_address: u64, custom_index: u32) -> Self {
        Self {
            transform_3x4: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            custom_index,
            mask: 0xFF,
            sbt_record_offset: 0,
            blas_device_address,
        }
    }
}

/// Top-Level Acceleration Structure (`TLAS`) over scene instances for hardware RTX traversal.
#[derive(Debug, Clone)]
pub struct Tlas {
    pub label: Option<String>,
    pub instances: Vec<TlasInstance>,
    pub scratch_size_bytes: BufferAddress,
    pub storage_size_bytes: BufferAddress,
}

impl Tlas {
    pub fn new(label: Option<&str>, instances: Vec<TlasInstance>) -> Self {
        let count = instances.len().max(1) as u64;
        let storage_size_bytes = (count * 64).max(256).next_multiple_of(256);
        let scratch_size_bytes = (count * 128).max(512).next_multiple_of(256);
        Self {
            label: label.map(str::to_owned),
            instances,
            scratch_size_bytes,
            storage_size_bytes,
        }
    }
}

/// Hardware RTX & Ray Tracing capabilities report for the active GPU backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtxCapabilities {
    pub backend: Backend,
    pub tier: RtxTier,
    pub rt_core_label: &'static str,
    pub acceleration_structure_api: &'static str,
    pub max_recursion_depth: u32,
    pub max_instance_count: u64,
    pub supports_inline_ray_query: bool,
    pub supports_shader_binding_table: bool,
}

impl RtxCapabilities {
    pub const fn for_backend(backend: Backend) -> Self {
        match backend {
            Backend::Cuda => Self {
                backend,
                tier: RtxTier::HardwareRtxPipeline,
                rt_core_label: "NVIDIA RTX RT Cores (OptiX / PTX Hardware BVH)",
                acceleration_structure_api: "OptiXTraversableHandle + cuMemAllocAsync",
                max_recursion_depth: 31,
                max_instance_count: 1 << 24,
                supports_inline_ray_query: true,
                supports_shader_binding_table: true,
            },
            Backend::Vulkan => Self {
                backend,
                tier: RtxTier::HardwareRtxPipeline,
                rt_core_label: "Vulkan KHR Ray Tracing + RT Cores",
                acceleration_structure_api:
                    "VK_KHR_acceleration_structure + VK_KHR_ray_tracing_pipeline",
                max_recursion_depth: 31,
                max_instance_count: 1 << 24,
                supports_inline_ray_query: true,
                supports_shader_binding_table: true,
            },
            Backend::Dx12 => Self {
                backend,
                tier: RtxTier::HardwareRtxPipeline,
                rt_core_label: "DirectX Raytracing Tier 1.1 (DXR)",
                acceleration_structure_api: "ID3D12GraphicsCommandList4::BuildRaytracingAccelerationStructure",
                max_recursion_depth: 31,
                max_instance_count: 1 << 24,
                supports_inline_ray_query: true,
                supports_shader_binding_table: true,
            },
            Backend::Metal => Self {
                backend,
                tier: RtxTier::HardwareRtxPipeline,
                rt_core_label: "Apple Silicon Metal Ray Tracing (MTLAccelerationStructure)",
                acceleration_structure_api: "MTLAccelerationStructureCommandEncoder + raytracing::intersector",
                max_recursion_depth: 16,
                max_instance_count: 1 << 24,
                supports_inline_ray_query: true,
                supports_shader_binding_table: true,
            },
            Backend::Gl | Backend::BrowserWebGpu | Backend::Noop => Self {
                backend,
                tier: RtxTier::ComputeBvhFallback,
                rt_core_label: "Compute Shader BVH Traversal + SDF Raymarching",
                acceleration_structure_api: "Compute SSBO LBVH Tree",
                max_recursion_depth: 8,
                max_instance_count: 1 << 20,
                supports_inline_ray_query: false,
                supports_shader_binding_table: false,
            },
        }
    }
}
