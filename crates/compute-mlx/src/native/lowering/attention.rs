//! Eager and compiled attention share geometry, guards, and native/custom nodes.
use super::*;
use tensor_core::{AttentionMask, AttentionOptions, AttentionPlan, HasLowDtype, LowDtype};
mod float;
mod low;
pub(in crate::native) use float::attention;
pub(in crate::native) use low::attention_low_f32;

struct LowSpec<'a>(&'a TensorSpec, LowDtype);
impl HasShape for LowSpec<'_> {
    fn shape(&self) -> &Shape {
        &self.0.shape
    }
}
impl HasLowDtype for LowSpec<'_> {
    fn low_dtype(&self) -> LowDtype {
        self.1
    }
}
fn low_dtype(spec: &TensorSpec) -> Result<LowDtype, MlxError> {
    match spec.dtype {
        MlxDtype::F16 => Ok(LowDtype::F16),
        MlxDtype::Bf16 => Ok(LowDtype::Bf16),
        _ => Err(MlxError::Dtype),
    }
}
fn plan<L: Lowering>(
    g: &L,
    inputs: [&L::Value; 3],
    mask: &AttentionMask<'_, L::Value, L::Value>,
    options: AttentionOptions,
    low: bool,
) -> Result<AttentionPlan, MlxError> {
    let q = g.spec(inputs[0])?;
    let k = g.spec(inputs[1])?;
    let v = g.spec(inputs[2])?;
    let mask = match mask {
        AttentionMask::None => None,
        AttentionMask::Keep(m) => Some(require(g, m, MlxDtype::U32)?),
        AttentionMask::Additive(m) => Some(require(g, m, MlxDtype::F32)?),
    };
    let shape = mask.as_ref().map(|m| &m.shape);
    let plan = if low {
        tensor_core::low_attention_plan(
            &LowSpec(&q, low_dtype(&q)?),
            &LowSpec(&k, low_dtype(&k)?),
            &LowSpec(&v, low_dtype(&v)?),
            shape,
            options,
        )?
    } else {
        if [q.dtype, k.dtype, v.dtype]
            .iter()
            .any(|&d| d != MlxDtype::F32)
        {
            return Err(MlxError::Dtype);
        }
        AttentionPlan::new(&q.shape, &k.shape, &v.shape, shape, options)?
    };
    MlxBackend::dimensions(&plan.output)?;
    Ok(plan)
}
fn require<L: Lowering>(g: &L, value: &L::Value, dtype: MlxDtype) -> Result<TensorSpec, MlxError> {
    let spec = g.spec(value)?;
    if spec.dtype != dtype {
        return Err(MlxError::Dtype);
    }
    Ok(spec)
}
fn zeros<L: Lowering>(g: &mut L, shape: Shape) -> Result<L::Value, MlxError> {
    g.native(
        NativeOp::Zeros,
        &[],
        TensorSpec {
            shape,
            dtype: MlxDtype::F32,
        },
    )
}
fn scalar<L: Lowering>(g: &mut L, value: f32) -> Result<L::Value, MlxError> {
    let shape = Shape::new(vec![])?;
    let words = g.constant_u32(shape.clone(), &[value.to_bits()])?;
    g.native(
        NativeOp::ViewF32,
        &[words],
        TensorSpec {
            shape,
            dtype: MlxDtype::F32,
        },
    )
}
fn shape_op<L: Lowering>(
    g: &mut L,
    input: L::Value,
    op: NativeOp,
    shape: Shape,
) -> Result<L::Value, MlxError> {
    let dtype = g.spec(&input)?.dtype;
    g.native(op, &[input], TensorSpec { shape, dtype })
}
fn reshape<L: Lowering>(g: &mut L, input: L::Value, shape: Shape) -> Result<L::Value, MlxError> {
    let before = g.spec(&input)?.shape;
    if before.numel() != shape.numel() {
        return Err(TensorError::ElementCountMismatch {
            expected: before.numel(),
            actual: shape.numel(),
        }
        .into());
    }
    shape_op(g, input, NativeOp::Reshape, shape)
}
fn broadcast<L: Lowering>(g: &mut L, input: L::Value, shape: Shape) -> Result<L::Value, MlxError> {
    let before = g.spec(&input)?.shape;
    if before.broadcast(&shape)? != shape {
        return Err(TensorError::IncompatibleBroadcast {
            left: before.dims().to_vec(),
            right: shape.dims().to_vec(),
        }
        .into());
    }
    shape_op(g, input, NativeOp::Broadcast, shape)
}
fn binary<L: Lowering>(
    g: &mut L,
    left: L::Value,
    right: L::Value,
    op: BinaryOp,
) -> Result<L::Value, MlxError> {
    let a = g.spec(&left)?;
    let b = require(g, &right, a.dtype)?;
    let shape = a.shape.broadcast(&b.shape)?;
    g.native(
        NativeOp::Binary(op),
        &[left, right],
        TensorSpec {
            shape,
            dtype: a.dtype,
        },
    )
}
fn compare<L: Lowering>(
    g: &mut L,
    left: L::Value,
    right: L::Value,
    op: CompareOp,
) -> Result<L::Value, MlxError> {
    let a = g.spec(&left)?;
    let b = require(g, &right, a.dtype)?;
    let shape = a.shape.broadcast(&b.shape)?;
    g.native(
        NativeOp::Compare(op),
        &[left, right],
        TensorSpec {
            shape,
            dtype: MlxDtype::U32,
        },
    )
}
fn select<L: Lowering>(
    g: &mut L,
    mask: L::Value,
    yes: L::Value,
    no: L::Value,
) -> Result<L::Value, MlxError> {
    let a = require(g, &yes, MlxDtype::F32)?;
    let b = require(g, &no, MlxDtype::F32)?;
    let m = require(g, &mask, MlxDtype::U32)?;
    let shape = tensor_core::select_shape(&m.shape, &a.shape, &b.shape)?;
    g.native(
        NativeOp::Select,
        &[mask, yes, no],
        TensorSpec {
            shape,
            dtype: MlxDtype::F32,
        },
    )
}
fn indices<L: Lowering>(g: &mut L, count: usize) -> Result<L::Value, MlxError> {
    g.native(
        NativeOp::ArangeU32(count),
        &[],
        TensorSpec {
            shape: Shape::new(vec![count])?,
            dtype: MlxDtype::U32,
        },
    )
}
fn causal_mask<L: Lowering>(
    g: &mut L,
    p: &AttentionPlan,
    offset: i32,
) -> Result<L::Value, MlxError> {
    let q = indices(g, p.queries)?;
    let k = indices(g, p.keys)?;
    let q = reshape(g, q, Shape::new(vec![p.queries, 1])?)?;
    let k = reshape(g, k, Shape::new(vec![1, p.keys])?)?;
    let magnitude = g.constant_u32(Shape::new(vec![])?, &[offset.unsigned_abs()])?;
    // Each dimension fits i32; unsigned magnitude+position fits u32 even MIN.
    if offset >= 0 {
        let end = binary(g, q, magnitude, BinaryOp::Add)?;
        compare(g, k, end, CompareOp::LessEqual)
    } else {
        let begin = binary(g, k, magnitude, BinaryOp::Add)?;
        compare(g, begin, q, CompareOp::LessEqual)
    }
}
fn operand<L: Lowering>(
    g: &mut L,
    input: L::Value,
    promoted: &Shape,
    batch: &Shape,
    flatten: bool,
) -> Result<L::Value, MlxError> {
    let input = if flatten {
        reshape(g, input, promoted.clone())?
    } else {
        input
    };
    let tail = &promoted.dims()[promoted.rank() - 3..];
    let mut expanded = batch.dims().to_vec();
    expanded.extend_from_slice(tail);
    let input = broadcast(g, input, Shape::new(expanded)?)?;
    if !flatten {
        return Ok(input);
    }
    let mut flat = vec![batch.numel()];
    flat.extend_from_slice(tail);
    reshape(g, input, Shape::new(flat)?)
}
