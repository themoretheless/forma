use crate::KernelError;

#[derive(Debug)]
pub enum ComputeError {
    Kernel(KernelError),
    Buffer(gpu_compute::BufferError),
    TooLarge {
        bytes: u64,
        limit: u64,
    },
    LengthMismatch {
        expected: usize,
        actual: usize,
    },
    ShapeMismatch {
        expected: [usize; 2],
        actual: [usize; 2],
    },
    OutOfBounds,
    BudgetExceeded {
        requested: u64,
        budget: u64,
    },
    ForeignArray,
    AliasedOutput,
    Readback(String),
    Transfer(gpu_compute::ReadbackError),
    ReadbackConsumed,
}

impl std::fmt::Display for ComputeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kernel(e) => e.fmt(f),
            Self::Buffer(e) => e.fmt(f),
            Self::TooLarge { bytes, limit } => {
                write!(f, "GPU allocation needs {bytes} bytes; limit is {limit}")
            }
            Self::LengthMismatch { expected, actual } => {
                write!(f, "array length {actual}; expected {expected}")
            }
            Self::ShapeMismatch { expected, actual } => write!(
                f,
                "matrix shape {}x{}; expected {}x{}",
                actual[0], actual[1], expected[0], expected[1]
            ),
            Self::BudgetExceeded { requested, budget } => write!(
                f,
                "scratch pool needs {requested} bytes; budget is {budget}"
            ),
            Self::OutOfBounds => write!(f, "array range is out of bounds"),
            Self::ForeignArray => write!(f, "array belongs to another compute runtime"),
            Self::AliasedOutput => write!(
                f,
                "output shares storage with an input; use a separate output array"
            ),
            Self::Transfer(e) => e.fmt(f),
            Self::Readback(e) => write!(f, "GPU readback failed: {e}"),
            Self::ReadbackConsumed => write!(f, "readback result was already consumed"),
        }
    }
}
impl std::error::Error for ComputeError {}
impl From<KernelError> for ComputeError {
    fn from(e: KernelError) -> Self {
        Self::Kernel(e)
    }
}

impl From<gpu_compute::ReadbackError> for ComputeError {
    fn from(error: gpu_compute::ReadbackError) -> Self {
        match error {
            gpu_compute::ReadbackError::Consumed => Self::ReadbackConsumed,
            other => Self::Transfer(other),
        }
    }
}

impl From<gpu_compute::BufferError> for ComputeError {
    fn from(e: gpu_compute::BufferError) -> Self {
        Self::Buffer(e)
    }
}
