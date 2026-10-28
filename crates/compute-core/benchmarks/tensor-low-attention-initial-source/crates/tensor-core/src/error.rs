#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TensorError {
    ShapeOverflow,
    LayoutOverflow,
    IndexCountOverflow {
        count: usize,
    },
    RankMismatch {
        expected: usize,
        actual: usize,
    },
    AxisOutOfBounds {
        axis: usize,
        rank: usize,
    },
    DuplicateAxis {
        axis: usize,
    },
    InvalidPermutation,
    IncompatibleBroadcast {
        left: Vec<usize>,
        right: Vec<usize>,
    },
    ElementCountMismatch {
        expected: usize,
        actual: usize,
    },
    StorageOutOfBounds {
        required: usize,
        actual: usize,
    },
    NonContiguousReshape,
    EmptyReduction,
    InvalidEpsilon,
    InvalidAttention(&'static str),
    InvalidAttentionScale,
    LowDtypeMismatch {
        left: crate::LowDtype,
        right: crate::LowDtype,
    },
    MatmulRank {
        left: usize,
        right: usize,
    },
    MatmulInnerDimension {
        left: usize,
        right: usize,
    },
    InvalidSlice {
        axis: usize,
        start: usize,
        len: usize,
        dimension: usize,
    },
}
impl std::fmt::Display for TensorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShapeOverflow => f.write_str("tensor element count overflows usize"),
            Self::LayoutOverflow => f.write_str("tensor storage address overflows usize"),
            Self::IndexCountOverflow { count } => {
                write!(f, "index operation count {count} exceeds u32::MAX")
            }
            Self::RankMismatch { expected, actual } => {
                write!(f, "tensor rank {actual}; expected {expected}")
            }
            Self::AxisOutOfBounds { axis, rank } => {
                write!(f, "axis {axis} is outside tensor rank {rank}")
            }
            Self::DuplicateAxis { axis } => write!(f, "axis {axis} occurs more than once"),
            Self::InvalidPermutation => {
                f.write_str("permutation must contain every axis exactly once")
            }
            Self::IncompatibleBroadcast { left, right } => {
                write!(f, "cannot broadcast shapes {left:?} and {right:?}")
            }
            Self::ElementCountMismatch { expected, actual } => {
                write!(f, "tensor contains {actual} elements; expected {expected}")
            }
            Self::StorageOutOfBounds { required, actual } => write!(
                f,
                "tensor layout needs {required} storage elements; available {actual}"
            ),
            Self::NonContiguousReshape => {
                f.write_str("materialize a noncontiguous tensor before reshaping its view")
            }
            Self::EmptyReduction => {
                f.write_str("min, max and mean require a nonempty contraction for each output")
            }
            Self::InvalidEpsilon => {
                f.write_str("normalization epsilon must be finite and strictly positive")
            }
            Self::InvalidAttention(reason) => write!(f, "invalid attention: {reason}"),
            Self::InvalidAttentionScale => f.write_str("attention scale must be finite"),
            Self::LowDtypeMismatch { left, right } => {
                write!(f, "low-precision dtypes must match: {left:?} and {right:?}")
            }
            Self::MatmulRank { left, right } => write!(
                f,
                "matmul operands need rank at least one; received {left} and {right}"
            ),
            Self::MatmulInnerDimension { left, right } => write!(
                f,
                "matrix contraction dimensions differ: {left} and {right}"
            ),
            Self::InvalidSlice {
                axis,
                start,
                len,
                dimension,
            } => write!(
                f,
                "slice {start}..+{len} exceeds dimension {dimension} on axis {axis}"
            ),
        }
    }
}
impl std::error::Error for TensorError {}
