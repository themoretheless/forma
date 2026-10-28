//! Fixed-shape declarative graphs over the public MLX-C compilation API.
use super::{
    custom_metal::{Kernel, KernelKey, MetalConfig},
    lowering::{Lowering, NativeOp, TensorSpec},
    *,
};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

mod attention;
mod builder;
mod callback;
mod compaction;
mod indexing;
mod low_builder;
mod scan;
mod scatter;
mod statistics;
mod typed;
pub use typed::{MlxProgramInput, MlxProgramOutput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MlxValue {
    owner: u64,
    index: usize,
}

#[derive(Debug)]
enum Operation {
    Input(usize),
    Native(NativeOp, Vec<usize>),
    Constant(Vec<u32>),
    Metal(KernelKey, Vec<usize>, MetalConfig),
}
#[derive(Debug)]
struct Node {
    spec: TensorSpec,
    operation: Operation,
}

// Prepared payload data contains borrowed raw handles, never owning wrappers
// whose Drop would reacquire Api::call. Program resources outlive the closure.
enum PreparedOp {
    Input(usize),
    Native(NativeOp, Vec<usize>),
    Constant(Handle),
    Metal(Handle, Vec<usize>, MetalConfig),
}
struct PreparedNode {
    spec: TensorSpec,
    operation: PreparedOp,
}
#[derive(Default)]
struct Resources {
    _kernels: Vec<Rc<Kernel>>,
    _constants: Vec<MlxTensor>,
}

/// A shape-checked graph. Operations record recipes without capturing runtime
/// input allocations. Low recipes share the eager implementation and kernels.
pub struct MlxProgramBuilder {
    backend: MlxBackend,
    owner: u64,
    inputs: Vec<TensorSpec>,
    nodes: Vec<Node>,
}
#[derive(Default)]
struct TraceState {
    count: AtomicUsize,
    error: std::sync::Mutex<Option<String>>,
}

/// A native compiled graph on one backend's explicit GPU stream. Runs produce
/// fresh lazy results; the first valid run traces and later runs may reuse it.
/// Native global compilation settings are observed without changing them.
pub struct MlxCompiledProgram {
    context: Rc<Context>,
    closure: Handle,
    inputs: Vec<TensorSpec>,
    outputs: Vec<TensorSpec>,
    state: Arc<TraceState>,
    _resources: Resources,
}
impl std::fmt::Debug for MlxCompiledProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MlxCompiledProgram")
            .field("inputs", &self.inputs)
            .field("outputs", &self.outputs)
            .field("trace_count", &self.trace_count())
            .finish()
    }
}
impl MlxBackend {
    /// Complete optional compile ABI availability; not an enabled-mode, fusion
    /// or allocation-reuse guarantee. External MLX_DISABLE_COMPILE is respected.
    pub fn compile_available(&self) -> bool {
        self.context.api.compile.is_some()
    }
    pub fn program(&self) -> MlxProgramBuilder {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .expect("MLX program identity space exhausted");
        MlxProgramBuilder {
            backend: self.clone(),
            owner,
            inputs: Vec::new(),
            nodes: Vec::new(),
        }
    }
}
impl MlxProgramBuilder {
    fn value(&self, value: MlxValue) -> Result<&Node, MlxError> {
        if value.owner != self.owner {
            return Err(MlxError::ForeignValue);
        }
        self.nodes.get(value.index).ok_or(MlxError::ForeignValue)
    }
    fn push(&mut self, spec: TensorSpec, operation: Operation) -> Result<MlxValue, MlxError> {
        MlxBackend::dimensions(&spec.shape)?;
        let value = MlxValue {
            owner: self.owner,
            index: self.nodes.len(),
        };
        self.nodes.push(Node { spec, operation });
        Ok(value)
    }
    fn transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, MlxError>,
    ) -> Result<T, MlxError> {
        let nodes = self.nodes.len();
        let inputs = self.inputs.len();
        let result = operation(self);
        if result.is_err() {
            self.nodes.truncate(nodes);
            self.inputs.truncate(inputs);
        }
        result
    }
    /// Freeze the graph and prepare kernel/constant owners before entering the
    /// native callback. Tracing remains lazy; aliases and empty output lists are
    /// legal. No global compiler/default-device settings are changed.
    pub fn compile(self, outputs: &[MlxValue]) -> Result<MlxCompiledProgram, MlxError> {
        let output_specs = outputs
            .iter()
            .map(|&v| self.value(v).map(|n| n.spec.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        let api = &self.backend.context.api;
        let compile = api.compile.as_ref().ok_or(MlxError::CompileUnavailable)?;
        let mut resources = Resources::default();
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for node in self.nodes {
            let operation = match node.operation {
                Operation::Input(slot) => PreparedOp::Input(slot),
                Operation::Native(op, inputs) => PreparedOp::Native(op, inputs),
                Operation::Constant(words) => {
                    let tensor = self.backend.upload_u32(node.spec.shape.clone(), &words)?;
                    let raw = tensor.array.raw;
                    resources._constants.push(tensor);
                    PreparedOp::Constant(raw)
                }
                Operation::Metal(key, inputs, config) => {
                    let kernel = self.backend.custom_kernel(key)?;
                    let raw = kernel.raw;
                    resources._kernels.push(kernel);
                    PreparedOp::Metal(raw, inputs, config)
                }
            };
            nodes.push(PreparedNode {
                spec: node.spec,
                operation,
            });
        }
        let state = Arc::new(TraceState::default());
        let graph = Box::new(callback::Graph {
            api: api.clone(),
            stream: self.backend.context.stream,
            nodes,
            input_specs: self.inputs.clone(),
            outputs: outputs.iter().map(|v| v.index).collect(),
            state: state.clone(),
        });
        let payload = Box::into_raw(graph).cast();
        let mut source = Handle {
            ctx: std::ptr::null_mut(),
        };
        let mut closure = source;
        let result = api.call(|_| unsafe {
            // Payload ownership transfers even if native construction fails.
            source =
                (compile.closure_new_func_payload)(callback::trace, payload, callback::destroy);
            closure = (compile.closure_new)();
            if source.ctx.is_null() {
                return Err("MLX failed to create a closure".into());
            }
            Api::check((compile.compile)(&mut closure, source, false))?;
            if closure.ctx.is_null() {
                return Err("MLX failed to populate a compiled closure".into());
            }
            Ok(())
        });
        let _ = api.call(|_| unsafe {
            if !source.ctx.is_null() {
                (compile.closure_free)(source);
            }
            if result.is_err() && !closure.ctx.is_null() {
                (compile.closure_free)(closure);
            }
            Ok(())
        });
        native("compile program", result)?;
        Ok(MlxCompiledProgram {
            context: self.backend.context.clone(),
            closure,
            inputs: self.inputs,
            outputs: output_specs,
            state,
            _resources: resources,
        })
    }
}
impl Lowering for MlxProgramBuilder {
    type Value = MlxValue;
    fn spec(&self, value: &MlxValue) -> Result<TensorSpec, MlxError> {
        Ok(self.value(*value)?.spec.clone())
    }
    fn native(
        &mut self,
        op: NativeOp,
        inputs: &[MlxValue],
        output: TensorSpec,
    ) -> Result<MlxValue, MlxError> {
        for &value in inputs {
            self.value(value)?;
        }
        self.push(
            output,
            Operation::Native(op, inputs.iter().map(|v| v.index).collect()),
        )
    }
    fn constant_u32(&mut self, shape: Shape, words: &[u32]) -> Result<MlxValue, MlxError> {
        if shape.numel() != words.len() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: words.len(),
            }
            .into());
        }
        self.push(
            TensorSpec {
                shape,
                dtype: MlxDtype::U32,
            },
            Operation::Constant(words.to_vec()),
        )
    }
    fn metal(
        &mut self,
        key: KernelKey,
        inputs: &[MlxValue],
        output: TensorSpec,
        bf16: bool,
        launch: (usize, usize),
    ) -> Result<MlxValue, MlxError> {
        for &value in inputs {
            self.value(value)?;
        }
        if self.backend.context.api.metal.is_none() {
            return Err(MlxError::Unavailable(
                "MLX-C custom Metal ABI is unavailable".into(),
            ));
        }
        let config = MetalConfig::new(
            &output.shape,
            output.dtype,
            bf16,
            launch,
            key.zeroed_atomic_u32,
        )?;
        self.push(
            output,
            Operation::Metal(key, inputs.iter().map(|v| v.index).collect(), config),
        )
    }
}
impl MlxCompiledProgram {
    /// Native callback invocations, not GPU execution count.
    pub fn trace_count(&self) -> usize {
        self.state.count.load(Ordering::Relaxed)
    }
    /// Backward-compatible f32-only transport. Use run_typed for unsigned or low
    /// signatures so low arrays remain behind their checked wrapper.
    pub fn run(&self, inputs: &[&MlxTensor]) -> Result<Vec<MlxTensor>, MlxError> {
        if self
            .inputs
            .iter()
            .chain(&self.outputs)
            .any(|s| s.dtype != MlxDtype::F32)
        {
            return Err(MlxError::Dtype);
        }
        self.run_raw(inputs)
    }
    fn run_raw(&self, inputs: &[&MlxTensor]) -> Result<Vec<MlxTensor>, MlxError> {
        if inputs.len() != self.inputs.len() {
            return Err(MlxError::ProgramInputCount {
                expected: self.inputs.len(),
                actual: inputs.len(),
            });
        }
        for (index, (input, expected)) in inputs.iter().zip(&self.inputs).enumerate() {
            if !Rc::ptr_eq(&self.context, &input.array.context) {
                return Err(MlxError::ForeignContext);
            }
            if input.dtype != expected.dtype {
                return Err(MlxError::Dtype);
            }
            if input.shape != expected.shape {
                return Err(MlxError::ProgramInputShape {
                    index,
                    expected: expected.shape.clone(),
                    actual: input.shape.clone(),
                });
            }
        }
        *self.state.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let api = &self.context.api;
        let compile = api.compile.as_ref().ok_or(MlxError::CompileUnavailable)?;
        let input_handles: Vec<_> = inputs.iter().map(|t| t.array.raw).collect();
        let mut handles = Vec::with_capacity(self.outputs.len());
        let result = api.call(|a| unsafe {
            let input = callback::Vector::new(
                compile,
                (compile.vector_array_new_data)(input_handles.as_ptr(), input_handles.len()),
            )?;
            let mut output = callback::Vector::new(compile, (compile.vector_array_new)())?;
            Api::check((compile.closure_apply)(
                &mut output.raw,
                self.closure,
                input.raw,
            ))?;
            if (compile.vector_array_size)(output.raw) != self.outputs.len() {
                return Err("MLX compiled output count differs from the program".into());
            }
            for (index, spec) in self.outputs.iter().enumerate() {
                handles.push((a.array_new)());
                let raw = handles.last_mut().expect("just appended");
                Api::check((compile.vector_array_get)(raw, output.raw, index))?;
                validate_array(a, *raw, &spec.shape, spec.dtype)?;
            }
            Ok(())
        });
        if let Err(error) = result {
            let _ = api.call(|a| unsafe {
                for raw in handles.drain(..) {
                    (a.array_free)(raw);
                }
                Ok(())
            });
            let message = self
                .state
                .error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .unwrap_or(error);
            return Err(MlxError::Native {
                operation: "run compiled program",
                message,
            });
        }
        Ok(handles
            .into_iter()
            .zip(&self.outputs)
            .map(|(raw, spec)| MlxTensor {
                array: Rc::new(Array {
                    raw,
                    context: self.context.clone(),
                }),
                shape: spec.shape.clone(),
                dtype: spec.dtype,
            })
            .collect())
    }
}
impl Drop for MlxCompiledProgram {
    fn drop(&mut self) {
        if let Some(compile) = self.context.api.compile.as_ref() {
            let _ = self
                .context
                .api
                .call(|_| unsafe { Api::check((compile.closure_free)(self.closure)) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compound_lowering_rolls_back_appended_nodes_after_late_failure() {
        let backend = match MlxBackend::new_gpu() {
            Ok(backend) => backend,
            Err(error) => {
                assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
                return;
            }
        };
        let mut graph = backend.program();
        let input = graph.input(Shape::new(vec![1]).unwrap()).unwrap();
        let before = graph.nodes.len();
        let result: Result<MlxValue, _> = graph.transaction(|graph| {
            graph.constant_u32(Shape::new(vec![])?, &[7])?;
            graph.unary(input, UnaryOp::Square)?;
            Err(MlxError::TooLarge)
        });
        assert!(matches!(result, Err(MlxError::TooLarge)));
        assert_eq!(graph.nodes.len(), before);
        assert_eq!(graph.inputs.len(), 1);
        let output = graph.unary(input, UnaryOp::Negate).unwrap();
        assert_eq!(output.index, before);
        assert!(matches!(
            graph.value(output).unwrap().operation,
            Operation::Native(NativeOp::Unary(UnaryOp::Negate), _)
        ));
    }
}
