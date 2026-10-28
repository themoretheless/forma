use crate::{Binding, ComputeRuntime, Kernel, KernelError};

pub(crate) struct TensorKernels {
    pub elementwise: Kernel,
    pub sum: Kernel,
    pub reduce: [Kernel; 2],
    pub matmul: Kernel,
}
impl TensorKernels {
    fn new(device: &crate::wgpu::Device) -> Result<Self, KernelError> {
        let binary = [
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ];
        let unary = [
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ];
        Ok(Self {
            elementwise: Kernel::new(
                device,
                "strided tensor elementwise",
                crate::shaders::TENSOR_ELEMENTWISE_WGSL,
                "main",
                &binary,
            )?,
            sum: Kernel::new(
                device,
                "strided f32 tensor sum",
                crate::shaders::TENSOR_SUM_WGSL,
                "main",
                &unary,
            )?,
            reduce: [
                Kernel::new(
                    device,
                    "strided f32 tensor reduction",
                    crate::shaders::TENSOR_REDUCE_WGSL,
                    "main",
                    &unary,
                )?,
                Kernel::new(
                    device,
                    "strided u32 tensor reduction",
                    &uint_reduction_source(),
                    "main",
                    &unary,
                )?,
            ],
            matmul: Kernel::new(
                device,
                "strided batched tensor matmul",
                crate::shaders::TENSOR_MATMUL_WGSL,
                "main",
                &binary,
            )?,
        })
    }
}

fn uint_reduction_source() -> String {
    crate::shaders::TENSOR_REDUCE_WGSL
        .replace("alias Value = f32;", "alias Value = u32;")
        .replace(
            "const MIN_IDENTITY: Value = 3.4028234663852886e38;",
            "const MIN_IDENTITY: Value = 0xffffffffu;",
        )
        .replace(
            "const MAX_IDENTITY: Value = -3.4028234663852886e38;",
            "const MAX_IDENTITY: Value = 0u;",
        )
}

impl ComputeRuntime {
    pub(crate) fn tensor_kernels(&self) -> Result<&TensorKernels, KernelError> {
        if let Some(kernels) = self.tensor.get() {
            return Ok(kernels);
        }
        let kernels = TensorKernels::new(self.device())?;
        let _ = self.tensor.set(kernels);
        Ok(self.tensor.get().expect("tensor kernels initialized"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn uint_reduction_specialization_validates() {
        let module = naga::front::wgsl::parse_str(&super::uint_reduction_source()).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
