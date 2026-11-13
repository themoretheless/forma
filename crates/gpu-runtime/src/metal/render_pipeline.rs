//! Render pipeline creation.

use super::*;

/// The Metal name of a shader entry point: `main` is reserved in MSL.
fn entry_name(raw: &str) -> &str {
    if raw == "main" {
        "_wgpu_entry_main"
    } else {
        raw
    }
}

/// The pipeline handed back to the caller, for a state that was just built or found in the cache.
fn finished_pipeline(
    desc: &crate::RenderPipelineDescriptor<'_>,
    pso: Arc<ObjCObject>,
    depth_stencil_state: Option<Arc<ObjCObject>>,
) -> crate::RenderPipeline {
    crate::RenderPipeline {
        pso: Some(pso),
        vertex_bindings: desc.vertex.module.bindings.clone(),
        fragment_bindings: desc.fragment.as_ref().map(|fs| fs.module.bindings.clone()),
        depth_stencil_state,
        primitive: desc.primitive,
        depth_bias: desc
            .depth_stencil
            .as_ref()
            .map(|d| d.bias)
            .unwrap_or_default(),
    }
}

type MsgSetU64 = unsafe extern "C" fn(*mut c_void, *mut c_void, u64);
type MsgGetObj = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
type MsgSubscript = unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void;

impl MetalDevice {
    pub fn create_render_pipeline(
        &self,
        desc: &crate::RenderPipelineDescriptor<'_>,
    ) -> Result<crate::RenderPipeline, String> {
        let vs_ep = entry_name(desc.vertex.entry_point.unwrap_or("vs"));
        let fs_state = desc.fragment.as_ref();
        let fs_ep = entry_name(fs_state.and_then(|f| f.entry_point).unwrap_or("fs"));

        let vs_src = msl_for_pipeline(
            &desc.vertex.module.msl_source,
            desc.vertex.compilation_options.constants,
        )?;
        let fs_src = fs_state
            .map(|fs| msl_for_pipeline(&fs.module.msl_source, fs.compilation_options.constants))
            .transpose()?;

        let cache_key = RenderPipelineCacheKey {
            vs_src: vs_src.as_ref().to_owned(),
            fs_src: fs_src.as_ref().map(|s| s.as_ref().to_owned()),
            vs_ep: vs_ep.to_owned(),
            fs_ep: fs_ep.to_owned(),
            vertex_buffers: desc
                .vertex
                .buffers
                .iter()
                .map(|vb_opt| {
                    vb_opt.map(|vb| CachedVertexBufferLayout {
                        array_stride: vb.array_stride,
                        step_mode: vb.step_mode,
                        attributes: vb.attributes.to_vec(),
                    })
                })
                .collect(),
            targets: fs_state.map(|fs| fs.targets.to_vec()).unwrap_or_default(),
            depth_stencil: desc
                .depth_stencil
                .as_ref()
                .map(|ds| (ds.format, ds.depth_write_enabled, ds.depth_compare)),
            sample_count: desc.multisample.count,
        };

        if let Ok(cache) = self.render_pipeline_cache.lock() {
            if let Some((pso, dss)) = cache.get(&cache_key) {
                return Ok(finished_pipeline(desc, pso.clone(), dss.clone()));
            }
        }

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        unsafe {
            let s = selectors();
            let f_alloc: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let f_init: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let f_set_obj: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) = msg_send();
            let f_set_u64: MsgSetU64 = msg_send();

            let vs_lib = self.compile_library(&vs_src).map_err(|description| {
                format!("Vertex MSL compilation failed: {description}\n--- MSL ---\n{vs_src}")
            })?;
            let vs_fn = shader_function(&vs_lib, vs_ep)
                .ok_or_else(|| format!("Vertex function `{vs_ep}` not found"))?;

            let rpd_ptr = f_init(
                f_alloc(s.cls_mtl_render_pipeline_descriptor, s.alloc),
                s.init,
            );
            let rpd = ObjCObject(rpd_ptr);
            f_set_obj(rpd.as_ptr(), s.set_vertex_function, vs_fn.as_ptr());

            // The library and the function stay alive until the pipeline state exists.
            let mut _fragment_holders: Option<(ObjCObject, ObjCObject)> = None;
            if let Some(fs) = fs_state {
                let fs_src = fs_src.as_deref().unwrap_or_default();
                let fs_lib = if vs_src.as_ref() == fs_src {
                    vs_lib.clone()
                } else {
                    self.compile_library(fs_src).map_err(|description| {
                        format!("Fragment MSL compilation failed: {description}")
                    })?
                };
                let fs_fn = shader_function(&fs_lib, fs_ep)
                    .ok_or_else(|| format!("Fragment function `{fs_ep}` not found"))?;
                f_set_obj(rpd.as_ptr(), s.set_fragment_function, fs_fn.as_ptr());
                configure_color_targets(&rpd, fs.targets);
                _fragment_holders = Some((fs_lib, fs_fn));
            }

            configure_vertex_layout(&rpd, desc.vertex.buffers);
            let depth_stencil_state = desc
                .depth_stencil
                .as_ref()
                .and_then(|ds| self.configure_depth_stencil(&rpd, ds));

            if desc.multisample.count > 1 {
                f_set_u64(
                    rpd.as_ptr(),
                    s.set_sample_count,
                    desc.multisample.count as u64,
                );
            }

            let f_new_rpso: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut *mut c_void,
            ) -> *mut c_void = msg_send();
            let mut pso_err: *mut c_void = ptr::null_mut();
            let pso_ptr = f_new_rpso(
                self.raw.as_ptr(),
                s.new_render_pipeline_with_desc,
                rpd.as_ptr(),
                &mut pso_err,
            );
            if pso_ptr.is_null() {
                return Err(format!(
                    "MTLRenderPipelineState creation failed: {}",
                    nserror_description(pso_err)
                ));
            }
            let pso = Arc::new(ObjCObject(pso_ptr));
            if let Ok(mut cache) = self.render_pipeline_cache.lock() {
                cache.insert(cache_key, (pso.clone(), depth_stencil_state.clone()));
            }
            Ok(finished_pipeline(desc, pso, depth_stencil_state))
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        unimplemented!("Metal render pipelines are only available on macOS and iOS")
    }

    /// Compiles MSL into a library, or returns Metal's description of why it would not.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe fn compile_library(&self, source: &str) -> Result<ObjCObject, String> {
        let s = selectors();
        let f_lib: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
            *mut c_void,
            *mut *mut c_void,
        ) -> *mut c_void = msg_send();
        let mut err: *mut c_void = ptr::null_mut();
        let library = f_lib(
            self.raw.as_ptr(),
            s.new_library_with_source,
            nsstring_from_str(source),
            ptr::null_mut(),
            &mut err,
        );
        if library.is_null() {
            return Err(nserror_description(err));
        }
        Ok(ObjCObject(library))
    }

    /// Sets the depth format on the descriptor and builds the depth-stencil state object.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe fn configure_depth_stencil(
        &self,
        rpd: &ObjCObject,
        ds: &crate::DepthStencilState,
    ) -> Option<Arc<ObjCObject>> {
        let s = selectors();
        let f_alloc: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
        let f_init: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
        let f_set_u64: MsgSetU64 = msg_send();
        let f_set_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) = msg_send();
        f_set_u64(
            rpd.as_ptr(),
            s.set_depth_attachment_pixel_format,
            ds.format.to_mtl_pixel_format(),
        );
        let dsd_ptr = f_init(f_alloc(s.cls_mtl_depth_stencil_descriptor, s.alloc), s.init);
        let dsd = ObjCObject(dsd_ptr);
        f_set_bool(
            dsd.as_ptr(),
            s.set_depth_write_enabled,
            ds.depth_write_enabled.unwrap_or(false),
        );
        f_set_u64(
            dsd.as_ptr(),
            s.set_depth_compare_function,
            ds.depth_compare
                .unwrap_or(crate::CompareFunction::Always)
                .to_mtl(),
        );
        let f_new_dss: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
            msg_send();
        let dss_ptr = f_new_dss(self.raw.as_ptr(), s.new_depth_stencil_state, dsd.as_ptr());
        (!dss_ptr.is_null()).then(|| Arc::new(ObjCObject(dss_ptr)))
    }
}

/// Looks a function up in a compiled library.
#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn shader_function(library: &ObjCObject, name: &str) -> Option<ObjCObject> {
    let f_fn: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
        msg_send();
    let function = f_fn(
        library.as_ptr(),
        selectors().new_function_with_name,
        nsstring_from_str(name),
    );
    (!function.is_null()).then(|| ObjCObject(function))
}

/// Pixel format, write mask and blending of each colour attachment.
#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn configure_color_targets(rpd: &ObjCObject, targets: &[Option<crate::ColorTargetState>]) {
    let s = selectors();
    let f_set_u64: MsgSetU64 = msg_send();
    let f_set_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) = msg_send();
    let f_get_obj: MsgGetObj = msg_send();
    let f_sub: MsgSubscript = msg_send();
    let color_atts = f_get_obj(rpd.as_ptr(), s.color_attachments);
    for (i, target_opt) in targets.iter().enumerate() {
        let Some(target) = target_opt else { continue };
        let att = f_sub(color_atts, s.object_at_indexed_subscript, i);
        f_set_u64(att, s.set_pixel_format, target.format.to_mtl_pixel_format());
        f_set_u64(att, s.set_write_mask, target.write_mask.bits() as u64);
        let Some(blend) = &target.blend else { continue };
        f_set_bool(att, s.set_blending_enabled, true);
        let settings = [
            (
                s.set_source_rgb_blend_factor,
                blend.color.src_factor.to_mtl(),
            ),
            (
                s.set_destination_rgb_blend_factor,
                blend.color.dst_factor.to_mtl(),
            ),
            (s.set_rgb_blend_operation, blend.color.operation.to_mtl()),
            (
                s.set_source_alpha_blend_factor,
                blend.alpha.src_factor.to_mtl(),
            ),
            (
                s.set_destination_alpha_blend_factor,
                blend.alpha.dst_factor.to_mtl(),
            ),
            (s.set_alpha_blend_operation, blend.alpha.operation.to_mtl()),
        ];
        for (selector, value) in settings {
            f_set_u64(att, selector, value);
        }
    }
}

/// Stride, step function and attributes of each vertex buffer.
#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn configure_vertex_layout(
    rpd: &ObjCObject,
    buffers: &[Option<crate::VertexBufferLayout<'_>>],
) {
    if buffers.is_empty() {
        return;
    }
    let s = selectors();
    let f_set_u64: MsgSetU64 = msg_send();
    let f_get_obj: MsgGetObj = msg_send();
    let f_sub: MsgSubscript = msg_send();
    let vd = f_get_obj(rpd.as_ptr(), s.vertex_descriptor);
    let attrs = f_get_obj(vd, s.attributes);
    let layouts = f_get_obj(vd, s.layouts);
    for (slot_idx, vb_opt) in buffers.iter().enumerate() {
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
