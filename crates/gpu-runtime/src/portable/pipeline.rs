//! Portable shader compilation, pipeline creation, bind groups, textures, and queries.

use crate::portable::{PortableDevice, convert, pop_scope_error};
use crate::{
    Backend, BindGroup, BindGroupDescriptor, BindGroupLayout, BindGroupLayoutDescriptor,
    BindingResource, ComputePipeline, ComputePipelineDescriptor, Error, ErrorFilter, Extent3d,
    PipelineLayout, PipelineLayoutDescriptor, QuerySet, QuerySetDescriptor, RenderPipeline,
    RenderPipelineDescriptor, Sampler, SamplerDescriptor, ShaderModule, TexelCopyBufferLayout,
    TexelCopyTextureInfo, Texture, TextureDescriptor, TextureViewDescriptor, validation_error,
};
use gpu_shader as naga;
use gpu_shader::ShaderStage;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub(crate) fn validate_backend_shader(
    backend: Backend,
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
) -> Result<(), String> {
    match backend {
        Backend::Vulkan => {
            naga::back::spv::write_vec(module, info, &Default::default(), &Default::default())
                .map(|_| ())
        }
        Backend::Dx12 => {
            naga::back::hlsl::write_string(module, info, &Default::default(), &Default::default())
                .map(|_| ())
        }
        Backend::Gl => {
            naga::back::glsl::write_all_entry_points(module, info, &Default::default()).map(|_| ())
        }
        Backend::Metal | Backend::BrowserWebGpu | Backend::Noop | Backend::Cuda => Ok(()),
    }
}

pub(crate) fn create_shader_module(
    portable: &PortableDevice,
    label: crate::Label<'_>,
    wgsl_src: &str,
    module: Arc<naga::Module>,
    info: &naga::valid::ModuleInfo,
    record_error: &dyn Fn(Error),
) -> ShaderModule {
    if let Err(reason) = validate_backend_shader(portable.backend, &module, info) {
        record_error(validation_error(reason.clone()));
        return ShaderModule::invalid_with_module(module, reason);
    }

    let _dx12_guard = super::lock_dx12_if(portable.backend == Backend::Dx12);
    let scope = portable.push_inner_scope(ErrorFilter::Validation);
    portable
        .portable_shader_compilations
        .fetch_add(1, Ordering::Relaxed);
    let raw = portable
        .device
        .create_shader_module(upstream_wgpu::ShaderModuleDescriptor {
            label,
            source: upstream_wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(wgsl_src)),
        });
    if let Some(err) = pop_scope_error(scope) {
        let description = err.to_string();
        record_error(err);
        return ShaderModule::invalid_with_module(module, description);
    }
    ShaderModule::from_portable(module, raw)
}

pub(crate) fn create_bind_group_layout(
    portable: &PortableDevice,
    desc: &BindGroupLayoutDescriptor<'_>,
) -> BindGroupLayout {
    let entries: Vec<upstream_wgpu::BindGroupLayoutEntry> = desc
        .entries
        .iter()
        .map(convert::to_wgpu_bind_group_layout_entry)
        .collect();
    let raw = portable
        .device
        .create_bind_group_layout(&upstream_wgpu::BindGroupLayoutDescriptor {
            label: desc.label,
            entries: &entries,
        });
    BindGroupLayout {
        portable: Some(raw),
    }
}

pub(crate) fn create_pipeline_layout(
    portable: &PortableDevice,
    desc: &PipelineLayoutDescriptor<'_>,
) -> PipelineLayout {
    let layouts: Vec<Option<&upstream_wgpu::BindGroupLayout>> = desc
        .bind_group_layouts
        .iter()
        .map(|slot| slot.and_then(|l| l.portable.as_ref()))
        .collect();
    let raw = portable
        .device
        .create_pipeline_layout(&upstream_wgpu::PipelineLayoutDescriptor {
            label: desc.label,
            bind_group_layouts: &layouts,
            immediate_size: desc.immediate_size,
        });
    PipelineLayout {
        portable: Some(raw),
    }
}

pub(crate) fn create_bind_group(
    portable: &PortableDevice,
    desc: &BindGroupDescriptor<'_>,
) -> BindGroup {
    let Some(layout) = desc.layout.portable.as_ref() else {
        return BindGroup::empty();
    };
    let mut entries = Vec::with_capacity(desc.entries.len());
    for entry in desc.entries {
        let resource = match &entry.resource {
            BindingResource::Buffer(binding) => {
                let Some(buf) = binding.buffer.inner.portable.as_ref() else {
                    return BindGroup::empty();
                };
                upstream_wgpu::BindingResource::Buffer(upstream_wgpu::BufferBinding {
                    buffer: &buf.raw,
                    offset: binding.offset,
                    size: binding.size,
                })
            }
            BindingResource::Sampler(sampler) => {
                let Some(raw) = sampler.portable.as_ref() else {
                    return BindGroup::empty();
                };
                upstream_wgpu::BindingResource::Sampler(raw)
            }
            BindingResource::TextureView(view) => {
                let Some(raw) = view.portable.as_ref() else {
                    return BindGroup::empty();
                };
                upstream_wgpu::BindingResource::TextureView(raw)
            }
        };
        entries.push(upstream_wgpu::BindGroupEntry {
            binding: entry.binding,
            resource,
        });
    }
    let raw = portable
        .device
        .create_bind_group(&upstream_wgpu::BindGroupDescriptor {
            label: desc.label,
            layout,
            entries: &entries,
        });
    BindGroup::from_portable(raw)
}

fn find_entry_point<'a>(
    module: &'a naga::Module,
    stage: ShaderStage,
    requested: Option<&str>,
) -> Result<&'a naga::EntryPoint, String> {
    if let Some(name) = requested {
        module
            .entry_points
            .iter()
            .find(|ep| ep.stage == stage && ep.name == name)
            .ok_or_else(|| format!("{stage} pipeline entry point '{name}' not found"))
    } else {
        let mut iter = module.entry_points.iter().filter(|ep| ep.stage == stage);
        let first = iter
            .next()
            .ok_or_else(|| format!("no {stage} entry point in shader module"))?;
        if iter.next().is_some() {
            return Err(format!(
                "multiple {stage} entry points in shader module; specify entry_point"
            ));
        }
        Ok(first)
    }
}

pub(crate) fn create_compute_pipeline(
    portable: &PortableDevice,
    desc: &ComputePipelineDescriptor<'_>,
    report_invalid: &dyn Fn(String),
) -> ComputePipeline {
    let ep = match find_entry_point(&desc.module.module, ShaderStage::Compute, desc.entry_point) {
        Ok(ep) => ep,
        Err(msg) => {
            report_invalid(format!("compute pipeline compilation failed: {msg}"));
            return ComputePipeline::invalid(desc.module);
        }
    };
    let Some(shader) = desc.module.portable.as_ref() else {
        report_invalid("compute pipeline missing portable shader module".to_string());
        return ComputePipeline::invalid(desc.module);
    };
    let _dx12_guard = super::lock_dx12_if(portable.backend == Backend::Dx12);
    let scope = portable.push_inner_scope(ErrorFilter::Validation);
    let raw = portable
        .device
        .create_compute_pipeline(&upstream_wgpu::ComputePipelineDescriptor {
            label: desc.label,
            layout: desc.layout.and_then(|l| l.portable.as_ref()),
            module: shader,
            entry_point: Some(ep.name.as_str()),
            compilation_options: upstream_wgpu::PipelineCompilationOptions {
                constants: desc.compilation_options.constants,
                zero_initialize_workgroup_memory: desc
                    .compilation_options
                    .zero_initialize_workgroup_memory,
            },
            cache: None,
        });
    if let Some(err) = pop_scope_error(scope) {
        report_invalid(format!("compute pipeline compilation failed: {err}"));
        return ComputePipeline::invalid(desc.module);
    }
    ComputePipeline::from_portable(desc.module, raw, ep.workgroup_size)
}

pub(crate) fn create_render_pipeline(
    portable: &PortableDevice,
    desc: &RenderPipelineDescriptor<'_>,
    report_invalid: &dyn Fn(String),
) -> RenderPipeline {
    let vs_ep = match find_entry_point(
        &desc.vertex.module.module,
        ShaderStage::Vertex,
        desc.vertex.entry_point,
    ) {
        Ok(ep) => ep,
        Err(msg) => {
            report_invalid(format!("render pipeline compilation failed: {msg}"));
            return RenderPipeline::invalid(desc);
        }
    };
    let Some(vs_module) = desc.vertex.module.portable.as_ref() else {
        report_invalid("render pipeline missing portable vertex shader module".to_string());
        return RenderPipeline::invalid(desc);
    };

    let fs_ep_name: Option<String>;
    let fs_module: Option<&upstream_wgpu::ShaderModule>;
    let mut wgpu_targets = Vec::new();
    if let Some(fs) = &desc.fragment {
        let ep = match find_entry_point(&fs.module.module, ShaderStage::Fragment, fs.entry_point) {
            Ok(ep) => ep,
            Err(msg) => {
                report_invalid(format!("render pipeline compilation failed: {msg}"));
                return RenderPipeline::invalid(desc);
            }
        };
        let Some(module) = fs.module.portable.as_ref() else {
            report_invalid("render pipeline missing portable fragment shader module".to_string());
            return RenderPipeline::invalid(desc);
        };
        fs_ep_name = Some(ep.name.clone());
        fs_module = Some(module);
        for t in fs.targets {
            wgpu_targets.push(t.as_ref().map(|target| upstream_wgpu::ColorTargetState {
                format: convert::to_wgpu_texture_format(target.format),
                blend: target.blend.map(convert::to_wgpu_blend_state),
                write_mask: convert::to_wgpu_color_writes(target.write_mask),
            }));
        }
    } else {
        fs_ep_name = None;
        fs_module = None;
    }

    let attr_storage: Vec<Option<Vec<upstream_wgpu::VertexAttribute>>> = desc
        .vertex
        .buffers
        .iter()
        .map(|slot| {
            slot.as_ref().map(|layout| {
                layout
                    .attributes
                    .iter()
                    .copied()
                    .map(convert::to_wgpu_vertex_attribute)
                    .collect()
            })
        })
        .collect();
    let vertex_buffers: Vec<Option<upstream_wgpu::VertexBufferLayout<'_>>> = desc
        .vertex
        .buffers
        .iter()
        .zip(attr_storage.iter())
        .map(|(slot, attrs)| match (slot, attrs) {
            (Some(layout), Some(attrs)) => Some(upstream_wgpu::VertexBufferLayout {
                array_stride: layout.array_stride,
                step_mode: convert::to_wgpu_step_mode(layout.step_mode),
                attributes: attrs,
            }),
            _ => None,
        })
        .collect();

    let _dx12_guard = super::lock_dx12_if(portable.backend == Backend::Dx12);
    let scope = portable.push_inner_scope(ErrorFilter::Validation);
    let raw = portable
        .device
        .create_render_pipeline(&upstream_wgpu::RenderPipelineDescriptor {
            label: desc.label,
            layout: desc.layout.and_then(|l| l.portable.as_ref()),
            vertex: upstream_wgpu::VertexState {
                module: vs_module,
                entry_point: Some(vs_ep.name.as_str()),
                compilation_options: upstream_wgpu::PipelineCompilationOptions {
                    constants: desc.vertex.compilation_options.constants,
                    zero_initialize_workgroup_memory: desc
                        .vertex
                        .compilation_options
                        .zero_initialize_workgroup_memory,
                },
                buffers: &vertex_buffers,
            },
            primitive: convert::to_wgpu_primitive_state(desc.primitive),
            depth_stencil: desc.depth_stencil.map(convert::to_wgpu_depth_stencil_state),
            multisample: upstream_wgpu::MultisampleState {
                count: desc.multisample.count,
                mask: desc.multisample.mask,
                alpha_to_coverage_enabled: desc.multisample.alpha_to_coverage_enabled,
            },
            fragment: desc.fragment.as_ref().zip(fs_module).map(|(fs, module)| {
                upstream_wgpu::FragmentState {
                    module,
                    entry_point: fs_ep_name.as_deref(),
                    compilation_options: upstream_wgpu::PipelineCompilationOptions {
                        constants: fs.compilation_options.constants,
                        zero_initialize_workgroup_memory: fs
                            .compilation_options
                            .zero_initialize_workgroup_memory,
                    },
                    targets: &wgpu_targets,
                }
            }),
            multiview_mask: desc.multiview_mask,
            cache: None,
        });
    if let Some(err) = pop_scope_error(scope) {
        report_invalid(format!("render pipeline compilation failed: {err}"));
        return RenderPipeline::invalid(desc);
    }
    RenderPipeline::from_portable(desc, raw)
}

pub(crate) fn create_texture(portable: &PortableDevice, desc: &TextureDescriptor<'_>) -> Texture {
    let view_formats: Vec<upstream_wgpu::TextureFormat> = desc
        .view_formats
        .iter()
        .copied()
        .map(convert::to_wgpu_texture_format)
        .collect();
    let raw = portable
        .device
        .create_texture(&upstream_wgpu::TextureDescriptor {
            label: desc.label,
            size: upstream_wgpu::Extent3d {
                width: desc.size.width,
                height: desc.size.height,
                depth_or_array_layers: desc.size.depth_or_array_layers,
            },
            mip_level_count: desc.mip_level_count.max(1),
            sample_count: desc.sample_count.max(1),
            dimension: convert::to_wgpu_texture_dimension(desc.dimension),
            format: convert::to_wgpu_texture_format(desc.format),
            usage: convert::to_wgpu_texture_usages(desc.usage),
            view_formats: &view_formats,
        });
    Texture::from_portable(raw, desc)
}

pub(crate) fn create_texture_view(
    texture: &Texture,
    desc: &TextureViewDescriptor<'_>,
) -> Option<upstream_wgpu::TextureView> {
    let raw = texture.portable.as_ref()?;
    Some(raw.create_view(&upstream_wgpu::TextureViewDescriptor {
        label: desc.label,
        format: desc.format.map(convert::to_wgpu_texture_format),
        dimension: desc.dimension.map(convert::to_wgpu_view_dimension),
        usage: desc.usage.map(convert::to_wgpu_texture_usages),
        aspect: convert::to_wgpu_texture_aspect(desc.aspect),
        base_mip_level: desc.base_mip_level,
        mip_level_count: desc.mip_level_count,
        base_array_layer: desc.base_array_layer,
        array_layer_count: desc.array_layer_count,
    }))
}

pub(crate) fn create_sampler(portable: &PortableDevice, desc: &SamplerDescriptor<'_>) -> Sampler {
    let raw = portable
        .device
        .create_sampler(&upstream_wgpu::SamplerDescriptor {
            label: desc.label,
            address_mode_u: convert::to_wgpu_address_mode(desc.address_mode_u),
            address_mode_v: convert::to_wgpu_address_mode(desc.address_mode_v),
            address_mode_w: convert::to_wgpu_address_mode(desc.address_mode_w),
            mag_filter: convert::to_wgpu_filter_mode(desc.mag_filter),
            min_filter: convert::to_wgpu_filter_mode(desc.min_filter),
            mipmap_filter: convert::to_wgpu_mipmap_filter_mode(desc.mipmap_filter),
            lod_min_clamp: desc.lod_min_clamp,
            lod_max_clamp: desc.lod_max_clamp,
            compare: desc.compare.map(convert::to_wgpu_compare_function),
            anisotropy_clamp: desc.anisotropy_clamp,
            border_color: None,
        });
    Sampler::from_portable(raw)
}

pub(crate) fn create_query_set(
    portable: &PortableDevice,
    desc: &QuerySetDescriptor<'_>,
) -> QuerySet {
    let raw = portable
        .device
        .create_query_set(&upstream_wgpu::QuerySetDescriptor {
            label: desc.label,
            ty: convert::to_wgpu_query_type(desc.ty),
            count: desc.count,
        });
    QuerySet::from_portable(raw, desc.count)
}

pub(crate) fn write_texture(
    portable: &PortableDevice,
    texture: TexelCopyTextureInfo<'_>,
    data: &[u8],
    data_layout: TexelCopyBufferLayout,
    size: Extent3d,
) {
    let Some(raw) = texture.texture.portable.as_ref() else {
        return;
    };
    portable.queue.write_texture(
        upstream_wgpu::TexelCopyTextureInfo {
            texture: raw,
            mip_level: texture.mip_level,
            origin: upstream_wgpu::Origin3d {
                x: texture.origin.x,
                y: texture.origin.y,
                z: texture.origin.z,
            },
            aspect: convert::to_wgpu_texture_aspect(texture.aspect),
        },
        data,
        upstream_wgpu::TexelCopyBufferLayout {
            offset: data_layout.offset,
            bytes_per_row: data_layout.bytes_per_row,
            rows_per_image: data_layout.rows_per_image,
        },
        upstream_wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: size.depth_or_array_layers,
        },
    );
}
