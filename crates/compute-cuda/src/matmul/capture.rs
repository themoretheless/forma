//! Graph-owned cuBLAS state. All initialization happens before stream capture.
use super::*;
use gpu_compute::cuda::cudarc::driver::result as driver;
use std::sync::{Mutex, MutexGuard};

const ALIGNMENT: usize = 256;
const MIN_WORKSPACE: usize = 16 * 1024;

fn bind_cleanup(stream: &CudaStream) -> bool {
    // bind_to_thread consumes cudarc's stored async/Drop error first. Cleanup
    // must still run when that earlier error is pending on a valid context.
    let context = stream.context();
    match unsafe { driver::ctx::set_current(context.cu_ctx()) } {
        Ok(()) => true,
        Err(error) => {
            context.record_err::<()>(Err(error));
            false
        }
    }
}

/// CUDA 12.8's recommended per-handle workspace. This is a capacity default,
/// not a performance promise; callers may select another checked capacity.
/// https://docs.nvidia.com/cuda/archive/12.8.0/cublas/index.html#cublassetworkspace
pub(crate) fn capture_default_workspace_bytes(capability: (i32, i32)) -> usize {
    if matches!(capability.0, 9 | 10) {
        32 * 1024 * 1024
    } else {
        4 * 1024 * 1024
    }
}

/// Exact physical allocation charged before any GPU resources are created.
/// Padding keeps the advertised usable capacity independent of pointer alignment.
pub(crate) fn capture_workspace_allocation_bytes(requested: usize) -> Result<usize, CudaError> {
    if requested < MIN_WORKSPACE {
        return Err(CudaError::InvalidInput(
            "cuBLAS capture workspace must be at least 16 KiB",
        ));
    }
    requested
        .checked_add(ALIGNMENT - 1)
        .filter(|&bytes| bytes <= isize::MAX as usize)
        .ok_or(CudaError::InvalidInput(
            "cuBLAS capture workspace allocation is too large",
        ))
}

fn workspace_pointer(base: u64, allocation: usize, requested: usize) -> Result<u64, CudaError> {
    let required = capture_workspace_allocation_bytes(requested)?;
    if allocation < required {
        return Err(CudaError::InvalidInput(
            "cuBLAS workspace allocation is shorter than its checked capacity",
        ));
    }
    let aligned = base
        .checked_add((ALIGNMENT - 1) as u64)
        .map(|value| value & !((ALIGNMENT - 1) as u64))
        .ok_or(CudaError::InvalidInput("cuBLAS workspace pointer overflow"))?;
    let end = aligned
        .checked_add(requested as u64)
        .ok_or(CudaError::InvalidInput("cuBLAS workspace pointer overflow"))?;
    let allocation_end = base
        .checked_add(allocation as u64)
        .ok_or(CudaError::InvalidInput("cuBLAS workspace pointer overflow"))?;
    if end > allocation_end {
        return Err(CudaError::InvalidInput(
            "cuBLAS aligned workspace exceeds its allocation",
        ));
    }
    Ok(aligned)
}

/// Unlike cudarc::CudaSlice, this private allocation never creates dependency
/// events. The owning graph retains it until graph destruction and synchronizes
/// before releasing resources; no caller can alias or access the workspace.
#[derive(Debug)]
struct Workspace {
    base: u64,
    stream: Arc<CudaStream>,
}
impl Drop for Workspace {
    fn drop(&mut self) {
        // This allocation was created synchronously outside capture. Freeing it
        // synchronously also protects cleanup after partial initialization.
        let context = self.stream.context();
        if bind_cleanup(&self.stream) {
            context.record_err(unsafe { driver::free_sync(self.base) });
        }
    }
}

#[derive(Debug)]
struct Handle(sys::cublasHandle_t);
// The opaque cuBLAS handle may move between threads. Its only access is behind
// the owner's mutex, with its retained CUDA context bound on the calling thread.
unsafe impl Send for Handle {}

/// One immutable-stream handle and private workspace per captured program.
/// GEMMs in that program share workspace because their launches are ordered on
/// that one stream. The mutex also prevents interleaved host call sequences.
#[derive(Debug)]
pub(crate) struct CaptureBlas {
    handle: Mutex<Handle>,
    workspace: Workspace,
}
pub(crate) struct CaptureHandle<'a>(MutexGuard<'a, Handle>);
impl CaptureHandle<'_> {
    pub(crate) fn raw(&self) -> sys::cublasHandle_t {
        self.0.0
    }
}
impl CaptureBlas {
    pub(crate) fn stream(&self) -> &Arc<CudaStream> {
        &self.workspace.stream
    }
    pub(crate) fn lock(&self) -> Result<CaptureHandle<'_>, CudaError> {
        self.workspace.stream.context().bind_to_thread()?;
        self.handle
            .lock()
            .map(CaptureHandle)
            .map_err(|_| CudaError::InvalidInput("cuBLAS capture handle is poisoned"))
    }
}
impl Drop for CaptureBlas {
    fn drop(&mut self) {
        // No cudarc CudaBlas::Drop unwrap and no configuration changes during
        // replay. The workspace field drops only after handle destruction.
        let handle = self
            .handle
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        if !handle.0.is_null() && bind_cleanup(&self.workspace.stream) {
            self.workspace.stream.context().record_err(
                unsafe { result::destroy_handle(handle.0) }.map_err(|_| {
                    // The context error channel stores driver errors only;
                    // preserve a cleanup failure there instead of panicking.
                    driver::DriverError(
                        gpu_compute::cuda::cudarc::driver::sys::CUresult::CUDA_ERROR_UNKNOWN,
                    )
                }),
            );
            handle.0 = std::ptr::null_mut();
        }
    }
}

impl CudaRuntime {
    /// Allocate and configure a graph-private handle before capture. The caller
    /// must budget `capture_workspace_allocation_bytes(requested)` beforehand.
    /// No event tracking settings or eager cuBLAS handle state are changed.
    pub(crate) fn prepare_capture_blas(
        &self,
        requested: usize,
    ) -> Result<Arc<CaptureBlas>, CudaError> {
        let bytes = capture_workspace_allocation_bytes(requested)?;
        if self.device.stream.cu_stream().is_null() {
            return Err(CudaError::InvalidInput(
                "cuBLAS capture requires an explicit CUDA stream",
            ));
        }
        crate::libraries::require_cublas_capture()?;
        self.device.context.bind_to_thread()?;
        // No CudaSlice wrapper: upgrade_device_ptr would create tracked events.
        let workspace = Workspace {
            base: unsafe { driver::malloc_sync(bytes) }?,
            stream: self.device.stream.clone(),
        };
        let pointer = workspace_pointer(workspace.base, bytes, requested)?;
        let handle = result::create_handle().map_err(|e| CudaError::Blas(e.to_string()))?;
        let owner = CaptureBlas {
            handle: Mutex::new(Handle(handle)),
            workspace,
        };
        // SetStream resets user workspace, so it MUST precede SetWorkspace.
        // HOST alpha/beta values are captured by value by cuBLAS, not referenced
        // on later launches: https://docs.nvidia.com/cuda/archive/12.8.0/cublas/index.html#cuda-graphs-support
        unsafe {
            result::set_stream(handle, self.device.stream.cu_stream() as _)
                .map_err(|e| CudaError::Blas(e.to_string()))?;
            sys::cublasSetPointerMode_v2(
                handle,
                sys::cublasPointerMode_t::CUBLAS_POINTER_MODE_HOST,
            )
            .result()
            .map_err(|e| CudaError::Blas(e.to_string()))?;
            sys::cublasSetWorkspace_v2(handle, pointer as *mut _, requested)
                .result()
                .map_err(|e| CudaError::Blas(e.to_string()))?;
        }
        Ok(Arc::new(owner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn graph_workspace_budget_includes_padding_and_rejects_underflow_or_overflow() {
        for n in [0, 1, MIN_WORKSPACE - 1] {
            assert!(capture_workspace_allocation_bytes(n).is_err());
        }
        for n in [MIN_WORKSPACE, MIN_WORKSPACE + 1, 4 * 1024 * 1024] {
            assert_eq!(capture_workspace_allocation_bytes(n).unwrap(), n + 255);
        }
        assert_eq!(
            capture_workspace_allocation_bytes(isize::MAX as usize - 255).unwrap(),
            isize::MAX as usize
        );
        assert!(capture_workspace_allocation_bytes(isize::MAX as usize - 254).is_err());
        assert!(capture_workspace_allocation_bytes(usize::MAX).is_err());
    }
    #[test]
    fn every_base_alignment_preserves_the_full_workspace_capacity() {
        let requested = MIN_WORKSPACE + 17;
        let allocation = capture_workspace_allocation_bytes(requested).unwrap();
        for offset in 0..256 {
            let base = 0x1_0000 + offset;
            let pointer = workspace_pointer(base, allocation, requested).unwrap();
            assert_eq!(pointer % 256, 0);
            assert!(pointer >= base && pointer - base < 256);
            assert!(pointer + requested as u64 <= base + allocation as u64);
        }
        assert!(workspace_pointer(0x1000, allocation - 1, requested).is_err());
        assert!(workspace_pointer(u64::MAX - 3, allocation, requested).is_err());
        assert!(workspace_pointer(u64::MAX - 255, allocation, requested).is_err());
    }
    #[test]
    fn recommended_workspace_defaults_follow_cuda_12_8_architecture_table() {
        for cc in [(5, 0), (7, 5), (8, 0), (8, 9), (12, 0)] {
            assert_eq!(capture_default_workspace_bytes(cc), 4 * 1024 * 1024);
        }
        for cc in [(9, 0), (10, 0), (10, 3)] {
            assert_eq!(capture_default_workspace_bytes(cc), 32 * 1024 * 1024);
        }
    }
}
