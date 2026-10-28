//! Callback-only ownership: all raw handles are freed directly while the
//! enclosing Api::call holds its global lock. Never construct Array here.
use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(super) struct Graph {
    pub api: Arc<Api>,
    pub stream: Handle,
    pub nodes: Vec<Node>,
    pub input_shapes: Vec<Shape>,
    pub outputs: Vec<usize>,
    pub state: Arc<TraceState>,
}

pub(super) struct Vector<'a> {
    pub raw: Handle,
    api: &'a ffi::CompileApi,
}
impl<'a> Vector<'a> {
    pub fn new(api: &'a ffi::CompileApi, raw: Handle) -> Result<Self, String> {
        if raw.ctx.is_null() {
            Err("MLX failed to create an array vector".into())
        } else {
            Ok(Self { raw, api })
        }
    }
}
impl Drop for Vector<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.vector_array_free)(self.raw);
        }
    }
}

struct Arrays<'a> {
    api: &'a Api,
    handles: Vec<Handle>,
}
impl<'a> Arrays<'a> {
    fn new(api: &'a Api) -> Self {
        Self {
            api,
            handles: Vec::new(),
        }
    }
    fn empty(&mut self) {
        let raw = unsafe { (self.api.array_new)() };
        // MLX's empty array constructor intentionally returns a null
        // placeholder. The following setter populates it through Handle*.
        self.handles.push(raw);
    }
    fn record(&mut self, operation: impl FnOnce(*mut Handle) -> i32) -> Result<Handle, String> {
        self.empty();
        let raw = self.handles.last_mut().expect("just appended");
        // C set_ operations may replace the handle, so track the updated value
        // even if the operation fails or a later validation rejects its result.
        Api::check(operation(raw))?;
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

fn trace_graph(graph: &Graph, output: *mut Handle, input: Handle) -> Result<(), String> {
    let api = &graph.api;
    let compile = api
        .compile
        .as_ref()
        .ok_or("MLX compilation symbols unavailable")?;
    graph.state.count.fetch_add(1, Ordering::Relaxed);
    let mut arrays = Arrays::new(api);
    let count = unsafe { (compile.vector_array_size)(input) };
    if count != graph.input_shapes.len() {
        return Err("MLX callback input count mismatch".into());
    }
    let mut arguments = Vec::with_capacity(count);
    for (index, shape) in graph.input_shapes.iter().enumerate() {
        let raw = arrays.record(|out| unsafe { (compile.vector_array_get)(out, input, index) })?;
        validate_array(api, raw, shape, MlxDtype::F32)?;
        arguments.push(raw);
    }
    let mut values: Vec<Handle> = Vec::with_capacity(graph.nodes.len());
    let stream = graph.stream;
    for node in &graph.nodes {
        let raw = match &node.operation {
            Operation::Input(slot) => arguments[*slot],
            Operation::Alias(value) => values[*value],
            op => {
                let raw = if node.shape.is_empty() || matches!(op, Operation::Zeros) {
                    arrays.record(|out| unsafe {
                        (api.zeros)(
                            out,
                            node.dimensions.as_ptr(),
                            node.dimensions.len(),
                            ffi::F32,
                            stream,
                        )
                    })?
                } else {
                    let contiguous = if let Operation::Reshape(value) = op {
                        Some(arrays.record(|out| unsafe {
                            (api.contiguous)(out, values[*value], false, stream)
                        })?)
                    } else {
                        None
                    };
                    arrays.record(|out| unsafe {
                        match op {
                            Operation::Unary(value, op) => {
                                unary_function(api, *op)(out, values[*value], stream)
                            }
                            Operation::Binary(left, right, op) => binary_function(api, *op)(
                                out,
                                values[*left],
                                values[*right],
                                stream,
                            ),
                            Operation::Sum(value, axes, keep) => (api.sum_axes)(
                                out,
                                values[*value],
                                axes.as_ptr(),
                                axes.len(),
                                *keep,
                                stream,
                            ),
                            Operation::Reshape(_) => (api.reshape)(
                                out,
                                contiguous.expect("reshape contiguous input"),
                                node.dimensions.as_ptr(),
                                node.dimensions.len(),
                                stream,
                            ),
                            Operation::Permute(value, axes) => (api.transpose_axes)(
                                out,
                                values[*value],
                                axes.as_ptr(),
                                axes.len(),
                                stream,
                            ),
                            Operation::Broadcast(value) => (api.broadcast_to)(
                                out,
                                values[*value],
                                node.dimensions.as_ptr(),
                                node.dimensions.len(),
                                stream,
                            ),
                            Operation::Matmul(left, right) => {
                                (api.matmul)(out, values[*left], values[*right], stream)
                            }
                            Operation::Input(_) | Operation::Alias(_) | Operation::Zeros => {
                                unreachable!("handled before recording")
                            }
                        }
                    })?
                };
                validate_array(api, raw, &node.shape, MlxDtype::F32)?;
                raw
            }
        };
        values.push(raw);
    }
    let selected: Vec<_> = graph.outputs.iter().map(|&i| values[i]).collect();
    // The result vector is native-owned. set_data copies the arrays into it,
    // retaining their lazy graphs before our temporary array handles are freed.
    Api::check(unsafe {
        (compile.vector_array_set_data)(output, selected.as_ptr(), selected.len())
    })
}

fn boundary(state: &TraceState, operation: impl FnOnce() -> Result<(), String>) -> i32 {
    let error = match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => return 0,
        Ok(Err(message)) => message,
        Err(_) => "Rust panic contained inside the MLX program trace".to_owned(),
    };
    *state.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
    1
}

pub(super) unsafe extern "C" fn trace(
    output: *mut Handle,
    input: Handle,
    payload: *mut c_void,
) -> i32 {
    if payload.is_null() || output.is_null() || input.ctx.is_null() {
        return 1;
    }
    let graph = unsafe { &*payload.cast::<Graph>() };
    boundary(&graph.state, || trace_graph(graph, output, input))
}

pub(super) unsafe extern "C" fn destroy(payload: *mut c_void) {
    if !payload.is_null() {
        // Graph contains only Rust graph data, an Arc<Api> and a borrowed stream
        // handle. It never drops a Context/Array or reacquires the native lock.
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
            drop(Box::from_raw(payload.cast::<Graph>()))
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_contains_panics_releases_locals_and_recovers() {
        struct CountDrop(Arc<AtomicUsize>);
        impl Drop for CountDrop {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let dropped = Arc::new(AtomicUsize::new(0));
        let state = TraceState::default();
        assert_eq!(
            boundary(&state, || {
                let _guard = CountDrop(dropped.clone());
                panic!("injected trace failure");
            }),
            1
        );
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        assert!(
            state
                .error
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .contains("panic contained")
        );
        assert_eq!(boundary(&state, || Err("injected native error".into())), 1);
        assert_eq!(boundary(&state, || Ok(())), 0);
    }

    #[test]
    fn native_callback_failure_releases_arrays_and_allows_next_trace() {
        let api = match Api::global() {
            Ok(api) if api.compile.is_some() => api,
            result => {
                assert!(
                    std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(),
                    "compile ABI unavailable: {}",
                    result.err().unwrap_or_default()
                );
                return;
            }
        };
        let compile = api.compile.as_ref().unwrap();
        let null = Handle {
            ctx: std::ptr::null_mut(),
        };
        let mut stream = null;
        let mut input = null;
        let mut output = null;
        api.call(|a| unsafe {
            let device = (a.device_new_type)(0, 0);
            stream = (a.stream_new_device)(device);
            (a.device_free)(device);
            let mut arrays = Arrays::new(a);
            let values = [1.0_f32, 2.0];
            let dims = [2];
            let array = arrays.record(|out| {
                (a.array_set_data)(out, values.as_ptr().cast(), dims.as_ptr(), 1, ffi::F32)
            })?;
            input = (compile.vector_array_new_data)(&array, 1);
            output = (compile.vector_array_new)();
            Ok(())
        })
        .unwrap();
        let state = Arc::new(TraceState::default());
        let input_shape = Shape::new(vec![2]).unwrap();
        let graph = Box::new(Graph {
            api: api.clone(),
            stream,
            nodes: vec![
                Node {
                    shape: input_shape.clone(),
                    dimensions: vec![2],
                    operation: Operation::Input(0),
                },
                // Deliberately bypass the public builder to test a native error
                // after input and contiguous temporary handles were acquired.
                Node {
                    shape: Shape::new(vec![3]).unwrap(),
                    dimensions: vec![3],
                    operation: Operation::Reshape(0),
                },
            ],
            input_shapes: vec![input_shape.clone()],
            outputs: vec![1],
            state: state.clone(),
        });
        let payload = Box::into_raw(graph);
        let closure = api
            .call(|_| unsafe {
                Ok((compile.closure_new_func_payload)(
                    trace,
                    payload.cast(),
                    destroy,
                ))
            })
            .unwrap();
        let rejected = api
            .call(|_| unsafe { Api::check((compile.closure_apply)(&mut output, closure, input)) });
        // Test-only mutation while no apply is active; the native closure still
        // owns this payload. Production graphs are immutable after construction.
        unsafe {
            (*payload).nodes.pop();
            (*payload).outputs = vec![0];
        }
        let recovered = api.call(|a| unsafe {
            Api::check((compile.closure_apply)(&mut output, closure, input))?;
            let mut arrays = Arrays::new(a);
            let value = arrays.record(|out| (compile.vector_array_get)(out, output, 0))?;
            validate_array(a, value, &input_shape, MlxDtype::F32)
        });
        let cleanup = api.call(|a| unsafe {
            (compile.vector_array_free)(output);
            (compile.vector_array_free)(input);
            (compile.closure_free)(closure);
            Api::check((a.stream_free)(stream))
        });
        assert!(rejected.is_err());
        recovered.unwrap();
        cleanup.unwrap();
        assert_eq!(state.count.load(Ordering::Relaxed), 2);
        assert!(state.error.lock().unwrap().is_some());
    }
}
