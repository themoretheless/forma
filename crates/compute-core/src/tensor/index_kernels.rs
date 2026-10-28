use crate::{Binding, ComputeRuntime, GpuElement, Kernel, KernelError, shaders, wgpu};

pub(crate) struct TensorIndexKernels {
    pub copy: [Kernel; 2],
    pub compare: [Kernel; 2],
    pub select: [Kernel; 2],
    pub scan_blocks: [Kernel; 2],
    pub scan_add: [Kernel; 2],
    pub gather: [Kernel; 2],
    pub compact: [Kernel; 2],
    pub index_count: Kernel,
    pub count_reset: Kernel,
}
pub(super) fn kind<T: GpuElement>() -> usize {
    usize::from(std::any::TypeId::of::<T>() == std::any::TypeId::of::<u32>())
}
fn typed(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    bindings: &[Binding],
) -> Result<[Kernel; 2], KernelError> {
    Ok([
        Kernel::new(device, label, source, "main", bindings)?,
        Kernel::new(
            device,
            label,
            &source.replace("alias Value = f32;", "alias Value = u32;"),
            "main",
            bindings,
        )?,
    ])
}
impl TensorIndexKernels {
    fn new(device: &wgpu::Device) -> Result<Self, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        Ok(Self {
            copy: typed(device, "tensor copy", shaders::TENSOR_COPY_WGSL, &[R, R, W])?,
            compare: typed(
                device,
                "tensor compare",
                shaders::TENSOR_COMPARE_WGSL,
                &[R, R, R, W],
            )?,
            select: typed(
                device,
                "tensor select",
                shaders::TENSOR_SELECT_WGSL,
                &[R, R, R, R, W],
            )?,
            scan_blocks: typed(
                device,
                "tensor scan blocks",
                shaders::TENSOR_SCAN_BLOCKS_WGSL,
                &[R, R, W, W],
            )?,
            scan_add: typed(
                device,
                "tensor scan offsets",
                shaders::TENSOR_SCAN_ADD_WGSL,
                &[R, R, W],
            )?,
            gather: typed(
                device,
                "tensor gather",
                shaders::TENSOR_GATHER_WGSL,
                &[R, R, R, W],
            )?,
            compact: typed(
                device,
                "tensor compact",
                shaders::TENSOR_COMPACT_WGSL,
                &[R, R, R, R, R, W, W],
            )?,
            index_count: Kernel::new(
                device,
                "tensor invalid index count",
                shaders::TENSOR_INDEX_COUNT_WGSL,
                "main",
                &[R, R, W],
            )?,
            count_reset: Kernel::new(
                device,
                "tensor count reset",
                shaders::TENSOR_COUNT_RESET_WGSL,
                "main",
                &[R, W],
            )?,
        })
    }
}
impl ComputeRuntime {
    pub(crate) fn tensor_index_kernels(&self) -> Result<&TensorIndexKernels, KernelError> {
        if let Some(kernels) = self.tensor_index.get() {
            return Ok(kernels);
        }
        let kernels = TensorIndexKernels::new(self.device())?;
        let _ = self.tensor_index.set(kernels);
        Ok(self
            .tensor_index
            .get()
            .expect("tensor index kernels initialized"))
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn integer_specializations_validate() {
        for source in [
            super::shaders::TENSOR_COPY_WGSL,
            super::shaders::TENSOR_COMPARE_WGSL,
            super::shaders::TENSOR_SELECT_WGSL,
            super::shaders::TENSOR_SCAN_BLOCKS_WGSL,
            super::shaders::TENSOR_SCAN_ADD_WGSL,
            super::shaders::TENSOR_GATHER_WGSL,
            super::shaders::TENSOR_COMPACT_WGSL,
        ] {
            let module = naga::front::wgsl::parse_str(
                &source.replace("alias Value = f32;", "alias Value = u32;"),
            )
            .unwrap();
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
