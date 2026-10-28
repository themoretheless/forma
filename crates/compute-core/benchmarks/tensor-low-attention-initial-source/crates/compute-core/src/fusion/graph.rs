use super::{FusedKernel, FusedSumKernel, FusionError};
use crate::{BinaryOp, Binding, CompareOp, KernelCache, UnaryOp, gpu_compute::GpuContext};
use std::{collections::HashMap, fmt::Write, sync::Arc};

/// Opaque node owned by one FusionGraph. Cross-graph use is an explicit error.
#[derive(Clone, Debug)]
pub struct Expression {
    index: usize,
    owner: Arc<()>,
}
/// Boolean expression belonging to one graph. Use `FusionGraph::select` to
/// produce f32 values; predicates cannot be passed to arithmetic accidentally.
#[derive(Clone, Debug)]
pub struct Predicate(Expression);
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Node {
    Input(usize),
    Constant(u32),
    Unary(UnaryOp, usize),
    Binary(BinaryOp, usize, usize),
    Affine(usize, u32, u32),
    Compare(CompareOp, usize, usize),
    Select(usize, usize, usize),
}
impl Node {
    fn operands(self) -> [Option<usize>; 3] {
        match self {
            Self::Input(_) | Self::Constant(_) => [None, None, None],
            Self::Unary(_, a) | Self::Affine(a, _, _) => [Some(a), None, None],
            Self::Binary(_, a, b) | Self::Compare(_, a, b) => [Some(a), Some(b), None],
            Self::Select(condition, a, b) => [Some(condition), Some(a), Some(b)],
        }
    }
}
/// Scalar f32 expression builder. Interning reuses identical operations; compile
/// removes unreachable nodes. Operand order and arithmetic grouping are kept:
/// no reassociation or host constant folding changes floating-point semantics.
/// Inputs/intermediates must respect the same finite-value/function domains as
/// UnaryOp and BinaryOp. Data already on the GPU is not inspected on the host.
pub struct FusionGraph {
    owner: Arc<()>,
    inputs: usize,
    nodes: Vec<Node>,
    interned: HashMap<Node, usize>,
}
impl FusionGraph {
    pub fn new(input_count: usize) -> Self {
        Self {
            owner: Arc::new(()),
            inputs: input_count,
            nodes: Vec::new(),
            interned: HashMap::new(),
        }
    }
    fn insert(&mut self, node: Node) -> Expression {
        let index = *self.interned.entry(node).or_insert_with(|| {
            let n = self.nodes.len();
            self.nodes.push(node);
            n
        });
        Expression {
            index,
            owner: self.owner.clone(),
        }
    }
    fn check(&self, expression: &Expression) -> Result<usize, FusionError> {
        if !Arc::ptr_eq(&self.owner, &expression.owner) {
            return Err(FusionError::ForeignExpression);
        }
        Ok(expression.index)
    }
    pub fn input(&mut self, slot: usize) -> Result<Expression, FusionError> {
        if slot >= self.inputs {
            return Err(FusionError::InputSlot {
                slot,
                count: self.inputs,
            });
        }
        Ok(self.insert(Node::Input(slot)))
    }
    pub fn constant(&mut self, value: f32) -> Result<Expression, FusionError> {
        if !value.is_finite() {
            return Err(FusionError::NonfiniteConstant);
        }
        Ok(self.insert(Node::Constant(value.to_bits())))
    }
    pub fn unary(&mut self, op: UnaryOp, value: &Expression) -> Result<Expression, FusionError> {
        let a = self.check(value)?;
        Ok(self.insert(Node::Unary(op, a)))
    }
    pub fn binary(
        &mut self,
        op: BinaryOp,
        a: &Expression,
        b: &Expression,
    ) -> Result<Expression, FusionError> {
        let a = self.check(a)?;
        let b = self.check(b)?;
        Ok(self.insert(Node::Binary(op, a, b)))
    }
    pub fn affine(
        &mut self,
        value: &Expression,
        scale: f32,
        offset: f32,
    ) -> Result<Expression, FusionError> {
        let a = self.check(value)?;
        if !scale.is_finite() || !offset.is_finite() {
            return Err(FusionError::NonfiniteConstant);
        }
        Ok(self.insert(Node::Affine(a, scale.to_bits(), offset.to_bits())))
    }
    pub fn compare(
        &mut self,
        op: CompareOp,
        a: &Expression,
        b: &Expression,
    ) -> Result<Predicate, FusionError> {
        Ok(Predicate(self.insert(Node::Compare(
            op,
            self.check(a)?,
            self.check(b)?,
        ))))
    }
    /// Selects a value per element. Both value expressions are evaluated, as in
    /// WGSL `select`; this does not guard invalid arithmetic in either branch.
    pub fn select(
        &mut self,
        condition: &Predicate,
        when_true: &Expression,
        when_false: &Expression,
    ) -> Result<Expression, FusionError> {
        let condition = self.check(&condition.0)?;
        let a = self.check(when_true)?;
        let b = self.check(when_false)?;
        Ok(self.insert(Node::Select(condition, a, b)))
    }
    /// Compiles map + sum without a full-sized mapped array. Reduction uses
    /// f32 and changes summation order relative to separate map/reduce calls.
    pub fn compile_sum(
        &self,
        context: &GpuContext,
        output: &Expression,
    ) -> Result<FusedSumKernel, FusionError> {
        self.compile_sum_cached(&mut KernelCache::new(context, 0), output)
    }
    pub fn compile_sum_cached(
        &self,
        cache: &mut KernelCache,
        output: &Expression,
    ) -> Result<FusedSumKernel, FusionError> {
        Ok(FusedSumKernel(self.compile_mode(
            cache,
            std::slice::from_ref(output),
            true,
        )?))
    }
    pub fn compile(
        &self,
        context: &GpuContext,
        outputs: &[Expression],
    ) -> Result<FusedKernel, FusionError> {
        self.compile_cached(&mut KernelCache::new(context, 0), outputs)
    }
    /// Equivalent generated source reuses an existing compiled pipeline.
    pub fn compile_cached(
        &self,
        cache: &mut KernelCache,
        outputs: &[Expression],
    ) -> Result<FusedKernel, FusionError> {
        self.compile_mode(cache, outputs, false)
    }
    fn compile_mode(
        &self,
        cache: &mut KernelCache,
        outputs: &[Expression],
        sum: bool,
    ) -> Result<FusedKernel, FusionError> {
        if outputs.is_empty() {
            return Err(FusionError::NoOutputs);
        }
        let roots: Vec<_> = outputs
            .iter()
            .map(|e| self.check(e))
            .collect::<Result<_, _>>()?;
        let mut live = vec![false; self.nodes.len()];
        let mut pending = roots.clone();
        while let Some(i) = pending.pop() {
            if live[i] {
                continue;
            }
            live[i] = true;
            pending.extend(self.nodes[i].operands().into_iter().flatten());
        }
        let mut used_inputs: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                if live[i]
                    && let Node::Input(slot) = n
                {
                    return Some(*slot);
                }
                None
            })
            .collect();
        used_inputs.sort_unstable();
        let context = cache.context().clone();
        let limit = context.device.limits().max_storage_buffers_per_shader_stage;
        let requested = used_inputs.len().saturating_add(outputs.len());
        if requested > limit as usize {
            return Err(FusionError::BindingLimit { requested, limit });
        }
        let mut bindings = vec![Binding::Uniform];
        bindings.extend(vec![Binding::StorageRead; used_inputs.len()]);
        bindings.extend(vec![Binding::StorageReadWrite; outputs.len()]);
        let mut source =
            String::from("struct Params { count: u32, _pad0: u32, _pad1: u32, _pad2: u32,\n");
        for (position, _) in used_inputs.iter().enumerate() {
            writeln!(source, "step{position}: u32,").unwrap();
        }
        source.push_str("};\n@group(0) @binding(0) var<uniform> params: Params;\n");
        for (position, slot) in used_inputs.iter().enumerate() {
            writeln!(
                source,
                "@group(0) @binding({}) var<storage, read> input{slot}: array<f32>;",
                position + 1
            )
            .unwrap();
        }
        for output in 0..outputs.len() {
            writeln!(
                source,
                "@group(0) @binding({}) var<storage, read_write> output{output}: array<f32>;",
                1 + used_inputs.len() + output
            )
            .unwrap();
        }
        source.push_str("const WG: u32 = 256;\n");
        if sum {
            source.push_str("var<workgroup> scratch: array<f32, WG>;\n");
        }
        source.push_str("@compute @workgroup_size(WG)\nfn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {\n");
        if sum {
            source.push_str("var accumulator = 0.0;\n");
        }
        source.push_str("let stride = groups.x * WG;\nfor(var i = gid.x; i < params.count;) {\n");
        let mut names = vec![String::new(); self.nodes.len()];
        let mut operations = 0;
        let mut serial = 0;
        for (i, node) in self.nodes.iter().copied().enumerate() {
            if !live[i] {
                continue;
            }
            let expression = match node {
                Node::Input(slot) => {
                    let position = used_inputs.binary_search(&slot).unwrap();
                    format!("input{slot}[i * params.step{position}]")
                }
                Node::Constant(bits) => format!("bitcast<f32>({bits}u)"),
                Node::Unary(op, a) => {
                    operations += 1;
                    unary(op, &names[a])
                }
                Node::Binary(op, a, b) => {
                    operations += 1;
                    binary(op, &names[a], &names[b])
                }
                Node::Affine(a, scale, offset) => {
                    operations += 1;
                    format!(
                        "({} * bitcast<f32>({scale}u)) + bitcast<f32>({offset}u)",
                        names[a]
                    )
                }
                Node::Compare(op, a, b) => {
                    operations += 1;
                    let operator = match op {
                        CompareOp::Equal => "==",
                        CompareOp::NotEqual => "!=",
                        CompareOp::Less => "<",
                        CompareOp::LessEqual => "<=",
                        CompareOp::Greater => ">",
                        CompareOp::GreaterEqual => ">=",
                    };
                    format!("{} {operator} {}", names[a], names[b])
                }
                Node::Select(condition, a, b) => {
                    operations += 1;
                    format!("select({}, {}, {})", names[b], names[a], names[condition])
                }
            };
            names[i] = format!("v{serial}");
            serial += 1;
            writeln!(source, "let {} = {expression};", names[i]).unwrap();
        }
        for (output, root) in roots.into_iter().enumerate() {
            if sum {
                writeln!(source, "accumulator += {};", names[root]).unwrap();
            } else {
                writeln!(source, "output{output}[i] = {};", names[root]).unwrap();
            }
        }
        // Avoid u32 wraparound for constant/scalar-only sums near u32::MAX.
        source.push_str("if params.count - i <= stride { break; }\ni += stride;\n}\n");
        if sum {
            source.push_str("scratch[lid.x] = accumulator;\nfor(var step = WG / 2u; step > 0u; step >>= 1u) {\nworkgroupBarrier();\nif lid.x < step { scratch[lid.x] += scratch[lid.x + step]; }\n}\nif lid.x == 0u { output0[wid.x] = scratch[0]; }\n");
        }
        source.push_str("}\n");
        let kernel = cache.get("fused expression", &source, "main", &bindings)?;
        Ok(FusedKernel {
            context,
            kernel,
            inputs: self.inputs,
            used_inputs,
            outputs: outputs.len(),
            operations,
        })
    }
}
fn unary(op: UnaryOp, a: &str) -> String {
    match op {
        UnaryOp::Negate => format!("-{a}"),
        UnaryOp::Abs => format!("abs({a})"),
        UnaryOp::Square => format!("{a} * {a}"),
        UnaryOp::Sqrt => format!("sqrt({a})"),
        UnaryOp::Reciprocal => format!("1.0 / {a}"),
        UnaryOp::Exp => format!("exp({a})"),
        UnaryOp::Log => format!("log({a})"),
        UnaryOp::Sin => format!("sin({a})"),
        UnaryOp::Cos => format!("cos({a})"),
    }
}
fn binary(op: BinaryOp, a: &str, b: &str) -> String {
    match op {
        BinaryOp::Add => format!("{a} + {b}"),
        BinaryOp::Subtract => format!("{a} - {b}"),
        BinaryOp::Multiply => format!("{a} * {b}"),
        BinaryOp::Divide => format!("{a} / {b}"),
        BinaryOp::Min => format!("min({a}, {b})"),
        BinaryOp::Max => format!("max({a}, {b})"),
    }
}
