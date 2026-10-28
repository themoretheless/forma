//! Backend-independent tensor shapes, layouts, operations and native execution
//! contracts. This crate has no GPU, CUDA, MLX or foreign-library dependencies.
mod attention;
mod backend;
mod error;
mod indexing;
mod layout;
mod low_attention;
mod low_index;
mod low_ops;
mod low_precision;
mod low_scatter;
mod low_statistics;
mod ops;
mod reduction;
mod scatter;
mod shape;
mod statistics;

/// Shared backend tests, opt-in for integration-test and qualification binaries.
#[cfg(feature = "conformance")]
pub mod conformance;

pub use attention::{AttentionMask, AttentionOptions, AttentionPlan, TensorAttentionBackend};
pub use backend::{BackendKind, HasShape, MatmulPrecision, TensorBackend};
pub use error::TensorError;
pub use indexing::{
    Compacted, Gathered, ScanOptions, TensorIndexBackend, compact_shape, gather_shape,
    select_shape, validate_index_count,
};
pub use layout::Layout;
pub use low_attention::{TensorLowAttentionBackend, low_attention_plan};
pub use low_index::{TensorLowIndexBackend, low_select_shape};
pub use low_ops::{TensorLowOpsBackend, low_binary_shape};
pub use low_precision::{HasLowDtype, LowDtype, LowPrecisionSupport, LowStorage, TensorLowBackend};
pub use low_scatter::{TensorLowScatterBackend, low_scatter_updates_shape};
pub use low_statistics::TensorLowStatsBackend;
pub use ops::{BinaryOp, CompareOp, UnaryOp};
pub use reduction::{ReduceOp, TensorReduceBackend, mean_shape, reduction_shape};
pub use scatter::{ScatterOp, Scattered, TensorScatterBackend, scatter_updates_shape};
pub use shape::{MatmulPlan, Shape, matmul_shape};
pub use statistics::{
    Moments, TensorStatsBackend, normalization_shape, statistics_shape, validate_epsilon,
};
