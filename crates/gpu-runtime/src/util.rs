use crate::{Buffer, BufferInner, BufferUsages, Device, Label};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

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
        let (raw, ptr) = self.metal.alloc_buffer_with_bytes(desc.contents, options);
        let size = (desc.contents.len() as u64).max(4);
        Buffer {
            inner: Arc::new(BufferInner {
                device: self.metal.clone(),
                raw,
                ptr,
                size,
                usage: desc.usage,
                options,
                mapped_ready: AtomicBool::new(false),
            }),
        }
    }
}
