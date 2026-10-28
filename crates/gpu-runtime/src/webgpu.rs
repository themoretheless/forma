//! **Browser WebGPU** (`navigator.gpu` / W3C WebGPU API) runtime & capability profile.
//!
//! In a browser (`target_arch = "wasm32"`), JavaScript/WASM memory (`WebAssembly.Memory`) is isolated
//! from the GPU process (`BrowserSandbox`), so even on an Apple Silicon host the browser's WebGPU API
//! requires explicit `mappedAtCreation` or `GPUQueue.writeBuffer` transfers across the sandbox boundary.

#![allow(unsafe_code, dead_code)]

use crate::platform::{
    ApiEnvironment, BackendKind, BackendReport, BackendShaderIr, MemoryArchitecture,
    StorageDirectCapability,
};
use crate::transfer::{TransferStrategy, UnifiedTransferPlan};

/// Browser WebGPU WGSL language extension features exposed by `navigator.gpu.wgslLanguageFeatures`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WgslLanguageFeature {
    LinearIndexing,
    ImmediateAddressSpace,
    BufferView,
    SwizzleAssignment,
    PrimitiveIndex,
    SubgroupId,
    SubgroupUniformity,
    TextureAndSamplerLet,
    TextureFormatsTier1,
    TextureFormatsTier2,
    UniformBufferStandardLayout,
}

impl WgslLanguageFeature {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LinearIndexing => "linear_indexing",
            Self::ImmediateAddressSpace => "immediate_address_space",
            Self::BufferView => "buffer_view",
            Self::SwizzleAssignment => "swizzle_assignment",
            Self::PrimitiveIndex => "primitive_index",
            Self::SubgroupId => "subgroup_id",
            Self::SubgroupUniformity => "subgroup_uniformity",
            Self::TextureAndSamplerLet => "texture_and_sampler_let",
            Self::TextureFormatsTier1 => "texture_formats_tier1",
            Self::TextureFormatsTier2 => "texture_formats_tier2",
            Self::UniformBufferStandardLayout => "uniform_buffer_standard_layout",
        }
    }
}

/// Browser WebGPU adapter & device runtime descriptor (`navigator.gpu`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebGpuDevice {
    pub name: String,
    /// Host GPU underlying the browser (`Uma` when browser runs on Apple Silicon / integrated SoC,
    /// `Duma` when browser runs on discrete PCIe GPU), while WASM linear memory itself remains sandboxed.
    pub host_memory_architecture: MemoryArchitecture,
    /// Whether `GPUBufferDescriptor.mappedAtCreation = true` avoids an extra staging copy in the browser's GPU process.
    pub mapped_at_creation_fast_path: bool,
    /// Whether `subgroups` (`enable subgroups;`) is available on this browser `GPUAdapter`.
    pub supports_subgroups: bool,
    /// Whether `timestamp-query` is available on this browser `GPUAdapter`.
    pub supports_timestamp_query: bool,
    /// Minimum subgroup size reported by `adapter.limits.subgroupMinSize`.
    pub subgroup_min_size: u32,
    /// Maximum subgroup size reported by `adapter.limits.subgroupMaxSize`.
    pub subgroup_max_size: u32,
}

impl WebGpuDevice {
    /// Probes or constructs the Browser WebGPU device profile.
    pub fn probe_available() -> Option<Self> {
        let host_uma = cfg!(any(
            all(target_os = "macos", target_arch = "aarch64"),
            target_os = "ios",
            target_os = "android"
        ));
        Some(Self {
            name: "W3C Browser WebGPU (navigator.gpu)".to_string(),
            host_memory_architecture: if host_uma {
                MemoryArchitecture::Uma
            } else {
                MemoryArchitecture::Duma
            },
            mapped_at_creation_fast_path: host_uma,
            supports_subgroups: true,
            supports_timestamp_query: true,
            subgroup_min_size: 32,
            subgroup_max_size: 32,
        })
    }

    /// Returns the canonical [`BackendReport`] for Browser WebGPU (`ApiEnvironment::BrowserSandbox`, `BackendShaderIr::Wgsl`).
    #[inline]
    pub const fn backend_report(&self) -> BackendReport {
        BackendReport {
            kind: BackendKind::WebGpu,
            label: "webgpu",
            environment: ApiEnvironment::BrowserSandbox,
            shader_ir: BackendShaderIr::Wgsl,
            native_api: false,
            browser_api: true,
            metal: false,
        }
    }

    /// Browser WebGPU cannot map OS host files (`mmap`) directly into GPU memory without a copy (`StorageDirectCapability::None`).
    #[inline]
    pub const fn storage_direct_capability(&self) -> StorageDirectCapability {
        StorageDirectCapability::None
    }

    /// Computes the optimal browser transfer plan for `byte_len` bytes across the WASM -> `navigator.gpu` boundary.
    #[inline]
    pub const fn transfer_plan(&self, byte_len: u64) -> UnifiedTransferPlan {
        let fits_rebar_or_mapped = byte_len <= 16 * 1024 * 1024;
        UnifiedTransferPlan {
            memory_architecture: MemoryArchitecture::Duma,
            storage_direct: StorageDirectCapability::None,
            tech: if fits_rebar_or_mapped {
                TransferStrategy::DumaRebarWindow
            } else {
                TransferStrategy::DumaStagedDmaRing
            },
            zero_copy_cpu_write: fits_rebar_or_mapped,
            zero_copy_gpu_read: true,
            uses_page_aligned_no_copy: false,
            uses_rebar_aperture: fits_rebar_or_mapped,
            staging_ring_chunks: if fits_rebar_or_mapped { 1 } else { 4 },
        }
    }
}
