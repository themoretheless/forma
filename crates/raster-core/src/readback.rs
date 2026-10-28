//! RGBA row layout over the platform byte transport.
use gpu_compute::{ByteReadback, ReadbackError, wgpu};
use std::time::Duration;

pub struct RgbaReadback {
    bytes: ByteReadback,
    width: u32,
    height: u32,
    row_bytes: u32,
}
impl RgbaReadback {
    pub(crate) fn target(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<wgpu::Texture, ReadbackError> {
        let limit = device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > limit || height > limit {
            return Err(ReadbackError::InvalidRange);
        }
        let row = width
            .checked_mul(4)
            .ok_or(ReadbackError::InvalidRange)?
            .div_ceil(256) as u64
            * 256;
        if row * u64::from(height) > device.limits().max_buffer_size {
            return Err(ReadbackError::InvalidRange);
        }
        Ok(device.create_texture(&wgpu::TextureDescriptor {
            label: Some("raster target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }))
    }

    pub(crate) fn record(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<Self, ReadbackError> {
        let row_bytes = (width * 4).div_ceil(256) * 256;
        let size = u64::from(row_bytes) * u64::from(height);
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("RGBA staging"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        Ok(Self {
            bytes: ByteReadback::map_on_submit(device, encoder, &staging, size)?,
            width,
            height,
            row_bytes,
        })
    }
    pub(crate) fn submitted(&mut self, index: wgpu::SubmissionIndex) {
        self.bytes.submitted(index);
    }
    pub fn try_read(&mut self) -> Result<Option<Vec<u8>>, ReadbackError> {
        let (width, height, row_bytes) = (self.width, self.height, self.row_bytes);
        self.bytes
            .try_read_with(|bytes| unpack(bytes, width, height, row_bytes))
    }
    pub fn cancel(&mut self) {
        self.bytes.cancel();
    }
    pub fn wait(mut self, timeout: Duration) -> Result<Vec<u8>, ReadbackError> {
        let (width, height, row_bytes) = (self.width, self.height, self.row_bytes);
        self.bytes
            .wait_with(timeout, |bytes| unpack(bytes, width, height, row_bytes))
    }
}

fn unpack(bytes: &[u8], width: u32, height: u32, row_bytes: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in bytes.chunks_exact(row_bytes as usize) {
        rgba.extend_from_slice(&row[..width as usize * 4]);
    }
    rgba
}
