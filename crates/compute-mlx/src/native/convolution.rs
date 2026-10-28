use super::*;
use tensor_core::{
    ConvOptions, ConvPlan, LowDtype, TensorConvBackend, TensorLowConvBackend, low_convolution_plan,
};

impl TensorConvBackend for MlxBackend {
    fn conv(
        &self,
        input: &MlxTensor,
        weight: &MlxTensor,
        options: &ConvOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.check(weight, Some(MlxDtype::F32))?;
        let plan = ConvPlan::new(input.shape(), weight.shape(), options)?;
        self.convolution(input, weight, plan, None)
    }
}

impl TensorLowConvBackend for MlxBackend {
    fn conv_low_f32(
        &self,
        input: &MlxLowTensor,
        weight: &MlxLowTensor,
        options: &ConvOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        self.check_low(weight)?;
        let plan = low_convolution_plan(input, weight, options)?;
        self.convolution(&input.tensor, &weight.tensor, plan, Some(input.dtype))
    }
}

impl MlxBackend {
    fn convolution(
        &self,
        input: &MlxTensor,
        weight: &MlxTensor,
        plan: ConvPlan,
        low: Option<LowDtype>,
    ) -> Result<MlxTensor, MlxError> {
        MlxBackend::dimensions(&plan.output)?;
        // An empty spatial input may have a nonempty padded result. In that
        // case every sample is padding, so there must be no native input load.
        if plan.output.is_empty() || plan.input.is_empty() {
            return self.zeros(plan.output, MlxDtype::F32);
        }
        let words = parameters(&plan)?;
        if self.context.api.metal.is_none() {
            return Err(MlxError::Native {
                operation: "convolution",
                message: "MLX-C custom Metal ABI is unavailable".into(),
            });
        }
        let params = self.upload_u32(Shape::new(vec![words.len()])?, &words)?;
        // Specialization preserves the same geometry while using native f32
        // loads or direct raw low loads. No expanded input or im2col exists.
        let load = if low.is_some() {
            "float a = low_decode(as_type<ushort>(input[input_at]), BF);\n\
             float b = low_decode(as_type<ushort>(weight[weight_at]), BF);\n\
             if (BF) sum += low_product(a, b); else sum = metal::fma(a, b, sum);"
        } else {
            "sum = metal::fma(input[input_at], weight[weight_at], sum);"
        };
        let source = include_str!("../metal/convolution.metal").replace("CONV_PRODUCT", load);
        let launch = low_kernels::reduction_launch(plan.output.numel(), 1);
        self.custom_metal(
            low_kernels::key(source, &["input", "weight", "params"]),
            &[input, weight, &params],
            plan.output,
            MlxDtype::F32,
            low == Some(LowDtype::Bf16),
            launch,
        )
    }
}

// U64 values are carried as little-endian u32 pairs, without restricting valid
// padding/stride options to signed Metal or MLX dimension sizes. ConvPlan has
// already checked output*stride + kernel*dilation <= padded_extent-1.
fn parameters(plan: &ConvPlan) -> Result<Vec<u32>, MlxError> {
    let product = |dims: &[usize]| {
        dims.iter()
            .try_fold(1usize, |n, &d| n.checked_mul(d).ok_or(MlxError::TooLarge))
    };
    let spatial = product(&plan.output.dims()[2..])?;
    let kernel = product(&plan.weight.dims()[2..])?;
    let contraction = kernel
        .checked_mul(plan.channels_per_group)
        .ok_or(MlxError::TooLarge)?;
    let mut words = Vec::with_capacity((6 + plan.spatial_rank * 4) * 2);
    let mut push_u64 = |v: usize| {
        words.push(v as u32);
        words.push((v as u64 >> 32) as u32);
    };
    for v in [
        plan.output.numel(),
        spatial,
        plan.channels_per_group,
        plan.outputs_per_group,
        kernel,
        contraction,
    ] {
        push_u64(v);
    }
    for axis in 0..plan.spatial_rank {
        for v in [
            plan.output.dims()[axis + 2],
            plan.options.strides[axis],
            plan.options.dilations[axis],
            plan.options.padding_before[axis],
        ] {
            push_u64(v);
        }
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_preserves_large_unsigned_options_and_group_geometry() {
        let input = Shape::new(vec![2, 6, 5]).unwrap();
        let weight = Shape::new(vec![9, 2, 1]).unwrap();
        let mut options = ConvOptions::new(1);
        options.groups = 3;
        options.strides[0] = usize::MAX;
        options.dilations[0] = usize::MAX;
        let plan = ConvPlan::new(&input, &weight, &options).unwrap();
        let words = parameters(&plan).unwrap();
        let values: Vec<_> = words
            .as_chunks::<2>()
            .0
            .iter()
            .map(|w| u64::from(w[0]) | (u64::from(w[1]) << 32))
            .collect();
        assert_eq!(
            values,
            [
                18,
                1,
                2,
                3,
                1,
                2,
                1,
                usize::MAX as u64,
                usize::MAX as u64,
                0
            ]
        );
    }
}
