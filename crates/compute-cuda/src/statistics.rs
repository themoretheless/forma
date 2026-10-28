use crate::statistics_dispatch::{RowPlan, StatsInput, StatsOutputs, StatsRows};
use crate::{CudaError, CudaRuntime, CudaTensor, indexing::output};
use gpu_compute::cuda::{
    CudaFunction, CudaModule, CudaSlice, PushKernelArg, cudarc::driver::DeviceRepr,
};
use std::sync::Arc;
use tensor_core::{
    Moments, Shape, TensorBackend, TensorStatsBackend, normalization_shape, statistics_shape,
    validate_epsilon,
};

#[derive(Clone)]
pub(crate) struct StatisticsKernels {
    pub(crate) partial: [CudaFunction; 2],
    pub(crate) merge: CudaFunction,
    pub(crate) emit: [CudaFunction; 2],
    pub(crate) lse: CudaFunction,
    pub(crate) moments: [CudaFunction; 2],
}
impl StatisticsKernels {
    pub(crate) fn load(module: &Arc<CudaModule>) -> Result<Self, CudaError> {
        let pair = |name: &str| -> Result<_, CudaError> {
            Ok([
                module.load_function(name)?,
                module.load_function(&format!("{name}_low"))?,
            ])
        };
        Ok(Self {
            partial: pair("stats_partial")?,
            merge: module.load_function("stats_merge")?,
            emit: pair("stats_emit")?,
            lse: module.load_function("stats_lse")?,
            moments: pair("stats_moments")?,
        })
    }
}

struct RowStats {
    first: CudaTensor<f64>,
    second: CudaTensor<f64>,
    metadata: CudaSlice<u64>,
    plan: RowPlan,
}

impl CudaRuntime {
    fn stats_rows<T: DeviceRepr>(
        &self,
        input: &CudaTensor<T>,
        axes: &[usize],
        moments: bool,
        dtype: Option<u32>,
    ) -> Result<RowStats, CudaError> {
        let plan = RowPlan::new(
            input.layout(),
            axes,
            self.device.multiprocessors as usize * 8,
        )?;
        let row_shape = Shape::new(vec![plan.rows])?;
        let mut first = self.zeros_typed::<f64>(row_shape.clone())?;
        let mut second = self.zeros_typed::<f64>(row_shape)?;
        let mut partials = self.zeros_typed::<f64>(Shape::new(vec![plan.rows, plan.chunks])?)?;
        let metadata = self.metadata(&plan.metadata)?;
        for operation in if moments { [2u32, 3] } else { [0u32, 1] } {
            plan.partial(
                self,
                StatsInput {
                    values: input.storage.as_ref(),
                    dtype,
                },
                first.storage.as_ref(),
                output(&mut partials),
                &metadata,
                operation,
            )?;
            let destination = if operation.is_multiple_of(2) {
                &mut first
            } else {
                &mut second
            };
            plan.merge(
                self,
                partials.storage.as_ref(),
                output(destination),
                operation,
            )?;
        }
        Ok(RowStats {
            first,
            second,
            metadata,
            plan,
        })
    }

    pub(crate) fn normalize_loaded<T: DeviceRepr>(
        &self,
        input: &CudaTensor<T>,
        axes: &[usize],
        op: u32,
        epsilon: f32,
        dtype: Option<u32>,
    ) -> Result<CudaTensor, CudaError> {
        self.check(input)?;
        let shape = normalization_shape(input.shape(), axes)?;
        if shape.is_empty() {
            return self.zeros(shape);
        }
        let reduced = statistics_shape(input.shape(), axes, false)?;
        if input.shape().numel() == reduced.numel() {
            let mut result = self.zeros(shape)?;
            if op == 0 {
                let count = input.shape().numel() as u64;
                let one = 1f32;
                // Singleton softmax writes one without an input-sized temporary.
                unsafe {
                    self.device
                        .stream
                        .launch_builder(&self.reduction.fill[0])
                        .arg(output(&mut result))
                        .arg(&count)
                        .arg(&one)
                        .launch(self.config(count as usize))?;
                }
            }
            return Ok(result);
        }
        let mut result = self.zeros(shape)?;
        let rows = self.stats_rows(input, axes, op == 2, dtype)?;
        rows.plan.emit(
            self,
            StatsInput {
                values: input.storage.as_ref(),
                dtype,
            },
            StatsRows {
                first: rows.first.storage.as_ref(),
                second: rows.second.storage.as_ref(),
            },
            output(&mut result),
            &rows.metadata,
            (op, epsilon),
        )?;
        Ok(result)
    }
}

impl TensorStatsBackend for CudaRuntime {
    fn softmax(&self, input: &CudaTensor, axes: &[usize]) -> Result<CudaTensor, CudaError> {
        self.normalize_loaded(input, axes, 0, 0., None)
    }
    fn log_softmax(&self, input: &CudaTensor, axes: &[usize]) -> Result<CudaTensor, CudaError> {
        self.normalize_loaded(input, axes, 1, 0., None)
    }
    fn layer_norm(
        &self,
        input: &CudaTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<CudaTensor, CudaError> {
        validate_epsilon(epsilon)?;
        self.normalize_loaded(input, axes, 2, epsilon, None)
    }
    fn logsumexp(
        &self,
        input: &CudaTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor, CudaError> {
        self.check(input)?;
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        if shape.is_empty() {
            return self.zeros(shape);
        }
        if input.shape().numel() == shape.numel() {
            return self.reshape(input, shape);
        }
        self.stats_logsumexp_loaded(input, axes, shape, None)
    }
    fn moments(
        &self,
        input: &CudaTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<CudaTensor>, CudaError> {
        self.check(input)?;
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        let variance = self.zeros(shape.clone())?;
        if shape.is_empty() {
            return Ok(Moments {
                mean: self.zeros(shape)?,
                variance,
            });
        }
        if input.shape().numel() == shape.numel() {
            return Ok(Moments {
                mean: self.reshape(input, shape)?,
                variance,
            });
        }
        self.stats_moments_loaded(input, axes, shape, variance, None)
    }
}

impl CudaRuntime {
    // Callers validate shape/ownership and handle singleton/empty contractions.
    pub(crate) fn stats_logsumexp_loaded<T: DeviceRepr>(
        &self,
        input: &CudaTensor<T>,
        axes: &[usize],
        shape: Shape,
        dtype: Option<u32>,
    ) -> Result<CudaTensor, CudaError> {
        let mut result = self.zeros(shape)?;
        let rows = self.stats_rows(input, axes, false, dtype)?;
        rows.plan.logsumexp(
            self,
            StatsRows {
                first: rows.first.storage.as_ref(),
                second: rows.second.storage.as_ref(),
            },
            output(&mut result),
        )?;
        Ok(result)
    }
    pub(crate) fn stats_moments_loaded<T: DeviceRepr>(
        &self,
        input: &CudaTensor<T>,
        axes: &[usize],
        shape: Shape,
        mut variance: CudaTensor,
        dtype: Option<u32>,
    ) -> Result<Moments<CudaTensor>, CudaError> {
        let mut mean = self.zeros(shape)?;
        let rows = self.stats_rows(input, axes, true, dtype)?;
        rows.plan.moments(
            self,
            StatsInput {
                values: input.storage.as_ref(),
                dtype,
            },
            StatsRows {
                first: rows.first.storage.as_ref(),
                second: rows.second.storage.as_ref(),
            },
            StatsOutputs {
                mean: output(&mut mean),
                variance: output(&mut variance),
            },
            &rows.metadata,
        )?;
        Ok(Moments { mean, variance })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tensor_core::Layout;
    #[test]
    fn row_plan_preserves_strides_and_bounds_partial_storage() {
        let source = Layout::contiguous(Shape::new(vec![3, 5, 7]).unwrap())
            .unwrap()
            .permute(&[2, 0, 1])
            .unwrap();
        let plan = RowPlan::new(&source, &[2, 0], 1024).unwrap();
        assert_eq!((plan.rows, plan.reduce_count, plan.chunks), (3, 35, 1));
        assert_eq!(plan.metadata, [3, 35, 5, 5, 7, 7, 1, 1, 15]);
        for (dims, axes) in [(vec![1_048_583], vec![0]), (vec![257, 8193], vec![1])] {
            let shape = Shape::new(dims).unwrap();
            let plan =
                RowPlan::new(&Layout::contiguous(shape.clone()).unwrap(), &axes, 1024).unwrap();
            assert!(plan.rows * plan.chunks <= shape.numel());
            assert!(plan.chunks <= 1024);
        }
    }
}
