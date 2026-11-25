use crate::runtime::lazy_kernel;
use crate::{Binding, ComputeRuntime, GpuElement, Kernel, KernelError, shaders, wgpu};
use std::sync::OnceLock;

pub(crate) struct TensorIndexKernels {
    copy: [OnceLock<Kernel>; 2],
    compare: [OnceLock<Kernel>; 2],
    select: [OnceLock<Kernel>; 2],
    scan_blocks: [OnceLock<Kernel>; 2],
    scan_add: [OnceLock<Kernel>; 2],
    gather: [OnceLock<Kernel>; 2],
    compact: [OnceLock<Kernel>; 2],
    index_count: OnceLock<Kernel>,
    count_reset: OnceLock<Kernel>,
}
pub(super) fn kind<T: GpuElement>() -> usize {
    usize::from(std::any::TypeId::of::<T>() == std::any::TypeId::of::<u32>())
}
fn typed<'a>(
    pair: &'a [OnceLock<Kernel>; 2],
    device: &wgpu::Device,
    label: &str,
    source: &str,
    bindings: &[Binding],
    index: usize,
) -> Result<&'a Kernel, KernelError> {
    if index == 0 {
        lazy_kernel(&pair[0], || {
            Kernel::new(device, label, source, "main", bindings)
        })
    } else {
        lazy_kernel(&pair[1], || {
            Kernel::new(
                device,
                label,
                &source.replace("alias Value = f32;", "alias Value = u32;"),
                "main",
                bindings,
            )
        })
    }
}
impl TensorIndexKernels {
    fn new() -> Self {
        Self {
            copy: std::array::from_fn(|_| OnceLock::new()),
            compare: std::array::from_fn(|_| OnceLock::new()),
            select: std::array::from_fn(|_| OnceLock::new()),
            scan_blocks: std::array::from_fn(|_| OnceLock::new()),
            scan_add: std::array::from_fn(|_| OnceLock::new()),
            gather: std::array::from_fn(|_| OnceLock::new()),
            compact: std::array::from_fn(|_| OnceLock::new()),
            index_count: OnceLock::new(),
            count_reset: OnceLock::new(),
        }
    }

    pub(crate) fn copy(&self, device: &wgpu::Device, index: usize) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        typed(
            &self.copy,
            device,
            "tensor copy",
            shaders::TENSOR_COPY_WGSL,
            &[R, R, W],
            index,
        )
    }

    pub(crate) fn compare(
        &self,
        device: &wgpu::Device,
        index: usize,
    ) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        typed(
            &self.compare,
            device,
            "tensor compare",
            shaders::TENSOR_COMPARE_WGSL,
            &[R, R, R, W],
            index,
        )
    }

    pub(crate) fn select(
        &self,
        device: &wgpu::Device,
        index: usize,
    ) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        typed(
            &self.select,
            device,
            "tensor select",
            shaders::TENSOR_SELECT_WGSL,
            &[R, R, R, R, W],
            index,
        )
    }

    pub(crate) fn scan_blocks(
        &self,
        device: &wgpu::Device,
        index: usize,
    ) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        typed(
            &self.scan_blocks,
            device,
            "tensor scan blocks",
            shaders::TENSOR_SCAN_BLOCKS_WGSL,
            &[R, R, W, W],
            index,
        )
    }

    pub(crate) fn scan_add(
        &self,
        device: &wgpu::Device,
        index: usize,
    ) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        typed(
            &self.scan_add,
            device,
            "tensor scan offsets",
            shaders::TENSOR_SCAN_ADD_WGSL,
            &[R, R, W],
            index,
        )
    }

    pub(crate) fn gather(
        &self,
        device: &wgpu::Device,
        index: usize,
    ) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        typed(
            &self.gather,
            device,
            "tensor gather",
            shaders::TENSOR_GATHER_WGSL,
            &[R, R, R, W],
            index,
        )
    }

    pub(crate) fn compact(
        &self,
        device: &wgpu::Device,
        index: usize,
    ) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        typed(
            &self.compact,
            device,
            "tensor compact",
            shaders::TENSOR_COMPACT_WGSL,
            &[R, R, R, R, R, W, W],
            index,
        )
    }

    pub(crate) fn index_count(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.index_count, || {
            Kernel::new(
                device,
                "tensor invalid index count",
                shaders::TENSOR_INDEX_COUNT_WGSL,
                "main",
                &[R, R, W],
            )
        })
    }

    pub(crate) fn count_reset(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.count_reset, || {
            Kernel::new(
                device,
                "tensor count reset",
                shaders::TENSOR_COUNT_RESET_WGSL,
                "main",
                &[R, W],
            )
        })
    }
}
impl ComputeRuntime {
    pub(crate) fn tensor_index_kernels(&self) -> Result<&TensorIndexKernels, KernelError> {
        Ok(self.tensor_index.get_or_init(TensorIndexKernels::new))
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
