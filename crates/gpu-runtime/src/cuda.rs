//! Host-gated NVIDIA CUDA / RTX policy (`gpu-compute::cuda`, `compute-cuda`).
//!
//! CUDA is a compute-only driver API rather than a WebGPU graphics surface pipeline, and WGSL is
//! never lowered to fake offline PTX (`G-11`). NVIDIA GPUs inside `gpu-runtime::Instance` run via
//! Vulkan or DX12, while native CUDA PTX and RT hardware kernels execute through `gpu-compute`
//! only when an NVIDIA driver is present on Linux or Windows (`G-6`).

use crate::platform::TargetPlatform;

/// Whether CUDA feature plumbing is compiled for a CUDA-capable OS (`Linux` or `Windows`).
#[inline]
pub const fn is_compiled() -> bool {
    cfg!(feature = "cuda") && TargetPlatform::current().supports_cuda()
}

/// Non-intrusive host check for an NVIDIA CUDA driver installation.
///
/// Always returns `false` on non-CUDA OSes (`macOS`, `iOS`, `Android`, `wasm32`) and when `feature = "cuda"`
/// is disabled. Never fabricates an adapter or device profile.
pub fn probe_driver_available() -> bool {
    if !is_compiled() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new("/dev/nvidiactl").exists() {
            return true;
        }
        [
            "/usr/lib/x86_64-linux-gnu/libcuda.so.1",
            "/usr/lib/aarch64-linux-gnu/libcuda.so.1",
            "/usr/lib64/libcuda.so.1",
            "/usr/lib/libcuda.so.1",
        ]
        .iter()
        .any(|path| std::path::Path::new(path).exists())
    }
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        std::path::Path::new(&root)
            .join("System32")
            .join("nvcuda.dll")
            .exists()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        false
    }
}
