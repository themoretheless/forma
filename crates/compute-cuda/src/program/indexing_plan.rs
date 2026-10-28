//! Expand data-dependent indexing into resident passes and explicit resets.
use super::{
    operation::{Destination, Operation},
    plan::{BufferRef, CudaDtype, PlannedValue},
    preparation::Expanded,
};
use crate::{
    CudaError,
    indexing::{
        compact_dispatch::{CompactDispatch, NormalizeMaskDispatch},
        gather_dispatch::{GatherDispatch, InvalidIndicesDispatch},
        scan_dispatch::ScanPass,
        scatter_dispatch::{ScatterDispatch, ScatterOwnersDispatch},
    },
    low_dispatch::LowCastDispatch,
};
use tensor_core::{Layout, ScanOptions, ScatterOp};

impl Expanded {
    pub(super) fn scan(
        &mut self,
        source: PlannedValue,
        output: usize,
        axis: usize,
        options: ScanOptions,
        multiprocessors: u32,
    ) -> Result<(), CudaError> {
        if source.layout.shape().is_empty() {
            return Ok(());
        }
        let accumulator = self.scratch[output].dtype;
        let pass = match source.dtype {
            CudaDtype::F32 => {
                ScanPass::new::<f32, f32>(&source.layout, axis, options, multiprocessors)?
            }
            CudaDtype::U32 => {
                ScanPass::new::<u32, u32>(&source.layout, axis, options, multiprocessors)?
            }
            _ => ScanPass::new::<u16, f32>(&source.layout, axis, options, multiprocessors)?,
        };
        let totals_shape = pass.totals_shape.clone();
        let totals = self.scratch(totals_shape.clone(), accumulator)?;
        let carry = pass.carry();
        self.add(
            output,
            Operation::Scan {
                source: source.buffer,
                totals,
                pass,
            },
        );
        if let Some(pass) = carry {
            let offsets = self.scratch(totals_shape.clone(), accumulator)?;
            self.scan(
                PlannedValue {
                    input_views: Vec::new(),
                    buffer: BufferRef::Scratch(totals),
                    layout: Layout::contiguous(totals_shape)?,
                    dtype: accumulator,
                },
                offsets,
                1,
                ScanOptions {
                    inclusive: false,
                    reverse: false,
                },
                multiprocessors,
            )?;
            self.add(
                output,
                Operation::ScanCarry {
                    offsets: BufferRef::Scratch(offsets),
                    pass,
                },
            );
        }
        Ok(())
    }

    fn invalid_indices(
        &mut self,
        indices: &PlannedValue,
        length: usize,
        output: usize,
    ) -> Result<(), CudaError> {
        self.add(output, Operation::Zero);
        let pass = InvalidIndicesDispatch::new(&indices.layout, length)?;
        if pass.count != 0 {
            self.add(
                output,
                Operation::InvalidIndices {
                    indices: indices.buffer,
                    pass,
                },
            );
        }
        Ok(())
    }

    pub(super) fn gather(
        &mut self,
        source: PlannedValue,
        indices: PlannedValue,
        output: usize,
        invalid_count: usize,
        axis: usize,
    ) -> Result<(), CudaError> {
        self.invalid_indices(&indices, source.layout.shape().dims()[axis], invalid_count)?;
        if self.scratch[output].shape().is_empty() {
            return Ok(());
        }
        let pass = match source.dtype {
            CudaDtype::F32 => GatherDispatch::new::<f32>(&source.layout, &indices.layout, axis)?,
            CudaDtype::U32 => GatherDispatch::new::<u32>(&source.layout, &indices.layout, axis)?,
            _ => GatherDispatch::new::<u16>(&source.layout, &indices.layout, axis)?,
        };
        self.add(
            output,
            Operation::Gather {
                source: source.buffer,
                indices: indices.buffer,
                pass,
            },
        );
        Ok(())
    }

    pub(super) fn compact(
        &mut self,
        source: PlannedValue,
        mask: PlannedValue,
        output: usize,
        count: usize,
        multiprocessors: u32,
    ) -> Result<(), CudaError> {
        // Previous replay may have selected more entries; reinitialize the tail
        // as well as the scalar count before any data-dependent writes.
        self.add(output, Operation::Zero);
        self.add(count, Operation::Zero);
        if source.layout.shape().is_empty() {
            return Ok(());
        }
        let normalization = NormalizeMaskDispatch::new(source.layout.shape(), &mask.layout)?;
        let flags_shape = normalization.flags_shape.clone();
        let flags = self.scratch(flags_shape.clone(), CudaDtype::U32)?;
        let prefix = self.scratch(flags_shape.clone(), CudaDtype::U32)?;
        self.add(
            flags,
            Operation::Normalize {
                mask: mask.buffer,
                pass: normalization,
            },
        );
        self.scan(
            PlannedValue {
                input_views: Vec::new(),
                buffer: BufferRef::Scratch(flags),
                layout: Layout::contiguous(flags_shape)?,
                dtype: CudaDtype::U32,
            },
            prefix,
            0,
            ScanOptions {
                inclusive: false,
                reverse: false,
            },
            multiprocessors,
        )?;
        let pass = match source.dtype {
            CudaDtype::F32 => CompactDispatch::new::<f32>(&source.layout)?,
            CudaDtype::U32 => CompactDispatch::new::<u32>(&source.layout)?,
            _ => CompactDispatch::new::<u16>(&source.layout)?,
        };
        self.add(
            output,
            Operation::Compact {
                source: source.buffer,
                flags: BufferRef::Scratch(flags),
                prefix: BufferRef::Scratch(prefix),
                count,
                pass,
            },
        );
        Ok(())
    }

    pub(super) fn scatter(
        &mut self,
        source: PlannedValue,
        indices: PlannedValue,
        updates: PlannedValue,
        outputs: (usize, usize),
        op: ScatterOp,
        axis: usize,
    ) -> Result<(), CudaError> {
        let (output, invalid_count) = outputs;
        let output_dtype = self.scratch[output].dtype;
        let pass = match (source.dtype, output_dtype) {
            (CudaDtype::F32, CudaDtype::F32) => ScatterDispatch::new::<f32, f32>(
                op,
                source.layout.shape(),
                &indices.layout,
                &updates.layout,
                axis,
            )?,
            (CudaDtype::U32, CudaDtype::U32) => ScatterDispatch::new::<u32, u32>(
                op,
                source.layout.shape(),
                &indices.layout,
                &updates.layout,
                axis,
            )?,
            (_, CudaDtype::F32) => ScatterDispatch::new::<u16, f32>(
                op,
                source.layout.shape(),
                &indices.layout,
                &updates.layout,
                axis,
            )?,
            _ => ScatterDispatch::new::<u16, u16>(
                op,
                source.layout.shape(),
                &indices.layout,
                &updates.layout,
                axis,
            )?,
        };
        let raw_low = output_dtype.low_dtype().is_some();
        if raw_low && pass.has_work() {
            self.scratch[output].pad_low_words()?;
        }
        if source.dtype != output_dtype {
            if !source.layout.shape().is_empty() {
                self.add(
                    output,
                    Operation::Cast {
                        source: source.buffer,
                        pass: LowCastDispatch::new(
                            &source.layout,
                            source.dtype.low_dtype().ok_or(CudaError::Dtype)?,
                        )?,
                        to: CudaDtype::F32,
                    },
                );
            }
        } else {
            self.copy(source.clone(), Destination::Scratch(output))?;
        }
        self.invalid_indices(&indices, source.layout.shape().dims()[axis], invalid_count)?;
        if !pass.has_work() {
            return Ok(());
        }
        let owners = if pass.needs_owners() {
            let owners =
                ScatterOwnersDispatch::new(&indices.layout, source.layout.shape().dims()[axis])?;
            let slot = self.scratch(owners.shape.clone(), CudaDtype::U32)?;
            self.add(slot, Operation::Zero);
            self.add(
                slot,
                Operation::ScatterOwners {
                    indices: indices.buffer,
                    pass: owners,
                },
            );
            slot
        } else {
            invalid_count
        };
        self.add(
            output,
            Operation::Scatter {
                indices: indices.buffer,
                updates: updates.buffer,
                owners: BufferRef::Scratch(owners),
                pass,
            },
        );
        Ok(())
    }
}
