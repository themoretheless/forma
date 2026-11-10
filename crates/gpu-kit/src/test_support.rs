//! Shared test scaffolding: shader validation, a Metal device, clear passes and texture readback.
use crate::wgpu;

/// Parses and validates a WGSL source with the target's own compiler.
pub(crate) fn validate_wgsl(source: &str, capabilities: wgpu::naga::valid::Capabilities) {
    let module = wgpu::naga::front::wgsl::parse_str(source).unwrap();
    wgpu::naga::valid::Validator::new(wgpu::naga::valid::ValidationFlags::all(), capabilities)
        .validate(&module)
        .unwrap();
}

/// A device and queue on the Metal backend; the tests that call this must not fall back.
#[cfg(target_os = "macos")]
pub(crate) fn metal_device(features: wgpu::Features) -> (wgpu::Device, wgpu::Queue) {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    assert_eq!(adapter.get_info().backend, wgpu::Backend::Metal);
    eprintln!("adapter {:?}", adapter.get_info());
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    }))
    .unwrap()
}

/// A single-level 2D texture.
#[cfg(target_os = "macos")]
pub(crate) fn texture_2d(
    device: &wgpu::Device,
    size: wgpu::Extent3d,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// A pass that clears `color` to transparent and `depth` to the far plane.
#[cfg(target_os = "macos")]
pub(crate) fn clear_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}

/// A recorded copy of one mip level into a mappable buffer.
#[cfg(target_os = "macos")]
pub(crate) struct Readback {
    buffer: wgpu::Buffer,
    row: usize,
    row_bytes: usize,
    height: usize,
}

#[cfg(target_os = "macos")]
impl Readback {
    /// Records the copy of `mip` of `texture`, whose texels are `texel_bytes` wide.
    pub(crate) fn record(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        mip: u32,
        texel_bytes: u32,
    ) -> Self {
        let width = (texture.width() >> mip).max(1);
        let height = (texture.height() >> mip).max(1);
        let row = (width * texel_bytes).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: mip,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        Self {
            buffer,
            row: row as usize,
            row_bytes: (width * texel_bytes) as usize,
            height: height as usize,
        }
    }

    /// Waits for the submitted copy and returns the rows without their padding.
    pub(crate) fn read(self, device: &wgpu::Device) -> Vec<u8> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let out = {
            let mapped = self.buffer.slice(..).get_mapped_range().unwrap();
            (0..self.height)
                .flat_map(|y| &mapped[y * self.row..y * self.row + self.row_bytes])
                .copied()
                .collect()
        };
        self.buffer.unmap();
        out
    }
}
