//! One cast recipe for eager arrays and compiled values. BF16 encoding uses
//! integer rounding so native float-to-bfloat conversion cannot flush tiny data.
use super::{Lowering, NativeOp, TensorSpec};
use crate::native::{MlxDtype, MlxError};
use tensor_core::{BinaryOp, CompareOp, LowDtype, Shape};

pub(in crate::native) fn cast_to_low<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    dtype: LowDtype,
) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&input)?;
    if spec.dtype != MlxDtype::F32 {
        return Err(MlxError::Dtype);
    }
    let output = TensorSpec {
        shape: spec.shape.clone(),
        dtype: dtype.into(),
    };
    if dtype == LowDtype::F16 {
        return graph.native(NativeOp::Cast(MlxDtype::F16), &[input], output);
    }

    let words = TensorSpec {
        shape: spec.shape,
        dtype: MlxDtype::U32,
    };
    let bits = graph.native(NativeOp::ViewU32, &[input], words.clone())?;
    let scalar = Shape::new(vec![])?;
    let shift = graph.constant_u32(scalar.clone(), &[16])?;
    let one = graph.constant_u32(scalar.clone(), &[1])?;
    let half_minus_one = graph.constant_u32(scalar.clone(), &[0x7fff])?;
    let magnitude_mask = graph.constant_u32(scalar.clone(), &[0x7fff_ffff])?;
    let infinity = graph.constant_u32(scalar.clone(), &[0x7f80_0000])?;
    let canonical_nan = graph.constant_u32(scalar, &[0x7fc0])?;

    let upper = graph.native(
        NativeOp::RightShift,
        &[bits.clone(), shift.clone()],
        words.clone(),
    )?;
    let lsb = graph.native(NativeOp::BitwiseAnd, &[upper, one], words.clone())?;
    let rounded = graph.native(
        NativeOp::Binary(BinaryOp::Add),
        &[bits.clone(), half_minus_one],
        words.clone(),
    )?;
    let rounded = graph.native(
        NativeOp::Binary(BinaryOp::Add),
        &[rounded, lsb],
        words.clone(),
    )?;
    let rounded = graph.native(NativeOp::RightShift, &[rounded, shift], words.clone())?;
    let magnitude = graph.native(NativeOp::BitwiseAnd, &[bits, magnitude_mask], words.clone())?;
    let is_nan = graph.native(
        NativeOp::Compare(CompareOp::Greater),
        &[magnitude, infinity],
        words.clone(),
    )?;
    let encoded = graph.native(NativeOp::Select, &[is_nan, canonical_nan, rounded], words)?;
    graph.native(NativeOp::PackBf16, &[encoded], output)
}

pub(in crate::native) fn cast_to_f32<L: Lowering>(
    graph: &mut L,
    input: L::Value,
) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&input)?;
    if !matches!(spec.dtype, MlxDtype::F16 | MlxDtype::Bf16) {
        return Err(MlxError::Dtype);
    }
    graph.native(
        NativeOp::Cast(MlxDtype::F32),
        &[input],
        TensorSpec {
            shape: spec.shape,
            dtype: MlxDtype::F32,
        },
    )
}
