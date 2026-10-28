//! Shared f32/packed-low convolution traversal and transactional recording.
use super::{GpuTensor, TensorComputeError, checked_index};
use crate::{Binding, ComputeProgram, ComputeRuntime, Kernel, KernelError, wgpu};
use tensor_core::{ConvOptions, ConvPlan, Layout, LowDtype};
type Result<T> = std::result::Result<T, TensorComputeError>;

impl ComputeRuntime {
    pub(in crate::tensor) fn conv_kernel(
        &self,
        dtype: Option<LowDtype>,
    ) -> std::result::Result<&Kernel, KernelError> {
        let slot = &self.tensor_conv[dtype.map_or(0, |d| d as usize + 1)];
        if let Some(kernel) = slot.get() {
            return Ok(kernel);
        }
        let source = conv_source(dtype);
        let kernel = Kernel::new(
            self.device(),
            "direct tensor convolution",
            &source,
            "main",
            &[
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageReadWrite,
            ],
        )?;
        let _ = slot.set(kernel);
        Ok(slot.get().expect("convolution kernel initialized"))
    }
}

fn conv_source(dtype: Option<LowDtype>) -> String {
    let source = crate::shaders::TENSOR_CONV_WGSL;
    let Some(dtype) = dtype else {
        return source.to_owned();
    };
    let product = r#"fn product(a: u32, b: u32) -> f32 {
    let abits = low_decode((input[a / 2u] >> ((a % 2u) * 16u)) & 0xffffu, LOW_DTYPE);
    let bbits = low_decode((weight[b / 2u] >> ((b % 2u) * 16u)) & 0xffffu, LOW_DTYPE);
    // BF16 subnormal times a large finite value may be a normal f32 product.
    if (abits & 0x7fffffffu) != 0u && (abits & 0x7f800000u) == 0u {
        return float_up64(bitcast<f32>(abits)) * bitcast<f32>(float_down_bits(bbits, 64u));
    }
    if (bbits & 0x7fffffffu) != 0u && (bbits & 0x7f800000u) == 0u {
        return bitcast<f32>(float_down_bits(abits, 64u)) * float_up64(bitcast<f32>(bbits));
    }
    return bitcast<f32>(abits) * bitcast<f32>(bbits);
}"#;
    let source = source
        .replace("input: array<f32>", "input: array<u32>")
        .replace("weight: array<f32>", "weight: array<u32>")
        .replace(
            "fn product(a: u32, b: u32) -> f32 { return input[a] * weight[b]; }",
            product,
        );
    format!(
        "const LOW_DTYPE: u32 = {}u;\n{}\n{}\n{source}",
        dtype as u32,
        include_str!("../../shaders/low_codec.wgsl"),
        include_str!("../../shaders/float_power2.wgsl")
    )
}

impl<'a> ComputeProgram<'a> {
    /// Records grouped 1D/2D/3D cross-correlation with f32 accumulation.
    /// No im2col tensor or host readback is created.
    pub fn tensor_conv(
        &mut self,
        input: &GpuTensor,
        weight: &GpuTensor,
        options: &ConvOptions,
    ) -> Result<GpuTensor> {
        self.index_check(input)?;
        self.index_check(weight)?;
        let plan = ConvPlan::new(input.shape(), weight.shape(), options)?;
        let output = self.index_new(plan.output)?;
        self.tensor_conv_into(input, weight, options, &output)?;
        Ok(output)
    }
    /// Records into distinct contiguous storage. Errors leave this recorder
    /// unchanged; successful replay rewrites zeros as well as nonzero results.
    pub fn tensor_conv_into(
        &mut self,
        input: &GpuTensor,
        weight: &GpuTensor,
        options: &ConvOptions,
        output: &GpuTensor,
    ) -> Result<()> {
        self.index_check(input)?;
        self.index_check(weight)?;
        let plan = ConvPlan::new(input.shape(), weight.shape(), options)?;
        self.conv_loaded(
            &plan,
            [input.layout(), weight.layout()],
            [input.values().buffer(), weight.values().buffer()],
            None,
            output,
        )
    }
    pub(in crate::tensor) fn conv_loaded(
        &mut self,
        plan: &ConvPlan,
        layouts: [&Layout; 2],
        buffers: [&wgpu::Buffer; 2],
        dtype: Option<LowDtype>,
        output: &GpuTensor,
    ) -> Result<()> {
        self.index_output(output, &plan.output, &buffers)?;
        let count = checked_index(plan.output.numel())?;
        let groups = self.index_groups(count);
        let meta = conv_metadata(plan, layouts, output.layout().offset(), groups)?;
        if count == 0 {
            return Ok(());
        }
        let kernel = self.runtime.conv_kernel(dtype)?;
        self.index_dispatch(
            kernel,
            &meta,
            &[buffers[0], buffers[1], output.values().buffer()],
            groups,
        )
    }
}

fn conv_metadata(
    plan: &ConvPlan,
    layouts: [&Layout; 2],
    output_offset: usize,
    groups: u32,
) -> Result<Vec<u32>> {
    let [input, weight] = layouts;
    // Empty contractions do not need to represent a potentially huge kernel
    // volume, but every axis and padded coordinate must still fit the shader.
    let volume = if plan.channels_per_group == 0 || plan.output.numel() == 0 {
        0
    } else {
        plan.weight.dims()[2..]
            .iter()
            .try_fold(1usize, |a, b| a.checked_mul(*b))
            .ok_or(TensorComputeError::IndexTooLarge)?
    };
    let terms = volume
        .checked_mul(plan.channels_per_group)
        .ok_or(TensorComputeError::IndexTooLarge)?;
    let mut values = vec![
        plan.output.numel(),
        groups as usize,
        plan.spatial_rank,
        plan.weight.dims()[0],
        plan.outputs_per_group,
        plan.channels_per_group,
        volume,
        terms,
        input.offset(),
        weight.offset(),
        output_offset,
        input.strides()[0],
        input.strides()[1],
        weight.strides()[0],
        weight.strides()[1],
    ];
    for axis in 0..plan.spatial_rank {
        let i = axis + 2;
        checked_index(
            plan.input.dims()[i]
                .checked_add(plan.options.padding_before[axis])
                .and_then(|n| n.checked_add(plan.options.padding_after[axis]))
                .ok_or(TensorComputeError::IndexTooLarge)?,
        )?;
        checked_index(
            (plan.weight.dims()[i] - 1)
                .checked_mul(plan.options.dilations[axis])
                .and_then(|n| n.checked_add(1))
                .ok_or(TensorComputeError::IndexTooLarge)?,
        )?;
        values.extend([
            plan.input.dims()[i],
            plan.output.dims()[i],
            plan.weight.dims()[i],
            plan.options.strides[axis],
            plan.options.dilations[axis],
            plan.options.padding_before[axis],
            input.strides()[i],
            weight.strides()[i],
        ]);
    }
    values.into_iter().map(checked_index).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn convolution_specializations_validate() {
        for dtype in [
            None,
            Some(tensor_core::LowDtype::F16),
            Some(tensor_core::LowDtype::Bf16),
        ] {
            let source = super::conv_source(dtype);
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
