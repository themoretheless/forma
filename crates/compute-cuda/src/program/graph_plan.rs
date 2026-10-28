//! Rebase logical input views onto graph-owned dense slots without reserving
//! the caller's physical stride span. The ordinary prepared plan is unchanged.
use super::plan::{
    BufferRef, CudaProgramPlan, InputView, PlannedValue, Step, TensorSpec, validate_layout,
};
use crate::CudaError;
use tensor_core::Layout;

impl CudaProgramPlan {
    pub(super) fn dense_inputs(&self) -> Result<Self, CudaError> {
        let mut remap = Remap {
            original_inputs: &self.inputs,
            plan: Self {
                inputs: self
                    .inputs
                    .iter()
                    .map(|spec| {
                        Ok(TensorSpec {
                            layout: Layout::contiguous(spec.shape().clone())?,
                            dtype: spec.dtype,
                        })
                    })
                    .collect::<Result<_, CudaError>>()?,
                scratch: self.scratch.clone(),
                steps: Vec::with_capacity(self.steps.len()),
                outputs: Vec::with_capacity(self.outputs.len()),
            },
            cache: Vec::new(),
            copies: Vec::new(),
        };
        for step in &self.steps {
            let mut step = step.clone();
            for source in operands(&mut step) {
                *source = remap.value(source)?;
            }
            if let Step::Matmul {
                left,
                right,
                output,
                ..
            } = &mut step
            {
                let active = !remap.plan.scratch[*output].shape().is_empty()
                    && left.layout.shape().dims().last() != Some(&0);
                if active {
                    *left = remap.materialize(left)?;
                    *right = remap.materialize(right)?;
                }
            }
            remap.plan.steps.push(step);
        }
        for value in &self.outputs {
            let value = remap.value(value)?;
            remap.plan.outputs.push(value);
        }
        Ok(remap.plan)
    }
}

struct Remap<'a> {
    original_inputs: &'a [TensorSpec],
    plan: CudaProgramPlan,
    // A plan is small host metadata. Exact equality retains offset/provenance
    // distinctions and shares any reshape copy across repeated consumers.
    cache: Vec<(PlannedValue, PlannedValue)>,
    copies: Vec<(PlannedValue, PlannedValue)>,
}
impl Remap<'_> {
    fn materialize(&mut self, value: &PlannedValue) -> Result<PlannedValue, CudaError> {
        if value.layout.is_contiguous() {
            return Ok(value.clone());
        }
        if let Some((_, result)) = self.copies.iter().find(|(key, _)| key == value) {
            return Ok(result.clone());
        }
        let layout = Layout::contiguous(value.layout.shape().clone())?;
        validate_layout(&layout, value.dtype)?;
        let output = self.plan.scratch.len();
        self.plan.scratch.push(TensorSpec {
            layout: layout.clone(),
            dtype: value.dtype,
        });
        self.plan.steps.push(Step::Unary {
            source: value.clone(),
            output,
            op: 10,
            scale: 1.,
            bias: 0.,
        });
        let result = PlannedValue {
            input_views: Vec::new(),
            buffer: BufferRef::Scratch(output),
            layout,
            dtype: value.dtype,
        };
        self.copies.push((value.clone(), result.clone()));
        Ok(result)
    }

    fn value(&mut self, value: &PlannedValue) -> Result<PlannedValue, CudaError> {
        let BufferRef::Input(slot) = value.buffer else {
            if !value.input_views.is_empty() {
                return Err(CudaError::InvalidInput(
                    "scratch value has input provenance",
                ));
            }
            return Ok(value.clone());
        };
        if let Some((_, result)) = self.cache.iter().find(|(key, _)| key == value) {
            return Ok(result.clone());
        }
        let input = self
            .original_inputs
            .get(slot)
            .ok_or(CudaError::InvalidInput(
                "graph input provenance slot is invalid",
            ))?;
        if input.dtype != value.dtype {
            return Err(CudaError::Dtype);
        }
        // Fail closed if a future builder edits a physical layout without
        // recording the corresponding logical view operation.
        let mut original = input.layout.clone();
        for operation in &value.input_views {
            original = operation.apply(&original)?;
        }
        if original != value.layout {
            return Err(CudaError::InvalidInput(
                "graph input view provenance does not match its layout",
            ));
        }
        let mut result = PlannedValue {
            input_views: Vec::new(),
            buffer: BufferRef::Input(slot),
            layout: self.plan.inputs[slot].layout.clone(),
            dtype: value.dtype,
        };
        for operation in &value.input_views {
            if matches!(operation, InputView::Reshape(_)) && !result.layout.is_contiguous() {
                result = self.materialize(&result)?;
            }
            result.layout = operation.apply(&result.layout)?;
            if matches!(result.buffer, BufferRef::Input(_)) {
                result.input_views.push(operation.clone());
            }
        }
        self.cache.push((value.clone(), result.clone()));
        Ok(result)
    }
}

/// Enumerate operands without duplicating operator lowering or output allocation.
fn operands(step: &mut Step) -> Vec<&mut PlannedValue> {
    match step {
        Step::Unary { source, .. }
        | Step::Cast { source, .. }
        | Step::Reduce { source, .. }
        | Step::Scan { source, .. }
        | Step::Statistics { source, .. } => vec![source],
        Step::Binary { left, right, .. }
        | Step::Compare { left, right, .. }
        | Step::Matmul { left, right, .. } => vec![left, right],
        Step::Select { mask, yes, no, .. } => vec![mask, yes, no],
        Step::Gather {
            source, indices, ..
        } => vec![source, indices],
        Step::Compact { source, mask, .. } => vec![source, mask],
        Step::Scatter {
            source,
            indices,
            updates,
            ..
        } => vec![source, indices, updates],
        Step::Attention {
            query,
            key,
            value,
            mask,
            ..
        } => {
            let mut sources = vec![query, key, value];
            if let Some((mask, _)) = mask {
                sources.push(mask);
            }
            sources
        }
    }
}

#[cfg(test)]
#[path = "graph_plan_tests.rs"]
mod tests;
