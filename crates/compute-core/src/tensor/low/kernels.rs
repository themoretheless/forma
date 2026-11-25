use crate::runtime::lazy_kernel;
use crate::{Binding, ComputeRuntime, Kernel, KernelError, shaders, wgpu};
use std::sync::OnceLock;

pub(crate) struct LowKernels {
    pack: OnceLock<Kernel>,
    unpack: OnceLock<Kernel>,
    matmul: OnceLock<Kernel>,
    arithmetic: OnceLock<Kernel>,
    reduce: OnceLock<Kernel>,
    compare: OnceLock<Kernel>,
    select: OnceLock<Kernel>,
    gather: OnceLock<Kernel>,
    compact: OnceLock<Kernel>,
    scan: OnceLock<Kernel>,
    scatter: [OnceLock<Kernel>; 2],
}

impl LowKernels {
    fn new() -> Self {
        Self {
            pack: OnceLock::new(),
            unpack: OnceLock::new(),
            matmul: OnceLock::new(),
            arithmetic: OnceLock::new(),
            reduce: OnceLock::new(),
            compare: OnceLock::new(),
            select: OnceLock::new(),
            gather: OnceLock::new(),
            compact: OnceLock::new(),
            scan: OnceLock::new(),
            scatter: std::array::from_fn(|_| OnceLock::new()),
        }
    }

    pub(crate) fn pack(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.pack, || {
            Kernel::new(
                device,
                "packed tensor cast/copy",
                shaders::TENSOR_LOW_PACK_WGSL,
                "main",
                &[R, R, W],
            )
        })
    }

    pub(crate) fn unpack(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.unpack, || {
            Kernel::new(
                device,
                "packed tensor decode",
                shaders::TENSOR_LOW_UNPACK_WGSL,
                "main",
                &[R, R, W],
            )
        })
    }

    pub(crate) fn matmul(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.matmul, || {
            Kernel::new(
                device,
                "packed tensor matmul",
                shaders::TENSOR_LOW_MATMUL_WGSL,
                "main",
                &[R, R, R, W],
            )
        })
    }

    pub(crate) fn arithmetic(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.arithmetic, || {
            Kernel::new(
                device,
                "packed tensor arithmetic",
                shaders::TENSOR_LOW_ARITHMETIC_WGSL,
                "main",
                &[R, R, R, W],
            )
        })
    }

    pub(crate) fn reduce(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.reduce, || {
            Kernel::new(
                device,
                "packed tensor reductions",
                &low_reduce_source(),
                "main",
                &[R, R, W],
            )
        })
    }

    pub(crate) fn compare(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.compare, || {
            Kernel::new(
                device,
                "packed tensor compare",
                &super::index_sources::compare(),
                "main",
                &[R, R, R, W],
            )
        })
    }

    pub(crate) fn select(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.select, || {
            Kernel::new(
                device,
                "packed tensor select",
                &super::index_sources::select(),
                "main",
                &[R, R, R, R, W],
            )
        })
    }

    pub(crate) fn gather(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.gather, || {
            Kernel::new(
                device,
                "packed tensor gather",
                &super::index_sources::gather(),
                "main",
                &[R, R, R, W],
            )
        })
    }

    pub(crate) fn compact(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.compact, || {
            Kernel::new(
                device,
                "packed tensor compact",
                &super::index_sources::compact(),
                "main",
                &[R, R, R, R, R, W, W],
            )
        })
    }

    pub(crate) fn scan(&self, device: &wgpu::Device) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        lazy_kernel(&self.scan, || {
            Kernel::new(
                device,
                "packed tensor scan",
                &super::index_sources::scan(),
                "main",
                &[R, R, W, W],
            )
        })
    }

    pub(crate) fn scatter(
        &self,
        device: &wgpu::Device,
        packed: bool,
    ) -> Result<&Kernel, KernelError> {
        use Binding::{StorageRead as R, StorageReadWrite as W};
        if !packed {
            lazy_kernel(&self.scatter[0], || {
                Kernel::new(
                    device,
                    "packed updates to f32 scatter",
                    &super::scatter_source::source(false),
                    "main",
                    &[R, R, R, R, W],
                )
            })
        } else {
            lazy_kernel(&self.scatter[1], || {
                Kernel::new(
                    device,
                    "packed scatter output",
                    &super::scatter_source::source(true),
                    "main",
                    &[R, R, R, R, W],
                )
            })
        }
    }
}

fn low_reduce_source() -> String {
    let traversal = shaders::TENSOR_REDUCE_WGSL
        .replace("alias Value = f32;", "alias Value = u32;")
        .replace(
            "const MIN_IDENTITY: Value = 3.4028234663852886e38;",
            "const MIN_IDENTITY: Value = 0x7f7fffffu;",
        )
        .replace(
            "const MAX_IDENTITY: Value = -3.4028234663852886e38;",
            "const MAX_IDENTITY: Value = 0xff7fffffu;",
        )
        .replace("return Value(1);", "return 0x3f800000u;")
        .replace(
            "return a * b;",
            "return bitcast<u32>(bitcast<f32>(a) * bitcast<f32>(b));",
        )
        .replace("return min(a, b);", "return float_bits_min(a, b);")
        .replace("return max(a, b);", "return float_bits_max(a, b);")
        .replace(
            "return a + b;",
            "return bitcast<u32>(bitcast<f32>(a) + bitcast<f32>(b));",
        )
        .replace("decode(row, 10u,", "decode(row, 12u,")
        .replace(
            "input[base + decode(item, 10u + params[1] * 2u, params[4])]",
            "low_load_value(base + decode(item, 12u + params[1] * 2u, params[4]))",
        )
        .replace(
            "scratch[0] / Value(params[9])",
            "low_finish_value(scratch[0])",
        );
    format!(
        "{}\n{}\n{}\n{traversal}",
        include_str!("../../../shaders/low_codec.wgsl"),
        include_str!("../../../shaders/float_bits_order.wgsl"),
        include_str!("../../../shaders/tensor_low_reduce.wgsl")
    )
}

impl ComputeRuntime {
    pub(crate) fn low_kernels(&self) -> Result<&LowKernels, KernelError> {
        Ok(self.tensor_low.get_or_init(LowKernels::new))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn packed_reduction_specialization_validates() {
        let source = super::low_reduce_source();
        assert!(!source.contains("10u + params[1]"));
        assert!(!source.contains("Value(1)"));
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
