use crate::CudaError;
use gpu_compute::cuda::cudarc;

const NVRTC_SYMBOLS: &[&str] = &[
    "nvrtcGetNumSupportedArchs",
    "nvrtcGetSupportedArchs",
    "nvrtcCreateProgram",
    "nvrtcCompileProgram",
    "nvrtcDestroyProgram",
    "nvrtcGetPTXSize",
    "nvrtcGetPTX",
    "nvrtcGetProgramLogSize",
    "nvrtcGetProgramLog",
];
const CUBLAS_SYMBOLS: &[&str] = &[
    "cublasCreate_v2",
    "cublasSetStream_v2",
    "cublasDestroy_v2",
    "cublasGemmEx",
];
const CUBLAS_CAPTURE_SYMBOLS: &[&str] = &["cublasSetPointerMode_v2", "cublasSetWorkspace_v2"];
const CUBLAS_CAPTURE_DRIVER_SYMBOLS: &[&str] =
    &["cuMemAlloc_v2", "cuMemFree_v2", "cuCtxSetCurrent"];

// cudarc 0.19.9 safe/core.rs and driver/result.rs: the private context wrapper,
// nonblocking stream, and the two outside-capture event bridges. Native graph
// capture/instantiate/launch symbols have their own complete platform API group.
const GRAPH_RUNTIME_DRIVER_SYMBOLS: &[&str] = &[
    "cuInit",
    "cuDeviceGet",
    "cuDeviceGetAttribute",
    "cuDevicePrimaryCtxRetain",
    "cuDevicePrimaryCtxRelease_v2",
    "cuCtxGetCurrent",
    "cuCtxSetCurrent",
    "cuStreamCreate",
    "cuStreamDestroy_v2",
    "cuStreamSynchronize",
    "cuStreamWaitEvent",
    "cuEventCreate",
    "cuEventRecord",
    "cuEventDestroy_v2",
    // DriverError formatting must not turn a recoverable CUDA error into a panic.
    "cuGetErrorName",
    "cuGetErrorString",
];

// cudarc's presence probe only dlopens; its function loader panics when an old
// loadable library lacks a symbol. Check the same retained library first, so
// even panic=abort builds return a useful error before calling into cudarc.
pub(crate) fn require_nvrtc() -> Result<(), CudaError> {
    if !unsafe { cudarc::nvrtc::sys::is_culib_present() } {
        return Err(CudaError::Unavailable(
            "NVRTC library is missing; provide compatible precompiled PTX with from_ptx",
        ));
    }
    validate_symbols("NVRTC", NVRTC_SYMBOLS, |symbol| unsafe {
        cudarc::nvrtc::sys::culib()
            .get::<*const std::ffi::c_void>(symbol.as_bytes())
            .is_ok()
    })
}
pub(crate) fn require_cublas() -> Result<(), CudaError> {
    if !unsafe { cudarc::cublas::sys::is_culib_present() } {
        return Err(CudaError::Unavailable("cuBLAS library is missing"));
    }
    validate_symbols("cuBLAS", CUBLAS_SYMBOLS, |symbol| unsafe {
        cudarc::cublas::sys::culib()
            .get::<*const std::ffi::c_void>(symbol.as_bytes())
            .is_ok()
    })
}
/// Graph-only additions remain optional for ordinary eager/prepared GEMM.
pub(crate) fn require_cublas_capture() -> Result<(), CudaError> {
    require_cublas()?;
    validate_symbols("cuBLAS", CUBLAS_CAPTURE_SYMBOLS, |symbol| unsafe {
        cudarc::cublas::sys::culib()
            .get::<*const std::ffi::c_void>(symbol.as_bytes())
            .is_ok()
    })?;
    require_driver_symbols(CUBLAS_CAPTURE_DRIVER_SYMBOLS)
}
/// Preflight before creating the private wrapper/stream or any graph-owned
/// allocation. Does not add graph-only requirements to ordinary eager use.
pub(crate) fn require_graph_runtime() -> Result<(), CudaError> {
    require_driver_symbols(GRAPH_RUNTIME_DRIVER_SYMBOLS)
}
fn require_driver_symbols(symbols: &[&'static str]) -> Result<(), CudaError> {
    if !unsafe { cudarc::driver::sys::is_culib_present() } {
        return Err(CudaError::Unavailable("CUDA driver library is missing"));
    }
    validate_symbols("CUDA driver", symbols, |symbol| unsafe {
        cudarc::driver::sys::culib()
            .get::<*const std::ffi::c_void>(symbol.as_bytes())
            .is_ok()
    })
}

fn validate_symbols(
    library: &'static str,
    symbols: &[&'static str],
    mut available: impl FnMut(&str) -> bool,
) -> Result<(), CudaError> {
    for &symbol in symbols {
        if !available(symbol) {
            return Err(CudaError::MissingSymbol { library, symbol });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_library_returns_its_missing_symbol_before_any_call() {
        let result = validate_symbols("NVRTC", NVRTC_SYMBOLS, |s| s != "nvrtcGetSupportedArchs");
        assert!(matches!(
            result,
            Err(CudaError::MissingSymbol {
                library: "NVRTC",
                symbol: "nvrtcGetSupportedArchs"
            })
        ));
        assert!(validate_symbols("cuBLAS", CUBLAS_SYMBOLS, |_| true).is_ok());
        assert!(matches!(
            validate_symbols("cuBLAS", CUBLAS_SYMBOLS, |s| s != "cublasDestroy_v2"),
            Err(CudaError::MissingSymbol {
                symbol: "cublasDestroy_v2",
                ..
            })
        ));
    }

    #[test]
    fn capture_symbols_are_preflighted_without_expanding_the_eager_requirement() {
        assert!(!CUBLAS_SYMBOLS.contains(&"cublasSetWorkspace_v2"));
        for &missing in CUBLAS_CAPTURE_SYMBOLS {
            assert!(matches!(
                validate_symbols("cuBLAS", CUBLAS_CAPTURE_SYMBOLS, |name| name != missing),
                Err(CudaError::MissingSymbol { library: "cuBLAS", symbol }) if symbol == missing
            ));
        }
        for &missing in CUBLAS_CAPTURE_DRIVER_SYMBOLS {
            assert!(matches!(
                validate_symbols("CUDA driver", CUBLAS_CAPTURE_DRIVER_SYMBOLS, |name| name != missing),
                Err(CudaError::MissingSymbol { library: "CUDA driver", symbol }) if symbol == missing
            ));
        }
    }
    #[test]
    fn graph_runtime_missing_entries_fail_before_any_later_entry_is_used() {
        for (index, &missing) in GRAPH_RUNTIME_DRIVER_SYMBOLS.iter().enumerate() {
            let mut visited = Vec::new();
            let result = validate_symbols("CUDA driver", GRAPH_RUNTIME_DRIVER_SYMBOLS, |name| {
                visited.push(name.to_owned());
                name != missing
            });
            assert!(matches!(result, Err(CudaError::MissingSymbol {
                library: "CUDA driver", symbol
            }) if symbol == missing));
            assert_eq!(visited.len(), index + 1);
            assert_eq!(visited.last().unwrap(), missing);
        }
        assert!(validate_symbols("CUDA driver", GRAPH_RUNTIME_DRIVER_SYMBOLS, |_| true).is_ok());
    }
}
