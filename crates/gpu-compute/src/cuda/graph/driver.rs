use super::lifecycle::Backend;
use super::*;
use crate::cuda::DriverError;

type SetContext = unsafe extern "C" fn(sys::CUcontext) -> sys::CUresult;
type StreamFlags = unsafe extern "C" fn(sys::CUstream, *mut u32) -> sys::CUresult;
type CaptureStatus =
    unsafe extern "C" fn(sys::CUstream, *mut sys::CUstreamCaptureStatus) -> sys::CUresult;
type Begin = unsafe extern "C" fn(sys::CUstream, sys::CUstreamCaptureMode) -> sys::CUresult;
type End = unsafe extern "C" fn(sys::CUstream, *mut sys::CUgraph) -> sys::CUresult;
type Instantiate = unsafe extern "C" fn(*mut sys::CUgraphExec, sys::CUgraph, u64) -> sys::CUresult;
type ExecStream = unsafe extern "C" fn(sys::CUgraphExec, sys::CUstream) -> sys::CUresult;
type DestroyGraph = unsafe extern "C" fn(sys::CUgraph) -> sys::CUresult;
type DestroyExec = unsafe extern "C" fn(sys::CUgraphExec) -> sys::CUresult;

/// Complete, checked CUDA Graph ABI group. Availability is not hardware proof.
#[derive(Clone, Copy)]
pub struct CudaGraphApi {
    set_context: SetContext,
    stream_flags: StreamFlags,
    capture_status: CaptureStatus,
    begin: Begin,
    end: End,
    instantiate: Instantiate,
    upload: ExecStream,
    launch: ExecStream,
    destroy_graph: DestroyGraph,
    destroy_exec: DestroyExec,
}
impl CudaGraphApi {
    pub fn load() -> Result<Self, CudaGraphError> {
        static API: std::sync::OnceLock<Result<CudaGraphApi, CudaGraphError>> =
            std::sync::OnceLock::new();
        API.get_or_init(|| {
            if !unsafe { sys::is_culib_present() } {
                return Err(CudaGraphError::Unavailable);
            }
            let library = unsafe { sys::culib() };
            macro_rules! get {
                ($name:literal,$ty:ty) => {{
                    let symbol = unsafe { library.get::<$ty>(concat!($name, "\0").as_bytes()) }
                        .map_err(|_| CudaGraphError::MissingSymbol($name))?;
                    *symbol
                }};
            }
            Ok(Self {
                set_context: get!("cuCtxSetCurrent", SetContext),
                stream_flags: get!("cuStreamGetFlags", StreamFlags),
                capture_status: get!("cuStreamIsCapturing", CaptureStatus),
                begin: get!("cuStreamBeginCapture_v2", Begin),
                end: get!("cuStreamEndCapture", End),
                instantiate: get!("cuGraphInstantiateWithFlags", Instantiate),
                upload: get!("cuGraphUpload", ExecStream),
                launch: get!("cuGraphLaunch", ExecStream),
                destroy_graph: get!("cuGraphDestroy", DestroyGraph),
                destroy_exec: get!("cuGraphExecDestroy", DestroyExec),
            })
        })
        .clone()
    }
}
fn check(operation: &'static str, code: sys::CUresult) -> Result<(), CudaGraphError> {
    if code == sys::CUresult::CUDA_SUCCESS {
        Ok(())
    } else {
        Err(CudaGraphError::Driver { operation, code })
    }
}
#[derive(Clone)]
pub(super) struct Driver {
    pub stream: Arc<CudaStream>,
    pub api: CudaGraphApi,
}
impl Backend for Driver {
    type Graph = sys::CUgraph;
    type Exec = sys::CUgraphExec;
    fn bind(&self) -> Result<(), CudaGraphError> {
        self.stream
            .context()
            .check_err()
            .map_err(|e| CudaGraphError::Driver {
                operation: "prior CUDA operation",
                code: e.0,
            })?;
        self.bind_cleanup()
    }
    fn bind_cleanup(&self) -> Result<(), CudaGraphError> {
        // Cleanup must not be skipped just because cudarc stored a prior error.
        check("cuCtxSetCurrent", unsafe {
            (self.api.set_context)(self.stream.context().cu_ctx())
        })
    }
    fn validate_stream(&self) -> Result<(), CudaGraphError> {
        let stream = self.stream.cu_stream();
        if stream.is_null() || stream as usize == 1 {
            return Err(CudaGraphError::LegacyStream);
        }
        let mut flags = 0;
        check("cuStreamGetFlags", unsafe {
            (self.api.stream_flags)(stream, &mut flags)
        })?;
        if flags & sys::CUstream_flags::CU_STREAM_NON_BLOCKING as u32 == 0 {
            return Err(CudaGraphError::BlockingStream);
        }
        if self.is_capturing()? {
            return Err(CudaGraphError::AlreadyCapturing);
        }
        Ok(())
    }
    fn is_capturing(&self) -> Result<bool, CudaGraphError> {
        let mut status = sys::CUstreamCaptureStatus::CU_STREAM_CAPTURE_STATUS_NONE;
        check("cuStreamIsCapturing", unsafe {
            (self.api.capture_status)(self.stream.cu_stream(), &mut status)
        })?;
        Ok(status != sys::CUstreamCaptureStatus::CU_STREAM_CAPTURE_STATUS_NONE)
    }
    fn begin(&self) -> Result<(), CudaGraphError> {
        check("cuStreamBeginCapture_v2", unsafe {
            (self.api.begin)(
                self.stream.cu_stream(),
                sys::CUstreamCaptureMode::CU_STREAM_CAPTURE_MODE_THREAD_LOCAL,
            )
        })
    }
    fn end(&self) -> (Self::Graph, Result<(), CudaGraphError>) {
        let mut graph = std::ptr::null_mut();
        let result = check("cuStreamEndCapture", unsafe {
            (self.api.end)(self.stream.cu_stream(), &mut graph)
        });
        (graph, result)
    }
    fn instantiate(&self, graph: Self::Graph) -> (Self::Exec, Result<(), CudaGraphError>) {
        let mut exec = std::ptr::null_mut();
        let result = check("cuGraphInstantiateWithFlags", unsafe {
            (self.api.instantiate)(&mut exec, graph, 0)
        });
        (exec, result)
    }
    fn upload(&self, exec: Self::Exec) -> Result<(), CudaGraphError> {
        check("cuGraphUpload", unsafe {
            (self.api.upload)(exec, self.stream.cu_stream())
        })
    }
    fn launch(&self, exec: Self::Exec) -> Result<(), CudaGraphError> {
        check("cuGraphLaunch", unsafe {
            (self.api.launch)(exec, self.stream.cu_stream())
        })
    }
    fn destroy_graph(&self, graph: Self::Graph) -> Result<(), CudaGraphError> {
        check("cuGraphDestroy", unsafe { (self.api.destroy_graph)(graph) })
    }
    fn destroy_exec(&self, exec: Self::Exec) -> Result<(), CudaGraphError> {
        check("cuGraphExecDestroy", unsafe {
            (self.api.destroy_exec)(exec)
        })
    }
    fn cleanup_error(&self, error: CudaGraphError) {
        if let CudaGraphError::Driver { code, .. } = error {
            self.stream
                .context()
                .record_err::<()>(Err(DriverError(code)));
        }
    }
}
