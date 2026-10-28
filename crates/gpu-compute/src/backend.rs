//! Portable GPU backend reporting, hardware execution topology, and compute shader generators.
//!
//! Following the **DRY** and **SOLID** principles, all hardware topology types (`BackendKind`,
//! `BackendReport`, `SubgroupReport`, `MemoryArchitecture`, `MemoryArchitectureProfile`,
//! `HardwareExecutionTopology`, `backend_label`, `recommend_workgroup_shape_2d`, `tuned_workgroup_size`)
//! and portable WGSL compute shader generators (`generate_wgsl_tiled_matmul_shader`,
//! `generate_wgsl_parallel_reduction_shader`) live in [`wgpu::platform`] / [`gpu_shader`] and are
//! re-exported here with runtime `GpuContext` probing.

use crate::GpuContext;

pub use gpu_shader::{
    compile_shader_multi_target, compile_wgsl_for_target, generate_wgsl_cad_vertex_transform_shader,
    generate_wgsl_parallel_reduction_shader, generate_wgsl_rtx_raytracing_shader,
    generate_wgsl_tiled_matmul_shader, ComputeShaderTarget, MultiTargetShaderArtifact,
};
pub use wgpu::{
    backend_label, recommend_workgroup_shape_2d, tuned_workgroup_size, ApiEnvironment,
    AsdPageBuffer, BackendKind, BackendReport, BackendShaderIr, HardwareExecutionTopology,
    MemoryArchitecture, MemoryArchitectureProfile, ModernMemoryTech, StorageDirectCapability,
    SubgroupReport, TargetPlatform, TransferStrategy, UnifiedTransferPlan,
};

/// Probes the preferred portable compute backend without constructing a
/// domain-specific kernel pipeline.
pub fn available_backend_report() -> Option<BackendReport> {
    static REPORT: std::sync::OnceLock<Option<BackendReport>> = std::sync::OnceLock::new();
    *REPORT.get_or_init(|| GpuContext::new().map(|context| context.backend_report()))
}

/// Reports native WGSL subgroup support for future kernels that can preserve
/// their tie-breaking semantics with subgroup reductions.
pub fn available_subgroup_report() -> Option<SubgroupReport> {
    static REPORT: std::sync::OnceLock<Option<SubgroupReport>> = std::sync::OnceLock::new();
    *REPORT.get_or_init(|| GpuContext::new().map(|context| context.subgroup_report()))
}

/// Probes the active GPU backend's 5-level hardware execution topology.
pub fn available_execution_topology() -> Option<HardwareExecutionTopology> {
    available_backend_report().map(|report| report.execution_topology())
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn tuned_workgroup_size_selects_metal_variant() {
        assert_eq!(tuned_workgroup_size(wgpu::Backend::Metal, 128, 256), 128);
        assert_eq!(backend_label(wgpu::Backend::Metal), "metal");
        assert_eq!(
            BackendReport::from_wgpu(wgpu::Backend::Metal),
            BackendReport {
                kind: BackendKind::Metal,
                label: "metal",
                environment: ApiEnvironment::NativeDriver,
                shader_ir: BackendShaderIr::Msl,
                native_api: true,
                browser_api: false,
                metal: true,
            }
        );
    }

    #[test]
    fn tuned_workgroup_size_falls_back_to_default_off_metal() {
        assert_eq!(tuned_workgroup_size(wgpu::Backend::Vulkan, 128, 256), 256);
        assert_eq!(tuned_workgroup_size(wgpu::Backend::Dx12, 128, 256), 256);
        assert_eq!(tuned_workgroup_size(wgpu::Backend::Gl, 128, 256), 256);
        assert_eq!(backend_label(wgpu::Backend::Vulkan), "vulkan");
        assert_eq!(backend_label(wgpu::Backend::Dx12), "dx12");
        assert_eq!(backend_label(wgpu::Backend::BrowserWebGpu), "webgpu");
        assert!(BackendReport::from_wgpu(wgpu::Backend::Vulkan).native_api);
        assert!(!BackendReport::from_wgpu(wgpu::Backend::Vulkan).browser_api);
        assert!(BackendReport::from_wgpu(wgpu::Backend::BrowserWebGpu).browser_api);
        assert!(!BackendReport::from_wgpu(wgpu::Backend::Noop).native_api);
    }

    #[test]
    fn available_backend_report_is_well_formed_when_present() {
        if let Some(report) = available_backend_report() {
            assert!(!report.label.is_empty());
            assert_eq!(
                report.label,
                backend_label(match report.kind {
                    BackendKind::Noop => wgpu::Backend::Noop,
                    BackendKind::Vulkan => wgpu::Backend::Vulkan,
                    BackendKind::Metal => wgpu::Backend::Metal,
                    BackendKind::Dx12 => wgpu::Backend::Dx12,
                    BackendKind::Gl => wgpu::Backend::Gl,
                    BackendKind::WebGpu => wgpu::Backend::BrowserWebGpu,
                    BackendKind::Cuda => wgpu::Backend::Cuda,
                })
            );
        }
    }

    #[test]
    fn available_subgroup_report_is_well_formed_when_present() {
        if let Some(report) = available_subgroup_report() {
            assert!(report.min_size <= report.max_size);
            if report.supported {
                assert!(report.min_size > 0);
            }
        }
    }

    #[test]
    fn execution_topology_and_2d_workgroup_recommendations_match_hardware_primitives() {
        let metal_topo = BackendReport::from_wgpu(wgpu::Backend::Metal).execution_topology();
        assert_eq!(metal_topo.memory_architecture, MemoryArchitecture::Uma);
        assert!(metal_topo.memory_architecture.profile().unified_physical_ram);
        assert!(metal_topo.memory_architecture.profile().zero_copy_primary_upload);
        assert!(!metal_topo.memory_architecture.profile().requires_explicit_staging_ring);
        assert_eq!(metal_topo.scalar_unit_label, "ALU / Execution Pipeline");
        assert_eq!(metal_topo.simd_width, 32);
        assert_eq!(metal_topo.simd_group_label, "SIMD-group (32 threads)");
        assert_eq!(metal_topo.max_threads_per_block, 1024);
        assert_eq!(metal_topo.local_sram_label, "threadgroup memory");
        assert_eq!(metal_topo.default_local_sram_bytes, 32768);
        assert_eq!(metal_topo.matrix_coprocessor_label, "simdgroup_matrix (8x8)");
        assert_eq!(metal_topo.matrix_tile_shape, [8, 8, 8]);
        assert_eq!(metal_topo.optimal_1d_workgroup_size, 128);
        assert_eq!(metal_topo.optimal_2d_tile_size, [32, 8]);

        let vk_topo = BackendReport::from_wgpu(wgpu::Backend::Vulkan).execution_topology();
        assert_eq!(vk_topo.memory_architecture, MemoryArchitecture::Duma);
        assert!(!vk_topo.memory_architecture.profile().unified_physical_ram);
        assert!(!vk_topo.memory_architecture.profile().zero_copy_primary_upload);
        assert!(vk_topo.memory_architecture.profile().requires_explicit_staging_ring);
        assert_eq!(vk_topo.scalar_unit_label, "CUDA Core / Streaming Processor");
        assert_eq!(vk_topo.simd_width, 32);
        assert_eq!(vk_topo.simd_group_label, "Warp (32 threads)");
        assert_eq!(vk_topo.local_sram_label, "__shared__ memory");
        assert_eq!(vk_topo.default_local_sram_bytes, 49152);
        assert_eq!(vk_topo.matrix_coprocessor_label, "Tensor Cores (wmma 16x16)");
        assert_eq!(vk_topo.matrix_tile_shape, [16, 16, 16]);
        assert_eq!(vk_topo.optimal_1d_workgroup_size, 256);
        assert_eq!(vk_topo.optimal_2d_tile_size, [16, 16]);

        let (tile_m, groups_m) = recommend_workgroup_shape_2d(wgpu::Backend::Metal, 100, 50);
        assert_eq!(tile_m, [32, 8]);
        assert_eq!(groups_m, [4, 7]);
        assert_eq!((tile_m[0] * tile_m[1]) % 32, 0);

        let (tile_v, groups_v) = recommend_workgroup_shape_2d(wgpu::Backend::Vulkan, 100, 50);
        assert_eq!(tile_v, [16, 16]);
        assert_eq!(groups_v, [7, 4]);
        assert_eq!((tile_v[0] * tile_v[1]) % 32, 0);

        if let Some(active) = available_execution_topology() {
            assert_eq!(active.simd_width, 32);
            assert_eq!(active.max_threads_per_block, 1024);
        }

        let (m_tiles, m_simds, m_sram) = metal_topo.estimate_matmul_occupancy(64, 64, 64);
        assert_eq!(m_tiles, 64); // (64/8) * (64/8) = 64 8x8 simdgroup_matrix tiles
        assert!(m_simds > 0);
        assert!(m_sram > 0 && m_sram <= metal_topo.default_local_sram_bytes);

        let (v_tiles, v_simds, v_sram) = vk_topo.estimate_matmul_occupancy(64, 64, 64);
        assert_eq!(v_tiles, 16); // (64/16) * (64/16) = 16 16x16 Tensor Core tiles
        assert!(v_simds > 0);
        assert!(v_sram > 0 && v_sram <= vk_topo.default_local_sram_bytes);
        assert_eq!(metal_topo.estimate_matmul_occupancy(0, 64, 64), (0, 0, 0));
    }

    #[test]
    fn wgsl_tiled_matmul_and_parallel_reduction_shader_generators_emit_valid_kernels() {
        let matmul_wgsl = generate_wgsl_tiled_matmul_shader(16);
        assert!(matmul_wgsl.contains("@compute @workgroup_size(16, 16, 1)"));
        assert!(matmul_wgsl.contains("var<workgroup> tile_a: array<f32, 256>;"));
        assert!(matmul_wgsl.contains("var<workgroup> tile_b: array<f32, 256>;"));
        assert!(matmul_wgsl.contains("workgroupBarrier();"));
        assert!(matmul_wgsl.contains("fma("));

        let reduce_wgsl = generate_wgsl_parallel_reduction_shader(256);
        assert!(reduce_wgsl.contains("@compute @workgroup_size(256, 1, 1)"));
        assert!(reduce_wgsl.contains("var<workgroup> shared_data: array<f32, 256>;"));
        assert!(reduce_wgsl.contains("workgroupBarrier();"));
        assert!(reduce_wgsl.contains("shared_data[tid] + shared_data[tid + stride]"));
    }

    #[test]
    fn asd_page_buffer_aligns_to_16k_and_preserves_data() {
        let payload = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let buf = AsdPageBuffer::from_slice(&payload);
        assert_eq!(buf.page_size(), 16384);
        assert_eq!(buf.aligned_capacity(), 16384);
        assert_eq!(buf.len(), 8);
        assert_eq!(buf.as_slice(), &payload);
    }

    #[test]
    fn unified_transfer_plan_selects_asd_uma_and_duma_appropriately() {
        let asd_plan = UnifiedTransferPlan::for_payload(wgpu::Backend::Metal, 65536, true);
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(asd_plan.memory_architecture, MemoryArchitecture::Uma);
            assert_eq!(
                asd_plan.storage_direct,
                StorageDirectCapability::MetalPageNoCopy
            );
            assert_eq!(asd_plan.tech, TransferStrategy::AsdPageNoCopy);
            assert!(asd_plan.uses_page_aligned_no_copy);
            assert!(asd_plan.zero_copy_cpu_write);
        }

        let duma_rebar =
            UnifiedTransferPlan::for_payload(wgpu::Backend::Vulkan, 1024 * 1024, false);
        assert_eq!(duma_rebar.memory_architecture, MemoryArchitecture::Duma);
        assert_eq!(duma_rebar.tech, TransferStrategy::DumaRebarWindow);
        assert!(duma_rebar.uses_rebar_aperture);
        assert_eq!(duma_rebar.staging_ring_chunks, 1);

        let duma_dma =
            UnifiedTransferPlan::for_payload(wgpu::Backend::Cuda, 64 * 1024 * 1024, false);
        assert_eq!(duma_dma.memory_architecture, MemoryArchitecture::Duma);
        assert_eq!(duma_dma.tech, TransferStrategy::DumaStagedDmaRing);
        assert!(!duma_dma.uses_rebar_aperture);
        assert_eq!(duma_dma.staging_ring_chunks, 4);
    }

    #[test]
    fn multi_target_shader_compiler_emits_msl_spirv_hlsl_ptx_glsl_and_wgsl() {
        let wgsl = generate_wgsl_parallel_reduction_shader(128);
        let artifact = compile_shader_multi_target(&wgsl).expect("valid multi-target compilation");
        assert!(artifact.msl.contains("kernel void"));
        assert!(!artifact.spirv_words.is_empty());
        assert!(artifact.hlsl.contains("[numthreads(128, 1, 1)]"));
        assert!(artifact.cuda_ptx.contains(".target sm_89"));
        assert!(artifact.glsl.contains("#version 460 core"));
        assert!(artifact.glsl.contains("layout(local_size_x = 128"));
        assert!(artifact.wgsl.contains("@compute @workgroup_size(128, 1, 1)"));
    }

    #[test]
    fn opengl_vulkan_and_rtx_raytracing_subsystems_work() {
        let gl = wgpu::opengl::OpenGlDevice::probe_available().expect("OpenGL device profile");
        assert!(gl.supports_compute_shaders);
        assert!(gl.supports_persistent_coherent_mapping);
        assert_ne!(gl.recommended_buffer_storage_flags(), 0);

        let vk = wgpu::vulkan::VulkanDevice::probe_available().expect("Vulkan 1.3 device profile");
        assert_eq!(vk.api_version.0, 1);
        assert_eq!(vk.api_version.1, 3);
        assert!(vk.supports_ray_tracing_pipeline);
        assert!(vk.supports_ray_query);
        assert!(vk.supports_external_memory_host_asd);
        assert_eq!(
            vk.storage_direct_capability(),
            StorageDirectCapability::VulkanExternalMemoryHost
        );

        let blas = wgpu::rtx::Blas::new(
            Some("cad-csg-blas"),
            vec![wgpu::rtx::BlasGeometryDescriptor {
                kind: wgpu::rtx::BlasGeometryKind::Triangles,
                vertex_count: 1024,
                primitive_count: 512,
                opaque: true,
                allow_compaction: true,
            }],
        );
        assert!(blas.compacted_size_bytes >= 512 * 64);
        let tlas = wgpu::rtx::Tlas::new(
            Some("cad-scene-tlas"),
            vec![wgpu::rtx::TlasInstance::identity(blas.device_address, 0)],
        );
        assert_eq!(tlas.instances.len(), 1);

        let rtx_vk = wgpu::rtx::RtxCapabilities::for_backend(wgpu::Backend::Vulkan);
        assert_eq!(rtx_vk.tier, wgpu::rtx::RtxTier::HardwareRtxPipeline);
        assert!(rtx_vk.supports_inline_ray_query);

        let rtx_cuda = wgpu::rtx::RtxCapabilities::for_backend(wgpu::Backend::Cuda);
        assert_eq!(rtx_cuda.tier, wgpu::rtx::RtxTier::HardwareRtxPipeline);

        let rtx_shader = generate_wgsl_rtx_raytracing_shader(8, 8);
        let rtx_artifact =
            compile_shader_multi_target(&rtx_shader).expect("RTX WGSL compiles to all 6 backends");
        assert!(rtx_artifact.msl.contains("kernel void"));
        assert!(rtx_artifact.glsl.contains("layout(local_size_x = 8, local_size_y = 8"));

        let webgpu =
            wgpu::webgpu::WebGpuDevice::probe_available().expect("Browser WebGPU profile");
        assert_eq!(webgpu.backend_report().environment, ApiEnvironment::BrowserSandbox);
        assert_eq!(webgpu.backend_report().shader_ir, BackendShaderIr::Wgsl);
        assert_eq!(webgpu.storage_direct_capability(), StorageDirectCapability::None);
        let webgpu_plan = webgpu.transfer_plan(1024 * 1024);
        assert_eq!(webgpu_plan.memory_architecture, MemoryArchitecture::Duma);
        assert_eq!(webgpu_plan.tech, TransferStrategy::DumaRebarWindow);
    }
}
