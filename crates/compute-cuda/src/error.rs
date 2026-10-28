use gpu_compute::cuda::DriverError;

#[derive(Debug)]
pub enum CudaError {
    Contract(tensor_core::TensorError),
    Unavailable(&'static str),
    MissingSymbol {
        library: &'static str,
        symbol: &'static str,
    },
    ForeignRuntime,
    SharedOutput,
    Dtype,
    ProgramPoisoned,
    PreparationBudget {
        kind: &'static str,
        required: usize,
        limit: usize,
    },
    InvalidInput(&'static str),
    Driver(DriverError),
    Compilation(String),
    Graph(gpu_compute::cuda::graph::CudaGraphError),
    Blas(String),
    UnsupportedPrecision(&'static str),
}
impl std::fmt::Display for CudaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract(error) => error.fmt(f),
            Self::Unavailable(reason) => write!(f, "CUDA unavailable: {reason}"),
            Self::MissingSymbol { library, symbol } => {
                write!(
                    f,
                    "incompatible CUDA {library} library: missing symbol {symbol}"
                )
            }
            Self::ForeignRuntime => f.write_str("tensor belongs to a different CUDA runtime"),
            Self::ProgramPoisoned => {
                f.write_str("prepared CUDA program is poisoned after an execution error")
            }
            Self::PreparationBudget {
                kind,
                required,
                limit,
            } => write!(
                f,
                "CUDA preparation {kind} budget exceeded: requires {required} bytes, limit {limit}"
            ),
            Self::Dtype => f.write_str("CUDA tensor/program dtype mismatch"),
            Self::SharedOutput => {
                f.write_str("updating CUDA storage requires an unshared contiguous allocation")
            }
            Self::InvalidInput(reason) => f.write_str(reason),
            Self::Driver(error) => error.fmt(f),
            Self::Compilation(error) => write!(f, "NVRTC compilation failed: {error}"),
            Self::Graph(error) => write!(f, "CUDA Graph failed: {error}"),
            Self::Blas(error) => write!(f, "cuBLAS failed: {error}"),
            Self::UnsupportedPrecision(reason) => {
                write!(f, "CUDA precision mode unsupported: {reason}")
            }
        }
    }
}
impl std::error::Error for CudaError {}
impl From<DriverError> for CudaError {
    fn from(value: DriverError) -> Self {
        Self::Driver(value)
    }
}
impl From<tensor_core::TensorError> for CudaError {
    fn from(value: tensor_core::TensorError) -> Self {
        Self::Contract(value)
    }
}

impl From<gpu_compute::cuda::graph::CudaGraphError> for CudaError {
    fn from(value: gpu_compute::cuda::graph::CudaGraphError) -> Self {
        Self::Graph(value)
    }
}
