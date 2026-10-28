//! **UMA** (Unified Memory Architecture):
//! Shared CPU+GPU physical DRAM (`Apple Silicon M1..M4`, `iOS`, `Android SoC`, `AMD/Intel APU`)
//! where primary storage/compute buffers are zero-copy mapped directly into host/device address space.

use crate::platform::{MemoryArchitecture, StorageDirectCapability};
use crate::transfer::{TransferStrategy, UnifiedTransferPlan};

/// Hardware profile and zero-copy allocation policy for **UMA** (Unified Memory Architecture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UmaMemoryPolicy {
    /// Physical RAM is shared coherently between CPU cores and GPU compute units.
    pub coherent_shared_ram: bool,
    /// Primary GPU storage buffers can be written directly by CPU pointers without a staging copy.
    pub zero_copy_primary_buffers: bool,
    /// Orthogonal ASD zero-copy page-aligned `mmap` capability on top of UMA (`MTLDevice::newBufferWithBytesNoCopy:` / `VK_EXT_external_memory_host`).
    pub storage_direct: StorageDirectCapability,
}

impl UmaMemoryPolicy {
    #[inline]
    pub const fn new(storage_direct: StorageDirectCapability) -> Self {
        Self {
            coherent_shared_ram: true,
            zero_copy_primary_buffers: true,
            storage_direct,
        }
    }

    /// Selects the optimal [`UnifiedTransferPlan`] on a **UMA** adapter (`AsdPageNoCopy` when page-aligned + ASD-capable, otherwise `UmaSharedPointer`).
    #[inline]
    pub const fn plan_transfer(&self, _byte_len: u64, page_aligned: bool) -> UnifiedTransferPlan {
        if page_aligned && !matches!(self.storage_direct, StorageDirectCapability::None) {
            UnifiedTransferPlan {
                memory_architecture: MemoryArchitecture::Uma,
                storage_direct: self.storage_direct,
                tech: TransferStrategy::AsdPageNoCopy,
                zero_copy_cpu_write: true,
                zero_copy_gpu_read: true,
                uses_page_aligned_no_copy: true,
                uses_rebar_aperture: false,
                staging_ring_chunks: 0,
            }
        } else {
            UnifiedTransferPlan {
                memory_architecture: MemoryArchitecture::Uma,
                storage_direct: self.storage_direct,
                tech: TransferStrategy::UmaSharedPointer,
                zero_copy_cpu_write: true,
                zero_copy_gpu_read: true,
                uses_page_aligned_no_copy: false,
                uses_rebar_aperture: false,
                staging_ring_chunks: 0,
            }
        }
    }
}
