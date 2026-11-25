use crate::runtime::lazy_kernel;
use crate::{Binding, ComputeRuntime, Kernel, KernelError, shaders, wgpu};
use std::sync::OnceLock;

pub(crate) struct TensorScatterKernels {
    reset: OnceLock<Kernel>,
    owners: OnceLock<Kernel>,
    apply: [OnceLock<Kernel>; 2],
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
    fn new() -> Self {
        Self {
            reset: OnceLock::new(),
            owners: OnceLock::new(),
            apply: std::array::from_fn(|_| OnceLock::new()),
        }
    }

    pub(crate) fn reset(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        lazy_kernel(&self.reset, || {
            Kernel::new(
                device,
                "reset scatter owners",
                shaders::TENSOR_SCATTER_RESET_WGSL,
                "main",
                &[Binding::StorageRead, Binding::StorageReadWrite],
            )
        })
    }

    pub(crate) fn owners(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        lazy_kernel(&self.owners, || {
            Kernel::new(
                device,
                "choose last scatter index",
                shaders::TENSOR_SCATTER_OWNERS_WGSL,
                "main",
                &[
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                ],
            )
        })
    }

    pub(crate) fn apply(&self, device: &wgpu::Device, kind: usize) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        if kind == 0 {
            lazy_kernel(&self.apply[0], || {
                Kernel::new(
                    device,
                    "scatter f32 updates",
                    shaders::TENSOR_SCATTER_WGSL,
                    "main",
                    &[R, R, R, R, W],
                )
            })
        } else {
            lazy_kernel(&self.apply[1], || {
                Kernel::new(
                    device,
                    "scatter u32 updates",
                    &uint_source(),
                    "main",
                    &[R, R, R, R, W],
                )
            })
        }
    }
}

impl ComputeRuntime {
    pub(crate) fn tensor_scatter_kernels(&self) -> Result<&TensorScatterKernels, KernelError> {
        Ok(self.tensor_scatter.get_or_init(TensorScatterKernels::new))
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
