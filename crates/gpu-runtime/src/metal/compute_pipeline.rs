//! Compute pipeline compilation.

use super::*;

impl MetalDevice {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn compile_compute_pipeline(
        &self,
        msl_source: &str,
        entry_point: &str,
        wg_total: u32,
    ) -> Result<Arc<ObjCObject>, String> {
        let threadgroup_multiple_of_32 = wg_total >= 32 && wg_total.is_multiple_of(32);
        let key = ComputePipelineCacheKey {
            msl_source: msl_source.to_string(),
            entry_point: entry_point.to_string(),
            threadgroup_multiple_of_32,
        };

        if let Ok(cache) = self.compute_pipeline_cache.lock() {
            if let Some(pso) = cache.get(&key) {
                return Ok(pso.clone());
            }
        }

        unsafe {
            let s = selectors();
            let src_ns = nsstring_from_str(msl_source);
            let mut err: *mut c_void = ptr::null_mut();
            let f_alloc: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let f_init: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let f_set_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) = msg_send();
            let opts_ptr = f_init(f_alloc(s.cls_mtl_compile_options, s.alloc), s.init);
            let opts = ObjCObject(opts_ptr);
            f_set_bool(opts.as_ptr(), s.set_fast_math_enabled, true);

            let f_lib: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut *mut c_void,
            ) -> *mut c_void = msg_send();

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
                msg_send();
            let func_ptr = f_fn(lib.as_ptr(), s.new_function_with_name, fn_ns);
            if func_ptr.is_null() {
                return Err(format!(
                    "MSL function `{entry_point}` not found in library\n--- MSL ---\n{msl_source}"
                ));
            }
            let func = ObjCObject(func_ptr);

            let mut pso_err: *mut c_void = ptr::null_mut();
            let f_set_obj: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) = msg_send();
            let cpd_ptr = f_init(
                f_alloc(s.cls_mtl_compute_pipeline_descriptor, s.alloc),
                s.init,
            );
            let cpd = ObjCObject(cpd_ptr);
            f_set_obj(cpd.as_ptr(), s.set_compute_function, func.as_ptr());
            f_set_bool(
                cpd.as_ptr(),
                s.set_thread_group_size_is_multiple,
                wg_total >= 32 && wg_total.is_multiple_of(32),
            );

            let f_pso_desc: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                usize,
                *mut c_void,
                *mut *mut c_void,
            ) -> *mut c_void = msg_send();
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
}
