//! Direct resident convolution. No operand materialization or im2col buffer.
use crate::{CudaError, CudaLowTensor, CudaRuntime, CudaTensor, indexing::output};
use gpu_compute::cuda::{CudaFunction, CudaModule, PushKernelArg, cudarc::driver::DeviceRepr};
use std::sync::Arc;
use tensor_core::{
    ConvOptions, ConvPlan, Layout, TensorConvBackend, TensorLowConvBackend, low_convolution_plan,
};

#[derive(Clone)]
pub(crate) struct ConvKernels([CudaFunction; 2]);
impl ConvKernels {
    pub(crate) fn load(module: &Arc<CudaModule>) -> Result<Self, CudaError> {
        Ok(Self([
            module.load_function("conv_f32")?,
            module.load_function("conv_low")?,
        ]))
    }
}

/// Header: spatial rank, C/group, O/group, kernel volume, input/weight offsets,
/// input N/C strides, weight O/C strides, O. Eight spatial arrays follow:
/// input/output/kernel dimensions, input/weight strides, stride, dilation, pad.
fn metadata(plan: &ConvPlan, input: &Layout, weight: &Layout) -> Result<Vec<u64>, CudaError> {
    if input.shape() != &plan.input || weight.shape() != &plan.weight {
        return Err(CudaError::InvalidInput(
            "convolution layouts differ from the plan",
        ));
    }
    crate::runtime::validate_logical_size::<f32>(&plan.output)?;
    // Avoid multiplying irrelevant huge dimensions in a zero contraction.
    if plan.output.is_empty() || plan.input.is_empty() {
        return Ok(Vec::new());
    }
    let fit = |value: usize| {
        u32::try_from(value)
            .map_err(|_| CudaError::InvalidInput("CUDA convolution coordinate exceeds u32"))
    };
    for &dim in input.shape().dims() {
        fit(dim)?;
    }
    // Physical layout arithmetic is u64 and checked by Layout, not u32.
    for &dim in weight.shape().dims().iter().chain(plan.output.dims()) {
        fit(dim)?;
    }
    for d in 0..plan.spatial_rank {
        let o = &plan.options;
        for value in [
            o.strides[d],
            o.dilations[d],
            o.padding_before[d],
            o.padding_after[d],
        ] {
            fit(value)?;
        }
        let padded = plan.input.dims()[d + 2]
            .checked_add(o.padding_before[d])
            .and_then(|n| n.checked_add(o.padding_after[d]))
            .ok_or(CudaError::InvalidInput(
                "CUDA convolution padded extent overflow",
            ))?;
        let effective = (plan.weight.dims()[d + 2] - 1)
            .checked_mul(o.dilations[d])
            .and_then(|n| n.checked_add(1))
            .ok_or(CudaError::InvalidInput(
                "CUDA convolution effective kernel overflow",
            ))?;
        fit(padded)?;
        fit(effective)?;
    }
    let kernel_count = plan.weight.dims()[2..]
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .ok_or(CudaError::InvalidInput(
            "CUDA convolution kernel volume overflow",
        ))?;
    let mut result = Vec::with_capacity(11 + plan.spatial_rank * 8);
    result.extend([
        plan.spatial_rank as u64,
        plan.channels_per_group as u64,
        plan.outputs_per_group as u64,
        kernel_count as u64,
        input.offset() as u64,
        weight.offset() as u64,
        input.strides()[0] as u64,
        input.strides()[1] as u64,
        weight.strides()[0] as u64,
        weight.strides()[1] as u64,
        plan.output.dims()[1] as u64,
    ]);
    for values in [
        &plan.input.dims()[2..],
        &plan.output.dims()[2..],
        &plan.weight.dims()[2..],
        &input.strides()[2..],
        &weight.strides()[2..],
        &plan.options.strides,
        &plan.options.dilations,
        &plan.options.padding_before,
    ] {
        result.extend(values.iter().map(|&n| n as u64));
    }
    Ok(result)
}

impl CudaRuntime {
    fn convolution_loaded<T: DeviceRepr>(
        &self,
        input: &CudaTensor<T>,
        weight: &CudaTensor<T>,
        plan: ConvPlan,
        dtype: Option<u32>,
    ) -> Result<CudaTensor, CudaError> {
        self.check(input)?;
        self.check(weight)?;
        let data = metadata(&plan, input.layout(), weight.layout())?;
        let mut result = self.zeros(plan.output)?;
        if data.is_empty() {
            return Ok(result);
        }
        let data = self.metadata(&data)?;
        let count = result.shape().numel() as u64;
        let kernel = &self.convolution.0[usize::from(dtype.is_some())];
        let mut args = self.device.stream.launch_builder(kernel);
        args.arg(input.storage.as_ref())
            .arg(weight.storage.as_ref())
            .arg(output(&mut result))
            .arg(&data)
            .arg(&count);
        if let Some(ref dtype) = dtype {
            args.arg(dtype);
        }
        // Layout validation covers every non-padding read, the result is fresh
        // and disjoint, and the metadata descriptor exactly matches the ABI.
        unsafe {
            args.launch(self.config(count as usize))?;
        }
        Ok(result)
    }
}
impl TensorConvBackend for CudaRuntime {
    fn conv(
        &self,
        input: &CudaTensor,
        weight: &CudaTensor,
        options: &ConvOptions,
    ) -> Result<CudaTensor, CudaError> {
        let plan = ConvPlan::new(input.shape(), weight.shape(), options)?;
        self.convolution_loaded(input, weight, plan, None)
    }
}
impl TensorLowConvBackend for CudaRuntime {
    fn conv_low_f32(
        &self,
        input: &CudaLowTensor,
        weight: &CudaLowTensor,
        options: &ConvOptions,
    ) -> Result<CudaTensor, CudaError> {
        let plan = low_convolution_plan(input, weight, options)?;
        self.convolution_loaded(
            &input.tensor,
            &weight.tensor,
            plan,
            Some(input.dtype as u32),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tensor_core::Shape;
    fn shape(dims: &[usize]) -> Shape {
        Shape::new(dims.to_vec()).unwrap()
    }

    #[test]
    fn metadata_keeps_grouped_strided_offsets_and_asymmetric_geometry() {
        let input = Layout::new(shape(&[2, 4, 5, 7]), vec![140, 35, 1, 5], 11).unwrap();
        let weight = Layout::new(shape(&[6, 2, 2, 3]), vec![12, 6, 1, 2], 3).unwrap();
        let options = ConvOptions {
            strides: vec![2, 1],
            dilations: vec![1, 2],
            padding_before: vec![1, 2],
            padding_after: vec![0, 1],
            groups: 2,
        };
        let plan = ConvPlan::new(input.shape(), weight.shape(), &options).unwrap();
        let data = metadata(&plan, &input, &weight).unwrap();
        assert_eq!(plan.output.dims(), [2, 6, 3, 6]);
        assert_eq!(&data[..11], [2, 2, 3, 6, 11, 3, 140, 35, 12, 6, 6]);
        assert_eq!(
            &data[11..],
            [5, 7, 3, 6, 2, 3, 1, 5, 1, 2, 2, 1, 1, 2, 1, 2]
        );
    }

    #[test]
    fn empty_and_zero_channel_plans_do_not_multiply_kernel_dimensions() {
        let input = Layout::contiguous(shape(&[1, 0, 1, 1, 1])).unwrap();
        let weight =
            Layout::contiguous(shape(&[2, 0, usize::MAX, usize::MAX, usize::MAX])).unwrap();
        let plan = ConvPlan::new(input.shape(), weight.shape(), &ConvOptions::new(3)).unwrap();
        assert!(metadata(&plan, &input, &weight).unwrap().is_empty());
        let input = Layout::contiguous(shape(&[1, 0, 3])).unwrap();
        let weight = Layout::contiguous(shape(&[2, 0, 1])).unwrap();
        let plan = ConvPlan::new(input.shape(), weight.shape(), &ConvOptions::new(1)).unwrap();
        assert_eq!(plan.output.dims(), [1, 2, 3]);
        assert!(metadata(&plan, &input, &weight).unwrap().is_empty());
    }

    #[test]
    fn active_coordinate_limits_are_checked_before_upload() {
        let input = Layout::new(shape(&[1, 1, 1]), vec![0; 3], 0).unwrap();
        let weight = input.clone();
        let options = ConvOptions {
            strides: vec![usize::MAX],
            ..ConvOptions::new(1)
        };
        let plan = ConvPlan::new(input.shape(), weight.shape(), &options).unwrap();
        assert!(metadata(&plan, &input, &weight).is_err());
        let options = ConvOptions {
            strides: vec![2],
            padding_before: vec![u32::MAX as usize],
            ..ConvOptions::new(1)
        };
        let plan = ConvPlan::new(input.shape(), weight.shape(), &options).unwrap();
        assert!(metadata(&plan, &input, &weight).is_err());
    }
}
