//! Fixed-shape declarative graphs over the public MLX-C compilation API.
use super::*;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

mod callback;

/// A value in one builder. Tokens from another builder are rejected even when
/// their node indices and shapes happen to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MlxValue {
    owner: u64,
    index: usize,
}

#[derive(Debug)]
enum Operation {
    Input(usize),
    Alias(usize),
    Zeros,
    Unary(usize, UnaryOp),
    Binary(usize, usize, BinaryOp),
    Sum(usize, Vec<i32>, bool),
    Reshape(usize),
    Permute(usize, Vec<i32>),
    Broadcast(usize),
    Matmul(usize, usize),
}

#[derive(Debug)]
struct Node {
    shape: Shape,
    dimensions: Vec<i32>,
    operation: Operation,
}

/// A validated f32 graph. Building a graph does not call native operations or
/// capture tensor allocations. All runtime inputs have explicit fixed shapes.
pub struct MlxProgramBuilder {
    backend: MlxBackend,
    owner: u64,
    inputs: Vec<Shape>,
    nodes: Vec<Node>,
}

#[derive(Default)]
struct TraceState {
    count: AtomicUsize,
    error: std::sync::Mutex<Option<String>>,
}

/// A native compiled graph retained on its backend's GPU stream.
///
/// `run` creates lazy results from new resident inputs. The first run traces
/// the graph; device evaluation still occurs through ordinary read/eval APIs.
/// The runtime's external compile mode is respected and never changed here.
pub struct MlxCompiledProgram {
    context: Rc<Context>,
    closure: Handle,
    inputs: Vec<Shape>,
    outputs: Vec<Shape>,
    state: Arc<TraceState>,
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
    /// Whether all public compilation/closure/vector symbols are available.
    /// This does not inspect or change MLX's global compile mode. In particular,
    /// `MLX_DISABLE_COMPILE` can leave this true while disabling trace reuse.
    pub fn compile_available(&self) -> bool {
        self.context.api.compile.is_some()
    }

    pub fn program(&self) -> MlxProgramBuilder {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
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

    fn push(&mut self, shape: Shape, operation: Operation) -> Result<MlxValue, MlxError> {
        let dimensions = MlxBackend::dimensions(&shape)?;
        let value = MlxValue {
            owner: self.owner,
            index: self.nodes.len(),
        };
        self.nodes.push(Node {
            shape,
            dimensions,
            operation,
        });
        Ok(value)
    }

    pub fn input(&mut self, shape: Shape) -> Result<MlxValue, MlxError> {
        let value = self.push(shape.clone(), Operation::Input(self.inputs.len()))?;
        self.inputs.push(shape);
        Ok(value)
    }

    pub fn unary(&mut self, value: MlxValue, op: UnaryOp) -> Result<MlxValue, MlxError> {
        let shape = self.value(value)?.shape.clone();
        self.push(shape, Operation::Unary(value.index, op))
    }

    pub fn binary(
        &mut self,
        left: MlxValue,
        right: MlxValue,
        op: BinaryOp,
    ) -> Result<MlxValue, MlxError> {
        let shape = self
            .value(left)?
            .shape
            .broadcast(&self.value(right)?.shape)?;
        self.push(shape, Operation::Binary(left.index, right.index, op))
    }

    pub fn sum_axes(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxValue, MlxError> {
        let input = &self.value(value)?.shape;
        let shape =
            tensor_core::reduction_shape(tensor_core::ReduceOp::Sum, input, axes, keep_dims)?;
        let operation = if axes.is_empty() {
            Operation::Alias(value.index)
        } else if input.is_empty() || shape.is_empty() {
            Operation::Zeros
        } else {
            Operation::Sum(
                value.index,
                axes.iter().map(|&axis| axis as i32).collect(),
                keep_dims,
            )
        };
        self.push(shape, operation)
    }

    pub fn reshape(&mut self, value: MlxValue, shape: Shape) -> Result<MlxValue, MlxError> {
        let expected = self.value(value)?.shape.numel();
        if expected != shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected,
                actual: shape.numel(),
            }
            .into());
        }
        self.push(shape, Operation::Reshape(value.index))
    }

    pub fn permute(&mut self, value: MlxValue, axes: &[usize]) -> Result<MlxValue, MlxError> {
        let shape = self.value(value)?.shape.permute(axes)?;
        self.push(
            shape,
            Operation::Permute(value.index, axes.iter().map(|&axis| axis as i32).collect()),
        )
    }

    pub fn broadcast_to(&mut self, value: MlxValue, shape: Shape) -> Result<MlxValue, MlxError> {
        let source = &self.value(value)?.shape;
        if source.broadcast(&shape)? != shape {
            return Err(TensorError::IncompatibleBroadcast {
                left: source.dims().to_vec(),
                right: shape.dims().to_vec(),
            }
            .into());
        }
        self.push(shape, Operation::Broadcast(value.index))
    }

    /// Ordinary f32 matmul, including vector promotion and batch broadcasting.
    pub fn matmul(&mut self, left: MlxValue, right: MlxValue) -> Result<MlxValue, MlxError> {
        let plan =
            tensor_core::MatmulPlan::new(&self.value(left)?.shape, &self.value(right)?.shape)?;
        let k = plan.left.dims()[plan.left.rank() - 1];
        let operation = if plan.output.is_empty() || k == 0 {
            Operation::Zeros
        } else {
            Operation::Matmul(left.index, right.index)
        };
        self.push(plan.output, operation)
    }

    /// Freeze the graph and construct its native compiled closure. Tracing is
    /// deferred until the first valid run. Empty/duplicate/input outputs are
    /// legal; each returned result retains the requested logical shape.
    pub fn compile(self, outputs: &[MlxValue]) -> Result<MlxCompiledProgram, MlxError> {
        let shapes = outputs
            .iter()
            .map(|&v| self.value(v).map(|n| n.shape.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        let api = &self.backend.context.api;
        let compile = api.compile.as_ref().ok_or(MlxError::CompileUnavailable)?;
        let state = Arc::new(TraceState::default());
        let graph = Box::new(callback::Graph {
            api: api.clone(),
            stream: self.backend.context.stream,
            nodes: self.nodes,
            input_shapes: self.inputs.clone(),
            outputs: outputs.iter().map(|v| v.index).collect(),
            state: state.clone(),
        });
        // MLX takes ownership of the payload on entry, including its exception
        // path. Only its payload destructor may release this box afterwards.
        let payload = Box::into_raw(graph).cast();
        let mut source = Handle {
            ctx: std::ptr::null_mut(),
        };
        let mut closure = source;
        let result = api.call(|_| unsafe {
            source =
                (compile.closure_new_func_payload)(callback::trace, payload, callback::destroy);
            closure = (compile.closure_new)();
            if source.ctx.is_null() {
                return Err("MLX failed to create a closure".into());
            }
            // closure_new is an empty destination placeholder; compile sets it.
            Api::check((compile.compile)(&mut closure, source, false))?;
            if closure.ctx.is_null() {
                return Err("MLX failed to populate a compiled closure".into());
            }
            Ok(())
        });
        // Handles stay outside Api::call until LAST_ERROR has also been
        // inspected; an apparently successful C return can still report error.
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
            outputs: shapes,
            state,
        })
    }
}

impl MlxCompiledProgram {
    /// Number of native callback invocations, not a GPU execution count.
    pub fn trace_count(&self) -> usize {
        self.state.count.load(Ordering::Relaxed)
    }

    pub fn run(&self, inputs: &[&MlxTensor]) -> Result<Vec<MlxTensor>, MlxError> {
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
            if input.dtype != MlxDtype::F32 {
                return Err(MlxError::Dtype);
            }
            if input.shape != *expected {
                return Err(MlxError::ProgramInputShape {
                    index,
                    expected: expected.clone(),
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
            for (index, shape) in self.outputs.iter().enumerate() {
                handles.push((a.array_new)());
                let raw = handles.last_mut().expect("just appended");
                Api::check((compile.vector_array_get)(raw, output.raw, index))?;
                validate_array(a, *raw, shape, MlxDtype::F32)?;
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
            .map(|(raw, shape)| MlxTensor {
                array: Rc::new(Array {
                    raw,
                    context: self.context.clone(),
                }),
                shape: shape.clone(),
                dtype: MlxDtype::F32,
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
