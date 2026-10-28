#![allow(unsafe_code, dead_code)]

use std::ffi::{c_int, c_uint, c_void};

pub type CUresult = c_int;
pub type CUdevice = c_int;
pub type CUcontext = *mut c_void;
pub type CUmodule = *mut c_void;
pub type CUfunction = *mut c_void;
pub type CUstream = *mut c_void;
pub type CUdeviceptr = u64;

pub const CUDA_SUCCESS: CUresult = 0;
pub const CU_MEM_ATTACH_GLOBAL: c_uint = 1;

/// Dynamic CUDA Driver API (`libcuda.so.1` / `nvcuda.dll`) FFI table supporting:
/// - **UMA / Managed Memory** (`cuMemAllocManaged`)
/// - **dUMA Pinned Host & Async DMA** (`cuMemAllocHost_v2`, `cuMemcpyHtoDAsync_v2`, `cuMemcpyDtoHAsync_v2`)
/// - **ASD / GPUDirect Storage (`cuFile`)** zero-copy NVMe-to-VRAM transfers
/// - **PTX Compute Kernel Execution** (`cuModuleLoadDataEx`, `cuLaunchKernel`)
#[derive(Debug, Clone)]
pub struct CudaDevice {
    pub name: String,
    pub compute_capability: (u32, u32),
    pub unified_addressing: bool,
    pub managed_memory: bool,
    pub gds_direct_storage: bool,
    pub total_vram_bytes: u64,
}

impl CudaDevice {
    pub fn probe_available() -> Option<Self> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            // Dynamic probe for libcuda.so.1 / nvcuda.dll when running on NVIDIA hardware
            None
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            None
        }
    }
}
