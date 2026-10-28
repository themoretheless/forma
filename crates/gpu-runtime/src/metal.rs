#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(target_os = "macos")]
#[link(name = "objc")]
extern "C" {
    pub fn objc_getClass(name: *const c_char) -> *mut c_void;
    pub fn sel_registerName(name: *const c_char) -> *mut c_void;
    pub fn objc_msgSend();
}

#[cfg(target_os = "macos")]
#[link(name = "Metal", kind = "framework")]
extern "C" {
    pub fn MTLCreateSystemDefaultDevice() -> *mut c_void;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {}

#[cfg(target_os = "macos")]
#[link(name = "Foundation", kind = "framework")]
extern "C" {}

#[derive(Clone, Copy)]
pub struct Selectors {
    pub release: *mut c_void,
    pub retain: *mut c_void,
    pub utf8_string: *mut c_void,
    pub string_with_utf8: *mut c_void,
    pub localized_description: *mut c_void,
    pub name: *mut c_void,
    pub has_unified_memory: *mut c_void,
    pub recommended_max_working_set_size: *mut c_void,
    pub max_buffer_length: *mut c_void,
    pub new_command_queue: *mut c_void,
    pub new_command_queue_with_max: *mut c_void,
    pub command_buffer: *mut c_void,
    pub command_buffer_unretained: *mut c_void,
    pub compute_command_encoder: *mut c_void,
    pub blit_command_encoder: *mut c_void,
    pub render_command_encoder_with_desc: *mut c_void,
    pub end_encoding: *mut c_void,
    pub commit: *mut c_void,
    pub wait_until_completed: *mut c_void,
    pub status: *mut c_void,
    pub new_buffer_with_length: *mut c_void,
    pub new_buffer_with_bytes: *mut c_void,
    pub new_buffer_with_bytes_no_copy: *mut c_void,
    pub contents: *mut c_void,
    pub length: *mut c_void,
    pub new_library_with_source: *mut c_void,
    pub new_function_with_name: *mut c_void,
    pub new_compute_pipeline_with_function: *mut c_void,
    pub new_render_pipeline_with_desc: *mut c_void,
    pub set_compute_pipeline_state: *mut c_void,
    pub set_buffer: *mut c_void,
    pub set_bytes: *mut c_void,
    pub set_texture: *mut c_void,
    pub set_sampler_state: *mut c_void,
    pub dispatch_threadgroups: *mut c_void,
    pub copy_from_buffer: *mut c_void,
    pub copy_from_texture_to_buffer: *mut c_void,
    pub copy_from_texture_to_texture: *mut c_void,
    pub fill_buffer: *mut c_void,
    pub new_counter_sample_buffer: *mut c_void,
    pub sample_counters_in_buffer: *mut c_void,
    pub resolve_counter_range: *mut c_void,
    pub supports_counter_sampling: *mut c_void,
    // Texture & Render descriptors
    pub tex_desc_2d: *mut c_void,
    pub set_usage: *mut c_void,
    pub set_storage_mode: *mut c_void,
    pub set_mipmap_level_count: *mut c_void,
    pub set_sample_count: *mut c_void,
    pub set_texture_type: *mut c_void,
    pub set_array_length: *mut c_void,
    pub set_depth: *mut c_void,
    pub new_texture_with_desc: *mut c_void,
    pub replace_region: *mut c_void,
    pub get_bytes: *mut c_void,
    pub width: *mut c_void,
    pub height: *mut c_void,
    pub mipmap_level_count: *mut c_void,
    pub pixel_format: *mut c_void,
    // Sampler
    pub alloc: *mut c_void,
    pub init: *mut c_void,
    pub set_min_filter: *mut c_void,
    pub set_mag_filter: *mut c_void,
    pub set_mip_filter: *mut c_void,
    pub set_s_address_mode: *mut c_void,
    pub set_t_address_mode: *mut c_void,
    pub set_r_address_mode: *mut c_void,
    pub set_compare_function: *mut c_void,
    pub new_sampler_state: *mut c_void,
    // Render pipeline & pass
    pub set_vertex_function: *mut c_void,
    pub set_fragment_function: *mut c_void,
    pub color_attachments: *mut c_void,
    pub object_at_indexed_subscript: *mut c_void,
    pub set_pixel_format: *mut c_void,
    pub set_write_mask: *mut c_void,
    pub set_blending_enabled: *mut c_void,
    pub set_source_rgb_blend_factor: *mut c_void,
    pub set_destination_rgb_blend_factor: *mut c_void,
    pub set_rgb_blend_operation: *mut c_void,
    pub set_source_alpha_blend_factor: *mut c_void,
    pub set_destination_alpha_blend_factor: *mut c_void,
    pub set_alpha_blend_operation: *mut c_void,
    pub set_depth_attachment_pixel_format: *mut c_void,
    pub vertex_descriptor: *mut c_void,
    pub attributes: *mut c_void,
    pub layouts: *mut c_void,
    pub set_format: *mut c_void,
    pub set_offset: *mut c_void,
    pub set_buffer_index: *mut c_void,
    pub set_stride: *mut c_void,
    pub set_step_function: *mut c_void,
    pub set_step_rate: *mut c_void,
    // Depth stencil
    pub set_depth_write_enabled: *mut c_void,
    pub set_depth_compare_function: *mut c_void,
    pub new_depth_stencil_state: *mut c_void,
    // Render pass descriptor
    pub render_pass_descriptor: *mut c_void,
    pub depth_attachment: *mut c_void,
    pub set_attachment_texture: *mut c_void,
    pub set_load_action: *mut c_void,
    pub set_store_action: *mut c_void,
    pub set_clear_color: *mut c_void,
    pub set_clear_depth: *mut c_void,
    pub set_slice: *mut c_void,
    pub set_level: *mut c_void,
    // Render command encoder methods
    pub set_render_pipeline_state: *mut c_void,
    pub set_depth_stencil_state: *mut c_void,
    pub set_cull_mode: *mut c_void,
    pub set_front_facing_winding: *mut c_void,
    pub set_depth_bias: *mut c_void,
    pub set_viewport: *mut c_void,
    pub set_scissor_rect: *mut c_void,
    pub set_vertex_buffer: *mut c_void,
    pub set_vertex_bytes: *mut c_void,
    pub set_fragment_buffer: *mut c_void,
    pub set_fragment_bytes: *mut c_void,
    pub set_vertex_texture: *mut c_void,
    pub set_fragment_texture: *mut c_void,
    pub set_vertex_sampler_state: *mut c_void,
    pub set_fragment_sampler_state: *mut c_void,
    pub draw_primitives: *mut c_void,
    pub draw_indexed_primitives: *mut c_void,
    // Classes
    pub cls_ns_string: *mut c_void,
    pub cls_mtl_compile_options: *mut c_void,
    pub cls_mtl_texture_descriptor: *mut c_void,
    pub cls_mtl_sampler_descriptor: *mut c_void,
    pub cls_mtl_render_pipeline_descriptor: *mut c_void,
    pub cls_mtl_depth_stencil_descriptor: *mut c_void,
    pub cls_mtl_render_pass_descriptor: *mut c_void,
    pub cls_mtl_counter_sample_buffer_descriptor: *mut c_void,
    pub cls_mtl_compute_pipeline_descriptor: *mut c_void,
    pub set_compute_function: *mut c_void,
    pub set_thread_group_size_is_multiple: *mut c_void,
    pub new_compute_pipeline_with_descriptor: *mut c_void,
    pub set_fast_math_enabled: *mut c_void,
    pub counter_sets: *mut c_void,
    pub count: *mut c_void,
    pub set_counter_set: *mut c_void,
    pub set_sample_count_counter: *mut c_void,
    pub gpu_start_time: *mut c_void,
    pub gpu_end_time: *mut c_void,
}

unsafe impl Send for Selectors {}
unsafe impl Sync for Selectors {}

static SELECTORS: OnceLock<Selectors> = OnceLock::new();

#[cfg(target_os = "macos")]
fn sel(name: &str) -> *mut c_void {
    let c = CString::new(name).unwrap();
    unsafe { sel_registerName(c.as_ptr()) }
}

#[cfg(target_os = "macos")]
fn cls(name: &str) -> *mut c_void {
    let c = CString::new(name).unwrap();
    unsafe { objc_getClass(c.as_ptr()) }
}

#[cfg(target_os = "macos")]
pub fn selectors() -> &'static Selectors {
    SELECTORS.get_or_init(|| Selectors {
        release: sel("release"),
        retain: sel("retain"),
        utf8_string: sel("UTF8String"),
        string_with_utf8: sel("stringWithUTF8String:"),
        localized_description: sel("localizedDescription"),
        name: sel("name"),
        has_unified_memory: sel("hasUnifiedMemory"),
        recommended_max_working_set_size: sel("recommendedMaxWorkingSetSize"),
        max_buffer_length: sel("maxBufferLength"),
        new_command_queue: sel("newCommandQueue"),
        new_command_queue_with_max: sel("newCommandQueueWithMaxCommandBufferCount:"),
        command_buffer: sel("commandBuffer"),
        command_buffer_unretained: sel("commandBufferWithUnretainedReferences"),
        compute_command_encoder: sel("computeCommandEncoder"),
        blit_command_encoder: sel("blitCommandEncoder"),
        render_command_encoder_with_desc: sel("renderCommandEncoderWithDescriptor:"),
        end_encoding: sel("endEncoding"),
        commit: sel("commit"),
        wait_until_completed: sel("waitUntilCompleted"),
        status: sel("status"),
        new_buffer_with_length: sel("newBufferWithLength:options:"),
        new_buffer_with_bytes: sel("newBufferWithBytes:length:options:"),
        new_buffer_with_bytes_no_copy: sel("newBufferWithBytesNoCopy:length:options:deallocator:"),
        contents: sel("contents"),
        length: sel("length"),
        new_library_with_source: sel("newLibraryWithSource:options:error:"),
        new_function_with_name: sel("newFunctionWithName:"),
        new_compute_pipeline_with_function: sel("newComputePipelineStateWithFunction:error:"),
        new_render_pipeline_with_desc: sel("newRenderPipelineStateWithDescriptor:error:"),
        set_compute_pipeline_state: sel("setComputePipelineState:"),
        set_buffer: sel("setBuffer:offset:atIndex:"),
        set_bytes: sel("setBytes:length:atIndex:"),
        set_texture: sel("setTexture:atIndex:"),
        set_sampler_state: sel("setSamplerState:atIndex:"),
        dispatch_threadgroups: sel("dispatchThreadgroups:threadsPerThreadgroup:"),
        copy_from_buffer: sel(
            "copyFromBuffer:sourceOffset:toBuffer:destinationOffset:size:",
        ),
        copy_from_texture_to_buffer: sel(
            "copyFromTexture:sourceSlice:sourceLevel:sourceOrigin:sourceSize:toBuffer:destinationOffset:destinationBytesPerRow:destinationBytesPerImage:",
        ),
        copy_from_texture_to_texture: sel(
            "copyFromTexture:sourceSlice:sourceLevel:sourceOrigin:sourceSize:toTexture:destinationSlice:destinationLevel:destinationOrigin:",
        ),
        fill_buffer: sel("fillBuffer:range:value:"),
        new_counter_sample_buffer: sel("newCounterSampleBufferWithDescriptor:error:"),
        sample_counters_in_buffer: sel("sampleCountersInBuffer:atSampleIndex:withBarrier:"),
        resolve_counter_range: sel("resolveCounterRange:"),
        supports_counter_sampling: sel("supportsCounterSampling:"),
        tex_desc_2d: sel("texture2DDescriptorWithPixelFormat:width:height:mipmapped:"),
        set_usage: sel("setUsage:"),
        set_storage_mode: sel("setStorageMode:"),
        set_mipmap_level_count: sel("setMipmapLevelCount:"),
        set_sample_count: sel("setSampleCount:"),
        set_texture_type: sel("setTextureType:"),
        set_array_length: sel("setArrayLength:"),
        set_depth: sel("setDepth:"),
        new_texture_with_desc: sel("newTextureWithDescriptor:"),
        replace_region: sel("replaceRegion:mipmapLevel:slice:withBytes:bytesPerRow:bytesPerImage:"),
        get_bytes: sel("getBytes:bytesPerRow:bytesPerImage:fromRegion:mipmapLevel:slice:"),
        width: sel("width"),
        height: sel("height"),
        mipmap_level_count: sel("mipmapLevelCount"),
        pixel_format: sel("pixelFormat"),
        alloc: sel("alloc"),
        init: sel("init"),
        set_min_filter: sel("setMinFilter:"),
        set_mag_filter: sel("setMagFilter:"),
        set_mip_filter: sel("setMipFilter:"),
        set_s_address_mode: sel("setSAddressMode:"),
        set_t_address_mode: sel("setTAddressMode:"),
        set_r_address_mode: sel("setRAddressMode:"),
        set_compare_function: sel("setCompareFunction:"),
        new_sampler_state: sel("newSamplerStateWithDescriptor:"),
        set_vertex_function: sel("setVertexFunction:"),
        set_fragment_function: sel("setFragmentFunction:"),
        color_attachments: sel("colorAttachments"),
        object_at_indexed_subscript: sel("objectAtIndexedSubscript:"),
        set_pixel_format: sel("setPixelFormat:"),
        set_write_mask: sel("setWriteMask:"),
        set_blending_enabled: sel("setBlendingEnabled:"),
        set_source_rgb_blend_factor: sel("setSourceRGBBlendFactor:"),
        set_destination_rgb_blend_factor: sel("setDestinationRGBBlendFactor:"),
        set_rgb_blend_operation: sel("setRgbBlendOperation:"),
        set_source_alpha_blend_factor: sel("setSourceAlphaBlendFactor:"),
        set_destination_alpha_blend_factor: sel("setDestinationAlphaBlendFactor:"),
        set_alpha_blend_operation: sel("setAlphaBlendOperation:"),
        set_depth_attachment_pixel_format: sel("setDepthAttachmentPixelFormat:"),
        vertex_descriptor: sel("vertexDescriptor"),
        attributes: sel("attributes"),
        layouts: sel("layouts"),
        set_format: sel("setFormat:"),
        set_offset: sel("setOffset:"),
        set_buffer_index: sel("setBufferIndex:"),
        set_stride: sel("setStride:"),
        set_step_function: sel("setStepFunction:"),
        set_step_rate: sel("setStepRate:"),
        set_depth_write_enabled: sel("setDepthWriteEnabled:"),
        set_depth_compare_function: sel("setDepthCompareFunction:"),
        new_depth_stencil_state: sel("newDepthStencilStateWithDescriptor:"),
        render_pass_descriptor: sel("renderPassDescriptor"),
        depth_attachment: sel("depthAttachment"),
        set_attachment_texture: sel("setTexture:"),
        set_load_action: sel("setLoadAction:"),
        set_store_action: sel("setStoreAction:"),
        set_clear_color: sel("setClearColor:"),
        set_clear_depth: sel("setClearDepth:"),
        set_slice: sel("setSlice:"),
        set_level: sel("setLevel:"),
        set_render_pipeline_state: sel("setRenderPipelineState:"),
        set_depth_stencil_state: sel("setDepthStencilState:"),
        set_cull_mode: sel("setCullMode:"),
        set_front_facing_winding: sel("setFrontFacingWinding:"),
        set_depth_bias: sel("setDepthBias:slopeScale:clamp:"),
        set_viewport: sel("setViewport:"),
        set_scissor_rect: sel("setScissorRect:"),
        set_vertex_buffer: sel("setVertexBuffer:offset:atIndex:"),
        set_vertex_bytes: sel("setVertexBytes:length:atIndex:"),
        set_fragment_buffer: sel("setFragmentBuffer:offset:atIndex:"),
        set_fragment_bytes: sel("setFragmentBytes:length:atIndex:"),
        set_vertex_texture: sel("setVertexTexture:atIndex:"),
        set_fragment_texture: sel("setFragmentTexture:atIndex:"),
        set_vertex_sampler_state: sel("setVertexSamplerState:atIndex:"),
        set_fragment_sampler_state: sel("setFragmentSamplerState:atIndex:"),
        draw_primitives: sel("drawPrimitives:vertexStart:vertexCount:instanceCount:baseInstance:"),
        draw_indexed_primitives: sel(
            "drawIndexedPrimitives:indexCount:indexType:indexBuffer:indexBufferOffset:instanceCount:baseVertex:baseInstance:",
        ),
        cls_ns_string: cls("NSString"),
        cls_mtl_compile_options: cls("MTLCompileOptions"),
        cls_mtl_texture_descriptor: cls("MTLTextureDescriptor"),
        cls_mtl_sampler_descriptor: cls("MTLSamplerDescriptor"),
        cls_mtl_render_pipeline_descriptor: cls("MTLRenderPipelineDescriptor"),
        cls_mtl_depth_stencil_descriptor: cls("MTLDepthStencilDescriptor"),
        cls_mtl_render_pass_descriptor: cls("MTLRenderPassDescriptor"),
        cls_mtl_counter_sample_buffer_descriptor: cls("MTLCounterSampleBufferDescriptor"),
        cls_mtl_compute_pipeline_descriptor: cls("MTLComputePipelineDescriptor"),
        set_compute_function: sel("setComputeFunction:"),
        set_thread_group_size_is_multiple: sel("setThreadGroupSizeIsMultipleOfThreadExecutionWidth:"),
        new_compute_pipeline_with_descriptor: sel("newComputePipelineStateWithDescriptor:options:reflection:error:"),
        set_fast_math_enabled: sel("setFastMathEnabled:"),
        counter_sets: sel("counterSets"),
        count: sel("count"),
        set_counter_set: sel("setCounterSet:"),
        set_sample_count_counter: sel("setSampleCount:"),
        gpu_start_time: sel("GPUStartTime"),
        gpu_end_time: sel("GPUEndTime"),
    })
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
    #[inline]
    pub fn from_retained(ptr: *mut c_void) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(Self(ptr))
        }
    }

    #[inline]
    pub fn from_unretained(ptr: *mut c_void) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            #[cfg(target_os = "macos")]
            unsafe {
                let s = selectors();
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                    std::mem::transmute(objc_msgSend as *const c_void);
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
        #[cfg(target_os = "macos")]
        unsafe {
            let s = selectors();
            let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            f(self.0, s.retain);
        }
        Self(self.0)
    }
}

impl Drop for ObjCObject {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if !self.0.is_null() {
            unsafe {
                let s = selectors();
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                    std::mem::transmute(objc_msgSend as *const c_void);
                f(self.0, s.release);
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub unsafe fn nsstring_from_str(st: &str) -> *mut c_void {
    let s = selectors();
    let c = CString::new(st).unwrap_or_default();
    let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const c_void);
    f(s.cls_ns_string, s.string_with_utf8, c.as_ptr())
}

#[cfg(target_os = "macos")]
pub unsafe fn nsstring_to_string(ns: *mut c_void) -> String {
    if ns.is_null() {
        return String::new();
    }
    let s = selectors();
    let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *const c_char =
        std::mem::transmute(objc_msgSend as *const c_void);
    let cstr = f(ns, s.utf8_string);
    if cstr.is_null() {
        String::new()
    } else {
        CStr::from_ptr(cstr).to_string_lossy().into_owned()
    }
}

#[cfg(target_os = "macos")]
pub unsafe fn nserror_description(err: *mut c_void) -> String {
    if err.is_null() {
        return "unknown Metal error".to_string();
    }
    let s = selectors();
    let f: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const c_void);
    let desc = f(err, s.localized_description);
    nsstring_to_string(desc)
}

#[derive(Clone)]
pub struct MetalDevice {
    pub raw: Arc<ObjCObject>,
    pub queue: Arc<ObjCObject>,
    pub name: String,
    pub has_unified_memory: bool,
    pub max_buffer_length: u64,
    pub recommended_working_set: u64,
    // Caches for zero-overhead pipeline and buffer reuse
    pub compute_pipeline_cache: Arc<Mutex<std::collections::HashMap<u64, Arc<ObjCObject>>>>,
    pub render_pipeline_cache: Arc<Mutex<std::collections::HashMap<u64, (Arc<ObjCObject>, Option<Arc<ObjCObject>>)>>>,
    pub buffer_pool: Arc<Mutex< Vec<(usize, u64, Arc<ObjCObject>, *mut u8)> >>,
    pub last_cmd_buf: Arc<Mutex<Option<(Arc<ObjCObject>, u64)>>>,
    pub(crate) pending_compute_ops: Arc<Mutex<Vec<crate::ComputeDispatchOp>>>,
    pub next_submission_index: Arc<std::sync::atomic::AtomicU64>,
}

unsafe impl Send for MetalDevice {}
unsafe impl Sync for MetalDevice {}

impl MetalDevice {
    #[cfg(target_os = "macos")]
    pub fn create_system_default() -> Option<Self> {
        unsafe {
            let dev_ptr = MTLCreateSystemDefaultDevice();
            let raw = Arc::new(ObjCObject::from_retained(dev_ptr)?);
            let s = selectors();

            let f_ptr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_bool: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_u64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64 =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_queue: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);

            let name_ns = f_ptr(raw.as_ptr(), s.name);
            let name = nsstring_to_string(name_ns);
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
                last_cmd_buf: Arc::new(Mutex::new(None)),
                pending_compute_ops: Arc::new(Mutex::new(Vec::with_capacity(32))),
                next_submission_index: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            })
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn create_system_default() -> Option<Self> {
        None
    }

    #[cfg(target_os = "macos")]
    pub fn alloc_buffer(&self, size: usize, options: u64) -> (Arc<ObjCObject>, *mut u8) {
        let aligned_size = ((size.max(16) + 255) & !255).max(256);
        if let Ok(mut pool) = self.buffer_pool.lock() {
            if let Some(pos) = pool
                .iter()
                .position(|(sz, opt, _, _)| *sz == aligned_size && *opt == options)
            {
                let (_, _, buf, ptr) = pool.swap_remove(pos);
                if !ptr.is_null() {
                    unsafe {
                        ptr::write_bytes(ptr, 0, size);
                    }
                }
                return (buf, ptr);
            }
        }
        unsafe {
            let s = selectors();
            let f_buf: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, u64) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_contents: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut u8 =
                std::mem::transmute(objc_msgSend as *const c_void);
            let raw_buf = f_buf(self.raw.as_ptr(), s.new_buffer_with_length, aligned_size, options);
            let ptr = if (options & 0xF0) == (2 << 4) {
                // MTLStorageModePrivate has no CPU contents pointer
                ptr::null_mut()
            } else {
                f_contents(raw_buf, s.contents)
            };
            (Arc::new(ObjCObject(raw_buf)), ptr)
        }
    }

    #[cfg(target_os = "macos")]
    pub fn alloc_buffer_with_bytes(&self, bytes: &[u8], options: u64) -> (Arc<ObjCObject>, *mut u8) {
        if bytes.is_empty() {
            return self.alloc_buffer(16, options);
        }
        let aligned_size = ((bytes.len() + 255) & !255).max(256);
        if let Ok(mut pool) = self.buffer_pool.lock() {
            if let Some(pos) = pool
                .iter()
                .position(|(sz, opt, _, ptr)| *sz == aligned_size && *opt == options && !ptr.is_null())
            {
                let (_, _, buf, ptr) = pool.swap_remove(pos);
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
                }
                return (buf, ptr);
            }
        }
        unsafe {
            let s = selectors();
            let f_buf: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, u64) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_contents: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut u8 =
                std::mem::transmute(objc_msgSend as *const c_void);
            let raw_buf = f_buf(self.raw.as_ptr(), s.new_buffer_with_length, aligned_size, options);
            let ptr = f_contents(raw_buf, s.contents);
            if !ptr.is_null() {
                ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            }
            (Arc::new(ObjCObject(raw_buf)), ptr)
        }
    }

    /// ASD (Apple Silicon Direct / Accelerated Storage Direct) zero-copy page-aligned buffer creation
    #[cfg(target_os = "macos")]
    pub unsafe fn alloc_buffer_no_copy(
        &self,
        page_aligned_ptr: *mut c_void,
        page_aligned_len: usize,
        options: u64,
    ) -> Option<(Arc<ObjCObject>, *mut u8)> {
        let s = selectors();
        let f_nocopy: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
            usize,
            u64,
            *mut c_void,
        ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
        let raw_buf = f_nocopy(
            self.raw.as_ptr(),
            s.new_buffer_with_bytes_no_copy,
            page_aligned_ptr,
            page_aligned_len,
            options,
            ptr::null_mut(),
        );
        if raw_buf.is_null() {
            return None;
        }
        Some((Arc::new(ObjCObject(raw_buf)), page_aligned_ptr as *mut u8))
    }

    pub fn recycle_buffer(&self, size: usize, options: u64, buf: Arc<ObjCObject>, ptr: *mut u8) {
        if Arc::strong_count(&buf) == 1 && !ptr.is_null() {
            let aligned_size = ((size.max(16) + 255) & !255).max(256);
            if let Ok(mut pool) = self.buffer_pool.lock() {
                if pool.len() < 256 {
                    pool.push((aligned_size, options, buf, ptr));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn compile_compute_pipeline(
        &self,
        msl_source: &str,
        entry_point: &str,
        wg_total: u32,
    ) -> Result<Arc<ObjCObject>, String> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        msl_source.hash(&mut hasher);
        entry_point.hash(&mut hasher);
        let key = hasher.finish();

        if let Ok(cache) = self.compute_pipeline_cache.lock() {
            if let Some(pso) = cache.get(&key) {
                return Ok(pso.clone());
            }
        }

        unsafe {
            let s = selectors();
            let src_ns = nsstring_from_str(msl_source);
            let mut err: *mut c_void = ptr::null_mut();
            let f_alloc: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_init: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_set_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) =
                std::mem::transmute(objc_msgSend as *const c_void);
            let opts_ptr = f_init(f_alloc(s.cls_mtl_compile_options, s.alloc), s.init);
            let opts = ObjCObject(opts_ptr);
            f_set_bool(opts.as_ptr(), s.set_fast_math_enabled, true);

            let f_lib: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut *mut c_void,
            ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);

            let lib_ptr = f_lib(
                self.raw.as_ptr(),
                s.new_library_with_source,
                src_ns,
                opts.as_ptr(),
                &mut err,
            );
            if lib_ptr.is_null() {
                return Err(format!(
                    "MSL compilation failed: {}\n--- MSL SOURCE ---\n{}",
                    nserror_description(err),
                    msl_source
                ));
            }
            let lib = ObjCObject(lib_ptr);

            let fn_ns = nsstring_from_str(entry_point);
            let f_fn: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let func_ptr = f_fn(lib.as_ptr(), s.new_function_with_name, fn_ns);
            if func_ptr.is_null() {
                return Err(format!(
                    "MSL function `{entry_point}` not found in library\n--- MSL ---\n{msl_source}"
                ));
            }
            let func = ObjCObject(func_ptr);

            let mut pso_err: *mut c_void = ptr::null_mut();
            let f_set_obj: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
                std::mem::transmute(objc_msgSend as *const c_void);
            let cpd_ptr = f_init(f_alloc(s.cls_mtl_compute_pipeline_descriptor, s.alloc), s.init);
            let cpd = ObjCObject(cpd_ptr);
            f_set_obj(cpd.as_ptr(), s.set_compute_function, func.as_ptr());
            f_set_bool(
                cpd.as_ptr(),
                s.set_thread_group_size_is_multiple,
                wg_total >= 32 && wg_total % 32 == 0,
            );

            let f_pso_desc: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                usize,
                *mut c_void,
                *mut *mut c_void,
            ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
            let pso_ptr = f_pso_desc(
                self.raw.as_ptr(),
                s.new_compute_pipeline_with_descriptor,
                cpd.as_ptr(),
                0,
                ptr::null_mut(),
                &mut pso_err,
            );
            if pso_ptr.is_null() {
                return Err(format!(
                    "MTLComputePipelineState creation failed: {}",
                    nserror_description(pso_err)
                ));
            }
            let pso = Arc::new(ObjCObject(pso_ptr));
            if let Ok(mut cache) = self.compute_pipeline_cache.lock() {
                cache.insert(key, pso.clone());
            }
            Ok(pso)
        }
    }

    #[cfg(target_os = "macos")]
    pub fn flush_pending_compute(&self) {
        let ops: Vec<crate::ComputeDispatchOp> = self
            .pending_compute_ops
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default();
        if ops.is_empty() {
            return;
        }
        unsafe {
            let s = selectors();
            let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                std::mem::transmute(objc_msgSend as *const c_void);

            let cmd_buf_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
            if cmd_buf_ptr.is_null() {
                return;
            }
            let cmd_buf = Arc::new(ObjCObject::from_unretained(cmd_buf_ptr).unwrap());
            let enc = f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
            crate::encode_compute_dispatches(enc, &ops);
            f_void(enc, s.end_encoding);
            f_void(cmd_buf.as_ptr(), s.commit);
            let idx = self.next_submission_index.load(std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut last) = self.last_cmd_buf.lock() {
                *last = Some((cmd_buf, idx));
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn wait_idle(&self) {
        self.flush_pending_compute();
        let cmd_opt = self.last_cmd_buf.lock().ok().and_then(|mut g| g.take());
        if let Some((cmd_buf, _)) = cmd_opt {
            unsafe {
                let s = selectors();
                let f_status: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize =
                    std::mem::transmute(objc_msgSend as *const c_void);
                let st = f_status(cmd_buf.as_ptr(), s.status);
                // MTLCommandBufferStatusCompleted = 4, Error = 5
                if st < 4 {
                    let f_wait: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                        std::mem::transmute(objc_msgSend as *const c_void);
                    f_wait(cmd_buf.as_ptr(), s.wait_until_completed);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn is_last_cmd_completed(&self) -> bool {
        if let Ok(pending) = self.pending_compute_ops.lock() {
            if !pending.is_empty() {
                return false;
            }
        }
        let Ok(guard) = self.last_cmd_buf.lock() else {
            return true;
        };
        let Some((cmd_buf, _)) = guard.as_ref() else {
            return true;
        };
        unsafe {
            let s = selectors();
            let f_status: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize =
                std::mem::transmute(objc_msgSend as *const c_void);
            f_status(cmd_buf.as_ptr(), s.status) >= 4
        }
    }

    pub fn create_render_pipeline(&self, desc: &crate::RenderPipelineDescriptor<'_>) -> crate::RenderPipeline {
        use std::hash::{Hash, Hasher};
        let vs_ep_raw = desc.vertex.entry_point.unwrap_or("vs");
        let vs_ep = if vs_ep_raw == "main" {
            "_wgpu_entry_main"
        } else {
            vs_ep_raw
        };
        let fs_state = desc.fragment.as_ref();
        let fs_ep_raw = fs_state.and_then(|f| f.entry_point).unwrap_or("fs");
        let fs_ep = if fs_ep_raw == "main" {
            "_wgpu_entry_main"
        } else {
            fs_ep_raw
        };

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        desc.vertex.module.msl_source.hash(&mut hasher);
        vs_ep.hash(&mut hasher);
        fs_ep.hash(&mut hasher);
        for vb in desc.vertex.buffers.iter().flatten() {
            vb.array_stride.hash(&mut hasher);
            vb.step_mode.hash(&mut hasher);
            for attr in vb.attributes {
                attr.format.hash(&mut hasher);
                attr.offset.hash(&mut hasher);
                attr.shader_location.hash(&mut hasher);
            }
        }
        if let Some(fs) = fs_state {
            for target in fs.targets.iter().flatten() {
                target.format.hash(&mut hasher);
                target.write_mask.hash(&mut hasher);
                target.blend.is_some().hash(&mut hasher);
            }
        }
        if let Some(ds) = &desc.depth_stencil {
            ds.format.hash(&mut hasher);
            ds.depth_write_enabled.hash(&mut hasher);
            ds.depth_compare.hash(&mut hasher);
        }
        let cache_key = hasher.finish();

        if let Ok(cache) = self.render_pipeline_cache.lock() {
            if let Some((pso, dss)) = cache.get(&cache_key) {
                return crate::RenderPipeline {
                    pso: pso.clone(),
                    depth_stencil_state: dss.clone(),
                    primitive: desc.primitive,
                    depth_bias: desc.depth_stencil.as_ref().map(|d| d.bias).unwrap_or_default(),
                };
            }
        }

        #[cfg(target_os = "macos")]
        unsafe {
            let s = selectors();
            let f_alloc: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_init: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_set_obj: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_set_u64: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_set_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_get_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_sub: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);

            let f_lib: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut *mut c_void,
            ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
            let mut err: *mut c_void = ptr::null_mut();
            let vs_src_ns = nsstring_from_str(&desc.vertex.module.msl_source);
            let vs_lib_ptr = f_lib(
                self.raw.as_ptr(),
                s.new_library_with_source,
                vs_src_ns,
                ptr::null_mut(),
                &mut err,
            );
            if vs_lib_ptr.is_null() {
                panic!(
                    "Vertex MSL compilation failed: {}\n--- MSL ---\n{}",
                    nserror_description(err),
                    desc.vertex.module.msl_source
                );
            }
            let vs_lib = ObjCObject(vs_lib_ptr);

            let f_fn: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let vs_fn_ptr = f_fn(
                vs_lib.as_ptr(),
                s.new_function_with_name,
                nsstring_from_str(vs_ep),
            );
            assert!(!vs_fn_ptr.is_null(), "Vertex function `{vs_ep}` not found");
            let vs_fn = ObjCObject(vs_fn_ptr);

            let rpd_ptr = f_init(
                f_alloc(s.cls_mtl_render_pipeline_descriptor, s.alloc),
                s.init,
            );
            let rpd = ObjCObject(rpd_ptr);
            f_set_obj(rpd.as_ptr(), s.set_vertex_function, vs_fn.as_ptr());

            let _fs_lib_holder: Option<ObjCObject>;
            let _fs_fn_holder: Option<ObjCObject>;
            if let Some(fs) = fs_state {
                let fs_lib = if ptr::eq(
                    desc.vertex.module.msl_source.as_ptr(),
                    fs.module.msl_source.as_ptr(),
                ) {
                    vs_lib.clone()
                } else {
                    let fs_src_ns = nsstring_from_str(&fs.module.msl_source);
                    let p = f_lib(
                        self.raw.as_ptr(),
                        s.new_library_with_source,
                        fs_src_ns,
                        ptr::null_mut(),
                        &mut err,
                    );
                    if p.is_null() {
                        panic!(
                            "Fragment MSL compilation failed: {}",
                            nserror_description(err)
                        );
                    }
                    ObjCObject(p)
                };
                let fs_fn_ptr = f_fn(
                    fs_lib.as_ptr(),
                    s.new_function_with_name,
                    nsstring_from_str(fs_ep),
                );
                assert!(
                    !fs_fn_ptr.is_null(),
                    "Fragment function `{fs_ep}` not found"
                );
                let fs_fn = ObjCObject(fs_fn_ptr);
                f_set_obj(rpd.as_ptr(), s.set_fragment_function, fs_fn.as_ptr());

                let color_atts = f_get_obj(rpd.as_ptr(), s.color_attachments);
                for (i, target_opt) in fs.targets.iter().enumerate() {
                    if let Some(target) = target_opt {
                        let att = f_sub(color_atts, s.object_at_indexed_subscript, i);
                        f_set_u64(att, s.set_pixel_format, target.format.to_mtl_pixel_format());
                        f_set_u64(att, s.set_write_mask, target.write_mask.bits() as u64);
                        if let Some(blend) = &target.blend {
                            f_set_bool(att, s.set_blending_enabled, true);
                            f_set_u64(
                                att,
                                s.set_source_rgb_blend_factor,
                                blend.color.src_factor.to_mtl(),
                            );
                            f_set_u64(
                                att,
                                s.set_destination_rgb_blend_factor,
                                blend.color.dst_factor.to_mtl(),
                            );
                            f_set_u64(
                                att,
                                s.set_rgb_blend_operation,
                                blend.color.operation.to_mtl(),
                            );
                            f_set_u64(
                                att,
                                s.set_source_alpha_blend_factor,
                                blend.alpha.src_factor.to_mtl(),
                            );
                            f_set_u64(
                                att,
                                s.set_destination_alpha_blend_factor,
                                blend.alpha.dst_factor.to_mtl(),
                            );
                            f_set_u64(
                                att,
                                s.set_alpha_blend_operation,
                                blend.alpha.operation.to_mtl(),
                            );
                        }
                    }
                }
                _fs_lib_holder = Some(fs_lib);
                _fs_fn_holder = Some(fs_fn);
            }

            if !desc.vertex.buffers.is_empty() {
                let vd = f_get_obj(rpd.as_ptr(), s.vertex_descriptor);
                let attrs = f_get_obj(vd, s.attributes);
                let layouts = f_get_obj(vd, s.layouts);
                for (slot_idx, vb_opt) in desc.vertex.buffers.iter().enumerate() {
                    let Some(vb) = vb_opt else { continue };
                    let mtl_buf_idx = gpu_shader::back::msl::MSL_VERTEX_BUFFER_BASE_SLOT as usize + slot_idx;
                    let layout = f_sub(layouts, s.object_at_indexed_subscript, mtl_buf_idx);
                    f_set_u64(layout, s.set_stride, vb.array_stride);
                    let step_fn = match vb.step_mode {
                        crate::VertexStepMode::Vertex => 1u64,
                        crate::VertexStepMode::Instance => 2u64,
                    };
                    f_set_u64(layout, s.set_step_function, step_fn);
                    f_set_u64(layout, s.set_step_rate, 1);

                    for attr in vb.attributes {
                        let a_desc = f_sub(
                            attrs,
                            s.object_at_indexed_subscript,
                            attr.shader_location as usize,
                        );
                        f_set_u64(a_desc, s.set_format, attr.format.to_mtl());
                        f_set_u64(a_desc, s.set_offset, attr.offset);
                        f_set_u64(a_desc, s.set_buffer_index, mtl_buf_idx as u64);
                    }
                }
            }

            let mut depth_stencil_state = None;
            if let Some(ds) = &desc.depth_stencil {
                f_set_u64(
                    rpd.as_ptr(),
                    s.set_depth_attachment_pixel_format,
                    ds.format.to_mtl_pixel_format(),
                );
                let dsd_ptr = f_init(
                    f_alloc(s.cls_mtl_depth_stencil_descriptor, s.alloc),
                    s.init,
                );
                let dsd = ObjCObject(dsd_ptr);
                f_set_bool(
                    dsd.as_ptr(),
                    s.set_depth_write_enabled,
                    ds.depth_write_enabled.unwrap_or(false),
                );
                f_set_u64(
                    dsd.as_ptr(),
                    s.set_depth_compare_function,
                    ds.depth_compare.unwrap_or(crate::CompareFunction::Always).to_mtl(),
                );
                let f_new_dss: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
                let dss_ptr = f_new_dss(
                    self.raw.as_ptr(),
                    s.new_depth_stencil_state,
                    dsd.as_ptr(),
                );
                if !dss_ptr.is_null() {
                    depth_stencil_state = Some(Arc::new(ObjCObject(dss_ptr)));
                }
            }

            if desc.multisample.count > 1 {
                f_set_u64(rpd.as_ptr(), s.set_sample_count, desc.multisample.count as u64);
            }

            let f_new_rpso: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut *mut c_void,
            ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
            let mut pso_err: *mut c_void = ptr::null_mut();
            let pso_ptr = f_new_rpso(
                self.raw.as_ptr(),
                s.new_render_pipeline_with_desc,
                rpd.as_ptr(),
                &mut pso_err,
            );
            if pso_ptr.is_null() {
                panic!(
                    "MTLRenderPipelineState creation failed: {}",
                    nserror_description(pso_err)
                );
            }
            let pso = Arc::new(ObjCObject(pso_ptr));
            if let Ok(mut cache) = self.render_pipeline_cache.lock() {
                cache.insert(cache_key, (pso.clone(), depth_stencil_state.clone()));
            }
            crate::RenderPipeline {
                pso,
                depth_stencil_state,
                primitive: desc.primitive,
                depth_bias: desc.depth_stencil.as_ref().map(|d| d.bias).unwrap_or_default(),
            }
        }
    }

    pub fn create_texture(&self, desc: &crate::TextureDescriptor<'_>) -> crate::Texture {
        #[cfg(target_os = "macos")]
        unsafe {
            let s = selectors();
            let f_td: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                u64,
                usize,
                usize,
                bool,
            ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
            let f_set_u64: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_new_tex: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
            ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);

            let td = f_td(
                s.cls_mtl_texture_descriptor,
                s.tex_desc_2d,
                desc.format.to_mtl_pixel_format(),
                desc.size.width.max(1) as usize,
                desc.size.height.max(1) as usize,
                desc.mip_level_count > 1,
            );
            f_set_u64(
                td,
                s.set_mipmap_level_count,
                desc.mip_level_count.max(1) as u64,
            );
            if desc.sample_count > 1 {
                f_set_u64(td, s.set_sample_count, desc.sample_count as u64);
                f_set_u64(td, s.set_texture_type, 4);
            } else if desc.dimension == crate::TextureDimension::D3 {
                f_set_u64(td, s.set_texture_type, 7);
                f_set_u64(
                    td,
                    s.set_depth,
                    desc.size.depth_or_array_layers.max(1) as u64,
                );
            } else if desc.size.depth_or_array_layers > 1 {
                f_set_u64(td, s.set_texture_type, 3);
                f_set_u64(
                    td,
                    s.set_array_length,
                    desc.size.depth_or_array_layers as u64,
                );
            }

            let mut mtl_usage = 0u64;
            if desc.usage.contains(crate::TextureUsages::TEXTURE_BINDING)
                || desc.usage.contains(crate::TextureUsages::COPY_SRC)
            {
                mtl_usage |= 1;
            }
            if desc.usage.contains(crate::TextureUsages::STORAGE_BINDING) {
                mtl_usage |= 2;
            }
            if desc.usage.contains(crate::TextureUsages::RENDER_ATTACHMENT) {
                mtl_usage |= 4;
            }
            f_set_u64(td, s.set_usage, mtl_usage.max(1));

            let is_depth = matches!(
                desc.format,
                crate::TextureFormat::Depth32Float | crate::TextureFormat::Depth24Plus
            );
            let storage_mode =
                if is_depth || desc.sample_count > 1 || !self.has_unified_memory {
                    2u64
                } else {
                    0u64
                };
            f_set_u64(td, s.set_storage_mode, storage_mode);

            let tex_ptr = f_new_tex(self.raw.as_ptr(), s.new_texture_with_desc, td);
            assert!(!tex_ptr.is_null(), "Failed to create MTLTexture");
            crate::Texture {
                raw: Arc::new(ObjCObject(tex_ptr)),
                size: desc.size,
                mip_level_count: desc.mip_level_count,
                sample_count: desc.sample_count,
                dimension: desc.dimension,
                format: desc.format,
                usage: desc.usage,
                storage_mode,
            }
        }
    }

    pub fn create_sampler(&self, desc: &crate::SamplerDescriptor<'_>) -> crate::Sampler {
        #[cfg(target_os = "macos")]
        unsafe {
            let s = selectors();
            let f_alloc: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_init: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_set_u64: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_new_samp: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
            ) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);

            let sd_ptr = f_init(f_alloc(s.cls_mtl_sampler_descriptor, s.alloc), s.init);
            let sd = ObjCObject(sd_ptr);
            f_set_u64(sd.as_ptr(), s.set_min_filter, desc.min_filter.to_mtl());
            f_set_u64(sd.as_ptr(), s.set_mag_filter, desc.mag_filter.to_mtl());
            let mip = match desc.mipmap_filter {
                crate::MipmapFilterMode::Nearest => 1u64,
                crate::MipmapFilterMode::Linear => 2u64,
            };
            f_set_u64(sd.as_ptr(), s.set_mip_filter, mip);
            f_set_u64(sd.as_ptr(), s.set_s_address_mode, desc.address_mode_u.to_mtl());
            f_set_u64(sd.as_ptr(), s.set_t_address_mode, desc.address_mode_v.to_mtl());
            f_set_u64(sd.as_ptr(), s.set_r_address_mode, desc.address_mode_w.to_mtl());
            if let Some(cmp) = desc.compare {
                f_set_u64(sd.as_ptr(), s.set_compare_function, cmp.to_mtl());
            }

            let samp_ptr = f_new_samp(self.raw.as_ptr(), s.new_sampler_state, sd.as_ptr());
            crate::Sampler {
                raw: Arc::new(ObjCObject(samp_ptr)),
            }
        }
    }

    pub fn write_buffer(&self, buffer: &crate::Buffer, offset: crate::BufferAddress, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.flush_pending_compute();
        if !self.is_last_cmd_completed() {
            self.wait_idle();
        }
        if buffer.inner.ptr.is_null() {
            #[cfg(target_os = "macos")]
            unsafe {
                let (stg_raw, _) = self.alloc_buffer_with_bytes(data, 0);
                let s = selectors();
                let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                    std::mem::transmute(objc_msgSend as *const c_void);
                let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                    std::mem::transmute(objc_msgSend as *const c_void);
                let f_copy: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    usize,
                    *mut c_void,
                    usize,
                    usize,
                ) = std::mem::transmute(objc_msgSend as *const c_void);
                let cmd_buf = f_obj(self.queue.as_ptr(), s.command_buffer);
                let blit = f_obj(cmd_buf, s.blit_command_encoder);
                f_copy(
                    blit,
                    s.copy_from_buffer,
                    stg_raw.as_ptr(),
                    0,
                    buffer.inner.raw.as_ptr(),
                    offset as usize,
                    data.len(),
                );
                f_void(blit, s.end_encoding);
                f_void(cmd_buf, s.commit);
                f_void(cmd_buf, s.wait_until_completed);
            }
            return;
        }
        unsafe {
            let dst = buffer.inner.ptr.add(offset as usize);
            ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }
    }

    pub fn write_texture(
        &self,
        texture: crate::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: crate::TexelCopyBufferLayout,
        size: crate::Extent3d,
    ) {
        if data.is_empty() || size.width == 0 || size.height == 0 {
            return;
        }
        #[cfg(target_os = "macos")]
        unsafe {
            let s = selectors();
            let bpr = data_layout
                .bytes_per_row
                .unwrap_or(size.width * texture.texture.format.bytes_per_pixel())
                as usize;
            let bpi = data_layout
                .rows_per_image
                .map(|r| r as usize * bpr)
                .unwrap_or(size.height as usize * bpr);
            let src_ptr = data.as_ptr().add(data_layout.offset as usize) as *const c_void;

            if texture.texture.storage_mode == 0 {
                let f_rep: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    MTLRegion,
                    usize,
                    usize,
                    *const c_void,
                    usize,
                    usize,
                ) = std::mem::transmute(objc_msgSend as *const c_void);
                let depth = if texture.texture.dimension == crate::TextureDimension::D3 {
                    size.depth_or_array_layers.max(1) as usize
                } else {
                    1usize
                };
                let slices = if texture.texture.dimension == crate::TextureDimension::D3 {
                    1usize
                } else {
                    size.depth_or_array_layers.max(1) as usize
                };
                for slice_idx in 0..slices {
                    let region = MTLRegion {
                        origin: MTLOrigin {
                            x: texture.origin.x as usize,
                            y: texture.origin.y as usize,
                            z: if texture.texture.dimension == crate::TextureDimension::D3 {
                                texture.origin.z as usize
                            } else {
                                0
                            },
                        },
                        size: MTLSize {
                            width: size.width as usize,
                            height: size.height as usize,
                            depth,
                        },
                    };
                    let slice_ptr = (src_ptr as *const u8).add(slice_idx * bpi) as *const c_void;
                    f_rep(
                        texture.texture.raw.as_ptr(),
                        s.replace_region,
                        region,
                        texture.mip_level as usize,
                        if texture.texture.dimension == crate::TextureDimension::D3 {
                            0
                        } else {
                            texture.origin.z as usize + slice_idx
                        },
                        slice_ptr,
                        bpr,
                        bpi,
                    );
                }
            }
        }
    }

    pub fn submit<I: IntoIterator<Item = crate::CommandBuffer>>(
        &self,
        command_buffers: I,
    ) -> crate::SubmissionIndex {
        use crate::{ComputeDispatchOp, RecordedOp, SubmissionIndex, encode_compute_dispatches, encode_non_compute_op};
        use std::sync::atomic::Ordering;

        let idx = self.next_submission_index.fetch_add(1, Ordering::Relaxed);

        #[cfg(target_os = "macos")]
        unsafe {
            let cbs: Vec<crate::CommandBuffer> = command_buffers.into_iter().collect();
            let is_pure_untimed_compute = !cbs.is_empty()
                && cbs.iter().all(|cb| {
                    cb.on_submit_callbacks.is_empty()
                        && !cb.ops.is_empty()
                        && cb.ops.iter().all(|op| {
                            matches!(
                                op,
                                RecordedOp::ComputePass {
                                    dispatches,
                                    start_ts: None,
                                    end_ts: None,
                                } if !dispatches.is_empty()
                            )
                        })
                });
            if is_pure_untimed_compute {
                let mut should_flush = false;
                if let Ok(mut pending) = self.pending_compute_ops.lock() {
                    for cb in cbs {
                        for op in cb.ops {
                            if let RecordedOp::ComputePass { mut dispatches, .. } = op {
                                pending.append(&mut dispatches);
                            }
                        }
                    }
                    if pending.len() >= 1 {
                        should_flush = true;
                    }
                }
                if should_flush {
                    self.flush_pending_compute();
                }
                return SubmissionIndex(idx);
            }

            let is_callback_only = cbs.iter().all(|cb| cb.ops.is_empty());
            if is_callback_only {
                self.wait_idle();
                for cb in cbs {
                    for callback in cb.on_submit_callbacks {
                        callback();
                    }
                }
                return SubmissionIndex(idx);
            }

            let is_small_uma_copy = cbs.len() == 1
                && cbs[0].ops.len() == 1
                && matches!(
                    &cbs[0].ops[0],
                    RecordedOp::CopyBufferToBuffer { src, dst, size, .. }
                        if *size <= 65536 && !src.inner.ptr.is_null() && !dst.inner.ptr.is_null()
                );
            if is_small_uma_copy {
                self.wait_idle();
                let mut cb = cbs.into_iter().next().unwrap();
                if let Some(RecordedOp::CopyBufferToBuffer {
                    src,
                    src_offset,
                    dst,
                    dst_offset,
                    size,
                }) = cb.ops.pop()
                {
                    if size > 0 {
                        ptr::copy_nonoverlapping(
                            src.inner.ptr.add(src_offset as usize),
                            dst.inner.ptr.add(dst_offset as usize),
                            size as usize,
                        );
                    }
                }
                for callback in cb.on_submit_callbacks {
                    callback();
                }
                return SubmissionIndex(idx);
            }

            let mut pending: Vec<ComputeDispatchOp> = self
                .pending_compute_ops
                .lock()
                .map(|mut v| std::mem::take(&mut *v))
                .unwrap_or_default();

            let s = selectors();
            let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                std::mem::transmute(objc_msgSend as *const c_void);

            let cmd_buf_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
            assert!(!cmd_buf_ptr.is_null(), "Failed to create MTLCommandBuffer");
            let cmd_buf = Arc::new(ObjCObject::from_unretained(cmd_buf_ptr).unwrap());

            let mut active_compute_enc: *mut c_void = ptr::null_mut();
            if !pending.is_empty() {
                active_compute_enc = f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
                encode_compute_dispatches(active_compute_enc, &pending);
                pending.clear();
            }
            let mut callbacks: Vec<Box<dyn FnOnce() + Send>> = Vec::new();

            for mut cb in cbs {
                callbacks.append(&mut cb.on_submit_callbacks);
                for op in cb.ops {
                    match op {
                        RecordedOp::ComputePass {
                            dispatches,
                            start_ts,
                            end_ts,
                        } => {
                            if dispatches.is_empty() {
                                if let Some((qs, i)) = start_ts {
                                    encode_non_compute_op(
                                        self,
                                        cmd_buf.as_ptr(),
                                        RecordedOp::WriteTimestamp {
                                            query_set: qs,
                                            index: i,
                                        },
                                    );
                                }
                                if let Some((qs, i)) = end_ts {
                                    encode_non_compute_op(
                                        self,
                                        cmd_buf.as_ptr(),
                                        RecordedOp::WriteTimestamp {
                                            query_set: qs,
                                            index: i,
                                        },
                                    );
                                }
                                continue;
                            }
                            if start_ts.is_some() || end_ts.is_some() {
                                if !active_compute_enc.is_null() {
                                    f_void(active_compute_enc, s.end_encoding);
                                    active_compute_enc = ptr::null_mut();
                                }
                                let sub_cmd_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
                                let sub_enc = f_obj(sub_cmd_ptr, s.compute_command_encoder);
                                encode_compute_dispatches(sub_enc, &dispatches);
                                f_void(sub_enc, s.end_encoding);
                                f_void(sub_cmd_ptr, s.commit);
                                f_void(sub_cmd_ptr, s.wait_until_completed);

                                let f_f64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f64 =
                                    std::mem::transmute(objc_msgSend as *const c_void);
                                let t_start_s = f_f64(sub_cmd_ptr, s.gpu_start_time);
                                let t_end_s = f_f64(sub_cmd_ptr, s.gpu_end_time);
                                let now_ns = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_nanos() as u64)
                                    .unwrap_or(0);
                                let (start_ns, end_ns) =
                                    if t_start_s > 0.0 && t_end_s >= t_start_s {
                                        ((t_start_s * 1e9) as u64, (t_end_s * 1e9) as u64)
                                    } else {
                                        (now_ns, now_ns + 1000)
                                    };
                                if let Some((qs, i)) = start_ts {
                                    if let Ok(mut ts) = qs.timestamps_ns.lock() {
                                        if (i as usize) < ts.len() {
                                            ts[i as usize] = start_ns;
                                        }
                                    }
                                }
                                if let Some((qs, i)) = end_ts {
                                    if let Ok(mut ts) = qs.timestamps_ns.lock() {
                                        if (i as usize) < ts.len() {
                                            ts[i as usize] = end_ns;
                                        }
                                    }
                                }
                                continue;
                            }
                            if active_compute_enc.is_null() {
                                active_compute_enc =
                                    f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
                            }
                            encode_compute_dispatches(active_compute_enc, &dispatches);
                        }
                        RecordedOp::RenderPass(mut rp) => {
                            if !active_compute_enc.is_null() {
                                f_void(active_compute_enc, s.end_encoding);
                                active_compute_enc = ptr::null_mut();
                            }
                            let start_ts = rp.start_ts.take();
                            let end_ts = rp.end_ts.take();
                            if start_ts.is_some() || end_ts.is_some() {
                                let sub_cmd_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
                                crate::encode_render_pass(sub_cmd_ptr, rp);
                                f_void(sub_cmd_ptr, s.commit);
                                f_void(sub_cmd_ptr, s.wait_until_completed);

                                let f_f64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f64 =
                                    std::mem::transmute(objc_msgSend as *const c_void);
                                let t_start_s = f_f64(sub_cmd_ptr, s.gpu_start_time);
                                let t_end_s = f_f64(sub_cmd_ptr, s.gpu_end_time);
                                let now_ns = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_nanos() as u64)
                                    .unwrap_or(0);
                                let (start_ns, end_ns) =
                                    if t_start_s > 0.0 && t_end_s > t_start_s {
                                        ((t_start_s * 1e9) as u64, (t_end_s * 1e9) as u64)
                                    } else {
                                        (now_ns, now_ns + 1000)
                                    };
                                if let Some((qs, i)) = start_ts {
                                    if let Ok(mut ts) = qs.timestamps_ns.lock() {
                                        if (i as usize) < ts.len() {
                                            ts[i as usize] = start_ns;
                                        }
                                    }
                                }
                                if let Some((qs, i)) = end_ts {
                                    if let Ok(mut ts) = qs.timestamps_ns.lock() {
                                        if (i as usize) < ts.len() {
                                            ts[i as usize] = end_ns;
                                        }
                                    }
                                }
                                continue;
                            }
                            crate::encode_render_pass(cmd_buf.as_ptr(), rp);
                        }
                        other => {
                            if !active_compute_enc.is_null() {
                                f_void(active_compute_enc, s.end_encoding);
                                active_compute_enc = ptr::null_mut();
                            }
                            encode_non_compute_op(self, cmd_buf.as_ptr(), other);
                        }
                    }
                }
            }

            if !active_compute_enc.is_null() {
                f_void(active_compute_enc, s.end_encoding);
            }

            f_void(cmd_buf.as_ptr(), s.commit);
            if let Ok(mut last) = self.last_cmd_buf.lock() {
                *last = Some((cmd_buf, idx));
            }
            if !callbacks.is_empty() {
                self.wait_idle();
                for cb in callbacks {
                    cb();
                }
            }
        }

        SubmissionIndex(idx)
    }
}

pub use selectors as metal_selectors;
pub use objc_msgSend as metal_msg_send;
