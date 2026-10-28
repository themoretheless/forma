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

pub(super) struct LowReductionPlan {
    pub input: MlxTensor,
    geometry: LowReductionGeometry,
}
impl std::ops::Deref for LowReductionPlan {
    type Target = LowReductionGeometry;
    fn deref(&self) -> &Self::Target {
        &self.geometry
    }
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

impl MlxBackend {
    /// Call after validating axes and handling empty inputs. The permutation
    /// only changes metadata; rows preserve the logical kept-axis order.
    pub(super) fn low_reduction_plan(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
    ) -> Result<LowReductionPlan, MlxError> {
        let geometry = LowReductionGeometry::new(input.shape(), axes)?;
        Ok(LowReductionPlan {
            input: self.permute(&input.tensor, &geometry.order)?,
            geometry,
        })
    }

    pub(super) fn low_fold_partials(
        &self,
        partial: MlxTensor,
        plan: &LowReductionPlan,
        shape: Shape,
        op: ReduceOp,
        mean: bool,
    ) -> Result<MlxTensor, MlxError> {
        if plan.parts == 1 {
            return Ok(partial);
        }
        let params = self.low_parameters(plan.count, plan.row_shape.rank(), 1)?;
        self.custom_metal(
            key(reduction_source(op, false, mean, false), &["inp", "params"]),
            &[&partial, &params],
            shape,
            MlxDtype::F32,
            false,
            reduction_launch(plan.rows, 1),
        )
    }

    pub(super) fn low_parameters(
        &self,
        count: usize,
        rank: usize,
        parts: usize,
    ) -> Result<MlxTensor, MlxError> {
        self.upload_u32(
            Shape::new(vec![4])?,
            &[
                u32::try_from(rank).map_err(|_| MlxError::TooLarge)?,
                count as u32,
                (count as u64 >> 32) as u32,
                u32::try_from(parts).map_err(|_| MlxError::TooLarge)?,
            ],
        )
    }
    /// MLX omits custom shape/stride arguments for scalar arrays. Promote only
    /// their metadata to [1]; kernels still emit the requested scalar output.
    pub(super) fn low_elementwise_view(
        &self,
        input: &MlxTensor,
        shape: &Shape,
    ) -> Result<MlxTensor, MlxError> {
        let shape = if shape.rank() == 0 {
            Shape::new(vec![1])?
        } else {
            shape.clone()
        };
        self.broadcast_to(input, shape)
    }
}
