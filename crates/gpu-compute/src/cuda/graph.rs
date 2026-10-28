//! Low-level CUDA Graph ownership. Captured pointers are borrowed, not retained.
//!
//! Callers must keep every captured allocation, module, event and library workspace
//! alive through the final replay's completion. cudarc slice events do not track a
//! raw graph replay: synchronize or explicitly publish completion before accessing
//! those buffers elsewhere. Capture never changes context event-tracking settings.
//!
//! See NVIDIA's [capture restrictions](https://docs.nvidia.com/cuda/archive/12.8.1/cuda-c-programming-guide/index.html#prohibited-and-unhandled-operations).
use super::{CudaStream, cudarc::driver::sys};
use std::{fmt, marker::PhantomData, rc::Rc, sync::Arc};
mod driver;
mod lifecycle;
#[cfg(test)]
mod tests;
pub use driver::CudaGraphApi;
use driver::Driver;
use lifecycle::{Capture, Executable};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CudaGraphError {
    Unavailable,
    MissingSymbol(&'static str),
    LegacyStream,
    BlockingStream,
    AlreadyCapturing,
    EmptyGraph,
    Driver {
        operation: &'static str,
        code: sys::CUresult,
    },
}
impl fmt::Display for CudaGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("CUDA driver library unavailable"),
            Self::MissingSymbol(name) => {
                write!(f, "CUDA Graph driver entry point unavailable: {name}")
            }
            Self::LegacyStream => {
                f.write_str("CUDA Graph capture requires an explicit nonblocking stream")
            }
            Self::BlockingStream => f.write_str("CUDA Graph capture requires a nonblocking stream"),
            Self::AlreadyCapturing => {
                f.write_str("CUDA stream is already capturing or invalidated")
            }
            Self::EmptyGraph => f.write_str("CUDA capture returned no graph or executable handle"),
            Self::Driver { operation, code } => {
                write!(f, "{operation} failed: {code:?} ({})", *code as u32)
            }
        }
    }
}
impl std::error::Error for CudaGraphError {}

/// A same-thread capture guard. Drop ends an abandoned/invalidated capture and
/// destroys any returned graph. Use `abort` to receive cleanup errors explicitly.
pub struct CudaGraphCapture {
    inner: Capture<Driver>,
    _local: PhantomData<Rc<()>>,
}
impl CudaGraphCapture {
    /// Begins thread-local capture after validating the stream and complete API.
    ///
    /// # Safety
    /// The caller must exclusively control this stream until finish/abort/drop,
    /// follow CUDA capture restrictions, and preserve all resources referenced by
    /// captured work through every replay. In particular, ordinary cudarc slice
    /// waits on pre-capture events are not valid capture dependencies. Do not
    /// synchronize/query captured work or drop captured resources during capture.
    pub unsafe fn begin(
        stream: Arc<CudaStream>,
        api: &CudaGraphApi,
    ) -> Result<Self, CudaGraphError> {
        Ok(Self {
            inner: Capture::begin(Driver { stream, api: *api })?,
            _local: PhantomData,
        })
    }
    pub fn finish(self) -> Result<CudaGraph, CudaGraphError> {
        Ok(CudaGraph {
            inner: self.inner.finish()?,
            _local: PhantomData,
        })
    }
    pub fn abort(self) -> Result<(), CudaGraphError> {
        self.inner.abort()
    }
}

/// Instantiated graph and its source graph, retaining only the origin stream.
/// This type is intentionally neither Send nor Sync. Methods require exclusive
/// access; CUDA graph objects are not internally synchronized.
pub struct CudaGraph {
    inner: Executable<Driver>,
    _local: PhantomData<Rc<()>>,
}
impl CudaGraph {
    /// Pre-uploads graph resources on the origin stream without running nodes.
    ///
    /// # Safety
    /// All captured resources must still be valid. The caller must preserve them
    /// while the asynchronous upload is pending, and keep the stream out of capture.
    pub unsafe fn upload(&mut self) -> Result<(), CudaGraphError> {
        self.inner.upload()
    }
    /// Replays on the origin stream. No slice event is refreshed by this method.
    ///
    /// # Safety
    /// All captured pointers/resources must remain valid and correctly ordered
    /// until replay completion. Avoid concurrent access to captured writable
    /// storage; explicitly synchronize before reading/freeing it on another stream.
    pub unsafe fn launch(&mut self) -> Result<(), CudaGraphError> {
        self.inner.launch()
    }
    pub fn stream(&self) -> &Arc<CudaStream> {
        &self.inner.backend.stream
    }
}
