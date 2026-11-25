use crate::runtime::lazy_kernel;
use crate::{Binding, ComputeRuntime, Kernel, KernelError};
use std::sync::OnceLock;

pub(crate) struct TensorKernels {
    elementwise: OnceLock<Kernel>,
    sum: OnceLock<Kernel>,
    reduce: [OnceLock<Kernel>; 2],
    matmul: OnceLock<Kernel>,
}

const UNARY_BINDINGS: [Binding; 3] = [
    Binding::StorageRead,
    Binding::StorageRead,
    Binding::StorageReadWrite,
];
const BINARY_BINDINGS: [Binding; 4] = [
    Binding::StorageRead,
    Binding::StorageRead,
    Binding::StorageRead,
    Binding::StorageReadWrite,
];

impl TensorKernels {
    fn new() -> Self {
        Self {
            elementwise: OnceLock::new(),
            sum: OnceLock::new(),
            reduce: std::array::from_fn(|_| OnceLock::new()),
            matmul: OnceLock::new(),
        }
    }

    pub(crate) fn elementwise(&self, device: &crate::wgpu::Device) -> Result<&Kernel, KernelError> {
        lazy_kernel(&self.elementwise, || {
            Kernel::new(
                device,
                "strided tensor elementwise",
                crate::shaders::TENSOR_ELEMENTWISE_WGSL,
                "main",
                &BINARY_BINDINGS,
            )
        })
    }

    pub(crate) fn sum(&self, device: &crate::wgpu::Device) -> Result<&Kernel, KernelError> {
        lazy_kernel(&self.sum, || {
            Kernel::new(
                device,
                "strided f32 tensor sum",
                crate::shaders::TENSOR_SUM_WGSL,
                "main",
                &UNARY_BINDINGS,
            )
        })
    }

    pub(crate) fn reduce(
        &self,
        device: &crate::wgpu::Device,
        kind: usize,
    ) -> Result<&Kernel, KernelError> {
        if kind == 0 {
            lazy_kernel(&self.reduce[0], || {
                Kernel::new(
                    device,
                    "strided f32 tensor reduction",
                    crate::shaders::TENSOR_REDUCE_WGSL,
                    "main",
                    &UNARY_BINDINGS,
                )
            })
        } else {
            lazy_kernel(&self.reduce[1], || {
                Kernel::new(
                    device,
                    "strided u32 tensor reduction",
                    &uint_reduction_source(),
                    "main",
                    &UNARY_BINDINGS,
                )
            })
        }
    }

    pub(crate) fn matmul(&self, device: &crate::wgpu::Device) -> Result<&Kernel, KernelError> {
        lazy_kernel(&self.matmul, || {
            Kernel::new(
                device,
                "strided batched tensor matmul",
                crate::shaders::TENSOR_MATMUL_WGSL,
                "main",
                &BINARY_BINDINGS,
            )
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
        Ok(self.tensor.get_or_init(TensorKernels::new))
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
