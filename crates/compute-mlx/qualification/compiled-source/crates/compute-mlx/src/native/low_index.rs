use super::{low_kernels::*, *};
use tensor_core::{
    Compacted, Gathered, LowDtype, ReduceOp, ScanOptions, TensorLowIndexBackend, low_binary_shape,
    low_select_shape,
};
const COMPARE: &str = include_str!("../metal/low_compare.metal");
const SELECT: &str = include_str!("../metal/low_select.metal");
const SCAN: &str = include_str!("../metal/low_scan.metal");

impl MlxBackend {
    /// Used by all dtype-generic indexing planners: native BF16 `where` flushes
    /// subnormals and canonicalizes NaNs, while this kernel only routes u16 bits.
    pub(super) fn select_low_values(
        &self,
        mask: &MlxTensor,
        left: &MlxTensor,
        right: &MlxTensor,
        shape: Shape,
    ) -> Result<MlxTensor, MlxError> {
        Self::dimensions(&shape)?;
        if shape.is_empty() {
            return self.zeros(shape, left.dtype);
        }
        let lhs = self.low_elementwise_view(left, &shape)?;
        let rhs = self.low_elementwise_view(right, &shape)?;
        let mask = self.low_elementwise_view(mask, &shape)?;
        self.custom_metal(
            key(SELECT.into(), &["lhs", "rhs", "mask"]),
            &[&lhs, &rhs, &mask],
            shape.clone(),
            left.dtype,
            left.dtype == MlxDtype::Bf16,
            element_launch(shape.numel()),
        )
    }
}

impl TensorLowIndexBackend for MlxBackend {
    fn compare_low(
        &self,
        op: CompareOp,
        left: &MlxLowTensor,
        right: &MlxLowTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(left)?;
        self.check_low(right)?;
        let shape = low_binary_shape(left, right)?;
        Self::dimensions(&shape)?;
        if shape.is_empty() {
            return self.zeros(shape, MlxDtype::U32);
        }
        let lhs = self.low_elementwise_view(&left.tensor, &shape)?;
        let rhs = self.low_elementwise_view(&right.tensor, &shape)?;
        self.custom_metal(
            key(
                COMPARE.replace("OP", &(op as u32).to_string()),
                &["lhs", "rhs"],
            ),
            &[&lhs, &rhs],
            shape.clone(),
            MlxDtype::U32,
            left.dtype == LowDtype::Bf16,
            element_launch(shape.numel()),
        )
    }

    fn select_low(
        &self,
        mask: &MlxTensor,
        on_true: &MlxLowTensor,
        on_false: &MlxLowTensor,
    ) -> Result<MlxLowTensor, MlxError> {
        self.check(mask, Some(MlxDtype::U32))?;
        self.check_low(on_true)?;
        self.check_low(on_false)?;
        let shape = low_select_shape(mask.shape(), on_true, on_false)?;
        let tensor = self.select_low_values(mask, &on_true.tensor, &on_false.tensor, shape)?;
        Ok(MlxLowTensor {
            tensor,
            dtype: on_true.dtype,
        })
    }

    fn gather_low(
        &self,
        input: &MlxLowTensor,
        indices: &MlxTensor,
        axis: usize,
    ) -> Result<Gathered<MlxLowTensor, MlxTensor>, MlxError> {
        self.check_low(input)?;
        let result = self.gather_values(&input.tensor, indices, axis)?;
        Ok(Gathered {
            values: MlxLowTensor {
                tensor: result.values,
                dtype: input.dtype,
            },
            invalid_count: result.invalid_count,
        })
    }

    fn compact_low(
        &self,
        input: &MlxLowTensor,
        mask: &MlxTensor,
    ) -> Result<Compacted<MlxLowTensor, MlxTensor>, MlxError> {
        self.check_low(input)?;
        let result = self.compact_values(&input.tensor, mask)?;
        Ok(Compacted {
            values: MlxLowTensor {
                tensor: result.values,
                dtype: input.dtype,
            },
            count: result.count,
        })
    }

    fn scan_low_f32(
        &self,
        input: &MlxLowTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        input.shape().validate_axes(&[axis])?;
        if input.shape().is_empty() {
            return self.zeros(input.shape().clone(), MlxDtype::F32);
        }
        // Move just the scan axis; native permutation retains the original
        // allocation/strides. Every row is independent in both directions.
        let rank = input.shape().rank();
        let order: Vec<_> = (0..rank).filter(|&a| a != axis).chain([axis]).collect();
        let mut inverse = vec![0; rank];
        for (position, &original) in order.iter().enumerate() {
            inverse[original] = position;
        }
        let permuted = self.permute(&input.tensor, &order)?;
        let count = input.shape().dims()[axis];
        let rows = input.shape().numel() / count;
        let parts = count.div_ceil(WIDTH);
        let params = self.low_parameters(count, rank - 1, parts)?;
        let carry = if parts == 1 {
            // MLX custom scalars are generated as values, not pointers. The
            // unused carry must remain a vector so carry[group] type-checks.
            self.zeros(Shape::new(vec![1])?, MlxDtype::F32)?
        } else {
            let mut dims = permuted.shape.dims().to_vec();
            dims[rank - 1] = parts;
            let totals = self.custom_metal(
                key(
                    reduction_source(ReduceOp::Sum, true, false, options.reverse),
                    &["inp", "params"],
                ),
                &[&permuted, &params],
                Shape::new(dims)?,
                MlxDtype::F32,
                input.dtype == LowDtype::Bf16,
                reduction_launch(rows, parts),
            )?;
            // Only small f32 block totals reach native cumsum. No low native
            // scan and no whole-input f32 conversion participate in the graph.
            self.scan(&totals, rank - 1, false, false)?
        };
        let source = SCAN
            .replace("REVERSE", if options.reverse { "true" } else { "false" })
            .replace(
                "INCLUSIVE",
                if options.inclusive { "true" } else { "false" },
            )
            .replace("CARRY", if parts > 1 { "true" } else { "false" });
        let prefix = self.custom_metal(
            key(source, &["inp", "params", "carry"]),
            &[&permuted, &params, &carry],
            permuted.shape.clone(),
            MlxDtype::F32,
            input.dtype == LowDtype::Bf16,
            reduction_launch(rows, parts),
        )?;
        // A metadata view restores original coordinate order, including reverse
        // traversal. Materializing row-major data remains the readback's job.
        self.permute(&prefix, &inverse)
    }
}
