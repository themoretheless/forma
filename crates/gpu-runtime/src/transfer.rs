//! Unified memory transfer coordinator dispatching between [` crate::uma::UmaMemoryPolicy`],
//! [`crate::duma::DumaMemoryPolicy`], and [`crate::asd::AsdPageBuffer`].

use crate::duma::DumaMemoryPolicy;
use crate::platform::{BackendReport, MemoryArchitecture, StorageDirectCapability};
use crate::uma::UmaMemoryPolicy;

/// Transfer mechanism chosen for a specific buffer/storage operation, cleanly separating
/// the adapter's physical [`MemoryArchitecture`] (`Uma` vs `Duma`) from how a specific transfer is performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferStrategy {
    /// **ASD** (Accelerated Storage Direct / Apple Silicon Direct / GPUDirect Storage / DirectStorage / `VK_EXT_external_memory_host`):
    /// Zero-copy page-aligned storage or `mmap` mapped directly into GPU virtual address space (works on top of both `UMA` and `dUMA`).
    AsdPageNoCopy,
    /// **UMA** (Unified Memory Architecture):
    /// Direct CPU read/write into shared physical DRAM (`MTLStorageModeShared` / `VK_MEMORY_PROPERTY_HOST_COHERENT_BIT`).
    UmaSharedPointer,
    /// **dUMA ReBAR** (Discrete Unified Memory Architecture via Resizable BAR / Smart Access Memory):
    /// Direct CPU write into host-visible PCIe BAR aperture window (`<= 16 MiB`) backed by discrete VRAM.
    DumaRebarWindow,
    /// **dUMA Pinned DMA Ring** (Discrete VRAM with multi-buffered asynchronous DMA staging ring):
    /// High-bandwidth streaming (`> 16 MiB`) across PCIe via 4-way double-buffered pinned host memory.
    DumaStagedDmaRing,
}

/// Backwards-compatible alias for [`TransferStrategy`].
pub type ModernMemoryTech = TransferStrategy;

/// Transfer planner selecting the fastest path by delegating to [`UmaMemoryPolicy`] or [`DumaMemoryPolicy`]
/// combined with orthogonal [`StorageDirectCapability`] (`ASD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnifiedTransferPlan {
    /// Physical RAM/VRAM topology of the adapter (`Uma` or `Duma`).
    pub memory_architecture: MemoryArchitecture,
    /// Orthogonal storage-direct / page-import capability of the backend (`MetalPageNoCopy`, `VulkanExternalMemoryHost`, `NvidiaGpuDirectStorage`, `MicrosoftDirectStorage`, or `None`).
    pub storage_direct: StorageDirectCapability,
    /// Effective transfer technology selected for this payload.
    pub tech: ModernMemoryTech,
    pub zero_copy_cpu_write: bool,
    pub zero_copy_gpu_read: bool,
    pub uses_page_aligned_no_copy: bool,
    pub uses_rebar_aperture: bool,
    pub staging_ring_chunks: u32,
}

impl UnifiedTransferPlan {
    /// Computes the optimal transfer plan for `byte_len` bytes on the given `backend`.
    pub const fn for_payload(backend: crate::Backend, byte_len: u64, page_aligned: bool) -> Self {
        let report = BackendReport::from_wgpu(backend);
        let storage_direct = report.storage_direct_capability();
        match report.memory_architecture() {
            MemoryArchitecture::Uma => {
                UmaMemoryPolicy::new(storage_direct).plan_transfer(byte_len, page_aligned)
            }
            MemoryArchitecture::Duma => {
                DumaMemoryPolicy::new(storage_direct).plan_transfer(byte_len, page_aligned)
            }
        }
    }
}
