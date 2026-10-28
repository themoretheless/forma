use super::{Binding, Kernel, KernelError};
use gpu_compute::block_on;

impl Kernel {
    pub fn binding_info(&self) -> &[BindingInfo] {
        &self.binding_info
    }

    /// Validates context identity, usage, alignment, minimum WGSL size and write
    /// aliases before constructing a bind group. Requires `from_context` or a
    /// `KernelCache`; raw-device constructors cannot prove context identity.
    pub fn bind(
        &self,
        context: &gpu_compute::GpuContext,
        views: &[gpu_compute::GpuBufferView<'_>],
    ) -> Result<wgpu::BindGroup, KernelBindingError> {
        let owner = self
            .context
            .as_ref()
            .ok_or(KernelBindingError::UntrackedDevice)?;
        if !owner.same_device(context) {
            return Err(KernelBindingError::Buffer(
                gpu_compute::BufferError::ForeignDevice,
            ));
        }
        if views.len() != self.bindings.len() {
            return Err(KernelBindingError::Count {
                expected: self.bindings.len(),
                actual: views.len(),
            });
        }
        let limits = context.device.limits();
        let mut resources = Vec::with_capacity(views.len());
        for (index, (view, info)) in views.iter().zip(&self.binding_info).enumerate() {
            let (usage, alignment, maximum) = if info.kind == Binding::Uniform {
                (
                    wgpu::BufferUsages::UNIFORM,
                    limits.min_uniform_buffer_offset_alignment,
                    limits.max_uniform_buffer_binding_size,
                )
            } else {
                (
                    wgpu::BufferUsages::STORAGE,
                    limits.min_storage_buffer_offset_alignment,
                    limits.max_storage_buffer_binding_size,
                )
            };
            view.validate(context, usage)
                .map_err(KernelBindingError::Buffer)?;
            if !view.offset().is_multiple_of(u64::from(alignment)) {
                return Err(KernelBindingError::Unaligned { binding: index });
            }
            if view.is_empty() || view.len_bytes() < info.min_size || view.len_bytes() > maximum {
                return Err(KernelBindingError::Size {
                    binding: index,
                    bytes: view.len_bytes(),
                    minimum: info.min_size.max(1),
                    maximum,
                });
            }
            for (other_index, other) in views[..index].iter().enumerate() {
                if view.raw() == other.raw()
                    && (info.kind == Binding::StorageReadWrite
                        || self.bindings[other_index] == Binding::StorageReadWrite)
                {
                    return Err(KernelBindingError::AliasedStorage {
                        first: other_index,
                        second: index,
                    });
                }
            }
            resources.push(wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: view.raw(),
                offset: view.offset(),
                size: std::num::NonZeroU64::new(view.len_bytes()),
            }));
        }
        let oom = context
            .device
            .push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = context.device.push_error_scope(wgpu::ErrorFilter::Internal);
        let scope = context
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let result = self.create_bind_group_resources(&context.device, &resources);
        let errors = [
            block_on(scope.pop()),
            block_on(internal.pop()),
            block_on(oom.pop()),
        ];
        if let Some(error) = errors.into_iter().flatten().next() {
            return Err(KernelBindingError::Device(error.to_string()));
        }
        Ok(result)
    }
}

/// Reflected requirements of one slot in the declared group-zero layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingInfo {
    pub kind: Binding,
    /// Includes one element for a runtime-sized trailing array; unused slots are zero.
    pub min_size: u64,
}

#[derive(Debug)]
pub enum KernelBindingError {
    UntrackedDevice,
    Buffer(gpu_compute::BufferError),
    Count {
        expected: usize,
        actual: usize,
    },
    Size {
        binding: usize,
        bytes: u64,
        minimum: u64,
        maximum: u64,
    },
    Unaligned {
        binding: usize,
    },
    AliasedStorage {
        first: usize,
        second: usize,
    },
    Device(String),
}
impl std::fmt::Display for KernelBindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "kernel binding failed: {self:?}")
    }
}
impl std::error::Error for KernelBindingError {}

pub(super) fn reflect(
    source: &str,
    entry: &str,
    bindings: &[Binding],
) -> Result<(u32, Vec<BindingInfo>), KernelError> {
    let fail = |message: String| KernelError { message };
    let module =
        naga::front::wgsl::parse_str(source).map_err(|e| fail(e.emit_to_string(source)))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| fail(e.to_string()))?;
    let (entry_index, point) = module
        .entry_points
        .iter()
        .enumerate()
        .find(|(_, p)| p.stage == naga::ShaderStage::Compute && p.name == entry)
        .ok_or_else(|| fail(format!("compute entry point {entry:?} not found")))?;
    if point.workgroup_size_overrides.is_some()
        || point.workgroup_size[1..] != [1, 1]
        || point.workgroup_size[0] == 0
    {
        return Err(fail(
            "Kernel requires a fixed one-dimensional workgroup size".into(),
        ));
    }
    let mut layout = naga::proc::Layouter::default();
    layout
        .update(module.to_ctx())
        .map_err(|e| fail(e.to_string()))?;
    let mut reflected: Vec<_> = bindings
        .iter()
        .map(|&kind| BindingInfo { kind, min_size: 0 })
        .collect();
    for (handle, global) in module.global_variables.iter() {
        let Some(slot) = global.binding.as_ref() else {
            continue;
        };
        if info.get_entry_point(entry_index)[handle].is_empty() {
            continue;
        }
        let expected = match global.space {
            naga::AddressSpace::Uniform => Binding::Uniform,
            naga::AddressSpace::Storage { access }
                if access.contains(naga::StorageAccess::STORE) =>
            {
                Binding::StorageReadWrite
            }
            naga::AddressSpace::Storage { .. } => Binding::StorageRead,
            _ => return Err(fail("Kernel supports uniform/storage buffers only".into())),
        };
        if slot.group != 0 || bindings.get(slot.binding as usize) != Some(&expected) {
            return Err(fail(format!(
                "shader @group({}) @binding({}) requires {expected:?}",
                slot.group, slot.binding
            )));
        }
        reflected[slot.binding as usize].min_size = u64::from(layout[global.ty].size);
    }
    Ok((point.workgroup_size[0], reflected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_validates_entry_point_and_used_buffer_contract_without_gpu() {
        let layout = [
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ];
        let source = crate::shaders::SCALE_ADD_WGSL;
        let (size, slots) = reflect(source, "main", &layout).unwrap();
        assert_eq!(size, 256);
        assert_eq!(
            slots.iter().map(|s| s.min_size).collect::<Vec<_>>(),
            [16, 4, 4]
        );
        assert!(reflect(source, "absent", &layout).is_err());
        assert!(reflect(source, "main", &[Binding::Uniform]).is_err());
        assert!(reflect(source, "main", &[Binding::StorageRead; 3]).is_err());
        assert!(reflect("@compute @workgroup_size(8, 8) fn main() {}", "main", &[]).is_err());
        let fixed = source.replace("const WG: u32 = 256;", "const WG: u32 = 96;");
        assert_eq!(reflect(&fixed, "main", &layout).unwrap().0, 96);
    }
}
