//! Pure typed lowering. No driver calls or native allocations occur here.
use super::{
    CudaPrepareOptions, CudaProgramStats,
    operation::{Destination, MatmulRequest, Operation},
    plan::{CudaDtype, CudaProgramPlan, Step, TensorSpec},
    preparation::Expanded,
    storage::ScratchSpec,
};
use crate::{
    CudaError,
    dispatch::{BinaryDispatch, UnaryDispatch},
    indexing::dispatch::{CompareDispatch, SelectDispatch},
    low_dispatch::{LowBinaryDispatch, LowCastDispatch, LowUnaryDispatch},
    matmul::GemmPlan,
};

pub(super) fn expand(
    plan: CudaProgramPlan,
    multiprocessors: u32,
    options: CudaPrepareOptions,
    mut policy: impl FnMut(MatmulRequest) -> Result<(), CudaError>,
) -> Result<Expanded, CudaError> {
    let mut expanded = Expanded {
        inputs: plan.inputs,
        outputs: plan
            .outputs
            .iter()
            .map(|v| TensorSpec {
                layout: v.layout.clone(),
                dtype: v.dtype,
            })
            .collect(),
        precisions: Vec::new(),
        scratch: plan.scratch.into_iter().map(ScratchSpec::new).collect(),
        schedule: Vec::new(),
        stats: CudaProgramStats::default(),
    };
    for step in plan.steps {
        match step {
            Step::Statistics {
                source,
                output,
                variance,
                axes,
                kind,
            } => expanded.statistics(source, (output, variance), &axes, kind, multiprocessors)?,
            Step::Attention {
                query,
                key,
                value,
                mask,
                output,
                plan,
            } => expanded.attention((query, key, value), mask, output, *plan, multiprocessors)?,
            Step::Scan {
                source,
                output,
                axis,
                options,
            } => expanded.scan(source, output, axis, options, multiprocessors)?,
            Step::Gather {
                source,
                indices,
                output,
                invalid_count,
                axis,
            } => expanded.gather(source, indices, output, invalid_count, axis)?,
            Step::Compact {
                source,
                mask,
                output,
                count,
            } => expanded.compact(source, mask, output, count, multiprocessors)?,
            Step::Scatter {
                source,
                indices,
                updates,
                output,
                invalid_count,
                op,
                axis,
            } => expanded.scatter(source, indices, updates, (output, invalid_count), op, axis)?,
            Step::Unary {
                source,
                output,
                op,
                scale,
                bias,
            } => {
                if op == 10 {
                    expanded.copy(source, Destination::Scratch(output))?;
                } else if !source.layout.shape().is_empty() {
                    let operation = if let Some(dtype) = source.dtype.low_dtype() {
                        Operation::LowUnary {
                            source: source.buffer,
                            pass: LowUnaryDispatch::new_code(&source.layout, op, dtype)?,
                        }
                    } else if source.dtype == CudaDtype::F32 {
                        Operation::Unary {
                            source: source.buffer,
                            pass: UnaryDispatch::new(&source.layout, op, scale, bias)?,
                        }
                    } else {
                        return Err(CudaError::Dtype);
                    };
                    expanded.add(output, operation);
                }
            }
            Step::Binary {
                left,
                right,
                output,
                op,
            } => {
                if !expanded.scratch[output].shape().is_empty() {
                    let operation = if let Some(dtype) = left.dtype.low_dtype() {
                        Operation::LowBinary {
                            left: left.buffer,
                            right: right.buffer,
                            pass: LowBinaryDispatch::new(&left.layout, &right.layout, op, dtype)?,
                        }
                    } else {
                        Operation::Binary {
                            left: left.buffer,
                            right: right.buffer,
                            pass: BinaryDispatch::new(&left.layout, &right.layout, op)?,
                        }
                    };
                    expanded.add(output, operation);
                }
            }
            Step::Compare {
                left,
                right,
                output,
                op,
            } => {
                if !expanded.scratch[output].shape().is_empty() {
                    expanded.add(
                        output,
                        Operation::Compare {
                            left: left.buffer,
                            right: right.buffer,
                            pass: CompareDispatch::new(op, &left.layout, &right.layout)?,
                        },
                    );
                }
            }
            Step::Select {
                mask,
                yes,
                no,
                output,
            } => {
                if !expanded.scratch[output].shape().is_empty() {
                    let pass = match yes.dtype {
                        CudaDtype::F32 => {
                            SelectDispatch::new::<f32>(&mask.layout, &yes.layout, &no.layout)?
                        }
                        CudaDtype::U32 => {
                            SelectDispatch::new::<u32>(&mask.layout, &yes.layout, &no.layout)?
                        }
                        _ => SelectDispatch::new::<u16>(&mask.layout, &yes.layout, &no.layout)?,
                    };
                    expanded.add(
                        output,
                        Operation::Select {
                            mask: mask.buffer,
                            yes: yes.buffer,
                            no: no.buffer,
                            pass,
                        },
                    );
                }
            }
            Step::Cast { source, output, to } => {
                if !source.layout.shape().is_empty() {
                    let dtype = to
                        .low_dtype()
                        .or(source.dtype.low_dtype())
                        .ok_or(CudaError::Dtype)?;
                    expanded.add(
                        output,
                        Operation::Cast {
                            source: source.buffer,
                            pass: LowCastDispatch::new(&source.layout, dtype)?,
                            to,
                        },
                    );
                }
            }
            Step::Reduce {
                source,
                output,
                op,
                axes,
                keep_dims,
                mean,
            } => {
                let shape = if mean {
                    tensor_core::mean_shape(source.layout.shape(), &axes, keep_dims)?
                } else {
                    tensor_core::reduction_shape(op, source.layout.shape(), &axes, keep_dims)?
                };
                if &shape != expanded.scratch[output].shape() {
                    return Err(CudaError::InvalidInput("invalid internal reduction output"));
                }
                expanded.reduction(source, output, op, &axes, mean, multiprocessors)?;
            }
            Step::Matmul {
                left,
                right,
                output,
                precision,
            } => {
                let request = match left.dtype.low_dtype() {
                    Some(dtype) => MatmulRequest::Low(dtype),
                    None => MatmulRequest::F32(precision),
                };
                policy(request)?;
                expanded.precisions.push(request);
                let gemm = GemmPlan::new(&left.layout, &right.layout)?;
                if !expanded.scratch[output].shape().is_empty() {
                    if gemm.is_zero() {
                        expanded.add(
                            output,
                            Operation::Fill {
                                count: expanded.scratch[output].shape().numel(),
                                value: 0,
                            },
                        );
                    } else {
                        expanded.add(
                            output,
                            Operation::Gemm {
                                left: left.buffer,
                                right: right.buffer,
                                plan: gemm,
                                request,
                            },
                        );
                    }
                }
            }
        }
    }
    for (i, value) in plan.outputs.into_iter().enumerate() {
        expanded.copy(value, Destination::Output(i))?;
    }
    expanded.account(options)?;
    Ok(expanded)
}
