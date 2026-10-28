//! Cross-platform hardware capability matrix across all 6 target operating systems:
//! **Windows**, **macOS**, **Linux**, **iOS**, **Android**, and **Browser (WebGPU)**,
//! together with the 5 fundamental GPU execution topology levels and **ASD / UMA / dUMA** memory tiers.

use crate::{Backend, Backends};

/// Target operating systems supported by the unified GPU & shader runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetPlatform {
    Windows,
    MacOs,
    Linux,
    Ios,
    Android,
    BrowserWebGpu,
}

impl TargetPlatform {
    #[inline]
    pub const fn current() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            Self::BrowserWebGpu
        }
        #[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
        {
            Self::MacOs
        }
        #[cfg(all(not(target_arch = "wasm32"), target_os = "ios"))]
        {
            Self::Ios
        }
        #[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
        {
            Self::Windows
        }
        #[cfg(all(not(target_arch = "wasm32"), target_os = "android"))]
        {
            Self::Android
        }
        #[cfg(all(
            not(target_arch = "wasm32"),
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "windows"),
            not(target_os = "android")
        ))]
        {
            Self::Linux
        }
    }

    pub const ALL: [Self; 6] = [
        Self::Windows,
        Self::MacOs,
        Self::Linux,
        Self::Ios,
        Self::Android,
        Self::BrowserWebGpu,
    ];

    #[inline]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Linux => "linux",
            Self::Ios => "ios",
            Self::Android => "android",
            Self::BrowserWebGpu => "browser-webgpu",
        }
    }

    #[inline]
    pub const fn primary_backend(self) -> Backend {
        match self {
            Self::MacOs | Self::Ios => Backend::Metal,
            Self::Windows => Backend::Dx12,
            Self::Linux | Self::Android => Backend::Vulkan,
            Self::BrowserWebGpu => Backend::BrowserWebGpu,
        }
    }

    #[inline]
    pub const fn supported_backends(self) -> Backends {
        match self {
            Self::MacOs => Backends(Backends::METAL.0 | Backends::VULKAN.0 | Backends::GL.0),
            Self::Ios => Backends(Backends::METAL.0 | Backends::VULKAN.0),
            Self::Windows => {
                Backends(Backends::DX12.0 | Backends::VULKAN.0 | Backends::CUDA.0 | Backends::GL.0)
            }
            Self::Linux => Backends(Backends::VULKAN.0 | Backends::CUDA.0 | Backends::GL.0),
            Self::Android => Backends(Backends::VULKAN.0 | Backends::GL.0),
            Self::BrowserWebGpu => Backends(Backends::BROWSER_WEBGPU.0 | Backends::GL.0),
        }
    }

    #[inline]
    pub const fn supports_cuda(self) -> bool {
        matches!(self, Self::Windows | Self::Linux)
    }

    #[inline]
    pub const fn default_page_size(self) -> usize {
        match self {
            Self::MacOs | Self::Ios => 16384, // Apple Silicon 16 KiB VM pages
            Self::Windows | Self::Linux | Self::Android | Self::BrowserWebGpu => 4096,
        }
    }
}

/// Physical and logical GPU memory architecture (`UMA` vs `dUMA`):
/// - [`MemoryArchitecture::Uma`] (Unified Memory Architecture, e.g. Apple Silicon Metal / Mobile SoCs / Integrated APUs):
///   CPU and GPU share a single coherent physical DRAM pool (`StorageModeShared`).
///   Host-to-device uploads map primary buffers directly (`MAP_WRITE | STORAGE`) with zero PCIe staging copies.
/// - [`MemoryArchitecture::Duma`] (Discrete / Decoupled Memory Architecture, e.g. NVIDIA CUDA / PCIe Vulkan / DX12 / WebGPU):
///   CPU RAM and GPU VRAM are physically or logically separated across a bus/staging boundary.
///   Uploads use Resizable BAR (`ReBAR` / `SAM`) or explicit staging ring buffers (`queue.write_buffer` / `cudaMemcpyAsync`) and device-local VRAM (`StorageModePrivate`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemoryArchitecture {
    /// Unified Memory Architecture (Apple Silicon Metal `StorageModeShared`, zero-copy CPU/GPU DRAM).
    Uma,
    /// Discrete / Decoupled Memory Architecture (dedicated VRAM + ReBAR/PCIe/staging transfer).
    Duma,
}

/// Orthogonal **ASD** (Accelerated Storage Direct) I/O & Zero-Copy Page Mapping capability that operates
/// *on top of* either [`MemoryArchitecture::Uma`] or [`MemoryArchitecture::Duma`]:
/// - On `UMA` (macOS / iOS Apple Silicon): `MTLDevice::newBufferWithBytesNoCopy:` maps 16 KiB page-aligned `mmap`/RAM directly into the GPU address space.
/// - On `dUMA` (Linux / Windows / Android): `cuFile` (GPUDirect Storage), `DirectStorage` (DX12), or `VK_EXT_external_memory_host` (Vulkan) streams storage pages straight into GPU buffers without CPU staging copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StorageDirectCapability {
    /// Standard buffer allocation path without direct page/storage mapping.
    None,
    /// Apple Silicon Direct / Metal `newBufferWithBytesNoCopy:` zero-copy VM page mapping (`16 KiB` pages).
    MetalPageNoCopy,
    /// Vulkan `VK_EXT_external_memory_host` / `VK_KHR_external_memory_fd` direct host/storage page import.
    VulkanExternalMemoryHost,
    /// NVIDIA GPUDirect Storage (`cuFileRead` / `cuFileWrite` direct NVMe -> VRAM DMA).
    NvidiaGpuDirectStorage,
    /// Microsoft DirectStorage (`IDStorageQueue` direct NVMe -> DX12 heap decompression).
    MicrosoftDirectStorage,
}

/// Strategy for uploading, storing, and reading back GPU buffers under [`MemoryArchitecture::Uma`] and [`MemoryArchitecture::Duma`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryArchitectureProfile {
    pub architecture: MemoryArchitecture,
    pub label: &'static str,
    pub unified_physical_ram: bool,
    pub zero_copy_primary_upload: bool,
    pub requires_explicit_staging_ring: bool,
    pub preferred_upload_strategy: &'static str,
    pub preferred_readback_strategy: &'static str,
}

impl MemoryArchitecture {
    #[inline]
    pub const fn profile(self) -> MemoryArchitectureProfile {
        match self {
            Self::Uma => MemoryArchitectureProfile {
                architecture: Self::Uma,
                label: "UMA (Unified Memory Architecture)",
                unified_physical_ram: true,
                zero_copy_primary_upload: true,
                requires_explicit_staging_ring: false,
                preferred_upload_strategy: "direct_mapped_primary_buffer",
                preferred_readback_strategy: "shared_coherent_map_read",
            },
            Self::Duma => MemoryArchitectureProfile {
                architecture: Self::Duma,
                label: "dUMA (Discrete Memory Architecture)",
                unified_physical_ram: false,
                zero_copy_primary_upload: false,
                requires_explicit_staging_ring: true,
                preferred_upload_strategy: "staging_ring_to_device_local_vram",
                preferred_readback_strategy: "dma_copy_to_staging_then_map",
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackendKind {
    Noop,
    Vulkan,
    Metal,
    Dx12,
    Gl,
    WebGpu,
    Cuda,
}

/// Host/API execution environment of a GPU backend (Native OS driver vs Browser Sandbox vs Noop).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApiEnvironment {
    /// Direct native OS/driver runtime (`Metal`, `Vulkan`, `Dx12`, `Cuda`, `OpenGL`).
    NativeDriver,
    /// Sandboxed W3C WebGPU browser runtime (`navigator.gpu`).
    BrowserSandbox,
    /// Headless / null stub backend (`Noop`).
    NoopStub,
}

/// Shader Intermediate Representation (IR) / Shading Language consumed by a GPU backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackendShaderIr {
    /// Apple Metal Shading Language (`MSL` C++14)
    Msl,
    /// Khronos `SPIR-V` binary words (Vulkan 1.3)
    Spirv,
    /// Microsoft `HLSL` / `DXIL` Shader Model 6.6 (DirectX 12)
    Hlsl,
    /// NVIDIA `PTX` (`Parallel Thread Execution` ISA for CUDA Driver API)
    Ptx,
    /// Khronos `GLSL` `#version 460 core` / `#version 320 es` (OpenGL / OpenGL ES)
    Glsl,
    /// W3C `WGSL` text (Browser WebGPU)
    Wgsl,
    /// None (`Noop` backend)
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendReport {
    pub kind: BackendKind,
    pub label: &'static str,
    pub environment: ApiEnvironment,
    pub shader_ir: BackendShaderIr,
    pub native_api: bool,
    pub browser_api: bool,
    pub metal: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubgroupReport {
    pub supported: bool,
    pub min_size: u32,
    pub max_size: u32,
}

/// Hardware execution topology across the 5 fundamental GPU architecture primitives
/// plus the memory architecture (`UMA` vs `dUMA`):
/// 1. Scalar execution unit (`ALU / Execution Pipeline` on Metal vs `CUDA Core / Streaming Processor`)
/// 2. 32-thread lockstep execution unit (`SIMD-group (32 threads)` on Metal vs `Warp (32 threads)`)
/// 3. Thread block / Threadgroup (`max_threads_per_block` up to 1024)
/// 4. On-chip programmable SRAM (`threadgroup memory` 32 KB vs `__shared__ memory` 48 KB)
/// 5. Matrix Coprocessor (`simdgroup_matrix (8x8)` on Apple Silicon vs `Tensor Cores (wmma 16x16)`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardwareExecutionTopology {
    pub memory_architecture: MemoryArchitecture,
    pub scalar_unit_label: &'static str,
    pub simd_width: u32,
    pub simd_group_label: &'static str,
    pub max_threads_per_block: u32,
    pub local_sram_label: &'static str,
    pub default_local_sram_bytes: u32,
    pub matrix_coprocessor_label: &'static str,
    pub matrix_tile_shape: [u32; 3],
    pub optimal_1d_workgroup_size: u32,
    pub optimal_2d_tile_size: [u32; 2],
}

impl HardwareExecutionTopology {
    /// Estimates matrix-multiply occupancy `(tile_count, total_simd_groups, local_sram_bytes_per_block)`
    /// for a product `[rows, inner] x [inner, cols] -> [rows, cols]`, mapping onto 32-thread
    /// Warps/SIMD-groups, threadgroups, and `simdgroup_matrix` (8x8) / Tensor Core (16x16) tiles.
    pub const fn estimate_matmul_occupancy(
        &self,
        rows: u32,
        inner: u32,
        cols: u32,
    ) -> (u32, u32, u32) {
        if rows == 0 || inner == 0 || cols == 0 {
            return (0, 0, 0);
        }
        let tm = if self.matrix_tile_shape[0] > 0 {
            self.matrix_tile_shape[0]
        } else {
            8
        };
        let tn = if self.matrix_tile_shape[1] > 0 {
            self.matrix_tile_shape[1]
        } else {
            8
        };
        let tk = if self.matrix_tile_shape[2] > 0 {
            self.matrix_tile_shape[2]
        } else {
            8
        };
        let tiles_m = rows.div_ceil(tm);
        let tiles_n = cols.div_ceil(tn);
        let tile_count = tiles_m.saturating_mul(tiles_n);

        let block_x = if self.optimal_2d_tile_size[0] > 0 {
            self.optimal_2d_tile_size[0]
        } else {
            16
        };
        let block_y = if self.optimal_2d_tile_size[1] > 0 {
            self.optimal_2d_tile_size[1]
        } else {
            16
        };
        let groups_x = cols.div_ceil(block_x);
        let groups_y = rows.div_ceil(block_y);
        let total_blocks = groups_x.saturating_mul(groups_y);
        let threads_per_block = block_x.saturating_mul(block_y);
        let simd_width = if self.simd_width > 0 {
            self.simd_width
        } else {
            32
        };
        let simd_groups_per_block = threads_per_block.div_ceil(simd_width);
        let total_simd_groups = total_blocks.saturating_mul(simd_groups_per_block);

        // Two f32 tiles in local SRAM per threadblock: A_tile (block_y x tk) + B_tile (tk x block_x)
        let sram_floats = block_y
            .saturating_mul(tk)
            .saturating_add(tk.saturating_mul(block_x));
        let raw_sram_bytes = sram_floats.saturating_mul(4);
        let local_sram_bytes_per_block = if raw_sram_bytes > self.default_local_sram_bytes {
            self.default_local_sram_bytes
        } else {
            raw_sram_bytes
        };

        (tile_count, total_simd_groups, local_sram_bytes_per_block)
    }
}

impl BackendReport {
    #[inline]
    pub const fn from_wgpu(backend: Backend) -> Self {
        match backend {
            Backend::Noop => Self {
                kind: BackendKind::Noop,
                label: "noop",
                environment: ApiEnvironment::NoopStub,
                shader_ir: BackendShaderIr::None,
                native_api: false,
                browser_api: false,
                metal: false,
            },
            Backend::Vulkan => Self {
                kind: BackendKind::Vulkan,
                label: "vulkan",
                environment: ApiEnvironment::NativeDriver,
                shader_ir: BackendShaderIr::Spirv,
                native_api: true,
                browser_api: false,
                metal: false,
            },
            Backend::Metal => Self {
                kind: BackendKind::Metal,
                label: "metal",
                environment: ApiEnvironment::NativeDriver,
                shader_ir: BackendShaderIr::Msl,
                native_api: true,
                browser_api: false,
                metal: true,
            },
            Backend::Dx12 => Self {
                kind: BackendKind::Dx12,
                label: "dx12",
                environment: ApiEnvironment::NativeDriver,
                shader_ir: BackendShaderIr::Hlsl,
                native_api: true,
                browser_api: false,
                metal: false,
            },
            Backend::Gl => Self {
                kind: BackendKind::Gl,
                label: "gl",
                environment: ApiEnvironment::NativeDriver,
                shader_ir: BackendShaderIr::Glsl,
                native_api: true,
                browser_api: false,
                metal: false,
            },
            Backend::BrowserWebGpu => Self {
                kind: BackendKind::WebGpu,
                label: "webgpu",
                environment: ApiEnvironment::BrowserSandbox,
                shader_ir: BackendShaderIr::Wgsl,
                native_api: false,
                browser_api: true,
                metal: false,
            },
            Backend::Cuda => Self {
                kind: BackendKind::Cuda,
                label: "cuda",
                environment: ApiEnvironment::NativeDriver,
                shader_ir: BackendShaderIr::Ptx,
                native_api: true,
                browser_api: false,
                metal: false,
            },
        }
    }

    /// Returns the default physical memory architecture (`Uma` vs `Duma`) for this backend on the current target OS.
    /// For explicit per-adapter unified-memory overriding (e.g. Android Vulkan UMA, Intel/AMD Integrated APU, NVIDIA Grace Hopper),
    /// use [`Self::memory_architecture_for_device`].
    #[inline]
    pub const fn memory_architecture(&self) -> MemoryArchitecture {
        match self.kind {
            BackendKind::Metal if cfg!(all(any(target_os = "macos", target_os = "ios"), target_arch = "aarch64")) => {
                MemoryArchitecture::Uma
            }
            BackendKind::Vulkan if cfg!(target_os = "android") => MemoryArchitecture::Uma,
            BackendKind::Metal
            | BackendKind::Vulkan
            | BackendKind::Dx12
            | BackendKind::Cuda
            | BackendKind::Gl
            | BackendKind::WebGpu
            | BackendKind::Noop => MemoryArchitecture::Duma,
        }
    }

    /// Resolves the physical memory architecture (`Uma` vs `Duma`) given the hardware adapter's actual `unified_memory` flag.
    #[inline]
    pub const fn memory_architecture_for_device(&self, adapter_has_unified_memory: bool) -> MemoryArchitecture {
        if adapter_has_unified_memory {
            MemoryArchitecture::Uma
        } else {
            self.memory_architecture()
        }
    }

    #[inline]
    pub const fn memory_profile(&self) -> MemoryArchitectureProfile {
        self.memory_architecture().profile()
    }

    #[inline]
    pub const fn storage_direct_capability(&self) -> StorageDirectCapability {
        match self.kind {
            BackendKind::Metal => StorageDirectCapability::MetalPageNoCopy,
            BackendKind::Vulkan => StorageDirectCapability::VulkanExternalMemoryHost,
            BackendKind::Cuda => StorageDirectCapability::NvidiaGpuDirectStorage,
            BackendKind::Dx12 => StorageDirectCapability::MicrosoftDirectStorage,
            BackendKind::Gl | BackendKind::WebGpu | BackendKind::Noop => {
                StorageDirectCapability::None
            }
        }
    }

    #[inline]
    pub const fn execution_topology(&self) -> HardwareExecutionTopology {
        match self.kind {
            BackendKind::Metal => HardwareExecutionTopology {
                memory_architecture: self.memory_architecture(),
                scalar_unit_label: "ALU / Execution Pipeline",
                simd_width: 32,
                simd_group_label: "SIMD-group (32 threads)",
                max_threads_per_block: 1024,
                local_sram_label: "threadgroup memory",
                default_local_sram_bytes: 32768,
                matrix_coprocessor_label: "simdgroup_matrix (8x8)",
                matrix_tile_shape: [8, 8, 8],
                optimal_1d_workgroup_size: 128,
                optimal_2d_tile_size: [32, 8],
            },
            BackendKind::Vulkan | BackendKind::Dx12 | BackendKind::Cuda => {
                HardwareExecutionTopology {
                    memory_architecture: MemoryArchitecture::Duma,
                    scalar_unit_label: "CUDA Core / Streaming Processor",
                    simd_width: 32,
                    simd_group_label: "Warp (32 threads)",
                    max_threads_per_block: 1024,
                    local_sram_label: "__shared__ memory",
                    default_local_sram_bytes: 49152,
                    matrix_coprocessor_label: "Tensor Cores (wmma 16x16)",
                    matrix_tile_shape: [16, 16, 16],
                    optimal_1d_workgroup_size: 256,
                    optimal_2d_tile_size: [16, 16],
                }
            }
            BackendKind::WebGpu | BackendKind::Gl | BackendKind::Noop => {
                HardwareExecutionTopology {
                    memory_architecture: self.memory_architecture(),
                    scalar_unit_label: "Portable Shader Execution Unit",
                    simd_width: 32,
                    simd_group_label: "Subgroup (32 lanes)",
                    max_threads_per_block: 1024,
                    local_sram_label: "workgroup shared memory",
                    default_local_sram_bytes: 32768,
                    matrix_coprocessor_label: "Tiled Workgroup Matrix (16x16)",
                    matrix_tile_shape: [16, 16, 16],
                    optimal_1d_workgroup_size: 256,
                    optimal_2d_tile_size: [16, 16],
                }
            }
        }
    }
}

#[inline]
pub const fn backend_label(backend: Backend) -> &'static str {
    BackendReport::from_wgpu(backend).label
}

/// Recommends a 2D workgroup shape `([wg_x, wg_y], [groups_x, groups_y])` aligned to the
/// 32-thread Warp/SIMD-group for a `width x height` 2D grid.
#[inline]
pub const fn recommend_workgroup_shape_2d(
    backend: Backend,
    width: u32,
    height: u32,
) -> ([u32; 2], [u32; 2]) {
    let topo = BackendReport::from_wgpu(backend).execution_topology();
    let tile = topo.optimal_2d_tile_size;
    let gx = if width == 0 {
        0
    } else {
        width.div_ceil(tile[0])
    };
    let gy = if height == 0 {
        0
    } else {
        height.div_ceil(tile[1])
    };
    (tile, [gx, gy])
}

/// Per-backend workgroup-size tuning for compute kernels that template their
/// WGSL source with a `WG` constant.
#[inline]
pub const fn tuned_workgroup_size(backend: Backend, metal_size: u32, default_size: u32) -> u32 {
    match backend {
        Backend::Metal => metal_size,
        _ => default_size,
    }
}
