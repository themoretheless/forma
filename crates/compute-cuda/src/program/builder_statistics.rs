use super::super::plan::StatisticsKind;
use super::*;
use tensor_core::{Moments, normalization_shape, statistics_shape, validate_epsilon};

impl CudaProgramPlanBuilder {
    fn statistics(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
        kind: StatisticsKind,
        low: bool,
    ) -> Result<(CudaValue, Option<CudaValue>), CudaError> {
        let source = if low {
            self.low(value)?
        } else {
            self.require(value, CudaDtype::F32)?
        };
        if let StatisticsKind::LayerNorm { epsilon } = kind {
            validate_epsilon(epsilon)?;
        }
        let shape = if matches!(kind, StatisticsKind::Moments | StatisticsKind::Logsumexp) {
            statistics_shape(source.layout.shape(), axes, keep_dims)?
        } else {
            normalization_shape(source.layout.shape(), axes)?
        };
        // Validate both moments allocations before reserving either slot.
        // keep_dims is fully represented by the reduced output shape.
        let layout = Layout::contiguous(shape.clone())?;
        validate_layout(&layout, CudaDtype::F32)?;
        self.transaction(|this| {
            let variance = if kind == StatisticsKind::Moments {
                let index = this.plan.scratch.len();
                this.plan.scratch.push(TensorSpec {
                    layout: layout.clone(),
                    dtype: CudaDtype::F32,
                });
                Some((
                    index,
                    this.push(PlannedValue {
                        input_views: Vec::new(),
                        buffer: BufferRef::Scratch(index),
                        layout,
                        dtype: CudaDtype::F32,
                    }),
                ))
            } else {
                None
            };
            let output = this.output(shape, CudaDtype::F32, |output| Step::Statistics {
                source,
                output,
                variance: variance.map(|v| v.0),
                axes: axes.to_vec(),
                kind,
            })?;
            Ok((output, variance.map(|v| v.1)))
        })
    }
    fn low_statistics(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
        kind: StatisticsKind,
    ) -> Result<(CudaValue, Option<CudaValue>), CudaError> {
        let dtype = self.low(value)?.dtype.low_dtype().unwrap();
        self.transaction(|this| {
            let (output, variance) = this.statistics(value, axes, keep_dims, kind, true)?;
            let output = this.cast_to_low(output, dtype)?;
            let variance = variance.map(|v| this.cast_to_low(v, dtype)).transpose()?;
            Ok((output, variance))
        })
    }
    pub fn softmax(&mut self, value: CudaValue, axes: &[usize]) -> Result<CudaValue, CudaError> {
        Ok(self
            .statistics(value, axes, false, StatisticsKind::Softmax, false)?
            .0)
    }
    pub fn softmax_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .statistics(value, axes, false, StatisticsKind::Softmax, true)?
            .0)
    }
    pub fn softmax_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .low_statistics(value, axes, false, StatisticsKind::Softmax)?
            .0)
    }
    pub fn log_softmax(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .statistics(value, axes, false, StatisticsKind::LogSoftmax, false)?
            .0)
    }
    pub fn log_softmax_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .statistics(value, axes, false, StatisticsKind::LogSoftmax, true)?
            .0)
    }
    pub fn log_softmax_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .low_statistics(value, axes, false, StatisticsKind::LogSoftmax)?
            .0)
    }
    pub fn logsumexp(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .statistics(value, axes, keep_dims, StatisticsKind::Logsumexp, false)?
            .0)
    }
    pub fn logsumexp_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .statistics(value, axes, keep_dims, StatisticsKind::Logsumexp, true)?
            .0)
    }
    pub fn logsumexp_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .low_statistics(value, axes, keep_dims, StatisticsKind::Logsumexp)?
            .0)
    }
    pub fn moments(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<CudaValue>, CudaError> {
        let (mean, variance) =
            self.statistics(value, axes, keep_dims, StatisticsKind::Moments, false)?;
        Ok(Moments {
            mean,
            variance: variance.expect("moments has two results"),
        })
    }
    pub fn moments_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<CudaValue>, CudaError> {
        let (mean, variance) =
            self.statistics(value, axes, keep_dims, StatisticsKind::Moments, true)?;
        Ok(Moments {
            mean,
            variance: variance.expect("moments has two results"),
        })
    }
    pub fn moments_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<CudaValue>, CudaError> {
        let (mean, variance) =
            self.low_statistics(value, axes, keep_dims, StatisticsKind::Moments)?;
        Ok(Moments {
            mean,
            variance: variance.expect("moments has two results"),
        })
    }
    pub fn layer_norm(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .statistics(
                value,
                axes,
                false,
                StatisticsKind::LayerNorm { epsilon },
                false,
            )?
            .0)
    }
    pub fn layer_norm_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .statistics(
                value,
                axes,
                false,
                StatisticsKind::LayerNorm { epsilon },
                true,
            )?
            .0)
    }
    pub fn layer_norm_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<CudaValue, CudaError> {
        Ok(self
            .low_statistics(value, axes, false, StatisticsKind::LayerNorm { epsilon })?
            .0)
    }
}
