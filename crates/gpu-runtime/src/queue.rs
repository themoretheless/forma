//! GPU submission queue and host-to-device write helpers.

use crate::metal::MetalDevice;
use crate::{
    Buffer, BufferAddress, BufferInner, BufferSize, BufferUsages, CommandBuffer, CommandErrorSink,
    Error, Extent3d, SubmissionIndex, SurfaceTexture, TexelCopyBufferLayout, TexelCopyTextureInfo,
    WriteOnly, validation_error,
};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(Clone)]
pub struct Queue {
    pub(crate) metal: MetalDevice,
    // One queue per logical device. Backend handles can be shared by separate
    // request_device calls; cloning the public queue must retain its identity.
    pub(crate) identity: Arc<()>,
    pub(crate) error_sink: CommandErrorSink,
    pub(crate) device_destroyed: Arc<AtomicBool>,
}

impl PartialEq for Queue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }
}

impl Eq for Queue {}

impl std::fmt::Debug for Queue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Queue")
            .field("name", &self.metal.name)
            .finish()
    }
}

impl Queue {
    #[inline]
    pub fn write_buffer(&self, buffer: &Buffer, offset: BufferAddress, data: &[u8]) {
        self.metal.write_buffer_with_error_sink(
            buffer,
            offset,
            data,
            Some(self.error_sink.capture()),
        );
    }

    #[inline]
    pub fn write_buffer_with<'a>(
        &'a self,
        buffer: &'a Buffer,
        offset: BufferAddress,
        size: BufferSize,
    ) -> Option<WriteOnly<'a, [u8]>> {
        if buffer.inner.ptr.is_null() {
            return None;
        }
        buffer.inner.write_view(offset, size.get())
    }

    #[inline]
    pub fn write_texture(
        &self,
        texture: TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: TexelCopyBufferLayout,
        size: Extent3d,
    ) {
        if data.is_empty() || size.width == 0 || size.height == 0 {
            return;
        }
        let problem = texture
            .texture
            .region_error(texture.mip_level, texture.origin, size)
            .or_else(|| texture.texture.data_error(data.len(), &data_layout, size));
        if let Some(problem) = problem {
            self.error_sink
                .capture()
                .command_failure(validation_error(format!("write_texture: {problem}")));
            return;
        }
        if texture.texture.storage_mode != 0 {
            self.write_private_texture(texture, data, data_layout, size);
            return;
        }
        self.metal.write_texture(texture, data, data_layout, size);
    }

    /// Uploads to a texture the CPU cannot address (private storage, as every texture is on a
    /// device without unified memory): the data goes through a staging buffer and a blit.
    fn write_private_texture(
        &self,
        texture: TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: TexelCopyBufferLayout,
        size: Extent3d,
    ) {
        // `write_texture` has checked that the layout fits `data`.
        let Some(payload) = usize::try_from(data_layout.offset)
            .ok()
            .and_then(|offset| data.get(offset..))
        else {
            return;
        };
        let sink = self.error_sink.capture();
        // MTLResourceStorageModeShared, so the bytes can be copied in from the CPU.
        let Some((raw, ptr)) = self.metal.alloc_buffer_with_bytes(payload, 0) else {
            sink.command_failure(Error::OutOfMemory {
                source: Arc::new(std::io::Error::new(
                    std::io::ErrorKind::OutOfMemory,
                    format!(
                        "write_texture: no staging buffer of {} bytes",
                        payload.len()
                    ),
                )),
            });
            return;
        };
        let staging = Buffer {
            inner: Arc::new(BufferInner {
                device: self.metal.clone(),
                raw,
                ptr,
                size: payload.len() as u64,
                usage: BufferUsages::COPY_SRC,
                options: 0,
                mapped_ready: AtomicBool::new(false),
                mapped_ranges: Default::default(),
            }),
        };
        let upload = CommandBuffer {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            ops: vec![crate::RecordedOp::CopyBufferToTexture {
                src_buf: staging.clone(),
                src_offset: 0,
                bytes_per_row: data_layout.row_stride(size.width, texture.texture.format),
                rows_per_image: data_layout.rows_per_image,
                dst_tex: texture.texture.clone(),
                dst_mip: texture.mip_level,
                dst_origin: texture.origin,
                size,
            }],
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            on_submit_callbacks: Vec::new(),
        };
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let _ = (staging, texture, size);
        self.metal.submit_with_error_sink(Some(upload), Some(sink));
        // The staging buffer returns to the pool when dropped; the blit must have read it first.
        let _ = self.metal.wait_for_submission(None, None);
    }

    #[inline]
    pub fn submit<I: IntoIterator<Item = CommandBuffer>>(
        &self,
        command_buffers: I,
    ) -> SubmissionIndex {
        if self
            .device_destroyed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.error_sink.capture().command_failure(validation_error(
                "Queue::submit called on a destroyed Device".to_string(),
            ));
            return SubmissionIndex(0);
        }
        self.metal
            .submit_with_error_sink(command_buffers, Some(self.error_sink.capture()))
    }

    /// Presents a surface texture; same as [`SurfaceTexture::present`].
    pub fn present(&self, surface_texture: SurfaceTexture) {
        surface_texture.present();
    }

    pub fn on_submitted_work_done<F>(&self, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let target_idx = self
            .metal
            .next_submission_index
            .load(std::sync::atomic::Ordering::Acquire)
            .saturating_sub(1);
        self.metal.enqueue_work_done(target_idx, Box::new(callback));
    }

    pub fn get_timestamp_period(&self) -> f32 {
        1.0
    }
}
