//! Non-Apple Metal stubs so the crate type-checks without `#[allow(dead_code)]`.

use super::{ObjCObject, WorkDoneCallback};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct MetalDevice {
    pub name: Arc<str>,
    pub registry_id: u64,
    pub is_low_power: bool,
    pub is_removable: bool,
    pub has_unified_memory: bool,
    pub max_buffer_length: u64,
    pub(crate) work_done_callbacks: Arc<Mutex<Vec<WorkDoneCallback>>>,
    pub(crate) scope_completion_service: Arc<crate::types::ScopeCompletionService>,
    pub next_submission_index: Arc<std::sync::atomic::AtomicU64>,
}

impl MetalDevice {
    pub fn enumerate_all() -> Vec<Self> {
        Vec::new()
    }

    pub fn create_system_default() -> Option<Self> {
        None
    }

    pub(crate) fn is_lost(&self) -> bool {
        false
    }

    pub(crate) fn register_loss_observer(&self, _observer: &Arc<crate::DeviceLossState>) {}

    pub(crate) fn enqueue_work_done(&self, _target_idx: u64, callback: WorkDoneCallback) {
        self.work_done_callbacks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(callback);
    }

    pub(crate) fn wait_for_submission(
        &self,
        _submission_index: Option<u64>,
        _timeout: Option<std::time::Duration>,
    ) -> Result<(), crate::PollError> {
        Ok(())
    }

    pub fn alloc_buffer(&self, _size: usize, _options: u64) -> Option<(Arc<ObjCObject>, *mut u8)> {
        unimplemented!("Metal buffers are only available on macOS and iOS")
    }

    pub fn alloc_buffer_with_bytes(
        &self,
        _bytes: &[u8],
        _options: u64,
    ) -> Option<(Arc<ObjCObject>, *mut u8)> {
        unimplemented!("Metal buffers are only available on macOS and iOS")
    }

    /// # Safety
    ///
    /// Only supported on macOS and iOS; panics on non-Apple targets.
    pub unsafe fn alloc_buffer_no_copy(
        &self,
        _page_aligned_ptr: *mut c_void,
        _page_aligned_len: usize,
        _options: u64,
    ) -> Option<(Arc<ObjCObject>, *mut u8)> {
        unimplemented!("Metal buffers are only available on macOS and iOS")
    }

    pub fn compile_compute_pipeline(
        &self,
        _msl_source: &str,
        _entry_point: &str,
        _wg_total: u32,
    ) -> Result<Arc<ObjCObject>, String> {
        unimplemented!("Metal pipelines are only available on macOS and iOS")
    }

    pub fn create_render_pipeline(
        &self,
        _desc: &crate::RenderPipelineDescriptor<'_>,
    ) -> Result<crate::RenderPipeline, String> {
        unimplemented!("Metal render pipelines are only available on macOS and iOS")
    }

    pub fn create_texture(&self, _desc: &crate::TextureDescriptor<'_>) -> crate::Texture {
        unimplemented!("Metal textures are only available on macOS and iOS")
    }

    pub fn create_sampler(&self, _desc: &crate::SamplerDescriptor<'_>) -> crate::Sampler {
        unimplemented!("Metal samplers are only available on macOS and iOS")
    }

    pub fn recycle_buffer(
        &self,
        _size: usize,
        _options: u64,
        _buf: Arc<ObjCObject>,
        _ptr: *mut u8,
    ) {
    }

    pub(crate) fn write_buffer_with_error_sink(
        &self,
        _buffer: &crate::Buffer,
        _offset: crate::BufferAddress,
        _data: &[u8],
        _error_sink: Option<crate::CommandErrorSink>,
    ) {
        unimplemented!("Metal buffers are only available on macOS and iOS")
    }

    pub fn write_texture(
        &self,
        _texture: crate::TexelCopyTextureInfo<'_>,
        _data: &[u8],
        _data_layout: crate::TexelCopyBufferLayout,
        _size: crate::Extent3d,
    ) {
        unimplemented!("Metal textures are only available on macOS and iOS")
    }

    pub(crate) fn submit_with_error_sink<I: IntoIterator<Item = crate::CommandBuffer>>(
        &self,
        _command_buffers: I,
        _error_sink: Option<crate::CommandErrorSink>,
    ) -> crate::SubmissionIndex {
        unimplemented!("Metal submission is only available on macOS and iOS")
    }

    pub fn wait_idle(&self) {}

    pub fn is_last_cmd_completed(&self) -> bool {
        true
    }

    pub(crate) fn device_loss_message(&self) -> Option<String> {
        None
    }

    pub(crate) fn fire_work_done_callbacks(&self) {
        let callbacks = std::mem::take(
            &mut *self
                .work_done_callbacks
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        );
        for callback in callbacks {
            callback();
        }
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn inject_device_loss_for_test(&self, _message: String) {}
}
