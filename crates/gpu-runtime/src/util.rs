use crate::{Buffer, BufferInner, BufferUsages, Device, Error, Label};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone)]
pub struct BufferInitDescriptor<'a> {
    pub label: Label<'a>,
    pub contents: &'a [u8],
    pub usage: BufferUsages,
}

pub trait DeviceExt {
    fn create_buffer_init(&self, desc: &BufferInitDescriptor<'_>) -> Buffer;
}

impl DeviceExt for Device {
    #[inline]
    fn create_buffer_init(&self, desc: &BufferInitDescriptor<'_>) -> Buffer {
        let options = 0u64; // MTLResourceStorageModeShared (UMA zero-copy)
        let size = (desc.contents.len() as u64).max(4);
        let (raw, ptr, stored_options) =
            match self.metal.alloc_buffer_with_bytes(desc.contents, options) {
                Some((raw, ptr)) => (raw, ptr, options),
                None => {
                    let msg = format!("Metal buffer_init allocation failed for size={size}");
                    self.record_error(Error::OutOfMemory {
                        source: Arc::new(std::io::Error::new(std::io::ErrorKind::OutOfMemory, msg)),
                    });
                    (
                        Arc::new(crate::metal::ObjCObject(std::ptr::null_mut())),
                        std::ptr::null_mut(),
                        0xFF,
                    )
                }
            };
        Buffer {
            inner: Arc::new(BufferInner {
                device: self.metal.clone(),
                raw,
                ptr,
                size,
                usage: desc.usage,
                options: stored_options,
                mapped_ready: AtomicBool::new(false),
            }),
        }
    }
}
