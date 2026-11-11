//! Orthographic neutral-material triangle rendering, separate from raster color management.
use crate::pipeline::{
    DEPTH_FORMAT, PipelineSpec, color_target, depth_test, render_pipeline, wgsl_module,
};
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

/// Centres source coordinates on their bounding box and scales them into the unit sphere,
/// in float64 so huge or tiny source units survive until the final float32 conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ModelFit {
    min: [f64; 3],
    /// Rebase at half scale when a full extent overflows f64.
    halve: bool,
    scale: f64,
    center: [f64; 3],
    radius: f64,
}

impl ModelFit {
    /// Bounds exactly `count` finite vertices, checking `cancelled` every 1024 triangles.
    /// Returns `None` for empty geometry.
    fn measure<T: Copy + Into<f64>>(
        triangles: impl Iterator<Item = [[T; 3]; 3]>,
        count: usize,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<Self>, ModelUploadError> {
        let mut visited = 0;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for (index, triangle) in triangles.enumerate() {
            if index % 1024 == 0 && cancelled() {
                return Err(ModelUploadError::Cancelled);
            }
            for source_vertex in triangle {
                let vertex: [f64; 3] = source_vertex.map(Into::into);
                if visited == count {
                    return Err(ModelUploadError::GeometryLimit);
                }
                for axis in 0..3 {
                    if !vertex[axis].is_finite() {
                        return Err(ModelUploadError::InvalidCoordinates);
                    }
                    min[axis] = min[axis].min(vertex[axis]);
                    max[axis] = max[axis].max(vertex[axis]);
                }
                visited += 1;
            }
        }
        if visited != count {
            return Err(ModelUploadError::GeometryLimit);
        }
        if count == 0 {
            return Ok(None);
        }
        // Rebase in scaled local coordinates. This avoids both overflowing
        // opposite-sign f64 bounds and underflowing half-extents near zero.
        let halve = (0..3).any(|axis| !(max[axis] - min[axis]).is_finite());
        let mut fit = Self {
            min,
            halve,
            scale: 1.,
            center: [0.; 3],
            radius: 1.,
        };
        let extents = std::array::from_fn::<_, 3, _>(|axis| fit.rebase(max[axis], axis));
        let scale = extents.into_iter().fold(0_f64, f64::max);
        fit.scale = if scale > 0. { scale } else { 1. };
        fit.center = extents.map(|v| v / fit.scale * 0.5);
        let radius = fit.center.into_iter().fold(0_f64, f64::hypot);
        fit.radius = if radius > 0. { radius } else { 1. };
        Ok(Some(fit))
    }

    fn rebase(&self, value: f64, axis: usize) -> f64 {
        if self.halve {
            value * 0.5 - self.min[axis] * 0.5
        } else {
            value - self.min[axis]
        }
    }

    /// The GPU position of a source vertex, or `None` when it is not finite.
    fn place(&self, vertex: [f64; 3]) -> Option<[f32; 3]> {
        let placed = std::array::from_fn::<_, 3, _>(|axis| {
            ((self.rebase(vertex[axis], axis) / self.scale - self.center[axis]) / self.radius)
                as f32
        });
        (vertex.iter().all(|v| v.is_finite()) && placed.iter().all(|v| v.is_finite()))
            .then_some(placed)
    }
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
        let shader = wgsl_module(
            device,
            "model shader",
            include_str!("../shaders/model_view.wgsl"),
        );
        let pipeline = render_pipeline(
            device,
            PipelineSpec {
                label: "model pipeline",
                layout: None,
                module: &shader,
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                })],
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth: Some(depth_test(true)),
                target: color_target(target, None),
            },
        );
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("model view"),
            contents: bytemuck::bytes_of(&Parameters {
                view: [1., 0., 0., 1.],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
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
        let Some(fit) = ModelFit::measure(triangles.clone(), count, &mut cancelled)? else {
            return Ok(PreparedModelUpload {
                owner: None,
                vertices: None,
                reservation: None,
                vertex_count: 0,
                budget: budget.cloned(),
            });
        };
        let reservation = budget.map(|budget| budget.try_reserve(bytes)).transpose()?;
        if cancelled() {
            return Err(ModelUploadError::Cancelled);
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model vertices"),
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
                    let normalized = fit
                        .place(source_vertex.map(Into::into))
                        .ok_or(ModelUploadError::InvalidCoordinates)?;
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
        if !crate::extent_fits(device, size)
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
            label: Some("model depth"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
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
            label: Some("model viewport"),
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
    use super::{ModelFit, ModelUploadError};
    use crate::test_support::validate_wgsl;
    use crate::wgpu;

    fn measure(triangles: &[[[f64; 3]; 3]]) -> Result<Option<ModelFit>, ModelUploadError> {
        ModelFit::measure(triangles.iter().copied(), triangles.len() * 3, &mut || {
            false
        })
    }

    #[test]
    fn fit_centres_and_scales_into_the_unit_sphere() {
        let fit = measure(&[[[10., 20., 30.], [12., 20., 30.], [10., 24., 31.]]])
            .unwrap()
            .unwrap();
        let corners = [[10., 20., 30.], [12., 24., 31.]].map(|v| fit.place(v).unwrap());
        // Opposite bounding-box corners land symmetric about the origin on the unit sphere.
        for (low, high) in corners[0].iter().zip(corners[1]) {
            assert!((low + high).abs() < 1e-6);
        }
        let length = corners[1].iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.).abs() < 1e-6);
    }

    #[test]
    fn fit_survives_extents_that_overflow_f64() {
        let fit = measure(&[[[-f64::MAX, 0., 0.], [f64::MAX, 0., 0.], [0., 1., 0.]]])
            .unwrap()
            .unwrap();
        assert!(fit.halve);
        let ends = [[-f64::MAX, 0., 0.], [f64::MAX, 0., 0.]].map(|v| fit.place(v).unwrap());
        assert!((ends[0][0] + 1.).abs() < 1e-6 && (ends[1][0] - 1.).abs() < 1e-6);
    }

    #[test]
    fn fit_rejects_invalid_counts_coordinates_and_cancellation() {
        assert_eq!(measure(&[]).unwrap(), None);
        assert!(matches!(
            measure(&[[[f64::NAN, 0., 0.], [0.; 3], [0.; 3]]]),
            Err(ModelUploadError::InvalidCoordinates)
        ));
        let one = [[[0.; 3]; 3]];
        assert!(matches!(
            ModelFit::measure(one.iter().copied(), 2, &mut || false),
            Err(ModelUploadError::GeometryLimit)
        ));
        assert!(matches!(
            ModelFit::measure(one.iter().copied(), 4, &mut || false),
            Err(ModelUploadError::GeometryLimit)
        ));
        assert!(matches!(
            ModelFit::measure(one.iter().copied(), 3, &mut || true),
            Err(ModelUploadError::Cancelled)
        ));
        let fit = measure(&one).unwrap().unwrap();
        assert_eq!(fit.place([0.; 3]), Some([0.; 3]));
        assert_eq!(fit.place([f64::INFINITY, 0., 0.]), None);
    }
    #[test]
    fn model_shader_validates() {
        validate_wgsl(
            include_str!("../shaders/model_view.wgsl"),
            wgpu::naga::valid::Capabilities::all(),
        );
    }
}
