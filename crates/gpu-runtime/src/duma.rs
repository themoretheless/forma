//! **dUMA** (Discrete / Decoupled Unified Memory Architecture):
//! Discrete PCIe VRAM (`NVIDIA RTX`, `AMD RDNA`, `Intel Arc`) with Resizable BAR (`ReBAR` / `SAM`)
//! host-visible aperture windows for small payloads (`<= 16 MiB`) and 4-way multi-buffered pinned DMA
//! staging rings for large streaming transfers (`> 16 MiB`).

use crate::platform::{MemoryArchitecture, StorageDirectCapability};
use crate::transfer::{TransferStrategy, UnifiedTransferPlan};

/// Default threshold (`16 MiB`) up to which discrete GPUs map payloads directly through the
/// PCIe Resizable BAR (`ReBAR` / Smart Access Memory) host-visible VRAM window.
pub const DUMA_REBAR_WINDOW_LIMIT_BYTES: u64 = 16 * 1024 * 1024;

/// Number of asynchronous pinned DMA ring chunks used when streaming payloads larger than [`DUMA_REBAR_WINDOW_LIMIT_BYTES`].
pub const DUMA_DMA_RING_CHUNKS: u32 = 4;

/// Hardware profile and PCIe ReBAR / DMA staging policy for **dUMA** (Discrete Unified Memory Architecture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DumaMemoryPolicy {
    /// Maximum payload size written directly into the PCIe Resizable BAR window without staging.
    pub rebar_window_limit_bytes: u64,
    /// Number of multi-buffered pinned host DMA chunks for large transfers.
    pub dma_staging_ring_chunks: u32,
    /// Orthogonal ASD direct storage capability (`GPUDirect Storage cuFile`, `DirectStorage`, `VK_EXT_external_memory_host`).
    pub storage_direct: StorageDirectCapability,
}

impl DumaMemoryPolicy {
    #[inline]
    pub const fn new(storage_direct: StorageDirectCapability) -> Self {
        Self {
            rebar_window_limit_bytes: DUMA_REBAR_WINDOW_LIMIT_BYTES,
            dma_staging_ring_chunks: DUMA_DMA_RING_CHUNKS,
            storage_direct,
        }
    }

    /// Selects the optimal [`UnifiedTransferPlan`] on a **dUMA** adapter:
    /// 1. `AsdPageNoCopy` when page-aligned and ASD is supported (`cuFile` / `DirectStorage` / `VK_EXT_external_memory_host`),
    /// 2. `DumaRebarWindow` when `byte_len <= 16 MiB` (direct write into PCIe ReBAR aperture),
    /// 3. `DumaStagedDmaRing` when `byte_len > 16 MiB` (4-way asynchronous pinned DMA staging ring).
    #[inline]
    pub const fn plan_transfer(&self, byte_len: u64, page_aligned: bool) -> UnifiedTransferPlan {
        if page_aligned && !matches!(self.storage_direct, StorageDirectCapability::None) {
            return UnifiedTransferPlan {
                memory_architecture: MemoryArchitecture::Duma,
                storage_direct: self.storage_direct,
                tech: TransferStrategy::AsdPageNoCopy,
                zero_copy_cpu_write: true,
                zero_copy_gpu_read: true,
                uses_page_aligned_no_copy: true,
                uses_rebar_aperture: false,
                staging_ring_chunks: 0,
            };
        }

        let fits_rebar = byte_len <= self.rebar_window_limit_bytes;
        UnifiedTransferPlan {
            memory_architecture: MemoryArchitecture::Duma,
            storage_direct: self.storage_direct,
            tech: if fits_rebar {
                TransferStrategy::DumaRebarWindow
            } else {
                TransferStrategy::DumaStagedDmaRing
            },
            zero_copy_cpu_write: fits_rebar,
            zero_copy_gpu_read: true,
            uses_page_aligned_no_copy: false,
            uses_rebar_aperture: fits_rebar,
            staging_ring_chunks: if fits_rebar {
                1
            } else {
                self.dma_staging_ring_chunks
            },
        }
    }
}
