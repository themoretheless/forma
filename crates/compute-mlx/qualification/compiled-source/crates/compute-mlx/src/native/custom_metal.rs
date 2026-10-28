//! The optional MLX-C custom Metal ABI. A context caches complete source keys;
//! globally unique names also prevent MLX's name-based library cache from
//! returning a different source's compiled kernel.
use super::*;
use std::{
    ffi::CString,
    hash::{Hash, Hasher},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct KernelKey {
    pub source: String,
    pub header: &'static str,
    pub inputs: &'static [&'static str],
    pub zeroed_atomic_u32: bool,
}

pub(super) struct Kernel {
    raw: Handle,
    api: Arc<Api>,
}
impl Drop for Kernel {
    fn drop(&mut self) {
        let _ = self.api.call(|a| {
            if let Some(m) = &a.metal {
                unsafe { (m.fast_metal_kernel_free)(self.raw) };
            }
            Ok(())
        });
    }
}

impl MlxBackend {
    fn custom_kernel(&self, key: KernelKey) -> Result<Rc<Kernel>, MlxError> {
        if let Some(kernel) = self.context.kernels.borrow().get(&key) {
            return Ok(kernel.clone());
        }
        static NEXT_KERNEL: AtomicU64 = AtomicU64::new(0);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hash);
        let name = CString::new(format!(
            "compute_mlx_{:016x}_{}",
            hash.finish(),
            NEXT_KERNEL.fetch_add(1, Ordering::Relaxed)
        ))
        .unwrap();
        let source = CString::new(key.source.as_str()).expect("internal Metal source has no NUL");
        let header = CString::new(key.header).expect("internal Metal header has no NUL");
        let names: Vec<_> = key
            .inputs
            .iter()
            .map(|n| CString::new(*n).unwrap())
            .collect();
        let pointers: Vec<_> = names.iter().map(|n| n.as_ptr()).collect();
        let output = [c"out".as_ptr()];
        let raw = native(
            "create custom Metal kernel",
            self.context.api.call(|a| unsafe {
                let m = a
                    .metal
                    .as_ref()
                    .ok_or("MLX-C custom Metal ABI is unavailable")?;
                let inputs = (m.vector_string_new_data)(pointers.as_ptr(), pointers.len());
                let outputs = (m.vector_string_new_data)(output.as_ptr(), output.len());
                if inputs.ctx.is_null() || outputs.ctx.is_null() {
                    (m.vector_string_free)(inputs);
                    (m.vector_string_free)(outputs);
                    return Err("MLX could not allocate kernel argument names".into());
                }
                let raw = (m.fast_metal_kernel_new)(
                    name.as_ptr(),
                    inputs,
                    outputs,
                    source.as_ptr(),
                    header.as_ptr(),
                    false,
                    key.zeroed_atomic_u32,
                );
                (m.vector_string_free)(inputs);
                (m.vector_string_free)(outputs);
                if raw.ctx.is_null() {
                    Err("MLX returned an empty custom Metal kernel".into())
                } else {
                    Ok(raw)
                }
            }),
        )?;
        let kernel = Rc::new(Kernel {
            raw,
            api: self.context.api.clone(),
        });
        self.context
            .kernels
            .borrow_mut()
            .insert(key, kernel.clone());
        Ok(kernel)
    }

    /// Output and grid sizes have already been checked by the operation. No
    /// input is materialized: generated shape/stride arguments address views.
    pub(super) fn custom_metal(
        &self,
        key: KernelKey,
        inputs: &[&MlxTensor],
        shape: Shape,
        dtype: MlxDtype,
        bf16: bool,
        launch: (usize, usize),
    ) -> Result<MlxTensor, MlxError> {
        let dimensions = Self::dimensions(&shape)?;
        let grid = i32::try_from(launch.0).map_err(|_| MlxError::TooLarge)?;
        let group = i32::try_from(launch.1).map_err(|_| MlxError::TooLarge)?;
        if shape.is_empty() || grid <= 0 || group <= 0 || group > 256 || group > grid {
            return Err(MlxError::Native {
                operation: "custom Metal launch",
                message: "invalid internal dispatch geometry".into(),
            });
        }
        for input in inputs {
            self.check(input, None)?;
        }
        let zeroed_atomic = key.zeroed_atomic_u32;
        if zeroed_atomic && dtype != MlxDtype::U32 {
            return Err(MlxError::Dtype);
        }
        let kernel = self.custom_kernel(key)?;
        let handles: Vec<_> = inputs.iter().map(|t| t.array.raw).collect();
        self.output("custom Metal", shape, dtype, |a, out| unsafe {
            // custom_kernel checked this optional symbol group before output().
            let m = a.metal.as_ref().unwrap();
            let input_vector = (m.vector_array_new_data)(handles.as_ptr(), handles.len());
            let mut outputs = (m.vector_array_new)();
            let config = (m.fast_metal_kernel_config_new)();
            let result = (|| {
                if input_vector.ctx.is_null() || outputs.ctx.is_null() || config.ctx.is_null() {
                    return Err("MLX could not allocate custom Metal arguments".to_owned());
                }
                Api::check((m.fast_metal_kernel_config_add_output_arg)(
                    config,
                    dimensions.as_ptr(),
                    dimensions.len(),
                    dtype.raw(),
                ))?;
                Api::check((m.fast_metal_kernel_config_set_grid)(config, grid, 1, 1))?;
                Api::check((m.fast_metal_kernel_config_set_thread_group)(
                    config, group, 1, 1,
                ))?;
                if zeroed_atomic {
                    Api::check((m.fast_metal_kernel_config_set_init_value)(config, 0.0))?;
                }
                Api::check((m.fast_metal_kernel_config_add_template_arg_bool)(
                    config,
                    c"BF".as_ptr(),
                    bf16,
                ))?;
                Api::check((m.fast_metal_kernel_config_add_template_arg_dtype)(
                    config,
                    c"OUT".as_ptr(),
                    dtype.raw(),
                ))?;
                Api::check((m.fast_metal_kernel_apply)(
                    &mut outputs,
                    kernel.raw,
                    input_vector,
                    config,
                    self.context.stream,
                ))?;
                Api::check((m.vector_array_get)(out, outputs, 0))
            })();
            (m.vector_array_free)(input_vector);
            (m.vector_array_free)(outputs);
            (m.fast_metal_kernel_config_free)(config);
            i32::from(result.is_err())
        })
    }
}
