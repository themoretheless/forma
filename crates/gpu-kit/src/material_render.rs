//! Immutable material draws, separate from scene decoding and visible publication.
use crate::pipeline::{PipelineSpec, color_target, depth_test, render_pipeline, wgsl_module};
use crate::wgpu;
use crate::{GpuResourceLease, MaterialTexture, MaterialTextureUploader};
use bytemuck::{Pod, Zeroable};
use memory_budget::MemoryBudget;
use std::sync::{Arc, OnceLock};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MaterialVertex {
    pub position: [f32; 3],
    /// World normal, or zero for a flat derivative normal.
    pub normal: [f32; 3],
    /// World tangent + handedness; w=0 requests a derivative UV frame.
    pub tangent: [f32; 4],
    pub color: [f32; 4],
    /// Base color, metallic/roughness, normal, occlusion, emissive.
    pub texcoords: [[f32; 2]; 5],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MaterialParameters {
    pub clip_from_world: [[f32; 4]; 4],
    /// Camera xyz; w is neutral diffuse environment intensity.
    pub eye: [f32; 4],
    /// Direction toward light; w scales radiance.
    pub light_direction: [f32; 4],
    pub light_color: [f32; 4],
    pub base_color: [f32; 4],
    /// RGB emission; w=1 selects orthographic eye direction, w=0 perspective eye position.
    pub emissive: [f32; 4],
    /// Metallic, roughness, normal scale, occlusion strength.
    pub factors: [f32; 4],
    /// Alpha cutoff, alpha mode (set by prepare), normal-map flag, sidedness (set by prepare).
    pub alpha: [f32; 4],
}
impl Default for MaterialParameters {
    fn default() -> Self {
        Self {
            clip_from_world: [
                [1., 0., 0., 0.],
                [0., 1., 0., 0.],
                [0., 0., 1., 0.],
                [0., 0., 0., 1.],
            ],
            eye: [0., 0., 2., 0.03],
            light_direction: [0., 0., 1., 1.],
            light_color: [1.; 4],
            base_color: [1.; 4],
            emissive: [0.; 4],
            factors: [1., 1., 1., 1.],
            alpha: [0.5, 0., 0., 0.],
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum MaterialAlpha {
    Opaque,
    Mask,
    Blend,
}
/// Fixed-function state of a draw; each combination has its own lazily built pipeline.
#[derive(Debug, Clone, Copy)]
pub struct MaterialRaster {
    pub alpha: MaterialAlpha,
    pub double_sided: bool,
    /// Clockwise front faces, for mirrored scene transforms.
    pub clockwise: bool,
}
impl MaterialRaster {
    const COMBINATIONS: usize = 12;
    fn alpha_mode(self) -> usize {
        match self.alpha {
            MaterialAlpha::Opaque => 0,
            MaterialAlpha::Mask => 1,
            MaterialAlpha::Blend => 2,
        }
    }
    fn pipeline_index(self) -> usize {
        self.alpha_mode() + 3 * usize::from(self.double_sided) + 6 * usize::from(self.clockwise)
    }
}
#[derive(Debug, thiserror::Error)]
pub enum MaterialRenderError {
    #[error("invalid material geometry, parameters or output target")]
    Invalid,
    #[error("material textures or draw belong to another upload context")]
    Owner,
    #[error("material preparation cancelled")]
    Cancelled,
    #[error(transparent)]
    Memory(#[from] memory_budget::BufferError),
}
/// Pipeline cache is bound to the upload context/device and output target.
#[derive(Debug)]
pub struct MaterialRenderer {
    uploader: MaterialTextureUploader,
    target: wgpu::TextureFormat,
    pipelines: [OnceLock<wgpu::RenderPipeline>; MaterialRaster::COMBINATIONS],
    owner: Arc<()>,
}
#[derive(Debug)]
pub struct MaterialDraw {
    vertices: wgpu::Buffer,
    _uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    count: u32,
    pipeline: usize,
    owner: Arc<()>,
    credit: Arc<memory_budget::Reservation>,
    textures: [MaterialTexture; 5],
}
impl MaterialDraw {
    /// Capture before submitting the encoded draw; retain through completion.
    pub fn resource_lease(&self) -> GpuResourceLease {
        let mut lease = GpuResourceLease::new([self.credit.clone()]);
        for texture in &self.textures {
            lease.extend(texture.resource_lease());
        }
        lease
    }
}
impl MaterialRenderer {
    pub fn new(
        uploader: MaterialTextureUploader,
        target: wgpu::TextureFormat,
    ) -> Result<Self, MaterialRenderError> {
        if !matches!(
            target,
            wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8UnormSrgb
                | wgpu::TextureFormat::Rgba16Float
        ) {
            return Err(MaterialRenderError::Invalid);
        }
        Ok(Self {
            uploader,
            target,
            pipelines: std::array::from_fn(|_| OnceLock::new()),
            owner: Arc::new(()),
        })
    }
    /// World-space expanded triangle vertices. No scene transform or UV guessing.
    /// Fit/rebase coordinates before packing: finite host values must have |v|<=1e15.
    /// Supply white neutral maps for unbound roles; normal map flag controls TBN use.
    pub fn prepare(
        &self,
        budget: &MemoryBudget,
        vertices: &[MaterialVertex],
        mut p: MaterialParameters,
        textures: [&MaterialTexture; 5],
        raster: MaterialRaster,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<MaterialDraw, MaterialRenderError> {
        if cancelled() {
            return Err(MaterialRenderError::Cancelled);
        }
        if textures.iter().any(|t| !self.uploader.owns(t)) {
            return Err(MaterialRenderError::Owner);
        }
        if textures
            .iter()
            .enumerate()
            .any(|(i, t)| t.is_color_role() != (i == 0 || i == 4))
        {
            return Err(MaterialRenderError::Invalid);
        }
        let device = self.uploader.device();
        let bytes = std::mem::size_of_val(vertices) as u64;
        if vertices.is_empty()
            || !vertices.len().is_multiple_of(3)
            || vertices.len() > u32::MAX as usize
            || bytes > device.limits().max_buffer_size
        {
            return Err(MaterialRenderError::Invalid);
        }
        for chunk in vertices.chunks(1024) {
            if cancelled() {
                return Err(MaterialRenderError::Cancelled);
            }
            if bytemuck::cast_slice::<MaterialVertex, f32>(chunk)
                .iter()
                .any(|&v| !crate::finite_bounded(v))
                || chunk.iter().any(|v| ![0., -1., 1.].contains(&v.tangent[3]))
            {
                return Err(MaterialRenderError::Invalid);
            }
        }
        if bytemuck::cast_slice::<MaterialParameters, f32>(&[p])
            .iter()
            .any(|&v| !crate::finite_bounded(v))
            || p.base_color
                .iter()
                .chain(p.emissive[..3].iter())
                .any(|v| !(0. ..=1.).contains(v))
            || [p.factors[0], p.factors[1], p.factors[3]]
                .iter()
                .any(|v| !(0. ..=1.).contains(v))
            || ![0., 1.].contains(&p.emissive[3])
            || p.alpha[0] < 0.
            || ![0., 1.].contains(&p.alpha[2])
            || p.eye[3] < 0.
            || p.light_direction[3] < 0.
            || p.light_color[..3].iter().any(|v| *v < 0.)
        {
            return Err(MaterialRenderError::Invalid);
        }
        p.alpha[1] = raster.alpha_mode() as f32;
        p.alpha[3] = if raster.double_sided { 1. } else { 0. };
        let index = raster.pipeline_index();
        let credit = Arc::new(budget.try_reserve(bytes + size_of::<MaterialParameters>() as u64)?);
        if cancelled() {
            return Err(MaterialRenderError::Cancelled);
        }
        let pipeline = self.pipelines[index].get_or_init(|| pipeline(device, self.target, raster));
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material triangle vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material parameters"),
            contents: bytemuck::bytes_of(&p),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let entries: [wgpu::BindGroupEntry<'_>; 11] = std::array::from_fn(|binding| {
            let resource = if binding == 0 {
                uniform.as_entire_binding()
            } else if binding % 2 == 1 {
                wgpu::BindingResource::TextureView(textures[(binding - 1) / 2].view())
            } else {
                wgpu::BindingResource::Sampler(textures[(binding - 1) / 2].sampler())
            };
            wgpu::BindGroupEntry {
                binding: binding as u32,
                resource,
            }
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material textures"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        if cancelled() {
            return Err(MaterialRenderError::Cancelled);
        }
        Ok(MaterialDraw {
            vertices: vertex_buffer,
            _uniform: uniform,
            bind,
            count: vertices.len() as u32,
            pipeline: index,
            owner: self.owner.clone(),
            credit,
            textures: textures.map(|t| t.clone()),
        })
    }
    /// Atomically admit one batch of view/eye uploads before changing any draw.
    /// Call before encoding; the context's own queue retains credits to completion.
    pub fn update_views<'a>(
        &self,
        budget: &MemoryBudget,
        draws: impl ExactSizeIterator<Item = &'a MaterialDraw> + Clone,
        clip: [[f32; 4]; 4],
        eye: [f32; 4],
    ) -> Result<(), MaterialRenderError> {
        #[repr(C)]
        #[derive(Clone, Copy, Pod, Zeroable)]
        struct Frame {
            clip: [[f32; 4]; 4],
            eye: [f32; 4],
        }
        let frame = Frame { clip, eye };
        if bytemuck::cast_slice::<Frame, f32>(&[frame])
            .iter()
            .any(|&v| !crate::finite_bounded(v))
            || eye[3] < 0.
        {
            return Err(MaterialRenderError::Invalid);
        }
        if draws
            .clone()
            .any(|draw| !Arc::ptr_eq(&self.owner, &draw.owner))
        {
            return Err(MaterialRenderError::Owner);
        }
        if draws.len() == 0 {
            return Ok(());
        }
        let bytes = (draws.len() as u64)
            .checked_mul(size_of::<Frame>() as u64)
            .ok_or(MaterialRenderError::Invalid)?;
        let queue = self.uploader.queue();
        let _queued = crate::QueuedUploadReservation {
            queue: queue.clone(),
            reservation: Some(budget.try_reserve(bytes)?),
            resources: GpuResourceLease::new(draws.clone().map(|draw| draw.credit.clone())),
        };
        for draw in draws {
            queue.write_buffer(&draw._uniform, 0, bytemuck::bytes_of(&frame));
        }
        Ok(())
    }
    /// Caller owns the pass (Depth32Float, compatible output), ordering and target.
    /// Blend draws disable depth writes and require caller-supplied sorting.
    pub fn encode(
        &self,
        draw: &MaterialDraw,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<(), MaterialRenderError> {
        if !Arc::ptr_eq(&self.owner, &draw.owner) {
            return Err(MaterialRenderError::Owner);
        }
        pass.set_pipeline(self.pipelines[draw.pipeline].get().unwrap());
        pass.set_bind_group(0, &draw.bind, &[]);
        pass.set_vertex_buffer(0, draw.vertices.slice(..));
        pass.draw(0..draw.count, 0..1);
        Ok(())
    }
}
fn pipeline(
    device: &wgpu::Device,
    target: wgpu::TextureFormat,
    raster: MaterialRaster,
) -> wgpu::RenderPipeline {
    let module = wgsl_module(
        device,
        "glTF material lighting",
        include_str!("../shaders/material_view.wgsl"),
    );
    let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x4,3=>Float32x4,4=>Float32x2,5=>Float32x2,6=>Float32x2,7=>Float32x2,8=>Float32x2];
    let blend = matches!(raster.alpha, MaterialAlpha::Blend);
    render_pipeline(
        device,
        PipelineSpec {
            label: "material renderer",
            layout: None,
            module: &module,
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<MaterialVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
            primitive: wgpu::PrimitiveState {
                front_face: if raster.clockwise {
                    wgpu::FrontFace::Cw
                } else {
                    wgpu::FrontFace::Ccw
                },
                cull_mode: if raster.double_sided {
                    None
                } else {
                    Some(wgpu::Face::Back)
                },
                ..Default::default()
            },
            depth: Some(depth_test(!blend)),
            target: color_target(target, blend.then_some(wgpu::BlendState::ALPHA_BLENDING)),
        },
    )
}

#[cfg(test)]
mod tests;
