use crate::{
    CudaError,
    policy::{GemmDimensions, row_major_gemm},
};
use std::ops::Range;
use tensor_core::{Layout, MatmulPlan, Shape, TensorError};

/// Checked dense, rank-promoted GEMM geometry. Broadcast offsets are represented
/// by O(rank) layouts, so a large batch never requires one host entry per GEMM.
#[derive(Clone, Debug)]
pub(crate) struct GemmPlan {
    output: Shape,
    work: Option<GemmWork>,
}
#[derive(Clone, Debug)]
struct GemmWork {
    dimensions: GemmDimensions,
    left_batches: Layout,
    right_batches: Layout,
    batches: usize,
    left_matrix_len: usize,
    right_matrix_len: usize,
    output_matrix_len: usize,
    left_required: usize,
    right_required: usize,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct GemmBatch {
    pub left: Range<usize>,
    pub right: Range<usize>,
    pub output: Range<usize>,
}
impl GemmPlan {
    pub(crate) fn new(left: &Layout, right: &Layout) -> Result<Self, CudaError> {
        Self::new_typed::<f32>(left, right)
    }
    pub(crate) fn new_typed<T>(left: &Layout, right: &Layout) -> Result<Self, CudaError> {
        if left.shape().rank() < 2 || right.shape().rank() < 2 {
            return Err(CudaError::InvalidInput(
                "GEMM operands must have promoted matrix ranks",
            ));
        }
        let matrix = MatmulPlan::new(left.shape(), right.shape())?;
        let output = matrix.matrix_output;
        output
            .numel()
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(CudaError::InvalidInput("GEMM output byte count overflows"))?;
        let rank = output.rank();
        let (rows, columns, inner) = (
            output.dims()[rank - 2],
            output.dims()[rank - 1],
            *left.shape().dims().last().unwrap(),
        );
        // These plans read no operands. In particular, K=0 does not need a
        // contiguous source, cuBLAS dimensions, or a cuBLAS handle.
        if output.is_empty() || inner == 0 {
            return Ok(Self { output, work: None });
        }
        if !left.is_contiguous() || !right.is_contiguous() {
            return Err(CudaError::InvalidInput(
                "GEMM operands must be dense matrices",
            ));
        }
        let dimensions = row_major_gemm(rows, inner, columns)?;
        let batches = Shape::new(output.dims()[..rank - 2].to_vec())?;
        let left_batches = batch_layout(left, &batches)?;
        let right_batches = batch_layout(right, &batches)?;
        let matrix_len = |a: usize, b: usize| {
            a.checked_mul(b).ok_or(CudaError::InvalidInput(
                "GEMM matrix element count overflows",
            ))
        };
        let left_matrix_len = matrix_len(rows, inner)?;
        let right_matrix_len = matrix_len(inner, columns)?;
        let output_matrix_len = matrix_len(rows, columns)?;
        let left_required = left.required_storage_len()?;
        let right_required = right.required_storage_len()?;
        // All strides are nonnegative. Proving the maximal matrix end fits
        // validates every batch view before any individual GEMM is enqueued.
        validate_batch_span(&left_batches, left_matrix_len, left_required)?;
        validate_batch_span(&right_batches, right_matrix_len, right_required)?;
        let work = GemmWork {
            dimensions,
            left_batches,
            right_batches,
            batches: batches.numel(),
            left_matrix_len,
            right_matrix_len,
            output_matrix_len,
            left_required,
            right_required,
        };
        Ok(Self {
            output,
            work: Some(work),
        })
    }
    pub(crate) fn output(&self) -> &Shape {
        &self.output
    }
    pub(crate) fn calls(&self) -> usize {
        self.work.as_ref().map_or(0, |work| work.batches)
    }
    /// No GEMM work; a nonempty output must be explicitly zeroed each replay.
    pub(crate) fn is_zero(&self) -> bool {
        self.work.is_none()
    }
    pub(super) fn dimensions(&self) -> Option<GemmDimensions> {
        self.work.as_ref().map(|work| work.dimensions)
    }
    pub(super) fn validate_storage(
        &self,
        left: usize,
        right: usize,
        output: usize,
    ) -> Result<(), CudaError> {
        let required = self
            .work
            .as_ref()
            .map_or([0, 0], |work| [work.left_required, work.right_required]);
        for (required, actual) in required
            .into_iter()
            .zip([left, right])
            .chain(std::iter::once((self.output.numel(), output)))
        {
            if actual < required {
                return Err(TensorError::StorageOutOfBounds { required, actual }.into());
            }
        }
        Ok(())
    }
    pub(super) fn batch(&self, index: usize) -> Result<GemmBatch, CudaError> {
        let work = self
            .work
            .as_ref()
            .ok_or(CudaError::InvalidInput("zero GEMM has no batches"))?;
        let left = work.left_batches.element_offset(index)?;
        let right = work.right_batches.element_offset(index)?;
        // Validated shapes and the maximal spans above prove all three ends
        // fit usize and the full validated storage requirements.
        let output = index * work.output_matrix_len;
        Ok(GemmBatch {
            left: left..left + work.left_matrix_len,
            right: right..right + work.right_matrix_len,
            output: output..output + work.output_matrix_len,
        })
    }
}
fn validate_batch_span(
    layout: &Layout,
    matrix_len: usize,
    available: usize,
) -> Result<(), CudaError> {
    let required = layout
        .required_storage_len()?
        .checked_sub(1)
        .and_then(|last| last.checked_add(matrix_len))
        .ok_or(CudaError::InvalidInput("GEMM batch span overflows"))?;
    if required > available {
        return Err(TensorError::StorageOutOfBounds {
            required,
            actual: available,
        }
        .into());
    }
    Ok(())
}
fn batch_layout(layout: &Layout, shape: &Shape) -> Result<Layout, CudaError> {
    let rank = layout.shape().rank();
    Ok(Layout::new(
        Shape::new(layout.shape().dims()[..rank - 2].to_vec())?,
        layout.strides()[..rank - 2].to_vec(),
        layout.offset(),
    )?
    .broadcast_to(shape.clone())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dense(dims: &[usize]) -> Layout {
        Layout::contiguous(Shape::new(dims.to_vec()).unwrap()).unwrap()
    }
    fn offset(layout: &Layout, offset: usize) -> Layout {
        Layout::new(layout.shape().clone(), layout.strides().to_vec(), offset).unwrap()
    }
    #[test]
    fn broadcast_batches_and_offsets_match_explicit_nested_loops() {
        let left = offset(&dense(&[2, 1, 3, 5]), 7);
        let right = offset(&dense(&[1, 4, 5, 7]), 11);
        let plan = GemmPlan::new(&left, &right).unwrap();
        assert_eq!(plan.output().dims(), [2, 4, 3, 7]);
        assert_eq!(plan.calls(), 8);
        assert_eq!(plan.dimensions().unwrap(), row_major_gemm(3, 5, 7).unwrap());
        for first in 0..2 {
            for second in 0..4 {
                let index = first * 4 + second;
                assert_eq!(
                    plan.batch(index).unwrap(),
                    GemmBatch {
                        left: 7 + first * 15..7 + (first + 1) * 15,
                        right: 11 + second * 35..11 + (second + 1) * 35,
                        output: index * 21..(index + 1) * 21,
                    }
                );
            }
        }
        assert!(plan.batch(8).is_err());
        assert!(plan.validate_storage(37, 151, 168).is_ok());
        for sizes in [(36, 151, 168), (37, 150, 168), (37, 151, 167)] {
            assert!(plan.validate_storage(sizes.0, sizes.1, sizes.2).is_err());
        }
    }
    #[test]
    fn promotion_preserves_vector_results_and_original_nonzero_offsets() {
        for (a, b, expected) in [
            (vec![5], vec![5], vec![]),
            (vec![5], vec![2, 5, 7], vec![2, 7]),
            (vec![2, 3, 5], vec![5], vec![2, 3]),
        ] {
            let original = MatmulPlan::new(
                &Shape::new(a.clone()).unwrap(),
                &Shape::new(b.clone()).unwrap(),
            )
            .unwrap();
            let left = offset(&dense(&a), 3).reshape(original.left).unwrap();
            let right = offset(&dense(&b), 4).reshape(original.right).unwrap();
            let plan = GemmPlan::new(&left, &right).unwrap();
            assert_eq!(original.output.dims(), expected);
            assert_eq!(plan.output().numel(), original.output.numel());
            assert_eq!(plan.batch(0).unwrap().left.start, 3);
            assert_eq!(plan.batch(0).unwrap().right.start, 4);
        }
    }
    #[test]
    fn matrix_repeats_without_allocating_a_descriptor_per_batch() {
        let plan = GemmPlan::new(&dense(&[1, 1]), &dense(&[1_000_000_000, 1, 1])).unwrap();
        assert_eq!(plan.calls(), 1_000_000_000);
        assert_eq!(
            plan.batch(999_999_999).unwrap(),
            GemmBatch {
                left: 0..1,
                right: 999_999_999..1_000_000_000,
                output: 999_999_999..1_000_000_000,
            }
        );
    }
    #[test]
    fn empty_and_zero_k_plans_skip_gemm_but_retain_output_requirements() {
        let empty = GemmPlan::new(&dense(&[0, 3, 5]), &dense(&[1, 5, 7])).unwrap();
        assert!(empty.is_zero());
        assert_eq!(empty.calls(), 0);
        assert!(empty.output().is_empty());
        let left = Layout::new(Shape::new(vec![3, 0]).unwrap(), vec![usize::MAX, 17], 0).unwrap();
        let right = Layout::new(Shape::new(vec![0, 7]).unwrap(), vec![usize::MAX, 23], 0).unwrap();
        let zero = GemmPlan::new(&left, &right).unwrap();
        assert!(zero.is_zero());
        assert_eq!(zero.output().dims(), [3, 7]);
        assert!(zero.validate_storage(0, 0, 21).is_ok());
        assert!(zero.validate_storage(0, 0, 20).is_err());
        assert!(zero.batch(0).is_err());
        // There is no cuBLAS call, so its i32 dimension bound is irrelevant.
        assert!(GemmPlan::new(&dense(&[i32::MAX as usize + 1, 0]), &dense(&[0, 1])).is_ok());
    }
    #[test]
    fn invalid_geometry_is_rejected_before_launch() {
        assert!(GemmPlan::new(&dense(&[3]), &dense(&[3, 4])).is_err());
        assert!(GemmPlan::new(&dense(&[3, 5]), &dense(&[6, 7])).is_err());
        assert!(GemmPlan::new(&dense(&[5, 3]).permute(&[1, 0]).unwrap(), &dense(&[5, 7])).is_err());
        assert!(
            GemmPlan::new(
                &dense(&[1, i32::MAX as usize + 1]),
                &dense(&[i32::MAX as usize + 1, 1])
            )
            .is_err()
        );
        let only_f32 = usize::MAX / 8 + 1;
        assert!(GemmPlan::new_typed::<f32>(&dense(&[only_f32, 0]), &dense(&[0, 1])).is_ok());
        assert!(GemmPlan::new_typed::<f64>(&dense(&[only_f32, 0]), &dense(&[0, 1])).is_err());
        let huge = usize::MAX / 4 + 1;
        assert!(GemmPlan::new(&dense(&[huge, 0]), &dense(&[0, 1])).is_err());
    }
}
