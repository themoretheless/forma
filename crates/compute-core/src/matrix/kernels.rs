use crate::{Binding, Kernel, KernelError, shaders, wgpu};

pub(crate) struct MatmulKernels {
    tiled: Kernel,
    aligned: Kernel,
    direct: Kernel,
    dot: Kernel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strategy {
    Tiled,
    Aligned,
    Direct,
    Dot,
}

/// Local Metal measurements justify only these bounded substitutions. Other
/// backends retain the general tile kernel until they have their own evidence.
fn strategy(backend: wgpu::Backend, rows: u32, inner: u32, columns: u32) -> Strategy {
    if backend != wgpu::Backend::Metal {
        return Strategy::Tiled;
    }
    let output = u64::from(rows) * u64::from(columns);
    if output <= 1024 && inner >= 256 {
        Strategy::Dot
    } else if inner != 0
        && rows.is_multiple_of(32)
        && inner.is_multiple_of(32)
        && columns.is_multiple_of(32)
        && (output >= 16384 || inner >= 128)
    {
        Strategy::Aligned
    } else if (output <= 32768 && inner <= 512) || (rows.min(columns) <= 16 && inner <= 64) {
        Strategy::Direct
    } else {
        Strategy::Tiled
    }
}

impl MatmulKernels {
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self, KernelError> {
        let bindings = [
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ];
        Ok(Self {
            tiled: Kernel::new(
                device,
                "general tiled matrix product",
                shaders::MATMUL_WGSL,
                "main",
                &bindings,
            )?,
            aligned: Kernel::new(
                device,
                "aligned vector matrix product",
                shaders::MATMUL_ALIGNED_WGSL,
                "main",
                &bindings,
            )?,
            direct: Kernel::new(
                device,
                "direct matrix product",
                shaders::MATMUL_DIRECT_WGSL,
                "main",
                &bindings,
            )?,
            dot: Kernel::new(
                device,
                "cooperative matrix inner reduction",
                shaders::MATMUL_DOT_WGSL,
                "main",
                &bindings,
            )?,
        })
    }

    pub(crate) fn select(
        &self,
        backend: wgpu::Backend,
        rows: u32,
        inner: u32,
        columns: u32,
    ) -> (&Kernel, u32) {
        match strategy(backend, rows, inner, columns) {
            Strategy::Tiled => (&self.tiled, rows.div_ceil(32) * columns.div_ceil(32)),
            Strategy::Aligned => (&self.aligned, (rows / 32) * (columns / 32)),
            Strategy::Direct => (&self.direct, self.direct.workgroup_count(rows * columns)),
            Strategy::Dot => (&self.dot, rows * columns),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Strategy, strategy};
    use crate::wgpu::Backend;

    #[test]
    fn measured_policy_respects_alignment_bounds_and_regression_cases() {
        for (shape, expected) in [
            ((8, 2048, 8), Strategy::Dot),
            ((32, 256, 32), Strategy::Dot),
            ((33, 256, 32), Strategy::Direct),
            ((256, 256, 256), Strategy::Aligned),
            ((512, 512, 512), Strategy::Aligned),
            ((64, 64, 64), Strategy::Direct),
            ((127, 259, 193), Strategy::Direct),
            ((65537, 1, 1), Strategy::Direct),
            ((511, 63, 257), Strategy::Tiled),
            ((16, 512, 4096), Strategy::Tiled),
            ((4096, 0, 32), Strategy::Tiled),
            ((17, 0, 19), Strategy::Direct),
        ] {
            let (m, k, n) = shape;
            assert_eq!(
                strategy(Backend::Metal, m, k, n),
                expected,
                "shape {shape:?}"
            );
            assert_eq!(strategy(Backend::Vulkan, m, k, n), Strategy::Tiled);
        }
    }
}
