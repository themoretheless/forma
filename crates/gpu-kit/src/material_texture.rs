//! Material texture upload; color is linearized before filtering, with linear alpha.
use crate::pipeline::{PipelineSpec, color_target, render_pipeline, wgsl_module};
use crate::wgpu;
use memory_budget::{MemoryBudget, Reservation};
use std::sync::{Arc, OnceLock};

#[derive(Debug, thiserror::Error)]
pub enum MaterialTextureError {
    #[error("invalid material texture dimensions or RGBA byte count")]
    InvalidLayout,
    #[error("unsupported glTF sampler enum")]
    InvalidSampler,
    #[error("16-bit material textures require FLOAT32_FILTERABLE on the device")]
    Float32FilteringUnavailable,
    #[error("material texture belongs to another uploader or lacks required mip levels")]
    IncompatibleTexture,
    #[error("material texture upload cancelled")]
    Cancelled,
    #[error(transparent)]
    Memory(#[from] memory_budget::BufferError),
}

/// glTF core sampler fields. Defaults select linear magnification and trilinear minification.
#[derive(Debug, Clone, Copy)]
pub struct MaterialSampler {
    pub mag: Option<u32>,
    pub min: Option<u32>,
    pub wrap_s: u32,
    pub wrap_t: u32,
}
impl Default for MaterialSampler {
    fn default() -> Self {
        Self {
            mag: None,
            min: None,
            wrap_s: 10497,
            wrap_t: 10497,
        }
    }
}
impl MaterialSampler {
    fn descriptor(self) -> Result<wgpu::SamplerDescriptor<'static>, MaterialTextureError> {
        use wgpu::{AddressMode as A, FilterMode as F, MipmapFilterMode as M};
        let wrap = |value| match value {
            10497 => Ok(A::Repeat),
            33071 => Ok(A::ClampToEdge),
            33648 => Ok(A::MirrorRepeat),
            _ => Err(MaterialTextureError::InvalidSampler),
        };
        let mag_filter = match self.mag.unwrap_or(9729) {
            9728 => F::Nearest,
            9729 => F::Linear,
            _ => return Err(MaterialTextureError::InvalidSampler),
        };
        let (min_filter, mipmap_filter, lod_max_clamp) = match self.min.unwrap_or(9987) {
            9728 => (F::Nearest, M::Nearest, 0.),
            9729 => (F::Linear, M::Nearest, 0.),
            9984 => (F::Nearest, M::Nearest, 32.),
            9985 => (F::Linear, M::Nearest, 32.),
            9986 => (F::Nearest, M::Linear, 32.),
            9987 => (F::Linear, M::Linear, 32.),
            _ => return Err(MaterialTextureError::InvalidSampler),
        };
        Ok(wgpu::SamplerDescriptor {
            label: Some("glTF material sampler"),
            address_mode_u: wrap(self.wrap_s)?,
            address_mode_v: wrap(self.wrap_t)?,
            mag_filter,
            min_filter,
            mipmap_filter,
            lod_max_clamp,
            ..Default::default()
        })
    }
}

/// Reusable device/queue-bound upload context. Clones share lazy mip pipelines.
/// Driver pipeline allocations are not included in per-texture logical credits.
#[derive(Debug, Clone)]
pub struct MaterialTextureUploader {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Arc<[OnceLock<wgpu::RenderPipeline>; 3]>,
    owner: Arc<()>,
}
impl MaterialTextureUploader {
    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub(crate) fn owns(&self, texture: &MaterialTexture) -> bool {
        Arc::ptr_eq(&self.owner, &texture.owner)
    }
    pub(crate) fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    /// Share the texture allocation with another validated sampler.
    pub fn with_sampler(
        &self,
        texture: &MaterialTexture,
        sampler: MaterialSampler,
    ) -> Result<MaterialTexture, MaterialTextureError> {
        let descriptor = sampler.descriptor()?;
        if !self.owns(texture)
            || (descriptor.lod_max_clamp > 0.
                && texture.texture.mip_level_count() == 1
                && texture.texture.width().max(texture.texture.height()) > 1)
        {
            return Err(MaterialTextureError::IncompatibleTexture);
        }
        Ok(MaterialTexture {
            sampler: self.device.create_sampler(&descriptor),
            ..texture.clone()
        })
    }
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
            pipelines: Arc::new(std::array::from_fn(|_| OnceLock::new())),
            owner: Arc::new(()),
        }
    }
}

/// Immutable candidate: publication/replacement is owned by the caller.
/// GPU use after upload must retain `resource_lease` until the draw completes.
#[derive(Debug, Clone)]
pub struct MaterialTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    credit: Arc<Reservation>,
    owner: Arc<()>,
    color_role: bool,
}
impl MaterialTexture {
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }
    pub fn is_color_role(&self) -> bool {
        self.color_role
    }
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn resource_lease(&self) -> crate::GpuResourceLease {
        crate::GpuResourceLease::new([self.credit.clone()])
    }
}
impl MaterialTextureUploader {
    /// Upload straight RGBA8. Color roles use sRGB RGB; alpha and data remain linear.
    /// Resident mip texels and aligned queued upload bytes are admitted separately.
    pub fn upload_rgba8(
        &self,
        budget: &MemoryBudget,
        size: [u32; 2],
        pixels: &[u8],
        srgb: bool,
        sampler: MaterialSampler,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<MaterialTexture, MaterialTextureError> {
        let format = if srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        self.upload_bytes(
            budget,
            Texels {
                size,
                pixels,
                format,
                color_role: srgb,
            },
            sampler,
            &mut cancelled,
        )
    }

    /// Preserve normalized 16-bit data in RGBA32Float. Color RGB is converted
    /// to linear before filtering; alpha stays linear. No 8-bit or half rounding.
    /// Caller must enable FLOAT32_FILTERABLE when requesting the device.
    pub fn upload_rgba16(
        &self,
        budget: &MemoryBudget,
        size: [u32; 2],
        pixels: &[u16],
        srgb: bool,
        sampler: MaterialSampler,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<MaterialTexture, MaterialTextureError> {
        if cancelled() {
            return Err(MaterialTextureError::Cancelled);
        }
        if !self
            .device
            .features()
            .contains(wgpu::Features::FLOAT32_FILTERABLE)
        {
            return Err(MaterialTextureError::Float32FilteringUnavailable);
        }
        sampler.descriptor()?;
        let samples = (u64::from(size[0]) * u64::from(size[1])).checked_mul(4);
        if !crate::extent_fits(&self.device, size) || samples != Some(pixels.len() as u64) {
            return Err(MaterialTextureError::InvalidLayout);
        }
        let mut linear = budget.try_buffer(pixels.len(), 0f32)?;
        for (i, (input, output)) in pixels.iter().zip(linear.iter_mut()).enumerate() {
            if i % 16384 == 0 && cancelled() {
                return Err(MaterialTextureError::Cancelled);
            }
            let v = f64::from(*input) / 65535.;
            *output = if srgb && i % 4 != 3 {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            } else {
                v
            } as f32;
        }
        self.upload_bytes(
            budget,
            Texels {
                size,
                pixels: bytemuck::cast_slice(&linear),
                format: wgpu::TextureFormat::Rgba32Float,
                color_role: srgb,
            },
            sampler,
            &mut cancelled,
        )
    }

    fn upload_bytes(
        &self,
        budget: &MemoryBudget,
        texels: Texels,
        sampler: MaterialSampler,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<MaterialTexture, MaterialTextureError> {
        let Texels {
            size,
            pixels,
            format,
            color_role,
        } = texels;
        let texel_bytes = if format == wgpu::TextureFormat::Rgba32Float {
            16
        } else {
            4
        };
        let device = &self.device;
        let queue = &self.queue;
        if cancelled() {
            return Err(MaterialTextureError::Cancelled);
        }
        let descriptor = sampler.descriptor()?;
        let [width, height] = size;
        if !crate::extent_fits(device, size)
            || u64::from(width) * u64::from(height) * texel_bytes != pixels.len() as u64
        {
            return Err(MaterialTextureError::InvalidLayout);
        }
        let mip_count = if descriptor.lod_max_clamp == 0. {
            1
        } else {
            width.max(height).ilog2() + 1
        };
        let bytes: u64 = (0..mip_count)
            .map(|m| u64::from((width >> m).max(1)) * u64::from((height >> m).max(1)) * texel_bytes)
            .sum();
        let credit = Arc::new(budget.try_reserve(bytes)?);
        let row = (u64::from(width) * texel_bytes).div_ceil(256) * 256;
        let queued = budget.try_reserve(row * u64::from(height))?;
        let _upload = crate::QueuedUploadReservation {
            queue: queue.clone(),
            reservation: Some(queued),
            resources: crate::GpuResourceLease::new([credit.clone()]),
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glTF material texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        if cancelled() {
            return Err(MaterialTextureError::Cancelled);
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * texel_bytes as u32),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        if mip_count > 1 {
            let pipeline =
                self.pipelines[mip_slot(format)].get_or_init(|| mip_pipeline(device, format));
            generate_mips(device, queue, &texture, pipeline, mip_count, cancelled)?;
        }
        if cancelled() {
            return Err(MaterialTextureError::Cancelled);
        }
        let view = texture.create_view(&Default::default());
        Ok(MaterialTexture {
            texture,
            view,
            sampler: device.create_sampler(&descriptor),
            credit,
            owner: self.owner.clone(),
            color_role,
        })
    }
}

/// Base-level texels of one upload, already in the texture's storage format.
struct Texels<'a> {
    size: [u32; 2],
    pixels: &'a [u8],
    format: wgpu::TextureFormat,
    color_role: bool,
}

/// The uploader's lazy mip pipeline for each texture format it writes.
fn mip_slot(format: wgpu::TextureFormat) -> usize {
    match format {
        wgpu::TextureFormat::Rgba8Unorm => 0,
        wgpu::TextureFormat::Rgba8UnormSrgb => 1,
        wgpu::TextureFormat::Rgba32Float => 2,
        _ => unreachable!("material textures are RGBA8 or RGBA32Float"),
    }
}

fn mip_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let module = wgsl_module(
        device,
        "material linear mip filter",
        include_str!("../shaders/material_mip.wgsl"),
    );
    render_pipeline(
        device,
        PipelineSpec {
            label: "material mip generation",
            layout: None,
            module: &module,
            buffers: &[],
            primitive: Default::default(),
            depth: None,
            target: color_target(format, None),
        },
    )
}

fn generate_mips(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    pipeline: &wgpu::RenderPipeline,
    count: u32,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), MaterialTextureError> {
    let mut encoder = device.create_command_encoder(&Default::default());
    for mip in 1..count {
        if cancelled() {
            return Err(MaterialTextureError::Cancelled);
        }
        let view = |base| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: base,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let input = view(mip - 1);
        let output = view(mip);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&input),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("material mip"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
    if cancelled() {
        return Err(MaterialTextureError::Cancelled);
    }
    queue.submit([encoder.finish()]);
    Ok(())
}

#[cfg(test)]
mod tests;
