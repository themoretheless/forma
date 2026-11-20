#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use std::ffi::{CStr, CString, c_char};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use std::ptr;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod buffers;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod compute_pipeline;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod pools;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod render_pipeline;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod selectors;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use selectors::*;
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod stub;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod submit;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod surface;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod textures;
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub use stub::*;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "objc")]
extern "C" {
    pub fn objc_getClass(name: *const c_char) -> *mut c_void;
    pub fn sel_registerName(name: *const c_char) -> *mut c_void;
    pub fn objc_msgSend();
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod message_send {
    pub trait Sealed {}
}

/// A function pointer type `objc_msgSend` can be called through: the receiver, the selector,
/// then the message's arguments.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) trait MessageSend: message_send::Sealed + Copy {}

#[cfg(any(target_os = "macos", target_os = "ios"))]
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
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!();
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!(A);
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!(A, B);
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!(A, B, C);
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!(A, B, C, D);
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!(A, B, C, D, E);
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!(A, B, C, D, E, F);
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!(A, B, C, D, E, F, G);
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!(A, B, C, D, E, F, G, H);
#[cfg(any(target_os = "macos", target_os = "ios"))]
message_send_arity!(A, B, C, D, E, F, G, H, I);
#[cfg(any(target_os = "macos", target_os = "ios"))]
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
    #[cfg(target_os = "macos")]
    fn MTLCopyAllDevices() -> *mut c_void;
    #[cfg(target_os = "macos")]
    fn MTLCopyAllDevicesWithObserver(
        observer: *mut *mut c_void,
        handler: *const block2::Block<dyn Fn(*mut c_void, *mut c_void)>,
    ) -> *mut c_void;
    #[cfg(target_os = "macos")]
    fn MTLRemoveDeviceObserver(observer: *mut c_void);
    #[cfg(target_os = "macos")]
    static MTLDeviceRemovalRequestedNotification: *mut c_void;
    #[cfg(target_os = "macos")]
    static MTLDeviceWasRemovedNotification: *mut c_void;
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

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
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

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLSize {
    pub width: usize,
    pub height: usize,
    pub depth: usize,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLOrigin {
    pub x: usize,
    pub y: usize,
    pub z: usize,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLRegion {
    pub origin: MTLOrigin,
    pub size: MTLSize,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NSRange {
    pub location: usize,
    pub length: usize,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MTLClearColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
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

#[cfg(any(target_os = "macos", target_os = "ios"))]
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
    #[cfg(any(target_os = "macos", target_os = "ios"))]
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
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[inline]
    pub unsafe fn from_unretained(ptr: *mut c_void) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            unsafe {
                let s = selectors();
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
                f(ptr, s.retain);
            }
            Some(Self(ptr))
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
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
    // SAFETY: Caller guarantees an active Objective-C runtime and autorelease pool context,
    // and `c` is a NUL-terminated UTF-8 string valid for the call duration.
    unsafe { f(s.cls_ns_string, s.string_with_utf8, c.as_ptr()) }
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
    // SAFETY: `ns` is a non-null `NSString`; `UTF8String` returns a NUL-terminated C string or null.
    unsafe {
        let cstr = f(ns, s.utf8_string);
        if cstr.is_null() {
            String::new()
        } else {
            CStr::from_ptr(cstr).to_string_lossy().into_owned()
        }
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
    // SAFETY: `err` is a non-null `NSError`, and `localizedDescription` returns a valid `NSString`.
    unsafe {
        let desc = f(err, s.localized_description);
        nsstring_to_string(desc)
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn command_error_is_device_loss(code: i64) -> bool {
    matches!(code, 4 | 11)
}
// MTLCommandBuffer.h: OutOfMemory=8. A numeric code in another NSError
// domain must not become a Metal allocation failure.
#[cfg(any(target_os = "macos", target_os = "ios"))]
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

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Default)]
pub(crate) struct PendingCompute {
    ops: Vec<crate::ComputeDispatchOp>,
    sinks: Vec<crate::CommandErrorSink>,
    last_idx: u64,
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn add_error_sink(sinks: &mut Vec<crate::CommandErrorSink>, sink: crate::CommandErrorSink) {
    if !sinks.iter().any(|old| old.same_submission(&sink)) {
        sinks.push(sink);
    }
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
enum CompletionCommand {
    Native(ObjCObject),
    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    Failed(crate::Error),
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
struct PendingCommand {
    command: CompletionCommand,
    sinks: Vec<crate::CommandErrorSink>,
    submission_index: u64,
}

#[cfg(any(test, target_os = "macos", target_os = "ios"))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputePipelineCacheKey {
    pub msl_source: String,
    pub entry_point: String,
    pub threadgroup_multiple_of_32: bool,
}

#[cfg(any(test, target_os = "macos", target_os = "ios"))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CachedVertexBufferLayout {
    pub array_stride: crate::BufferAddress,
    pub step_mode: crate::VertexStepMode,
    pub attributes: Vec<crate::VertexAttribute>,
}

#[cfg(any(test, target_os = "macos", target_os = "ios"))]
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

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub type CachedRenderPipelineState = (Arc<ObjCObject>, Option<Arc<ObjCObject>>);

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub struct PooledMetalBuffer {
    pub aligned_size: usize,
    pub options: u64,
    pub raw: Arc<ObjCObject>,
    pub ptr: *mut u8,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe impl Send for PooledMetalBuffer {}
#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe impl Sync for PooledMetalBuffer {}

#[cfg(target_os = "macos")]
pub(crate) struct DeviceObserverGuard(ObjCObject);

#[cfg(target_os = "macos")]
impl Drop for DeviceObserverGuard {
    fn drop(&mut self) {
        // SAFETY: `self.0` wraps the `+1` retained `id<NSObject>` token returned by
        // `MTLCopyAllDevicesWithObserver`; unregistering before `ObjCObject::drop` releases it
        // prevents further callbacks on the torn-down observer.
        unsafe {
            MTLRemoveDeviceObserver(self.0.as_ptr());
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
struct DeviceObserverTarget {
    registry_id: u64,
    backend_device_loss: Arc<Mutex<Option<String>>>,
    loss_observers: Arc<Mutex<Vec<std::sync::Weak<crate::DeviceLossState>>>>,
    scope_completion_service: Arc<crate::types::ScopeCompletionService>,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn notify_loss_targets(
    backend_device_loss: &Mutex<Option<String>>,
    loss_observers: &Mutex<Vec<std::sync::Weak<crate::DeviceLossState>>>,
    scope_completion_service: &crate::types::ScopeCompletionService,
    message: String,
) {
    let first_message = {
        let mut loss = backend_device_loss
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if loss.is_none() {
            *loss = Some(message);
        }
        loss.clone()
    };
    let Some(msg) = first_message else {
        return;
    };
    let live_observers = {
        let mut observers = loss_observers.lock().unwrap_or_else(|e| e.into_inner());
        let mut live = Vec::with_capacity(observers.len());
        observers.retain(|weak| {
            if let Some(strong) = weak.upgrade() {
                live.push(strong);
                true
            } else {
                false
            }
        });
        live
    };
    for observer in live_observers {
        observer.notify(crate::DeviceLostReason::Unknown, msg.clone());
    }
    scope_completion_service.notify();
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Clone)]
pub struct MetalDevice {
    pub raw: Arc<ObjCObject>,
    pub queue: Arc<ObjCObject>,
    pub name: Arc<str>,
    pub registry_id: u64,
    pub is_low_power: bool,
    pub is_removable: bool,
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
    /// Pending `Queue::on_submitted_work_done` / `map_async` callbacks ordered by submission index.
    pub(crate) work_done_callbacks: Arc<Mutex<Vec<PendingCallback>>>,
    backend_device_loss: Arc<Mutex<Option<String>>>,
    loss_observers: Arc<Mutex<Vec<std::sync::Weak<crate::DeviceLossState>>>>,
    #[cfg(target_os = "macos")]
    _device_observer: Option<Arc<DeviceObserverGuard>>,
    pub(crate) pending_compute_ops: Arc<Mutex<PendingCompute>>,
    pub(crate) scope_completion_service: Arc<crate::types::ScopeCompletionService>,
    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    pub(crate) blocking_wait_calls: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    pub(crate) metal_shader_compilations: Arc<std::sync::atomic::AtomicUsize>,
    pub next_submission_index: Arc<std::sync::atomic::AtomicU64>,
}

unsafe impl Send for MetalDevice {}
unsafe impl Sync for MetalDevice {}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl MetalDevice {
    /// Builds a [`MetalDevice`] wrapper from an already retained `id<MTLDevice>`.
    ///
    /// # Safety
    ///
    /// `raw` must wrap a valid `MTLDevice` object.
    unsafe fn from_raw_device(raw: Arc<ObjCObject>) -> Option<Self> {
        let s = selectors();
        let f_ptr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
        let f_bool: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool = msg_send();
        let f_u64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64 = msg_send();
        let f_queue: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
            msg_send();

        // SAFETY: Caller guarantees `raw` is a valid `MTLDevice`; autoreleased `name` string is
        // converted immediately via `nsstring_to_string`.
        let (
            name,
            registry_id,
            is_low_power,
            is_removable,
            has_unified_memory,
            max_buffer_length,
            recommended_working_set,
            queue,
        ) = unsafe {
            let name_ns = f_ptr(raw.as_ptr(), s.name);
            let name: Arc<str> = nsstring_to_string(name_ns).into();
            let registry_id = f_u64(raw.as_ptr(), s.registry_id);
            #[cfg(target_os = "macos")]
            let is_low_power = f_bool(raw.as_ptr(), s.is_low_power);
            #[cfg(not(target_os = "macos"))]
            let is_low_power = true;
            #[cfg(target_os = "macos")]
            let is_removable = f_bool(raw.as_ptr(), s.is_removable);
            #[cfg(not(target_os = "macos"))]
            let is_removable = false;
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
            (
                name,
                registry_id,
                is_low_power,
                is_removable,
                has_unified_memory,
                max_buffer_length,
                recommended_working_set,
                queue,
            )
        };

        Some(Self {
            raw,
            queue,
            name,
            registry_id,
            is_low_power,
            is_removable,
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
            loss_observers: Arc::new(Mutex::new(Vec::new())),
            #[cfg(target_os = "macos")]
            _device_observer: None,
            pending_compute_ops: Arc::new(Mutex::new(PendingCompute {
                ops: Vec::with_capacity(32),
                sinks: Vec::new(),
                last_idx: 0,
            })),
            scope_completion_service: Arc::new(crate::types::ScopeCompletionService::default()),
            #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
            blocking_wait_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
            metal_shader_compilations: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            next_submission_index: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        })
    }

    pub fn enumerate_all() -> Vec<Self> {
        #[cfg(target_os = "macos")]
        {
            let targets: Arc<Mutex<Vec<DeviceObserverTarget>>> = Arc::new(Mutex::new(Vec::new()));
            let callback_targets = targets.clone();
            let handler =
                block2::RcBlock::new(move |changed_dev: *mut c_void, notify_name: *mut c_void| {
                    if changed_dev.is_null() || notify_name.is_null() {
                        return;
                    }
                    let f_u64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64 = msg_send();
                    // SAFETY: Metal invokes `MTLDeviceNotificationHandler` with live `id<MTLDevice>` and
                    // `MTLDeviceNotificationName` pointers, while static notification names are immutable.
                    let (changed_id, name, is_removal) = unsafe {
                        let changed_id = f_u64(changed_dev, selectors().registry_id);
                        let name = nsstring_to_string(notify_name);
                        let removal_requested =
                            nsstring_to_string(MTLDeviceRemovalRequestedNotification);
                        let was_removed = nsstring_to_string(MTLDeviceWasRemovedNotification);
                        let is_removal = notify_name == MTLDeviceRemovalRequestedNotification
                            || notify_name == MTLDeviceWasRemovedNotification
                            || name == removal_requested
                            || name == was_removed
                            || name == "MTLDeviceRemovalRequestedNotification"
                            || name == "MTLDeviceWasRemovedNotification";
                        (changed_id, name, is_removal)
                    };
                    if !is_removal {
                        return;
                    }
                    let matched: Vec<DeviceObserverTarget> = {
                        let list = callback_targets.lock().unwrap_or_else(|e| e.into_inner());
                        list.iter()
                            .filter(|t| t.registry_id == changed_id)
                            .cloned()
                            .collect()
                    };
                    for target in matched {
                        notify_loss_targets(
                            &target.backend_device_loss,
                            &target.loss_observers,
                            &target.scope_completion_service,
                            format!("Metal device notification {name} for registry {changed_id}"),
                        );
                    }
                });

            let mut observer_ptr: *mut c_void = ptr::null_mut();
            // SAFETY: `MTLCopyAllDevicesWithObserver` writes a `+1` retained observer token into
            // `observer_ptr` and returns a `+1` retained `NSArray<id<MTLDevice>>`.
            let array_ptr = unsafe {
                MTLCopyAllDevicesWithObserver(&mut observer_ptr, block2::RcBlock::as_ptr(&handler))
            };
            let array_ptr = if array_ptr.is_null() {
                // SAFETY: Fallback enumeration without observer returns a `+1` retained `NSArray`.
                unsafe { MTLCopyAllDevices() }
            } else {
                array_ptr
            };
            let observer = ObjCObject::from_retained(observer_ptr)
                .map(|obj| Arc::new(DeviceObserverGuard(obj)));
            let Some(array) = ObjCObject::from_retained(array_ptr) else {
                return Vec::new();
            };

            let s = selectors();
            let f_usize: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize = msg_send();
            let f_at_idx: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
                msg_send();
            // SAFETY: `array` wraps a valid `NSArray<id<MTLDevice>>`.
            let count = unsafe { f_usize(array.as_ptr(), s.count) };
            let mut result = Vec::with_capacity(count);
            let mut recorded = targets.lock().unwrap_or_else(|e| e.into_inner());
            for i in 0..count {
                // SAFETY: `i < count` is in bounds and element `i` is a valid `id<MTLDevice>`.
                let dev_opt = unsafe {
                    let dev_ptr = f_at_idx(array.as_ptr(), s.object_at_index, i);
                    ObjCObject::from_unretained(dev_ptr)
                        .and_then(|obj| Self::from_raw_device(Arc::new(obj)))
                };
                if let Some(mut dev) = dev_opt {
                    dev._device_observer = observer.clone();
                    recorded.push(DeviceObserverTarget {
                        registry_id: dev.registry_id,
                        backend_device_loss: dev.backend_device_loss.clone(),
                        loss_observers: dev.loss_observers.clone(),
                        scope_completion_service: dev.scope_completion_service.clone(),
                    });
                    result.push(dev);
                }
            }
            result
        }
        #[cfg(target_os = "ios")]
        {
            Self::create_system_default().into_iter().collect()
        }
    }

    pub fn create_system_default() -> Option<Self> {
        // SAFETY: `MTLCreateSystemDefaultDevice` returns either null or a `+1` retained `MTLDevice`.
        let sys_raw = unsafe { ObjCObject::from_retained(MTLCreateSystemDefaultDevice()) }?;
        #[cfg(target_os = "macos")]
        {
            let f_u64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64 = msg_send();
            // SAFETY: `sys_raw` is a valid `MTLDevice`.
            let target_id = unsafe { f_u64(sys_raw.as_ptr(), selectors().registry_id) };
            let mut all = Self::enumerate_all();
            if let Some(pos) = all.iter().position(|d| d.registry_id == target_id) {
                return Some(all.swap_remove(pos));
            }
            if !all.is_empty() {
                return Some(all.swap_remove(0));
            }
        }
        // SAFETY: `sys_raw` is a valid retained `MTLDevice`.
        unsafe { Self::from_raw_device(Arc::new(sys_raw)) }
    }

    pub(crate) fn is_lost(&self) -> bool {
        self.backend_device_loss
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    pub(crate) fn register_loss_observer(&self, observer: &Arc<crate::DeviceLossState>) {
        {
            let mut list = self
                .loss_observers
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            list.push(Arc::downgrade(observer));
        }
        let existing = self
            .backend_device_loss
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(message) = existing {
            observer.notify(crate::DeviceLostReason::Unknown, message);
        }
    }

    pub(crate) fn record_backend_device_loss(&self, message: String) {
        notify_loss_targets(
            &self.backend_device_loss,
            &self.loss_observers,
            &self.scope_completion_service,
            message,
        );
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    pub(crate) fn handle_device_removal(&self, notification_name: &str) {
        if matches!(
            notification_name,
            "MTLDeviceRemovalRequestedNotification" | "MTLDeviceWasRemovedNotification"
        ) {
            self.record_backend_device_loss(format!(
                "Metal device notification {notification_name} for registry {}",
                self.registry_id
            ));
        }
    }

    #[cfg(test)]
    pub(crate) fn same_observer_for_test(&self, other: &Self) -> bool {
        #[cfg(target_os = "macos")]
        {
            matches!(
                (&self._device_observer, &other._device_observer),
                (Some(a), Some(b)) if Arc::ptr_eq(a, b)
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = other;
            true
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl MetalDevice {
    fn min_pending_submission_index(&self) -> Option<u64> {
        let compute_min = self
            .pending_compute_ops
            .lock()
            .ok()
            .and_then(|p| (!p.ops.is_empty()).then_some(p.last_idx));
        let cmd_min = self
            .pending_error_checks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|p| p.submission_index)
            .min();
        match (compute_min, cmd_min) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    }

    fn has_pending_up_to(&self, target_idx: u64) -> bool {
        if target_idx == 0 {
            return false;
        }
        self.min_pending_submission_index()
            .is_some_and(|min_idx| min_idx <= target_idx)
    }

    pub(crate) fn enqueue_work_done(&self, target_idx: u64, callback: WorkDoneCallback) {
        self.work_done_callbacks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(PendingCallback {
                target_idx,
                callback,
            });
    }

    /// Runs queued callbacks whose target submissions have completed, outside locks.
    pub(crate) fn fire_work_done_callbacks(&self) {
        let _ = self.device_loss_message();
        let min_pending = self.min_pending_submission_index();
        let ready = {
            let mut callbacks = self
                .work_done_callbacks
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if callbacks.is_empty() {
                return;
            }
            let is_ready = |target_idx: u64| {
                target_idx == 0 || min_pending.is_none_or(|min_idx| min_idx > target_idx)
            };
            let mut ready = Vec::new();
            let mut remaining = Vec::new();
            for item in callbacks.drain(..) {
                if is_ready(item.target_idx) {
                    ready.push(item.callback);
                } else {
                    remaining.push(item);
                }
            }
            *callbacks = remaining;
            ready
        };
        for callback in ready {
            callback();
        }
    }

    /// Waits on completion notifications until `submission_index` (or all currently submitted work
    /// when `None`) completes, without issuing a blocking `waitUntilCompleted` driver call.
    pub(crate) fn wait_for_submission(
        &self,
        submission_index: Option<u64>,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), crate::PollError> {
        use std::sync::atomic::Ordering;
        self.flush_pending_compute();
        let target_idx = match submission_index {
            Some(idx) => {
                if idx >= self.next_submission_index.load(Ordering::Acquire) {
                    return Err(crate::PollError::Timeout);
                }
                idx
            }
            None => self
                .next_submission_index
                .load(Ordering::Acquire)
                .saturating_sub(1),
        };
        let deadline = timeout.map(|d| std::time::Instant::now() + d);
        loop {
            let observed = self.scope_completion_service.epoch();
            self.fire_work_done_callbacks();
            if !self.has_pending_up_to(target_idx) {
                return Ok(());
            }
            let wait_slice = match deadline {
                Some(dl) => {
                    let now = std::time::Instant::now();
                    if now >= dl {
                        return Err(crate::PollError::Timeout);
                    }
                    dl.saturating_duration_since(now)
                        .min(std::time::Duration::from_millis(16))
                }
                None => std::time::Duration::from_millis(16),
            };
            self.scope_completion_service
                .wait_for_epoch_change(observed, wait_slice);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) struct PendingCallback {
    pub(crate) target_idx: u64,
    pub(crate) callback: WorkDoneCallback,
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
    use super::{
        MTLCommandBufferErrorDomain, MetalDevice, command_failure_error, msg_send,
        nsstring_from_str, objc_getClass, sel_registerName, selectors,
    };
    use std::error::Error as StdError;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::{Arc, Mutex};

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

    #[test]
    fn real_nserror_domain_and_codes_classify_hardware_faults_vs_foreign_domains() {
        let Some(metal) = MetalDevice::create_system_default() else {
            return;
        };
        let observer = Arc::new(crate::DeviceLossState::default());
        metal.register_loss_observer(&observer);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_out = seen.clone();
        observer.set_callback(Arc::new(move |reason, message| {
            seen_out.lock().unwrap().push((reason, message));
        }));

        // SAFETY: Constructs autoreleased Objective-C `NSError` instances inside a dedicated
        // `NSAutoreleasePool` to exercise `classify_command_nserror` directly.
        unsafe {
            let s = selectors();
            let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
            let f_err: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                isize,
                *mut c_void,
            ) -> *mut c_void = msg_send();

            let pool = f_obj(f_obj(s.cls_ns_autorelease_pool, s.alloc), s.init);
            let cls_nserror = objc_getClass(c"NSError".as_ptr());
            let sel_err = sel_registerName(c"errorWithDomain:code:userInfo:".as_ptr());
            let foreign_domain = nsstring_from_str("CustomForeignErrorDomain");

            for code in [4isize, 8, 11] {
                let err = f_err(cls_nserror, sel_err, foreign_domain, code, ptr::null_mut());
                let mut failure = None;
                metal.classify_command_nserror(err, &mut failure);
                assert!(
                    matches!(failure, Some(crate::Error::Internal { .. })),
                    "foreign domain code {code} must stay Internal"
                );
            }
            assert!(
                !metal.is_lost(),
                "foreign domain errors must not lose device"
            );
            assert!(seen.lock().unwrap().is_empty());

            let oom_err = f_err(
                cls_nserror,
                sel_err,
                MTLCommandBufferErrorDomain,
                8,
                ptr::null_mut(),
            );
            let mut oom_failure = None;
            metal.classify_command_nserror(oom_err, &mut oom_failure);
            let oom = oom_failure.expect("OOM must classify to an Error");
            assert!(matches!(oom, crate::Error::OutOfMemory { .. }));
            assert!(StdError::source(&oom).is_some());
            assert!(!metal.is_lost());

            for code in [2isize, 3] {
                let err = f_err(
                    cls_nserror,
                    sel_err,
                    MTLCommandBufferErrorDomain,
                    code,
                    ptr::null_mut(),
                );
                let mut failure = None;
                metal.classify_command_nserror(err, &mut failure);
                assert!(matches!(failure, Some(crate::Error::Internal { .. })));
                assert!(!metal.is_lost());
            }

            let removed_err = f_err(
                cls_nserror,
                sel_err,
                MTLCommandBufferErrorDomain,
                11,
                ptr::null_mut(),
            );
            let mut removed_failure = None;
            metal.classify_command_nserror(removed_err, &mut removed_failure);
            assert!(removed_failure.is_none());
            assert!(metal.is_lost());
            assert_eq!(seen.lock().unwrap().len(), 1);
            assert_eq!(seen.lock().unwrap()[0].0, crate::DeviceLostReason::Unknown);

            if let Some(metal2) = MetalDevice::create_system_default() {
                let revoked_err = f_err(
                    cls_nserror,
                    sel_err,
                    MTLCommandBufferErrorDomain,
                    4,
                    ptr::null_mut(),
                );
                let mut revoked_failure = None;
                metal2.classify_command_nserror(revoked_err, &mut revoked_failure);
                assert!(revoked_failure.is_none());
                assert!(metal2.is_lost());
            }

            if !pool.is_null() {
                f_void(pool, s.drain);
            }
        }
    }
}

/// MSL for one pipeline stage: the module's source with its `override` values applied.
///
/// # Errors
/// An undeclared or non-finite constant: like a shader that fails to compile, it makes the
/// pipeline invalid.
#[cfg(any(target_os = "macos", target_os = "ios"))]
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

/// EDR headroom of the screen hosting `view` (an `NSView`). AppKit is main-thread only.
///
/// # Safety
///
/// Must be called with either a null pointer or a valid `NSView` pointer on the main thread.
#[cfg(target_os = "macos")]
pub(crate) unsafe fn display_hdr_info(view: *mut c_void) -> crate::DisplayHdrInfo {
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_f64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f64 = msg_send();
    let f_bool: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool = msg_send();
    // SAFETY: Caller invokes AppKit access on main thread with null or a live `NSView`.
    let screen = unsafe {
        let ns_thread = objc_getClass(c"NSThread".as_ptr());
        if view.is_null()
            || ns_thread.is_null()
            || !f_bool(ns_thread, sel_registerName(c"isMainThread".as_ptr()))
        {
            return crate::DisplayHdrInfo::default();
        }
        let window = f_obj(view, sel_registerName(c"window".as_ptr()));
        if window.is_null() {
            ptr::null_mut()
        } else {
            f_obj(window, sel_registerName(c"screen".as_ptr()))
        }
    };
    if screen.is_null() {
        return crate::DisplayHdrInfo::default();
    }
    let read = |name: &std::ffi::CStr| {
        // SAFETY: `screen` is a non-null `NSScreen` responding to standard EDR CGFloat getters.
        Some(unsafe { f_f64(screen, sel_registerName(name.as_ptr())) } as f32)
    };
    crate::DisplayHdrInfo {
        headroom: read(c"maximumExtendedDynamicRangeColorComponentValue"),
        potential_headroom: read(c"maximumPotentialExtendedDynamicRangeColorComponentValue"),
    }
}
