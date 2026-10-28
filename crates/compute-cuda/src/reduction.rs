use crate::{CudaError, CudaRuntime, CudaTensor, indexing::output};
use gpu_compute::cuda::{
    CudaFunction, CudaModule, PushKernelArg,
    cudarc::driver::{DeviceRepr, ValidAsZeroBits},
};
use std::sync::Arc;
use tensor_core::{
    ReduceOp, Shape, TensorBackend, TensorF64Backend, TensorReduceBackend, mean_shape,
    reduction_shape,
};

pub(crate) mod dispatch;
use dispatch::{LoadedReduction, ReductionPass};

#[derive(Clone)]
pub(crate) struct ReductionKernels {
    pub(crate) axes: [CudaFunction; 3],
    pub(crate) all: [CudaFunction; 3],
    pub(crate) fill: [CudaFunction; 3],
}
impl ReductionKernels {
    pub(crate) fn load(module: &Arc<CudaModule>) -> Result<Self, CudaError> {
        let triple = |f: &str, u: &str, d: &str| -> Result<_, CudaError> {
            Ok([
                module.load_function(f)?,
                module.load_function(u)?,
                module.load_function(d)?,
            ])
        };
        Ok(Self {
            axes: triple("reduce_axes", "reduce_axes_u32", "reduce_axes_f64")?,
            all: triple("reduce_all", "reduce_all_u32", "reduce_all_f64")?,
            fill: triple("fill_f32", "fill_u32", "fill_f64")?,
        })
    }
}

// Reduction storage types are independent of indexing/scan kernel catalogs.
pub(crate) trait ReductionScalar: DeviceRepr + ValidAsZeroBits + Copy {
    const KIND: usize;
    const ONE: Self;
    fn materialize(
        rt: &CudaRuntime,
        input: &CudaTensor<Self>,
    ) -> Result<CudaTensor<Self>, CudaError>;
}
impl ReductionScalar for f32 {
    const KIND: usize = 0;
    const ONE: Self = 1.;
    fn materialize(
        rt: &CudaRuntime,
        input: &CudaTensor<Self>,
    ) -> Result<CudaTensor<Self>, CudaError> {
        <f32 as crate::indexing::CudaScalar>::materialize(rt, input)
    }
}
impl ReductionScalar for u32 {
    const KIND: usize = 1;
    const ONE: Self = 1;
    fn materialize(
        rt: &CudaRuntime,
        input: &CudaTensor<Self>,
    ) -> Result<CudaTensor<Self>, CudaError> {
        <u32 as crate::indexing::CudaScalar>::materialize(rt, input)
    }
}
impl ReductionScalar for f64 {
    const KIND: usize = 2;
    const ONE: Self = 1.;
    fn materialize(
        rt: &CudaRuntime,
        input: &CudaTensor<Self>,
    ) -> Result<CudaTensor<Self>, CudaError> {
        rt.materialize_f64(input)
    }
}

impl CudaRuntime {
    pub(crate) fn reduce_typed<T: ReductionScalar>(
        &self,
        op: ReduceOp,
        input: &CudaTensor<T>,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor<T>, CudaError> {
        self.check(input)?;
        let shape = reduction_shape(op, input.shape(), axes, keep_dims)?;
        if axes.is_empty() {
            return T::materialize(self, input);
        }
        if shape.is_empty() {
            return self.zeros_typed(shape);
        }
        if input.shape().is_empty() {
            let mut result = self.zeros_typed(shape.clone())?;
            if op == ReduceOp::Product {
                let n = shape.numel() as u64;
                // Only empty product needs one rather than the zero allocation;
                // min/max empty contractions were rejected by the shared helper.
                unsafe {
                    self.device
                        .stream
                        .launch_builder(&self.reduction.fill[T::KIND])
                        .arg(output(&mut result))
                        .arg(&n)
                        .arg(&T::ONE)
                        .launch(self.config(shape.numel()))?;
                }
            }
            return Ok(result);
        }
        if axes.len() == input.shape().rank() {
            return self.reduce_all_typed(op, input, shape);
        }
        self.reduce_axes_loaded(op, input, axes, shape, &self.reduction.axes[T::KIND], None)
    }

    /// Common axis dispatch for native inputs and decoded low inputs. Output
    /// type describes the accumulator, while dtype is an optional load policy.
    pub(crate) fn reduce_axes_loaded<I: DeviceRepr, O: DeviceRepr + ValidAsZeroBits>(
        &self,
        op: ReduceOp,
        input: &CudaTensor<I>,
        axes: &[usize],
        shape: Shape,
        kernel: &CudaFunction,
        dtype: Option<u32>,
    ) -> Result<CudaTensor<O>, CudaError> {
        let pass =
            ReductionPass::axes(op, &input.layout, axes, shape, self.device.multiprocessors)?;
        let mut result = self.zeros_typed(pass.output_shape.clone())?;
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            LoadedReduction {
                function: kernel,
                dtype,
            },
            input.storage.as_ref(),
            output(&mut result),
            &metadata,
        )?;
        Ok(result)
    }

    fn reduce_all_typed<T: ReductionScalar>(
        &self,
        op: ReduceOp,
        input: &CudaTensor<T>,
        shape: Shape,
    ) -> Result<CudaTensor<T>, CudaError> {
        let mut current = input.clone();
        loop {
            let next =
                self.reduce_all_loaded::<T, T>(op, &current, &self.reduction.all[T::KIND], None)?;
            if next.shape().numel() == 1 {
                return self.view(&next, next.layout.reshape(shape)?);
            }
            current = next;
        }
    }

    pub(crate) fn reduce_all_loaded<I: DeviceRepr, O: DeviceRepr + ValidAsZeroBits>(
        &self,
        op: ReduceOp,
        input: &CudaTensor<I>,
        kernel: &CudaFunction,
        dtype: Option<u32>,
    ) -> Result<CudaTensor<O>, CudaError> {
        let pass = ReductionPass::all(op, &input.layout, self.device.multiprocessors)?;
        let mut next = self.zeros_typed::<O>(pass.output_shape.clone())?;
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            LoadedReduction {
                function: kernel,
                dtype,
            },
            input.storage.as_ref(),
            output(&mut next),
            &metadata,
        )?;
        Ok(next)
    }
}

impl TensorReduceBackend for CudaRuntime {
    fn reduce_f32(
        &self,
        op: ReduceOp,
        input: &CudaTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor, CudaError> {
        self.reduce_typed(op, input, axes, keep_dims)
    }
    fn reduce_u32(
        &self,
        op: ReduceOp,
        input: &CudaTensor<u32>,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor<u32>, CudaError> {
        self.reduce_typed(op, input, axes, keep_dims)
    }
    fn mean_axes(
        &self,
        input: &CudaTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor, CudaError> {
        self.check(input)?;
        let shape = mean_shape(input.shape(), axes, keep_dims)?;
        if axes.is_empty() {
            return self.materialize(input);
        }
        if shape.is_empty() {
            return self.zeros(shape);
        }
        let denominator = (input.shape().numel() / shape.numel()) as f32;
        let sum = self.reduce_f32(ReduceOp::Sum, input, axes, keep_dims)?;
        // Divides directly (no reciprocal multiply or extra +0), with shape
        // metadata as a scalar kernel argument and no host intermediate values.
        self.unary_op(&sum, 11, denominator, 0.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tensor_core::Layout;

    #[test]
    fn reduction_metadata_keeps_axis_order_and_broadcast_strides() {
        let layout = Layout::contiguous(Shape::new(vec![2, 1, 3]).unwrap())
            .unwrap()
            .broadcast_to(Shape::new(vec![2, 5, 3]).unwrap())
            .unwrap()
            .permute(&[2, 1, 0])
            .unwrap();
        let out = reduction_shape(ReduceOp::Sum, layout.shape(), &[2, 0], false).unwrap();
        let pass = ReductionPass::axes(ReduceOp::Sum, &layout, &[2, 0], out.clone(), 40).unwrap();
        assert_eq!(pass.metadata, [5, 0, 2, 3, 3, 1]);
        assert_eq!(layout.shape().numel() / out.numel(), 6);
    }
}
