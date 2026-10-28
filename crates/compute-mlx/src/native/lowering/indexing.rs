//! Shared typed indexing recipes. Low movement keeps the original u16 payloads.
use super::*;
use crate::native::low_kernels::{element_launch, key};
use tensor_core::{Gathered, LowDtype, gather_shape, select_shape, validate_index_count};

pub(in crate::native) fn require<L: Lowering>(
    graph: &L,
    value: &L::Value,
    dtype: MlxDtype,
) -> Result<TensorSpec, MlxError> {
    let spec = graph.spec(value)?;
    if spec.dtype != dtype {
        return Err(MlxError::Dtype);
    }
    Ok(spec)
}

pub(in crate::native) fn matching<L: Lowering>(
    graph: &L,
    left: &L::Value,
    right: &L::Value,
) -> Result<(TensorSpec, TensorSpec), MlxError> {
    let a = graph.spec(left)?;
    let b = graph.spec(right)?;
    if a.dtype != b.dtype {
        let low = |dtype| match dtype {
            MlxDtype::F16 => Some(LowDtype::F16),
            MlxDtype::Bf16 => Some(LowDtype::Bf16),
            _ => None,
        };
        if let (Some(left), Some(right)) = (low(a.dtype), low(b.dtype)) {
            return Err(TensorError::LowDtypeMismatch { left, right }.into());
        }
        return Err(MlxError::Dtype);
    }
    Ok((a, b))
}

pub(in crate::native) fn scalar_u32<L: Lowering>(
    graph: &mut L,
    value: u32,
) -> Result<L::Value, MlxError> {
    graph.constant_u32(Shape::new(vec![])?, &[value])
}

pub(in crate::native) fn zeros<L: Lowering>(
    graph: &mut L,
    shape: Shape,
    dtype: MlxDtype,
) -> Result<L::Value, MlxError> {
    MlxBackend::dimensions(&shape)?;
    graph.native(NativeOp::Zeros, &[], TensorSpec { shape, dtype })
}

pub(in crate::native) fn reshape<L: Lowering>(
    graph: &mut L,
    value: L::Value,
    shape: Shape,
) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&value)?;
    if spec.shape.numel() != shape.numel() {
        return Err(TensorError::ElementCountMismatch {
            expected: spec.shape.numel(),
            actual: shape.numel(),
        }
        .into());
    }
    MlxBackend::dimensions(&shape)?;
    graph.native(NativeOp::Reshape, &[value], TensorSpec { shape, ..spec })
}

pub(in crate::native) fn broadcast<L: Lowering>(
    graph: &mut L,
    value: L::Value,
    shape: Shape,
) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&value)?;
    MlxBackend::dimensions(&shape)?;
    if spec.shape == shape {
        return Ok(value);
    }
    if spec.shape.broadcast(&shape)? != shape {
        return Err(TensorError::IncompatibleBroadcast {
            left: spec.shape.dims().to_vec(),
            right: shape.dims().to_vec(),
        }
        .into());
    }
    graph.native(NativeOp::Broadcast, &[value], TensorSpec { shape, ..spec })
}

pub(in crate::native) fn permute<L: Lowering>(
    graph: &mut L,
    value: L::Value,
    axes: &[usize],
) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&value)?;
    let shape = spec.shape.permute(axes)?;
    let axes = statistics::axes_i32(axes)?;
    graph.native(
        NativeOp::Permute(axes),
        &[value],
        TensorSpec { shape, ..spec },
    )
}

/// MLX custom kernels treat scalars as values and omit their metadata. A [1]
/// broadcast view retains storage while giving generated code a pointer input.
pub(in crate::native) fn element_view<L: Lowering>(
    graph: &mut L,
    value: L::Value,
    shape: &Shape,
) -> Result<L::Value, MlxError> {
    broadcast(
        graph,
        value,
        if shape.rank() == 0 {
            Shape::new(vec![1])?
        } else {
            shape.clone()
        },
    )
}

pub(in crate::native) fn binary_u32<L: Lowering>(
    graph: &mut L,
    left: L::Value,
    right: L::Value,
    op: BinaryOp,
) -> Result<L::Value, MlxError> {
    let a = require(graph, &left, MlxDtype::U32)?;
    let b = require(graph, &right, MlxDtype::U32)?;
    if op == BinaryOp::Divide {
        return Err(MlxError::Dtype);
    }
    let shape = a.shape.broadcast(&b.shape)?;
    MlxBackend::dimensions(&shape)?;
    graph.native(
        NativeOp::Binary(op),
        &[left, right],
        TensorSpec {
            shape,
            dtype: MlxDtype::U32,
        },
    )
}

pub(in crate::native) fn sum_u32<L: Lowering>(
    graph: &mut L,
    value: L::Value,
    axes: &[usize],
    keep: bool,
) -> Result<L::Value, MlxError> {
    let spec = require(graph, &value, MlxDtype::U32)?;
    let shape = spec.shape.reduce(axes, keep)?;
    MlxBackend::dimensions(&shape)?;
    if axes.is_empty() {
        return Ok(value);
    }
    if spec.shape.is_empty() {
        return zeros(graph, shape, MlxDtype::U32);
    }
    let axes = statistics::axes_i32(axes)?;
    graph.native(
        NativeOp::Reduce(ReduceOp::Sum, axes, keep),
        &[value],
        TensorSpec {
            shape,
            dtype: MlxDtype::U32,
        },
    )
}

pub(in crate::native) fn compare<L: Lowering>(
    graph: &mut L,
    left: L::Value,
    right: L::Value,
    op: CompareOp,
) -> Result<L::Value, MlxError> {
    let (a, b) = matching(graph, &left, &right)?;
    let shape = a.shape.broadcast(&b.shape)?;
    MlxBackend::dimensions(&shape)?;
    if shape.is_empty() {
        return zeros(graph, shape, MlxDtype::U32);
    }
    if matches!(a.dtype, MlxDtype::F16 | MlxDtype::Bf16) {
        let left = element_view(graph, left, &shape)?;
        let right = element_view(graph, right, &shape)?;
        let launch = element_launch(shape.numel());
        return graph.metal(
            key(
                include_str!("../../metal/low_compare.metal")
                    .replace("OP", &(op as u32).to_string()),
                &["lhs", "rhs"],
            ),
            &[left, right],
            TensorSpec {
                shape,
                dtype: MlxDtype::U32,
            },
            a.dtype == MlxDtype::Bf16,
            launch,
        );
    }
    graph.native(
        NativeOp::Compare(op),
        &[left, right],
        TensorSpec {
            shape,
            dtype: MlxDtype::U32,
        },
    )
}

pub(in crate::native) fn select<L: Lowering>(
    graph: &mut L,
    mask: L::Value,
    yes: L::Value,
    no: L::Value,
) -> Result<L::Value, MlxError> {
    let mask_spec = require(graph, &mask, MlxDtype::U32)?;
    let (a, b) = matching(graph, &yes, &no)?;
    let shape = select_shape(&mask_spec.shape, &a.shape, &b.shape)?;
    MlxBackend::dimensions(&shape)?;
    if shape.is_empty() {
        return zeros(graph, shape, a.dtype);
    }
    if matches!(a.dtype, MlxDtype::F16 | MlxDtype::Bf16) {
        let yes = element_view(graph, yes, &shape)?;
        let no = element_view(graph, no, &shape)?;
        let mask = element_view(graph, mask, &shape)?;
        let launch = element_launch(shape.numel());
        return graph.metal(
            key(
                include_str!("../../metal/low_select.metal").into(),
                &["lhs", "rhs", "mask"],
            ),
            &[yes, no, mask],
            TensorSpec {
                shape,
                dtype: a.dtype,
            },
            a.dtype == MlxDtype::Bf16,
            launch,
        );
    }
    graph.native(
        NativeOp::Select,
        &[mask, yes, no],
        TensorSpec {
            shape,
            dtype: a.dtype,
        },
    )
}

pub(in crate::native) struct CheckedIndices<V> {
    pub valid: V,
    pub safe: V,
    pub invalid_count: V,
}

pub(in crate::native) fn checked_indices<L: Lowering>(
    graph: &mut L,
    indices: L::Value,
    extent: usize,
) -> Result<CheckedIndices<L::Value>, MlxError> {
    let spec = require(graph, &indices, MlxDtype::U32)?;
    validate_index_count(&spec.shape)?;
    let extent = u32::try_from(extent).map_err(|_| MlxError::TooLarge)?;
    let extent = scalar_u32(graph, extent)?;
    let zero = scalar_u32(graph, 0)?;
    let valid = compare(graph, indices.clone(), extent, CompareOp::Less)?;
    let invalid = compare(graph, valid.clone(), zero.clone(), CompareOp::Equal)?;
    let invalid_count = sum_u32(
        graph,
        invalid,
        &(0..spec.shape.rank()).collect::<Vec<_>>(),
        false,
    )?;
    let safe = select(graph, valid.clone(), indices, zero)?;
    Ok(CheckedIndices {
        valid,
        safe,
        invalid_count,
    })
}

pub(in crate::native) fn gather<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    indices: L::Value,
    axis: usize,
) -> Result<Gathered<L::Value, L::Value>, MlxError> {
    let spec = graph.spec(&input)?;
    let index_spec = require(graph, &indices, MlxDtype::U32)?;
    let shape = gather_shape(&spec.shape, &index_spec.shape, axis)?;
    MlxBackend::dimensions(&shape)?;
    let native_axis = i32::try_from(axis).map_err(|_| MlxError::TooLarge)?;
    let checked = checked_indices(graph, indices, spec.shape.dims()[axis])?;
    let values = if spec.shape.dims()[axis] == 0 || shape.is_empty() {
        zeros(graph, shape, spec.dtype)?
    } else {
        let gathered = graph.native(
            NativeOp::TakeAxis(native_axis),
            &[input, checked.safe],
            TensorSpec {
                shape: shape.clone(),
                dtype: spec.dtype,
            },
        )?;
        let mut mask_dims = vec![1; axis];
        mask_dims.extend_from_slice(index_spec.shape.dims());
        mask_dims.resize(shape.rank(), 1);
        let valid = reshape(graph, checked.valid, Shape::new(mask_dims)?)?;
        let zero = zeros(graph, Shape::new(vec![])?, spec.dtype)?;
        select(graph, valid, gathered, zero)?
    };
    Ok(Gathered {
        values,
        invalid_count: checked.invalid_count,
    })
}
