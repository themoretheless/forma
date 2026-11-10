//! Orthographic neutral-material triangle rendering, separate from raster color management.
use crate::wgpu;
use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use wgpu::util::DeviceExt;

#[derive(Debug, Error)]
pub enum ModelUploadError {
    #[error("prepared model is not bound to this renderer")]
    RendererMismatch,
    #[error("prepared model uses an incompatible memory budget")]
    BudgetMismatch,
    #[error("model upload cancelled")]
    Cancelled,
    #[error(transparent)]
    Memory(#[from] memory_budget::BufferError),
    #[error("model exceeds GPU geometry budget")]
    GeometryLimit,
    #[error("model contains non-finite coordinates")]
    InvalidCoordinates,
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Parameters {
    view: [f32; 4],
}

/// Prepared geometry owns its GPU allocation and admission credit until installed or dropped.
/// Prepare and install with the same wgpu device.
#[derive(Debug)]
pub struct PreparedModelUpload {
    owner: Option<std::sync::Arc<()>>,
    vertices: Option<wgpu::Buffer>,
    reservation: Option<std::sync::Arc<memory_budget::Reservation>>,
    vertex_count: u32,
    budget: Option<memory_budget::MemoryBudget>,
}

/// Cloneable worker context bound to one renderer, device and memory budget.
#[derive(Debug, Clone)]
pub struct ModelUploadPreparer {
    device: wgpu::Device,
    budget: Option<memory_budget::MemoryBudget>,
    owner: std::sync::Arc<()>,
}
impl ModelUploadPreparer {
    pub fn prepare_with_cancel<T: Copy + Into<f64>>(
        &self,
        triangles: impl ExactSizeIterator<Item = [[T; 3]; 3]> + Clone,
        cancelled: impl FnMut() -> bool,
    ) -> Result<PreparedModelUpload, ModelUploadError> {
        let mut prepared = ModelRenderer::prepare_upload_with_cancel(
            &self.device,
            triangles,
            self.budget.as_ref(),
            cancelled,
        )?;
        prepared.owner = Some(self.owner.clone());
        Ok(prepared)
    }
}

#[derive(Debug)]
pub struct ModelRenderer {
    device: wgpu::Device,
    upload_owner: std::sync::Arc<()>,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertices: Option<wgpu::Buffer>,
    vertex_count: u32,
    depth: Option<(wgpu::Texture, wgpu::TextureView)>,
    // Resource handles drop before their logical admission credit is released.
    memory_budget: Option<memory_budget::MemoryBudget>,
    vertex_reservation: Option<std::sync::Arc<memory_budget::Reservation>>,
    depth_reservation: Option<std::sync::Arc<memory_budget::Reservation>>,
}
impl ModelRenderer {
    pub fn resource_lease(&self) -> crate::GpuResourceLease {
        crate::GpuResourceLease::new(
            self.vertex_reservation
                .iter()
                .cloned()
                .chain(self.depth_reservation.iter().cloned()),
        )
    }

    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        assert!(
            target.is_srgb() || target == wgpu::TextureFormat::Rgba16Float,
            "model target must be sRGB or linear RGBA16Float"
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Rrrah model shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/model_view.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Rrrah model layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Rrrah model pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    }],
                })],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Rrrah model view"),
            contents: bytemuck::bytes_of(&Parameters {
                view: [1., 0., 0., 1.],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            device: device.clone(),
            upload_owner: std::sync::Arc::new(()),
            pipeline,
            uniform,
            bind_group,
            vertices: None,
            vertex_count: 0,
            memory_budget: None,
            vertex_reservation: None,
            depth_reservation: None,
            depth: None,
        }
    }
    /// Caps owned vertex and Depth32F attachment footprints, including replacement overlap.
    /// Driver metadata, staging and in-flight command retention are excluded.
    pub fn new_with_budget(
        device: &wgpu::Device,
        target: wgpu::TextureFormat,
        budget: memory_budget::MemoryBudget,
    ) -> Self {
        let mut renderer = Self::new(device, target);
        renderer.memory_budget = Some(budget);
        renderer
    }

    /// Source units remain in the decoder. Upload coordinates are centered and
    /// fitted in float64 before converting centered local positions to GPU float32.
    pub fn upload<T: Copy + Into<f64>>(
        &mut self,
        device: &wgpu::Device,
        triangles: impl ExactSizeIterator<Item = [[T; 3]; 3]> + Clone,
    ) -> Result<(), ModelUploadError> {
        self.upload_with_cancel(device, triangles, || false)
    }

    pub fn upload_with_cancel<T: Copy + Into<f64>>(
        &mut self,
        device: &wgpu::Device,
        triangles: impl ExactSizeIterator<Item = [[T; 3]; 3]> + Clone,
        cancelled: impl FnMut() -> bool,
    ) -> Result<(), ModelUploadError> {
        if *device != self.device {
            return Err(ModelUploadError::RendererMismatch);
        }
        let prepared = self
            .upload_preparer()
            .prepare_with_cancel(triangles, cancelled)?;
        self.install_prepared_upload(prepared)?;
        Ok(())
    }

    /// May run on a worker. Dropping an obsolete result releases its allocation.
    pub fn prepare_upload_with_cancel<T: Copy + Into<f64>>(
        device: &wgpu::Device,
        triangles: impl ExactSizeIterator<Item = [[T; 3]; 3]> + Clone,
        budget: Option<&memory_budget::MemoryBudget>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<PreparedModelUpload, ModelUploadError> {
        if cancelled() {
            return Err(ModelUploadError::Cancelled);
        }
        let count = triangles
            .len()
            .checked_mul(3)
            .ok_or(ModelUploadError::GeometryLimit)?;
        let bytes = (count as u64)
            .checked_mul(12)
            .ok_or(ModelUploadError::GeometryLimit)?;
        if bytes > crate::MAX_EAGER_ATLAS_BYTES.min(device.limits().max_buffer_size)
            || count > u32::MAX as usize
        {
            return Err(ModelUploadError::GeometryLimit);
        }
        let mut visited = 0;
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for (index, triangle) in triangles.clone().enumerate() {
            if index % 1024 == 0 && cancelled() {
                return Err(ModelUploadError::Cancelled);
            }
            for source_vertex in triangle {
                let vertex = source_vertex.map(Into::into);
                if visited == count {
                    return Err(ModelUploadError::GeometryLimit);
                }
                for axis in 0..3 {
                    if !vertex[axis].is_finite() {
                        return Err(ModelUploadError::InvalidCoordinates);
                    }
                    bounds[0][axis] = bounds[0][axis].min(f64::from(vertex[axis]));
                    bounds[1][axis] = bounds[1][axis].max(f64::from(vertex[axis]));
                }
                visited += 1;
            }
        }
        if visited != count {
            return Err(ModelUploadError::GeometryLimit);
        }
        if count == 0 {
            return Ok(PreparedModelUpload {
                owner: None,
                vertices: None,
                reservation: None,
                vertex_count: 0,
                budget: budget.cloned(),
            });
        }
        // Rebase in scaled local coordinates. This avoids both overflowing
        // opposite-sign f64 bounds and underflowing half-extents near zero.
        let full_extents = std::array::from_fn::<_, 3, _>(|axis| bounds[1][axis] - bounds[0][axis]);
        let halve = full_extents.iter().any(|v| !v.is_finite());
        let difference = |value: f64, axis: usize| {
            if halve {
                value * 0.5 - bounds[0][axis] * 0.5
            } else {
                value - bounds[0][axis]
            }
        };
        let extents = std::array::from_fn::<_, 3, _>(|axis| difference(bounds[1][axis], axis));
        let scale = extents.into_iter().fold(0_f64, f64::max);
        let scale = if scale > 0. { scale } else { 1. };
        let center = extents.map(|v| v / scale * 0.5);
        let radius = center.into_iter().fold(0_f64, f64::hypot);
        let divisor = if radius > 0. { radius } else { 1. };
        let reservation = budget.map(|budget| budget.try_reserve(bytes)).transpose()?;
        if cancelled() {
            return Err(ModelUploadError::Cancelled);
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rrrah model vertices"),
            size: bytes,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        {
            let mut mapped = buffer
                .get_mapped_range_mut(..)
                .map_err(|_| ModelUploadError::GeometryLimit)?;
            let mut offset = 0;
            for (index, triangle) in triangles.enumerate() {
                if index % 1024 == 0 && cancelled() {
                    return Err(ModelUploadError::Cancelled);
                }
                for source_vertex in triangle {
                    let vertex = source_vertex.map(Into::into);
                    if !vertex.iter().all(|v| v.is_finite()) {
                        return Err(ModelUploadError::InvalidCoordinates);
                    }
                    let normalized = std::array::from_fn::<_, 3, _>(|axis| {
                        ((difference(vertex[axis], axis) / scale - center[axis]) / divisor) as f32
                    });
                    if !normalized.iter().all(|v| v.is_finite()) {
                        return Err(ModelUploadError::InvalidCoordinates);
                    }
                    if offset + 12 > mapped.len() {
                        return Err(ModelUploadError::GeometryLimit);
                    }
                    mapped
                        .slice(offset..offset + 12)
                        .copy_from_slice(bytemuck::bytes_of(&normalized));
                    offset += 12;
                }
            }
            if offset != mapped.len() {
                return Err(ModelUploadError::GeometryLimit);
            }
        }
        buffer.unmap();
        if cancelled() {
            return Err(ModelUploadError::Cancelled);
        }
        Ok(PreparedModelUpload {
            owner: None,
            vertices: Some(buffer),
            reservation: reservation.map(std::sync::Arc::new),
            vertex_count: count as u32,
            budget: budget.cloned(),
        })
    }

    pub fn upload_preparer(&self) -> ModelUploadPreparer {
        ModelUploadPreparer {
            device: self.device.clone(),
            budget: self.memory_budget.clone(),
            owner: self.upload_owner.clone(),
        }
    }

    /// Atomically replaces visible geometry after the caller accepts the result generation.
    pub fn install_prepared_upload(
        &mut self,
        prepared: PreparedModelUpload,
    ) -> Result<(), ModelUploadError> {
        if prepared.vertices.is_some()
            && self.memory_budget.as_ref().is_some_and(|expected| {
                !prepared
                    .budget
                    .as_ref()
                    .is_some_and(|actual| actual.is_descendant_of(expected))
            })
        {
            return Err(ModelUploadError::BudgetMismatch);
        }
        if !prepared
            .owner
            .as_ref()
            .is_some_and(|owner| std::sync::Arc::ptr_eq(owner, &self.upload_owner))
        {
            return Err(ModelUploadError::RendererMismatch);
        }
        self.vertices = prepared.vertices;
        self.vertex_reservation = prepared.reservation;
        self.vertex_count = prepared.vertex_count;
        Ok(())
    }
    pub fn clear_model(&mut self) {
        self.vertices = None;
        self.vertex_reservation = None;
        self.vertex_count = 0;
    }
    pub fn resident_bytes(&self) -> u64 {
        self.vertices.as_ref().map_or(0, wgpu::Buffer::size)
            + self
                .depth
                .as_ref()
                .map_or(0, |(t, _)| u64::from(t.width()) * u64::from(t.height()) * 4)
    }
    /// Call on viewport resize; unchanged dimensions reuse the depth attachment.
    /// Resources always use the renderer's device. The device argument remains
    /// for compatibility and cannot redirect allocation to another device.
    pub fn resize(
        &mut self,
        _device: &wgpu::Device,
        size: [u32; 2],
    ) -> Result<(), ModelUploadError> {
        let device = &self.device;
        if size.contains(&0)
            || size
                .iter()
                .any(|v| *v > device.limits().max_texture_dimension_2d)
            || u64::from(size[0]) * u64::from(size[1]) * 4 > crate::MAX_EAGER_ATLAS_BYTES
        {
            return Err(ModelUploadError::GeometryLimit);
        }
        if self
            .depth
            .as_ref()
            .is_some_and(|(t, _)| [t.width(), t.height()] == size)
        {
            return Ok(());
        }
        let reservation = self
            .memory_budget
            .as_ref()
            .map(|budget| budget.try_reserve(u64::from(size[0]) * u64::from(size[1]) * 4))
            .transpose()?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Rrrah model depth"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        self.depth = Some((texture, view));
        self.depth_reservation = reservation.map(std::sync::Arc::new);
        Ok(())
    }
    pub fn update_view(&self, queue: &wgpu::Queue, aspect: f32, yaw: f32, pitch: f32, zoom: f32) {
        let clean = |v: f32, default: f32| if v.is_finite() { v } else { default };
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&Parameters {
                view: [
                    clean(aspect, 1.).clamp(0.001, 1000.),
                    clean(yaw, 0.).rem_euclid(std::f32::consts::TAU),
                    clean(pitch, 0.).rem_euclid(std::f32::consts::TAU),
                    clean(zoom, 1.).clamp(0.02, 128.),
                ],
            }),
        );
    }
    /// The target dimensions must match the last successful resize.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some((_, depth)) = &self.depth else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Rrrah model viewport"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.018,
                        g: 0.018,
                        b: 0.018,
                        a: 1.,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(vertices) = &self.vertices {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(0..self.vertex_count, 0..1);
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::wgpu;
    #[test]
    fn model_shader_validates() {
        let module =
            wgpu::naga::front::wgsl::parse_str(include_str!("../shaders/model_view.wgsl")).unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
