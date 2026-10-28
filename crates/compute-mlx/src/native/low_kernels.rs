//! Shared launch, layout and source helpers for direct low-storage kernels.
use super::{custom_metal::KernelKey, *};
use tensor_core::ReduceOp;
const HEADER: &str = include_str!("../metal/low_common.h");
const REDUCE: &str = include_str!("../metal/low_reduce.metal");
pub(super) const WIDTH: usize = 256;

pub(super) fn element_launch(count: usize) -> (usize, usize) {
    let grid = count.min(65536);
    (grid, grid.min(WIDTH))
}
pub(super) fn reduction_launch(rows: usize, parts: usize) -> (usize, usize) {
    (rows.saturating_mul(parts).min(65535) * WIDTH, WIDTH)
}
pub(super) fn key(source: String, inputs: &'static [&'static str]) -> KernelKey {
    KernelKey {
        source,
        header: HEADER,
        inputs,
        zeroed_atomic_u32: false,
    }
}
pub(super) fn reduction_source(op: ReduceOp, low: bool, mean: bool, reverse: bool) -> String {
    transformed_reduction_source(op, low, mean, reverse, "", "")
}
pub(super) fn transformed_reduction_source(
    op: ReduceOp,
    low: bool,
    mean: bool,
    reverse: bool,
    prepare: &str,
    transform: &str,
) -> String {
    REDUCE
        .replace("OP", &(op as u32).to_string())
        .replace(
            "LOAD",
            if low {
                "low_decode(as_type<ushort>(loaded), BF)"
            } else {
                "loaded"
            },
        )
        .replace("MEAN", if mean { "true" } else { "false" })
        .replace("REVERSE", if reverse { "true" } else { "false" })
        .replace("PREPARE", prepare)
        .replace("TRANSFORM", transform)
}

pub(super) struct LowReductionGeometry {
    pub row_shape: Shape,
    pub order: Vec<usize>,
    pub inverse: Vec<usize>,
    pub rows: usize,
    pub count: usize,
    pub parts: usize,
}
impl LowReductionGeometry {
    pub fn new(shape: &Shape, axes: &[usize]) -> Result<Self, MlxError> {
        let row_shape = shape.reduce(axes, false)?;
        let rows = row_shape.numel();
        if shape.is_empty() || rows == 0 {
            return Err(MlxError::Contract(TensorError::EmptyReduction));
        }
        let count = shape.numel() / rows;
        let order: Vec<_> = (0..shape.rank())
            .filter(|a| !axes.contains(a))
            .chain(axes.iter().copied())
            .collect();
        let mut inverse = vec![0; order.len()];
        for (position, &axis) in order.iter().enumerate() {
            inverse[axis] = position;
        }
        Ok(Self {
            row_shape,
            order,
            inverse,
            rows,
            count,
            parts: count.div_ceil(4096).min(1024),
        })
    }
    pub fn partial_shape(&self, result: &Shape) -> Result<Shape, MlxError> {
        Ok(if self.parts == 1 {
            result.clone()
        } else {
            Shape::new(
                self.row_shape
                    .dims()
                    .iter()
                    .copied()
                    .chain([self.parts])
                    .collect::<Vec<_>>(),
            )?
        })
    }
}
