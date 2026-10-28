use super::{low_kernels::*, *};
use tensor_core::{
    LowDtype, ReduceOp, TensorLowBackend, TensorLowOpsBackend, low_binary_shape, mean_shape,
    reduction_shape,
};

const UNARY: &str = include_str!("../metal/low_unary.metal");
const BINARY: &str = include_str!("../metal/low_binary.metal");

impl MlxBackend {
    fn reduce_low_values(
        &self,
        op: ReduceOp,
        input: &MlxLowTensor,
        axes: &[usize],
        keep_dims: bool,
        mean: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        let shape = if mean {
            mean_shape(input.shape(), axes, keep_dims)?
        } else {
            reduction_shape(op, input.shape(), axes, keep_dims)?
        };
        Self::dimensions(&shape)?;
        if axes.is_empty() {
            return self.cast_to_f32(input);
        }
        if shape.is_empty() {
            return self.zeros(shape, MlxDtype::F32);
        }
        if input.shape().is_empty() {
            // Only sum/product can reach a nonempty output with count zero.
            if op == ReduceOp::Product {
                let dims = Self::dimensions(&shape)?;
                return self.output("empty low product", shape, MlxDtype::F32, |a, out| unsafe {
                    (a.ones)(
                        out,
                        dims.as_ptr(),
                        dims.len(),
                        ffi::F32,
                        self.context.stream,
                    )
                });
            }
            return self.zeros(shape, MlxDtype::F32);
        }
        let plan = self.low_reduction_plan(input, axes)?;
        let params = self.low_parameters(plan.count, plan.row_shape.rank(), plan.parts)?;
        let source = reduction_source(op, true, mean && plan.parts == 1, false);
        let partial = self.custom_metal(
            key(source, &["inp", "params"]),
            &[&plan.input, &params],
            plan.partial_shape(&shape)?,
            MlxDtype::F32,
            input.dtype == LowDtype::Bf16,
            reduction_launch(plan.rows, plan.parts),
        )?;
        self.low_fold_partials(partial, &plan, shape, op, mean)
    }
}

impl TensorLowOpsBackend for MlxBackend {
    fn unary_low(&self, op: UnaryOp, input: &MlxLowTensor) -> Result<MlxLowTensor, MlxError> {
        self.check_low(input)?;
        let shape = input.shape().clone();
        let tensor = if shape.is_empty() {
            self.zeros(shape, input.dtype.into())?
        } else {
            let storage = self.low_elementwise_view(&input.tensor, &shape)?;
            let source = UNARY.replace("OP", &(op as u32).to_string());
            self.custom_metal(
                key(source, &["inp"]),
                &[&storage],
                shape.clone(),
                input.dtype.into(),
                input.dtype == LowDtype::Bf16,
                element_launch(shape.numel()),
            )?
        };
        Ok(MlxLowTensor {
            tensor,
            dtype: input.dtype,
        })
    }

    fn binary_low(
        &self,
        op: BinaryOp,
        left: &MlxLowTensor,
        right: &MlxLowTensor,
    ) -> Result<MlxLowTensor, MlxError> {
        self.check_low(left)?;
        self.check_low(right)?;
        let shape = low_binary_shape(left, right)?;
        Self::dimensions(&shape)?;
        let tensor = if shape.is_empty() {
            self.zeros(shape, left.dtype.into())?
        } else {
            let lhs = self.low_elementwise_view(&left.tensor, &shape)?;
            let rhs = self.low_elementwise_view(&right.tensor, &shape)?;
            self.custom_metal(
                key(
                    BINARY.replace("OP", &(op as u32).to_string()),
                    &["lhs", "rhs"],
                ),
                &[&lhs, &rhs],
                shape.clone(),
                left.dtype.into(),
                left.dtype == LowDtype::Bf16,
                element_launch(shape.numel()),
            )?
        };
        Ok(MlxLowTensor {
            tensor,
            dtype: left.dtype,
        })
    }

    fn reduce_low_f32(
        &self,
        op: ReduceOp,
        input: &MlxLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.reduce_low_values(op, input, axes, keep_dims, false)
    }

    fn mean_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.reduce_low_values(ReduceOp::Sum, input, axes, keep_dims, true)
    }
}
