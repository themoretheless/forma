//! Retain stable row statistics and attention accumulators between executions.
use super::{
    operation::{Destination, Operation},
    plan::{BufferRef, CudaDtype, PlannedValue, StatisticsKind},
    preparation::Expanded,
};
use crate::{
    CudaError, attention_dispatch::AttentionDispatch, low_dispatch::LowCastDispatch,
    statistics_dispatch::RowPlan,
};
use tensor_core::{AttentionPlan, Shape, statistics_shape};
impl Expanded {
    fn statistic_copy(&mut self, source: PlannedValue, output: usize) -> Result<(), CudaError> {
        if let Some(dtype) = source.dtype.low_dtype() {
            let pass = LowCastDispatch::new(&source.layout, dtype)?;
            self.add(
                output,
                Operation::Cast {
                    source: source.buffer,
                    pass,
                    to: CudaDtype::F32,
                },
            );
        } else {
            self.copy(source, Destination::Scratch(output))?;
        }
        Ok(())
    }
    pub(super) fn statistics(
        &mut self,
        source: PlannedValue,
        outputs: (usize, Option<usize>),
        axes: &[usize],
        kind: StatisticsKind,
        multiprocessors: u32,
    ) -> Result<(), CudaError> {
        let (output, variance) = outputs;
        if source.layout.shape().is_empty() {
            return Ok(());
        }
        let reduced = statistics_shape(source.layout.shape(), axes, false)?;
        if source.layout.shape().numel() == reduced.numel() {
            match kind {
                StatisticsKind::Softmax => self.add(
                    output,
                    Operation::Fill {
                        count: source.layout.shape().numel(),
                        value: 1f32.to_bits(),
                    },
                ),
                StatisticsKind::LogSoftmax | StatisticsKind::LayerNorm { .. } => {
                    self.add(output, Operation::Zero)
                }
                StatisticsKind::Logsumexp => self.statistic_copy(source, output)?,
                StatisticsKind::Moments => {
                    self.statistic_copy(source, output)?;
                    self.add(
                        variance.ok_or(CudaError::InvalidInput("missing moments destination"))?,
                        Operation::Zero,
                    );
                }
            }
            return Ok(());
        }
        let groups =
            multiprocessors
                .checked_mul(8)
                .filter(|&g| g != 0)
                .ok_or(CudaError::InvalidInput(
                    "invalid CUDA statistics launch groups",
                ))?;
        let pass = RowPlan::new(&source.layout, axes, groups as usize)?;
        let first_id = self.scratch_f64(Shape::new(vec![pass.rows])?)?;
        let second_id = self.scratch_f64(Shape::new(vec![pass.rows])?)?;
        let partial_id = self.scratch_f64(Shape::new(vec![pass.rows, pass.chunks])?)?;
        let first = BufferRef::Scratch(first_id);
        let second = BufferRef::Scratch(second_id);
        let base = if matches!(
            kind,
            StatisticsKind::Moments | StatisticsKind::LayerNorm { .. }
        ) {
            2
        } else {
            0
        };
        for (op, destination) in [(base, first_id), (base + 1, second_id)] {
            self.add(
                partial_id,
                Operation::StatisticsPartial {
                    source: source.buffer,
                    center: first,
                    pass: pass.clone(),
                    op,
                },
            );
            self.add(
                destination,
                Operation::StatisticsMerge {
                    partials: BufferRef::Scratch(partial_id),
                    pass: pass.clone(),
                    op,
                },
            );
        }
        let operation = match kind {
            StatisticsKind::Logsumexp => Operation::StatisticsLse {
                first,
                second,
                pass,
            },
            StatisticsKind::Moments => Operation::StatisticsMoments {
                source: source.buffer,
                first,
                second,
                pass,
                variance: variance.ok_or(CudaError::InvalidInput("missing moments destination"))?,
            },
            kind => Operation::StatisticsEmit {
                source: source.buffer,
                first,
                second,
                pass,
                mode: match kind {
                    StatisticsKind::Softmax => (0, 0.),
                    StatisticsKind::LogSoftmax => (1, 0.),
                    StatisticsKind::LayerNorm { epsilon } => (2, epsilon),
                    _ => unreachable!(),
                },
            },
        };
        self.add(output, operation);
        Ok(())
    }
    pub(super) fn attention(
        &mut self,
        inputs: (PlannedValue, PlannedValue, PlannedValue),
        mask: Option<(PlannedValue, bool)>,
        output: usize,
        plan: AttentionPlan,
        multiprocessors: u32,
    ) -> Result<(), CudaError> {
        let (query, key, value) = inputs;
        if plan.output.is_empty() {
            return Ok(());
        }
        if plan.keys == 0 {
            self.add(output, Operation::Zero);
            return Ok(());
        }
        let mask_layout = mask.as_ref().map(|(v, keep)| (&v.layout, *keep));
        let pass = match query.dtype {
            CudaDtype::F32 => AttentionDispatch::new::<f32>(
                &plan,
                &query.layout,
                &key.layout,
                &value.layout,
                mask_layout,
                multiprocessors,
            )?,
            CudaDtype::F16 | CudaDtype::Bf16 => AttentionDispatch::new::<u16>(
                &plan,
                &query.layout,
                &key.layout,
                &value.layout,
                mask_layout,
                multiprocessors,
            )?,
            _ => return Err(CudaError::Dtype),
        };
        let accumulator = self.scratch_f64(plan.output)?;
        self.add(
            output,
            Operation::Attention {
                query: query.buffer,
                key: key.buffer,
                value: value.buffer,
                mask: mask.map(|(v, k)| (v.buffer, k)),
                accumulator,
                pass,
            },
        );
        Ok(())
    }
}
