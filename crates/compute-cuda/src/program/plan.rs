use crate::CudaError;
use tensor_core::{
    AttentionPlan, BinaryOp, CompareOp, Layout, LowDtype, MatmulPrecision, ReduceOp, ScanOptions,
    ScatterOp, Shape,
};

/// Storage type of a prepared input, intermediate or output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CudaDtype {
    F32,
    U32,
    F16,
    Bf16,
}
impl CudaDtype {
    pub fn byte_width(self) -> usize {
        match self {
            Self::F32 | Self::U32 => 4,
            Self::F16 | Self::Bf16 => 2,
        }
    }
    pub fn low_dtype(self) -> Option<LowDtype> {
        match self {
            Self::F16 => Some(LowDtype::F16),
            Self::Bf16 => Some(LowDtype::Bf16),
            Self::F32 | Self::U32 => None,
        }
    }
}
impl From<LowDtype> for CudaDtype {
    fn from(value: LowDtype) -> Self {
        match value {
            LowDtype::F16 => Self::F16,
            LowDtype::Bf16 => Self::Bf16,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TensorSpec {
    pub layout: Layout,
    pub dtype: CudaDtype,
}
impl TensorSpec {
    pub fn shape(&self) -> &Shape {
        self.layout.shape()
    }
}

/// Allocation identity in a prepared schedule. Views change only the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BufferRef {
    Input(usize),
    Scratch(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedValue {
    /// Logical transformations of an input-backed value, independent of its
    /// caller storage strides. Scratch-backed values keep this empty.
    pub input_views: Vec<InputView>,
    pub buffer: BufferRef,
    pub layout: Layout,
    pub dtype: CudaDtype,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InputView {
    Permute(Vec<usize>),
    Broadcast(Shape),
    Reshape(Shape),
    Promote { shape: Shape, left: bool },
}
impl InputView {
    pub(super) fn apply(&self, layout: &Layout) -> Result<Layout, CudaError> {
        Ok(match self {
            Self::Permute(axes) => layout.permute(axes)?,
            Self::Broadcast(shape) => layout.broadcast_to(shape.clone())?,
            Self::Reshape(shape) => layout.reshape(shape.clone())?,
            Self::Promote { shape, left } => {
                if layout.shape().rank() != 1 {
                    return Err(CudaError::InvalidInput("invalid input matrix promotion"));
                }
                let stride = layout.strides()[0];
                Layout::new(
                    shape.clone(),
                    if *left {
                        vec![0, stride]
                    } else {
                        vec![stride, 0]
                    },
                    layout.offset(),
                )?
            }
        })
    }
}
impl PlannedValue {
    fn view(&mut self, operation: InputView) -> Result<(), CudaError> {
        let layout = operation.apply(&self.layout)?;
        // Allocation-width checks belong to the consumer. For example a
        // broadcast u32 mask can traverse a larger low-precision result while
        // still using its original scalar allocation.
        if matches!(self.buffer, BufferRef::Input(_)) {
            self.input_views.push(operation);
        }
        self.layout = layout;
        Ok(())
    }
    pub(super) fn permute(&mut self, axes: &[usize]) -> Result<(), CudaError> {
        self.view(InputView::Permute(axes.to_vec()))
    }
    pub(super) fn broadcast_to(&mut self, shape: Shape) -> Result<(), CudaError> {
        self.view(InputView::Broadcast(shape))
    }
    pub(super) fn reshape(&mut self, shape: Shape) -> Result<(), CudaError> {
        self.view(InputView::Reshape(shape))
    }
    pub(super) fn promote(&mut self, shape: Shape, left: bool) -> Result<(), CudaError> {
        if self.layout.shape().rank() == 1 {
            self.view(InputView::Promote { shape, left })?;
        }
        Ok(())
    }
}

/// Pure recording. Device-specific preparation expands reduction levels and
/// GEMM calls, then checks the complete resource budget before allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StatisticsKind {
    Softmax,
    LogSoftmax,
    Logsumexp,
    Moments,
    LayerNorm { epsilon: f32 },
}
#[derive(Clone, Debug)]
pub(crate) enum Step {
    Statistics {
        source: PlannedValue,
        output: usize,
        variance: Option<usize>,
        axes: Vec<usize>,
        kind: StatisticsKind,
    },
    Attention {
        query: PlannedValue,
        key: PlannedValue,
        value: PlannedValue,
        mask: Option<(PlannedValue, bool)>,
        output: usize,
        plan: Box<AttentionPlan>,
    },
    Unary {
        source: PlannedValue,
        output: usize,
        op: u32,
        scale: f32,
        bias: f32,
    },
    Binary {
        left: PlannedValue,
        right: PlannedValue,
        output: usize,
        op: BinaryOp,
    },
    Compare {
        left: PlannedValue,
        right: PlannedValue,
        output: usize,
        op: CompareOp,
    },
    Select {
        mask: PlannedValue,
        yes: PlannedValue,
        no: PlannedValue,
        output: usize,
    },
    Cast {
        source: PlannedValue,
        output: usize,
        to: CudaDtype,
    },
    Reduce {
        source: PlannedValue,
        output: usize,
        op: ReduceOp,
        axes: Vec<usize>,
        keep_dims: bool,
        mean: bool,
    },
    Matmul {
        left: PlannedValue,
        right: PlannedValue,
        output: usize,
        precision: MatmulPrecision,
    },
    Scan {
        source: PlannedValue,
        output: usize,
        axis: usize,
        options: ScanOptions,
    },
    Gather {
        source: PlannedValue,
        indices: PlannedValue,
        output: usize,
        invalid_count: usize,
        axis: usize,
    },
    Compact {
        source: PlannedValue,
        mask: PlannedValue,
        output: usize,
        count: usize,
    },
    Scatter {
        source: PlannedValue,
        indices: PlannedValue,
        updates: PlannedValue,
        output: usize,
        invalid_count: usize,
        op: ScatterOp,
        axis: usize,
    },
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CudaProgramPlan {
    pub inputs: Vec<TensorSpec>,
    pub scratch: Vec<TensorSpec>,
    pub steps: Vec<Step>,
    /// Every entry is copied to a separate caller-owned output, including
    /// repeated entries and direct input/view outputs.
    pub outputs: Vec<PlannedValue>,
}

/// CUDA allocates one sentinel element for an empty logical tensor.
pub(crate) fn allocation_bytes(shape: &Shape, dtype: CudaDtype) -> Result<usize, CudaError> {
    shape
        .numel()
        .max(1)
        .checked_mul(dtype.byte_width())
        .ok_or(CudaError::InvalidInput("CUDA tensor byte count overflows"))
}

pub(crate) fn validate_layout(layout: &Layout, dtype: CudaDtype) -> Result<(), CudaError> {
    allocation_bytes(layout.shape(), dtype)?;
    crate::runtime::rank(layout.shape())?;
    layout
        .required_storage_len()?
        .max(1)
        .checked_mul(dtype.byte_width())
        .ok_or(CudaError::InvalidInput("CUDA layout byte span overflows"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_byte_limits_apply_to_views_and_empty_sentinels() {
        let empty = Shape::new(vec![0, usize::MAX]).unwrap();
        assert_eq!(allocation_bytes(&empty, CudaDtype::F32).unwrap(), 4);
        let empty_offset = Layout::new(empty, vec![0, 0], usize::MAX).unwrap();
        assert!(validate_layout(&empty_offset, CudaDtype::F32).is_err());

        let huge = Shape::new(vec![usize::MAX / 4 + 1]).unwrap();
        // A tiny zero-stride backing allocation does not waive logical limits.
        assert!(validate_layout(&Layout::new(huge, vec![0], 0).unwrap(), CudaDtype::F32).is_err());
        let sparse = Layout::new(Shape::new(vec![2]).unwrap(), vec![usize::MAX / 4], 0).unwrap();
        assert!(validate_layout(&sparse, CudaDtype::F32).is_err());
    }

    #[test]
    fn two_byte_storage_limits_and_empty_sentinels_are_type_specific() {
        let empty = Shape::new(vec![0]).unwrap();
        for dtype in [
            CudaDtype::F32,
            CudaDtype::U32,
            CudaDtype::F16,
            CudaDtype::Bf16,
        ] {
            assert_eq!(allocation_bytes(&empty, dtype).unwrap(), dtype.byte_width());
            let elements = usize::MAX / dtype.byte_width();
            assert_eq!(
                allocation_bytes(&Shape::new(vec![elements]).unwrap(), dtype).unwrap(),
                elements * dtype.byte_width()
            );
            assert!(allocation_bytes(&Shape::new(vec![elements + 1]).unwrap(), dtype).is_err());
        }
        let wide = Shape::new(vec![usize::MAX / 4 + 1]).unwrap();
        let layout = Layout::new(wide, vec![0], 0).unwrap();
        assert!(validate_layout(&layout, CudaDtype::Bf16).is_ok());
        assert!(validate_layout(&layout, CudaDtype::F32).is_err());
    }
}
