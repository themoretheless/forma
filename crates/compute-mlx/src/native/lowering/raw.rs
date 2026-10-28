//! Native operation emission while an outer Api::call owns the global lock.
use super::*;

pub(crate) struct Arrays<'a> {
    api: &'a Api,
    handles: Vec<Handle>,
}
impl<'a> Arrays<'a> {
    pub fn new(api: &'a Api) -> Self {
        Self {
            api,
            handles: Vec::new(),
        }
    }
    pub fn record(&mut self, operation: impl FnOnce(*mut Handle) -> i32) -> Result<Handle, String> {
        self.record_result(|out| Api::check(operation(out)))
    }
    pub fn record_result(
        &mut self,
        operation: impl FnOnce(*mut Handle) -> Result<(), String>,
    ) -> Result<Handle, String> {
        // Null is the valid initial destination placeholder in the MLX-C ABI.
        self.handles.push(unsafe { (self.api.array_new)() });
        let raw = self.handles.last_mut().expect("just appended");
        operation(raw)?;
        if raw.ctx.is_null() {
            return Err("MLX failed to populate an array".into());
        }
        Ok(*raw)
    }
}
impl Drop for Arrays<'_> {
    fn drop(&mut self) {
        for raw in self.handles.drain(..).rev() {
            unsafe {
                (self.api.array_free)(raw);
            }
        }
    }
}

pub(in crate::native) fn apply(
    api: &Api,
    stream: Handle,
    op: &NativeOp,
    inputs: &[Handle],
    output: &TensorSpec,
    out: *mut Handle,
) -> Result<(), String> {
    let arity = match op {
        NativeOp::Zeros | NativeOp::Ones | NativeOp::ArangeU32(_) => 0,
        NativeOp::FastAttention { masked, .. } => {
            if *masked {
                4
            } else {
                3
            }
        }
        NativeOp::Binary(_)
        | NativeOp::RightShift
        | NativeOp::BitwiseAnd
        | NativeOp::Compare(_)
        | NativeOp::Matmul
        | NativeOp::TakeAxis(_) => 2,
        NativeOp::Select | NativeOp::PutAlongAxis(_) | NativeOp::Scatter { .. } => 3,
        _ => 1,
    };
    if inputs.len() != arity {
        return Err("invalid prepared native operation arity".into());
    }
    let dims = MlxBackend::dimensions(&output.shape).map_err(|e| e.to_string())?;
    let mut arrays = Arrays::new(api);
    let code = unsafe {
        match op {
            NativeOp::Unary(op) => unary_function(api, *op)(out, inputs[0], stream),
            NativeOp::Binary(op) => binary_function(api, *op)(out, inputs[0], inputs[1], stream),
            NativeOp::RightShift => (api.right_shift)(out, inputs[0], inputs[1], stream),
            NativeOp::BitwiseAnd => (api.bitwise_and)(out, inputs[0], inputs[1], stream),
            NativeOp::Cast(dtype) => (api.astype)(out, inputs[0], dtype.raw(), stream),
            NativeOp::ViewU32 => (api.view)(out, inputs[0], ffi::U32, stream),
            NativeOp::ViewF32 => (api.view)(out, inputs[0], ffi::F32, stream),
            NativeOp::ArangeU32(count) => {
                (api.arange)(out, 0., *count as f64, 1., ffi::U32, stream)
            }
            NativeOp::Logsumexp(axes, keep) => {
                (api.logsumexp_axes)(out, inputs[0], axes.as_ptr(), axes.len(), *keep, stream)
            }
            NativeOp::Softmax(axes) => {
                (api.softmax_axes)(out, inputs[0], axes.as_ptr(), axes.len(), true, stream)
            }
            NativeOp::FastAttention {
                scale,
                causal,
                masked,
            } => {
                let empty = Handle {
                    ctx: std::ptr::null_mut(),
                };
                let mode = if *causal {
                    c"causal"
                } else if *masked {
                    c"array"
                } else {
                    c""
                };
                api.attention.call(
                    out,
                    inputs[0],
                    inputs[1],
                    inputs[2],
                    *scale,
                    mode.as_ptr(),
                    if *masked { inputs[3] } else { empty },
                    empty,
                    stream,
                )
            }
            NativeOp::PackBf16 => {
                let words = arrays.record(|out| (api.astype)(out, inputs[0], ffi::U16, stream))?;
                (api.view)(out, words, ffi::BF16, stream)
            }
            NativeOp::Compare(op) => {
                let function = compare_function(api, *op);
                let mask = arrays.record(|out| function(out, inputs[0], inputs[1], stream))?;
                (api.astype)(out, mask, ffi::U32, stream)
            }
            NativeOp::Select => (api.r#where)(out, inputs[0], inputs[1], inputs[2], stream),
            NativeOp::Zeros => {
                (api.zeros)(out, dims.as_ptr(), dims.len(), output.dtype.raw(), stream)
            }
            NativeOp::Ones => {
                (api.ones)(out, dims.as_ptr(), dims.len(), output.dtype.raw(), stream)
            }
            NativeOp::Reshape => {
                let input = arrays.record(|out| (api.contiguous)(out, inputs[0], false, stream))?;
                (api.reshape)(out, input, dims.as_ptr(), dims.len(), stream)
            }
            NativeOp::Permute(axes) => {
                (api.transpose_axes)(out, inputs[0], axes.as_ptr(), axes.len(), stream)
            }
            NativeOp::Broadcast => {
                (api.broadcast_to)(out, inputs[0], dims.as_ptr(), dims.len(), stream)
            }
            NativeOp::Reduce(op, axes, keep) => {
                let function = reduce_function(api, *op);
                function(out, inputs[0], axes.as_ptr(), axes.len(), *keep, stream)
            }
            NativeOp::Mean(axes, keep) => {
                (api.mean_axes)(out, inputs[0], axes.as_ptr(), axes.len(), *keep, stream)
            }
            NativeOp::Matmul => (api.matmul)(out, inputs[0], inputs[1], stream),
            NativeOp::Scan {
                axis,
                inclusive,
                reverse,
            } => api.cumsum.call(out, inputs[0], *axis, *reverse, *inclusive, output.dtype.raw(), stream),
            NativeOp::TakeAxis(axis) => (api.take_axis)(out, inputs[0], inputs[1], *axis, stream),
            NativeOp::PutAlongAxis(axis) => {
                (api.put_along_axis)(out, inputs[0], inputs[1], inputs[2], *axis, stream)
            }
            NativeOp::Scatter { op, axis } => {
                let operation = match op {
                    ScatterOp::Add => api.scatter_add_single,
                    ScatterOp::Multiply => api.scatter_prod_single,
                    ScatterOp::Min => api.scatter_min_single,
                    ScatterOp::Max => api.scatter_max_single,
                    ScatterOp::Replace => {
                        return Err("Replace requires checked owner selection".into());
                    }
                };
                operation(out, inputs[0], inputs[1], inputs[2], *axis, stream)
            }
        }
    };
    Api::check(code)
}
