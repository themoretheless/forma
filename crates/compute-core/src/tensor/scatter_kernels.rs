use crate::{Binding, ComputeRuntime, Kernel, KernelError, shaders, wgpu};

pub(crate) struct TensorScatterKernels {
    pub reset: Kernel,
    pub owners: Kernel,
    pub apply: [Kernel; 2],
}

fn uint_source() -> String {
    shaders::TENSOR_SCATTER_WGSL
        .replace("alias Value = f32;", "alias Value = u32;")
        .replace(
            "// u32 uses native atomics where available; specialized source replaces this.",
            "switch p[8] {\ncase 1u: { atomicAdd(&output[address], update); return; }\ncase 3u: { atomicMin(&output[address], update); return; }\ncase 4u: { atomicMax(&output[address], update); return; }\ndefault: {}\n}",
        )
}

impl TensorScatterKernels {
    fn new(device: &wgpu::Device) -> Result<Self, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        Ok(Self {
            reset: Kernel::new(
                device,
                "reset scatter owners",
                shaders::TENSOR_SCATTER_RESET_WGSL,
                "main",
                &[R, W],
            )?,
            owners: Kernel::new(
                device,
                "choose last scatter index",
                shaders::TENSOR_SCATTER_OWNERS_WGSL,
                "main",
                &[R, R, W],
            )?,
            apply: [
                Kernel::new(
                    device,
                    "scatter f32 updates",
                    shaders::TENSOR_SCATTER_WGSL,
                    "main",
                    &[R, R, R, R, W],
                )?,
                Kernel::new(
                    device,
                    "scatter u32 updates",
                    &uint_source(),
                    "main",
                    &[R, R, R, R, W],
                )?,
            ],
        })
    }
}

impl ComputeRuntime {
    pub(crate) fn tensor_scatter_kernels(&self) -> Result<&TensorScatterKernels, KernelError> {
        if let Some(kernels) = self.tensor_scatter.get() {
            return Ok(kernels);
        }
        let kernels = TensorScatterKernels::new(self.device())?;
        let _ = self.tensor_scatter.set(kernels);
        Ok(self
            .tensor_scatter
            .get()
            .expect("scatter kernels initialized"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn uint_scatter_specialization_validates() {
        let module = naga::front::wgsl::parse_str(&super::uint_source()).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
