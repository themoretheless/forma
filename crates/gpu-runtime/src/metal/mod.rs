#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use std::ffi::{CStr, CString, c_char};
use std::ptr;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

mod buffers;
mod compute_pipeline;
mod pools;
mod render_pipeline;
mod selectors;
mod submit;
mod surface;
mod textures;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use selectors::*;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "objc")]
extern "C" {
    pub fn objc_getClass(name: *const c_char) -> *mut c_void;
    pub fn sel_registerName(name: *const c_char) -> *mut c_void;
    pub fn objc_msgSend();
}

mod message_send {
    pub trait Sealed {}
}

/// A function pointer type `objc_msgSend` can be called through: the receiver, the selector,
/// then the message's arguments.
pub(crate) trait MessageSend: message_send::Sealed + Copy {}

macro_rules! message_send_arity {
    ($($argument:ident),*) => {
        impl<R $(, $argument)*> message_send::Sealed
            for unsafe extern "C" fn(*mut c_void, *mut c_void $(, $argument)*) -> R
        {
        }
        impl<R $(, $argument)*> MessageSend
            for unsafe extern "C" fn(*mut c_void, *mut c_void $(, $argument)*) -> R
        {
        }
    };
}
message_send_arity!();
message_send_arity!(A);
message_send_arity!(A, B);
message_send_arity!(A, B, C);
message_send_arity!(A, B, C, D);
message_send_arity!(A, B, C, D, E);
message_send_arity!(A, B, C, D, E, F);
message_send_arity!(A, B, C, D, E, F, G);
message_send_arity!(A, B, C, D, E, F, G, H);
message_send_arity!(A, B, C, D, E, F, G, H, I);
message_send_arity!(A, B, C, D, E, F, G, H, I, J);

/// `objc_msgSend` typed as the signature of one message.
///
/// The runtime exports a single untyped entry point; every call site names the signature of
/// the method it sends. This is the only place that entry point is given a type, and only a
/// function pointer that starts with a receiver and a selector can be asked for. Whether the
/// signature matches the method stays the caller's obligation, which is why the returned
/// pointer is `unsafe` to call.
///
/// Not for a message that returns a struct in memory on x86_64 (see [`layer_bounds`]).
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) fn msg_send<F: MessageSend>() -> F {
    const {
        assert!(std::mem::size_of::<F>() == std::mem::size_of::<*const c_void>());
    }
    let entry = objc_msgSend as *const c_void;
    // SAFETY: `F` is a function pointer type (the trait is sealed over them), which has the
    // size and representation of the code address read here.
    unsafe { std::mem::transmute_copy(&entry) }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "Metal", kind = "framework")]
extern "C" {
    pub fn MTLCreateSystemDefaultDevice() -> *mut c_void;
    static MTLCommandBufferErrorDomain: *mut c_void;
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    static kCGColorSpaceSRGB: *const c_void;
    static kCGColorSpaceDisplayP3: *const c_void;
    static kCGColorSpaceExtendedLinearSRGB: *const c_void;
    static kCGColorSpaceExtendedSRGB: *const c_void;
    static kCGColorSpaceExtendedDisplayP3: *const c_void;
    static kCGColorSpaceITUR_2100_PQ: *const c_void;
    static kCGColorSpaceITUR_2100_HLG: *const c_void;
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "QuartzCore", kind = "framework")]
extern "C" {}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "Foundation", kind = "framework")]
extern "C" {}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

#[cfg(all(any(target_os = "macos", target_os = "ios"), target_arch = "x86_64"))]
#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn objc_msgSend_stret();
}

/// The `bounds` of a `CALayer`.
///
/// A `CGRect` is too large for the x86_64 return registers, so there the message goes through
/// `objc_msgSend_stret`, which writes the result through a hidden pointer. Sending it through
/// `objc_msgSend` reads that pointer as the receiver and crashes. arm64 has one entry point
/// for every return type.
///
/// # Safety
/// `layer` must point to a live `CALayer`.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) unsafe fn layer_bounds(layer: *mut c_void) -> CGRect {
    #[cfg(target_arch = "x86_64")]
    let entry = objc_msgSend_stret as *const c_void;
    #[cfg(not(target_arch = "x86_64"))]
    let entry = objc_msgSend as *const c_void;
    // SAFETY: `bounds` takes no arguments and returns a `CGRect` by value, and `entry` is the
    // runtime's entry point for a return of that size on this architecture.
    unsafe {
        let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> CGRect =
            std::mem::transmute(entry);
        send(layer, selectors().bounds)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLSize {
    pub width: usize,
    pub height: usize,
    pub depth: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLOrigin {
    pub x: usize,
    pub y: usize,
    pub z: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLRegion {
    pub origin: MTLOrigin,
    pub size: MTLSize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NSRange {
    pub location: usize,
    pub length: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLClearColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLViewport {
    pub origin_x: f64,
    pub origin_y: f64,
    pub width: f64,
    pub height: f64,
    pub znear: f64,
    pub zfar: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLScissorRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug)]
pub struct ObjCObject(pub *mut c_void);

unsafe impl Send for ObjCObject {}
unsafe impl Sync for ObjCObject {}

impl ObjCObject {
    /// Wraps an already-retained Objective-C object pointer, releasing it on drop.
    #[inline]
    pub fn from_retained(ptr: *mut c_void) -> Option<Self> {
        if ptr.is_null() { None } else { Some(Self(ptr)) }
    }

    /// Retains an unowned Objective-C object pointer and releases it on drop.
    ///
    /// # Safety
    ///
    /// When non-null, `ptr` must point to a valid Objective-C object capable of
    /// receiving `retain` and `release`.
    #[inline]
    pub unsafe fn from_unretained(ptr: *mut c_void) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            unsafe {
                let s = selectors();
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
                f(ptr, s.retain);
            }
            Some(Self(ptr))
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut c_void {
        self.0
    }
}

impl Clone for ObjCObject {
    fn clone(&self) -> Self {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        unsafe {
            let s = selectors();
            let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            f(self.0, s.retain);
        }
        Self(self.0)
    }
}

impl Drop for ObjCObject {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if !self.0.is_null() {
            unsafe {
                let s = selectors();
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
                f(self.0, s.release);
            }
        }
    }
}

/// Creates an autoreleased `NSString` from `st`.
///
/// # Safety
///
/// Must be called with an active Objective-C runtime and autorelease pool context.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub unsafe fn nsstring_from_str(st: &str) -> *mut c_void {
    let s = selectors();
    let c = CString::new(st).unwrap_or_default();
    let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> *mut c_void =
        msg_send();
    f(s.cls_ns_string, s.string_with_utf8, c.as_ptr())
}

/// Converts an `NSString` pointer to an owned Rust `String`.
///
/// # Safety
///
/// When non-null, `ns` must point to a valid `NSString` instance.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub unsafe fn nsstring_to_string(ns: *mut c_void) -> String {
    if ns.is_null() {
        return String::new();
    }
    let s = selectors();
    let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *const c_char = msg_send();
    let cstr = f(ns, s.utf8_string);
    if cstr.is_null() {
        String::new()
    } else {
        CStr::from_ptr(cstr).to_string_lossy().into_owned()
    }
}

/// Formats an `NSError` pointer via its `localizedDescription`.
///
/// # Safety
///
/// When non-null, `err` must point to a valid `NSError` instance.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub unsafe fn nserror_description(err: *mut c_void) -> String {
    if err.is_null() {
        return "unknown Metal error".to_string();
    }
    let s = selectors();
    let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let desc = f(err, s.localized_description);
    nsstring_to_string(desc)
}

fn command_error_is_device_loss(code: i64) -> bool {
    matches!(code, 4 | 11)
}
// MTLCommandBuffer.h: OutOfMemory=8. A numeric code in another NSError
// domain must not become a Metal allocation failure.
pub(crate) fn command_failure_error(
    metal_domain: bool,
    code: i64,
    message: String,
) -> crate::Error {
    let source = Arc::new(std::io::Error::other(message.clone()));
    if metal_domain && code == 8 {
        crate::Error::OutOfMemory { source }
    } else {
        crate::Error::Internal {
            source,
            description: message,
        }
    }
}

#[derive(Default)]
pub(crate) struct PendingCompute {
    ops: Vec<crate::ComputeDispatchOp>,
    sinks: Vec<crate::CommandErrorSink>,
}
fn add_error_sink(sinks: &mut Vec<crate::CommandErrorSink>, sink: crate::CommandErrorSink) {
    if !sinks.iter().any(|old| old.same_submission(&sink)) {
        sinks.push(sink);
    }
}
enum CompletionCommand {
    Native(ObjCObject),
    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    Failed(crate::Error),
}
struct PendingCommand {
    command: CompletionCommand,
    sinks: Vec<crate::CommandErrorSink>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputePipelineCacheKey {
    pub msl_source: String,
    pub entry_point: String,
    pub threadgroup_multiple_of_32: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CachedVertexBufferLayout {
    pub array_stride: crate::BufferAddress,
    pub step_mode: crate::VertexStepMode,
    pub attributes: Vec<crate::VertexAttribute>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderPipelineCacheKey {
    pub vs_src: String,
    pub fs_src: Option<String>,
    pub vs_ep: String,
    pub fs_ep: String,
    pub vertex_buffers: Vec<Option<CachedVertexBufferLayout>>,
    pub targets: Vec<Option<crate::ColorTargetState>>,
    pub depth_stencil: Option<(
        crate::TextureFormat,
        Option<bool>,
        Option<crate::CompareFunction>,
    )>,
    pub sample_count: u32,
}

pub type CachedRenderPipelineState = (Arc<ObjCObject>, Option<Arc<ObjCObject>>);

pub struct PooledMetalBuffer {
    pub aligned_size: usize,
    pub options: u64,
    pub raw: Arc<ObjCObject>,
    pub ptr: *mut u8,
}

unsafe impl Send for PooledMetalBuffer {}
unsafe impl Sync for PooledMetalBuffer {}

#[derive(Clone)]
pub struct MetalDevice {
    pub raw: Arc<ObjCObject>,
    pub queue: Arc<ObjCObject>,
    pub name: Arc<str>,
    pub has_unified_memory: bool,
    pub max_buffer_length: u64,
    pub recommended_working_set: u64,
    // Caches for zero-overhead pipeline and buffer reuse
    pub compute_pipeline_cache:
        Arc<Mutex<std::collections::HashMap<ComputePipelineCacheKey, Arc<ObjCObject>>>>,
    pub render_pipeline_cache:
        Arc<Mutex<std::collections::HashMap<RenderPipelineCacheKey, CachedRenderPipelineState>>>,
    pub buffer_pool: Arc<Mutex<Vec<PooledMetalBuffer>>>,
    pub(crate) cmd_ops_pool: Arc<Mutex<Vec<Vec<crate::RecordedOp>>>>,
    pub(crate) render_cmds_pool: Arc<Mutex<Vec<Vec<crate::RenderCommand>>>>,
    pub(crate) color_atts_pool: Arc<Mutex<Vec<Vec<Option<crate::OwnedColorAttachment>>>>>,
    pub(crate) compute_dispatches_pool: Arc<Mutex<Vec<Vec<crate::ComputeDispatchOp>>>>,
    pub last_cmd_buf: Arc<Mutex<Option<(ObjCObject, u64)>>>,
    pending_error_checks: Arc<Mutex<Vec<PendingCommand>>>,
    /// `Queue::on_submitted_work_done` callbacks, run by the next `Device::poll`, as in WebGPU.
    pub(crate) work_done_callbacks: Arc<Mutex<Vec<WorkDoneCallback>>>,
    backend_device_loss: Arc<Mutex<Option<String>>>,
    pub(crate) pending_compute_ops: Arc<Mutex<PendingCompute>>,
    pub(crate) scope_completion_service: Arc<crate::types::ScopeCompletionService>,
    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    pub(crate) blocking_wait_calls: Arc<std::sync::atomic::AtomicUsize>,
    pub next_submission_index: Arc<std::sync::atomic::AtomicU64>,
}

unsafe impl Send for MetalDevice {}
unsafe impl Sync for MetalDevice {}

impl MetalDevice {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn create_system_default() -> Option<Self> {
        unsafe {
            let dev_ptr = MTLCreateSystemDefaultDevice();
            let raw = Arc::new(ObjCObject::from_retained(dev_ptr)?);
            let s = selectors();

            let f_ptr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let f_bool: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool = msg_send();
            let f_u64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64 = msg_send();
            let f_queue: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
                msg_send();

            let name_ns = f_ptr(raw.as_ptr(), s.name);
            let name: Arc<str> = nsstring_to_string(name_ns).into();
            let has_unified_memory = f_bool(raw.as_ptr(), s.has_unified_memory);
            let max_buffer_length = f_u64(raw.as_ptr(), s.max_buffer_length);
            let recommended_working_set = f_u64(raw.as_ptr(), s.recommended_max_working_set_size);

            let q_ptr = f_queue(raw.as_ptr(), s.new_command_queue_with_max, 512);
            let q_ptr = if q_ptr.is_null() {
                f_ptr(raw.as_ptr(), s.new_command_queue)
            } else {
                q_ptr
            };
            let queue = Arc::new(ObjCObject::from_retained(q_ptr)?);

            Some(Self {
                raw,
                queue,
                name,
                has_unified_memory,
                max_buffer_length: max_buffer_length.max(256 * 1024 * 1024),
                recommended_working_set,
                compute_pipeline_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
                render_pipeline_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
                buffer_pool: Arc::new(Mutex::new(Vec::new())),
                cmd_ops_pool: Arc::new(Mutex::new(Vec::with_capacity(8))),
                render_cmds_pool: Arc::new(Mutex::new(Vec::with_capacity(8))),
                color_atts_pool: Arc::new(Mutex::new(Vec::with_capacity(8))),
                compute_dispatches_pool: Arc::new(Mutex::new(Vec::with_capacity(8))),
                last_cmd_buf: Arc::new(Mutex::new(None)),
                pending_error_checks: Arc::new(Mutex::new(Vec::with_capacity(8))),
                work_done_callbacks: Arc::new(Mutex::new(Vec::new())),
                backend_device_loss: Arc::new(Mutex::new(None)),
                pending_compute_ops: Arc::new(Mutex::new(PendingCompute {
                    ops: Vec::with_capacity(32),
                    sinks: Vec::new(),
                })),
                scope_completion_service: Arc::new(crate::types::ScopeCompletionService::default()),
                #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
                blocking_wait_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                next_submission_index: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            })
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pub fn create_system_default() -> Option<Self> {
        None
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
impl MetalDevice {
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

    pub fn flush_pending_compute(&self) {}

    pub fn wait_idle(&self) {}

    pub fn is_last_cmd_completed(&self) -> bool {
        true
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use objc_msgSend as metal_msg_send;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use selectors as metal_selectors;

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod device_loss_classification_tests {
    use super::command_error_is_device_loss;
    #[test]
    fn only_access_revoked_or_removed_are_device_loss() {
        assert!(command_error_is_device_loss(4));
        assert!(command_error_is_device_loss(11));
        for code in [-1, 0, 1, 2, 3, 7, 8, 9, 10, 12, 99] {
            assert!(
                !command_error_is_device_loss(code),
                "command error {code} is not device loss"
            );
        }
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod command_failure_classification_tests {
    use super::command_failure_error;
    #[test]
    fn only_metal_oom_code_maps_to_out_of_memory() {
        let error = command_failure_error(true, 8, "Metal allocation exhausted".into());
        assert!(matches!(error, crate::Error::OutOfMemory { .. }));
        assert!(error.to_string().contains("Metal allocation exhausted"));
        for code in [-1, 0, 1, 2, 3, 7, 9, 10, 12, 99] {
            assert!(matches!(
                command_failure_error(true, code, "other".into()),
                crate::Error::Internal { .. }
            ));
        }
        for code in [4, 8, 11] {
            assert!(matches!(
                command_failure_error(false, code, "foreign domain".into()),
                crate::Error::Internal { .. }
            ));
        }
    }
}

/// MSL for one pipeline stage: the module's source with its `override` values applied.
///
/// # Errors
/// An undeclared or non-finite constant: like a shader that fails to compile, it makes the
/// pipeline invalid.
pub(crate) fn msl_for_pipeline<'a>(
    msl: &'a str,
    constants: &[(&str, f64)],
) -> Result<std::borrow::Cow<'a, str>, String> {
    if constants.is_empty() {
        Ok(std::borrow::Cow::Borrowed(msl))
    } else {
        gpu_shader::back::msl::with_pipeline_constants(msl, constants)
            .map(std::borrow::Cow::Owned)
            .map_err(|error| format!("invalid pipeline constant: {error}"))
    }
}

pub(crate) type WorkDoneCallback = Box<dyn FnOnce() + Send>;

impl MetalDevice {
    /// Runs the queued work-done callbacks once all submitted work has completed.
    pub(crate) fn fire_work_done_callbacks(&self) {
        if !self.is_last_cmd_completed() {
            return;
        }
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
}

/// EDR headroom of the screen hosting `view` (an `NSView`). AppKit is main-thread only.
#[cfg(target_os = "macos")]
pub(crate) unsafe fn display_hdr_info(view: *mut c_void) -> crate::DisplayHdrInfo {
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_f64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f64 = msg_send();
    let f_bool: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool = msg_send();
    let ns_thread = objc_getClass(c"NSThread".as_ptr());
    if view.is_null()
        || ns_thread.is_null()
        || !f_bool(ns_thread, sel_registerName(c"isMainThread".as_ptr()))
    {
        return crate::DisplayHdrInfo::default();
    }
    let window = f_obj(view, sel_registerName(c"window".as_ptr()));
    let screen = if window.is_null() {
        ptr::null_mut()
    } else {
        f_obj(window, sel_registerName(c"screen".as_ptr()))
    };
    if screen.is_null() {
        return crate::DisplayHdrInfo::default();
    }
    let read = |name: &std::ffi::CStr| Some(f_f64(screen, sel_registerName(name.as_ptr())) as f32);
    crate::DisplayHdrInfo {
        headroom: read(c"maximumExtendedDynamicRangeColorComponentValue"),
        potential_headroom: read(c"maximumPotentialExtendedDynamicRangeColorComponentValue"),
    }
}
