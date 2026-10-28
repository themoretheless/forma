//! Callback-only ownership: all raw handles are freed directly while the
//! enclosing Api::call holds its global lock. Never construct Array here.
use super::*;
use crate::native::lowering::raw::{self, Arrays};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(super) struct Graph {
    pub api: Arc<Api>,
    pub stream: Handle,
    pub nodes: Vec<PreparedNode>,
    pub input_specs: Vec<TensorSpec>,
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

fn trace_graph(graph: &Graph, output: *mut Handle, input: Handle) -> Result<(), String> {
    let api = &graph.api;
    let compile = api
        .compile
        .as_ref()
        .ok_or("MLX compilation symbols unavailable")?;
    graph.state.count.fetch_add(1, Ordering::Relaxed);
    let mut arrays = Arrays::new(api);
    let count = unsafe { (compile.vector_array_size)(input) };
    if count != graph.input_specs.len() {
        return Err("MLX callback input count mismatch".into());
    }
    let mut arguments = Vec::with_capacity(count);
    for (index, spec) in graph.input_specs.iter().enumerate() {
        let raw = arrays.record(|out| unsafe { (compile.vector_array_get)(out, input, index) })?;
        validate_array(api, raw, &spec.shape, spec.dtype)?;
        arguments.push(raw);
    }
    let mut values: Vec<Handle> = Vec::with_capacity(graph.nodes.len());
    for node in &graph.nodes {
        let value = match &node.operation {
            PreparedOp::Input(slot) => arguments[*slot],
            PreparedOp::Constant(raw) => *raw,
            PreparedOp::Native(op, indices) => {
                let inputs: Vec<_> = indices.iter().map(|&i| values[i]).collect();
                arrays.record_result(|out| {
                    if node.spec.shape.is_empty() {
                        raw::apply(api, graph.stream, &NativeOp::Zeros, &[], &node.spec, out)
                    } else {
                        raw::apply(api, graph.stream, op, &inputs, &node.spec, out)
                    }
                })?
            }
            PreparedOp::Metal(kernel, indices, config) => {
                let inputs: Vec<_> = indices.iter().map(|&i| values[i]).collect();
                arrays.record_result(|out| {
                    custom_metal::apply_raw(api, graph.stream, *kernel, &inputs, config, out)
                })?
            }
        };
        validate_array(api, value, &node.spec.shape, node.spec.dtype)?;
        values.push(value);
    }
    let selected: Vec<_> = graph.outputs.iter().map(|&i| values[i]).collect();
    // set_data copies array ownership into the C-owned callback result vector.
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
                PreparedNode {
                    spec: TensorSpec {
                        shape: input_shape.clone(),
                        dtype: MlxDtype::F32,
                    },
                    operation: PreparedOp::Input(0),
                },
                // Deliberately bypass the public builder to test a native error
                // after input and contiguous temporary handles were acquired.
                PreparedNode {
                    spec: TensorSpec {
                        shape: Shape::new(vec![3]).unwrap(),
                        dtype: MlxDtype::F32,
                    },
                    operation: PreparedOp::Native(NativeOp::Reshape, vec![0]),
                },
            ],
            input_specs: vec![TensorSpec {
                shape: input_shape.clone(),
                dtype: MlxDtype::F32,
            }],
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
