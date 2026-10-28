//! Share the existing checked reduction traversal across storage/accumulator types.
use super::{
    operation::{Destination, Operation},
    plan::{BufferRef, CudaDtype, PlannedValue},
    preparation::Expanded,
};
use crate::{
    CudaError, dispatch::UnaryDispatch, low_dispatch::LowCastDispatch,
    reduction::dispatch::ReductionPass,
};
use tensor_core::{Layout, ReduceOp};
impl Expanded {
    pub(super) fn reduction(
        &mut self,
        mut source: PlannedValue,
        output: usize,
        op: ReduceOp,
        axes: &[usize],
        mean: bool,
        multiprocessors: u32,
    ) -> Result<(), CudaError> {
        let shape = self.scratch[output].shape().clone();
        let accumulator = self.scratch[output].dtype;
        if shape.is_empty() {
            return Ok(());
        }
        if axes.is_empty() {
            if let Some(dtype) = source.dtype.low_dtype() {
                self.add(
                    output,
                    Operation::Cast {
                        source: source.buffer,
                        pass: LowCastDispatch::new(&source.layout, dtype)?,
                        to: CudaDtype::F32,
                    },
                );
                return Ok(());
            }
            return self.copy(source, Destination::Scratch(output));
        }
        if source.layout.shape().is_empty() {
            self.add(
                output,
                Operation::Fill {
                    count: shape.numel(),
                    value: u32::from(op == ReduceOp::Product),
                },
            );
            return Ok(());
        }
        let divisor = (source.layout.shape().numel() / shape.numel()) as f32;
        let reduction_output = if mean {
            self.scratch(shape.clone(), accumulator)?
        } else {
            output
        };
        if axes.len() == source.layout.shape().rank() {
            loop {
                let pass = ReductionPass::all(op, &source.layout, multiprocessors)?;
                let last = pass.output_shape.numel() == 1;
                let next = if last {
                    reduction_output
                } else {
                    self.scratch(pass.output_shape.clone(), accumulator)?
                };
                let next_layout = Layout::contiguous(pass.output_shape.clone())?;
                self.add(
                    next,
                    Operation::Reduction {
                        source: source.buffer,
                        pass,
                    },
                );
                if last {
                    break;
                }
                source = PlannedValue {
                    input_views: Vec::new(),
                    buffer: BufferRef::Scratch(next),
                    layout: next_layout,
                    dtype: accumulator,
                };
            }
        } else {
            let pass =
                ReductionPass::axes(op, &source.layout, axes, shape.clone(), multiprocessors)?;
            self.add(
                reduction_output,
                Operation::Reduction {
                    source: source.buffer,
                    pass,
                },
            );
        }
        if mean {
            self.add(
                output,
                Operation::Unary {
                    source: BufferRef::Scratch(reduction_output),
                    pass: UnaryDispatch::new(&Layout::contiguous(shape)?, 11, divisor, 0.)?,
                },
            );
        }
        Ok(())
    }
}
