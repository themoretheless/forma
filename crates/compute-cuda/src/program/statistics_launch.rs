//! Statistics and attention borrow resident state through the ordinary stream guards.
use super::{
    execution::Instruction,
    launch::source,
    operation::Operation,
    storage::{Storage, StorageMut, StorageRef},
    typed::CudaProgramInput,
};
use crate::{
    CudaError, CudaRuntime,
    attention_dispatch::{
        AttentionBuffers, AttentionInput, AttentionMaskStorage, AttentionOutputs,
    },
    statistics_dispatch::{StatsInput, StatsOutputs, StatsRows},
};
impl Instruction {
    pub(super) fn launch_statistics(
        &self,
        rt: &CudaRuntime,
        inputs: &[CudaProgramInput<'_>],
        scratch: &[Option<Storage>],
        output: StorageMut<'_>,
        auxiliary: Option<StorageMut<'_>>,
    ) -> Result<(), CudaError> {
        let input = |id| source(id, inputs, scratch);
        let metadata = || {
            self.metadata
                .as_ref()
                .expect("prepared statistics metadata exists")
        };
        let auxiliary = || {
            auxiliary.ok_or(CudaError::InvalidInput(
                "statistics auxiliary destination is missing",
            ))
        };
        match &self.operation {
            Operation::StatisticsPartial {
                source,
                center,
                pass,
                op,
            } => {
                let center = input(*center)?.f64()?;
                let output = output.f64()?;
                match input(*source)? {
                    StorageRef::F32(values) => pass.partial(
                        rt,
                        StatsInput {
                            values,
                            dtype: None,
                        },
                        center,
                        output,
                        metadata(),
                        *op,
                    ),
                    StorageRef::Low(values, d) => pass.partial(
                        rt,
                        StatsInput {
                            values,
                            dtype: Some(d as u32),
                        },
                        center,
                        output,
                        metadata(),
                        *op,
                    ),
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::StatisticsMerge { partials, pass, op } => {
                pass.merge(rt, input(*partials)?.f64()?, output.f64()?, *op)
            }
            Operation::StatisticsLse {
                first,
                second,
                pass,
            } => pass.logsumexp(
                rt,
                StatsRows {
                    first: input(*first)?.f64()?,
                    second: input(*second)?.f64()?,
                },
                output.f32()?,
            ),
            Operation::StatisticsEmit {
                source,
                first,
                second,
                pass,
                mode,
            } => {
                let rows = StatsRows {
                    first: input(*first)?.f64()?,
                    second: input(*second)?.f64()?,
                };
                let output = output.f32()?;
                match input(*source)? {
                    StorageRef::F32(values) => pass.emit(
                        rt,
                        StatsInput {
                            values,
                            dtype: None,
                        },
                        rows,
                        output,
                        metadata(),
                        *mode,
                    ),
                    StorageRef::Low(values, d) => pass.emit(
                        rt,
                        StatsInput {
                            values,
                            dtype: Some(d as u32),
                        },
                        rows,
                        output,
                        metadata(),
                        *mode,
                    ),
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::StatisticsMoments {
                source,
                first,
                second,
                pass,
                ..
            } => {
                let rows = StatsRows {
                    first: input(*first)?.f64()?,
                    second: input(*second)?.f64()?,
                };
                let output = StatsOutputs {
                    mean: output.f32()?,
                    variance: auxiliary()?.f32()?,
                };
                match input(*source)? {
                    StorageRef::F32(values) => pass.moments(
                        rt,
                        StatsInput {
                            values,
                            dtype: None,
                        },
                        rows,
                        output,
                        metadata(),
                    ),
                    StorageRef::Low(values, d) => pass.moments(
                        rt,
                        StatsInput {
                            values,
                            dtype: Some(d as u32),
                        },
                        rows,
                        output,
                        metadata(),
                    ),
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::Attention {
                query,
                key,
                value,
                mask,
                pass,
                ..
            } => {
                let mask = match mask {
                    None => AttentionMaskStorage::None,
                    Some((id, true)) => AttentionMaskStorage::Keep(input(*id)?.u32()?),
                    Some((id, false)) => AttentionMaskStorage::Additive(input(*id)?.f32()?),
                };
                let output = AttentionOutputs {
                    values: output.f32()?,
                    accumulator: auxiliary()?.f64()?,
                };
                match (input(*query)?, input(*key)?, input(*value)?) {
                    (StorageRef::F32(query), StorageRef::F32(key), StorageRef::F32(value)) => pass
                        .launch(
                            rt,
                            AttentionInput {
                                buffers: AttentionBuffers { query, key, value },
                                dtype: None,
                            },
                            mask,
                            output,
                            metadata(),
                        ),
                    (
                        StorageRef::Low(query, a),
                        StorageRef::Low(key, b),
                        StorageRef::Low(value, c),
                    ) if a == b && b == c => pass.launch(
                        rt,
                        AttentionInput {
                            buffers: AttentionBuffers { query, key, value },
                            dtype: Some(a as u32),
                        },
                        mask,
                        output,
                        metadata(),
                    ),
                    _ => Err(CudaError::Dtype),
                }
            }
            _ => Err(CudaError::InvalidInput(
                "not a statistics or attention instruction",
            )),
        }
    }
}
