use super::*;
use tensor_core::{
    Compacted, Gathered, ScanOptions, ScatterOp, Scattered, compact_shape, gather_shape,
    scatter_updates_shape,
};

impl CudaProgramPlanBuilder {
    pub fn scan(
        &mut self,
        value: CudaValue,
        axis: usize,
        options: ScanOptions,
    ) -> Result<CudaValue, CudaError> {
        let source = self.require(value, CudaDtype::F32)?;
        self.scan_value(source, axis, options, CudaDtype::F32)
    }
    pub fn scan_u32(
        &mut self,
        value: CudaValue,
        axis: usize,
        options: ScanOptions,
    ) -> Result<CudaValue, CudaError> {
        let source = self.require(value, CudaDtype::U32)?;
        self.scan_value(source, axis, options, CudaDtype::U32)
    }
    pub fn scan_low_f32(
        &mut self,
        value: CudaValue,
        axis: usize,
        options: ScanOptions,
    ) -> Result<CudaValue, CudaError> {
        let source = self.low(value)?;
        self.scan_value(source, axis, options, CudaDtype::F32)
    }
    pub fn scan_low(
        &mut self,
        value: CudaValue,
        axis: usize,
        options: ScanOptions,
    ) -> Result<CudaValue, CudaError> {
        let dtype = self.low(value)?.dtype.low_dtype().unwrap();
        self.transaction(|this| {
            let scanned = this.scan_low_f32(value, axis, options)?;
            this.cast_to_low(scanned, dtype)
        })
    }
    fn scan_value(
        &mut self,
        source: PlannedValue,
        axis: usize,
        options: ScanOptions,
        dtype: CudaDtype,
    ) -> Result<CudaValue, CudaError> {
        source.layout.shape().validate_axes(&[axis])?;
        self.output(source.layout.shape().clone(), dtype, |output| Step::Scan {
            source,
            output,
            axis,
            options,
        })
    }

    pub fn gather(
        &mut self,
        value: CudaValue,
        indices: CudaValue,
        axis: usize,
    ) -> Result<Gathered<CudaValue, CudaValue>, CudaError> {
        let source = self.require(value, CudaDtype::F32)?;
        self.gather_value(source, indices, axis)
    }
    pub fn gather_u32(
        &mut self,
        value: CudaValue,
        indices: CudaValue,
        axis: usize,
    ) -> Result<Gathered<CudaValue, CudaValue>, CudaError> {
        let source = self.require(value, CudaDtype::U32)?;
        self.gather_value(source, indices, axis)
    }
    pub fn gather_low(
        &mut self,
        value: CudaValue,
        indices: CudaValue,
        axis: usize,
    ) -> Result<Gathered<CudaValue, CudaValue>, CudaError> {
        let source = self.low(value)?;
        self.gather_value(source, indices, axis)
    }
    fn gather_value(
        &mut self,
        source: PlannedValue,
        indices: CudaValue,
        axis: usize,
    ) -> Result<Gathered<CudaValue, CudaValue>, CudaError> {
        let indices = self.require(indices, CudaDtype::U32)?;
        let shape = gather_shape(source.layout.shape(), indices.layout.shape(), axis)?;
        let (values, invalid_count) =
            self.output_with_count(shape, source.dtype, |output, invalid_count| Step::Gather {
                source,
                indices,
                output,
                invalid_count,
                axis,
            })?;
        Ok(Gathered {
            values,
            invalid_count,
        })
    }

    pub fn compact(
        &mut self,
        value: CudaValue,
        mask: CudaValue,
    ) -> Result<Compacted<CudaValue, CudaValue>, CudaError> {
        let source = self.require(value, CudaDtype::F32)?;
        self.compact_value(source, mask)
    }
    pub fn compact_u32(
        &mut self,
        value: CudaValue,
        mask: CudaValue,
    ) -> Result<Compacted<CudaValue, CudaValue>, CudaError> {
        let source = self.require(value, CudaDtype::U32)?;
        self.compact_value(source, mask)
    }
    pub fn compact_low(
        &mut self,
        value: CudaValue,
        mask: CudaValue,
    ) -> Result<Compacted<CudaValue, CudaValue>, CudaError> {
        let source = self.low(value)?;
        self.compact_value(source, mask)
    }
    fn compact_value(
        &mut self,
        source: PlannedValue,
        mask: CudaValue,
    ) -> Result<Compacted<CudaValue, CudaValue>, CudaError> {
        let mut mask = self.require(mask, CudaDtype::U32)?;
        let shape = compact_shape(source.layout.shape(), mask.layout.shape())?;
        mask.broadcast_to(source.layout.shape().clone())?;
        let (values, count) =
            self.output_with_count(shape, source.dtype, |output, count| Step::Compact {
                source,
                mask,
                output,
                count,
            })?;
        Ok(Compacted { values, count })
    }

    pub fn scatter(
        &mut self,
        value: CudaValue,
        indices: CudaValue,
        updates: CudaValue,
        op: ScatterOp,
        axis: usize,
    ) -> Result<Scattered<CudaValue, CudaValue>, CudaError> {
        let source = self.require(value, CudaDtype::F32)?;
        let updates = self.require(updates, CudaDtype::F32)?;
        self.scatter_values(source, indices, updates, op, axis, CudaDtype::F32)
    }
    pub fn scatter_u32(
        &mut self,
        value: CudaValue,
        indices: CudaValue,
        updates: CudaValue,
        op: ScatterOp,
        axis: usize,
    ) -> Result<Scattered<CudaValue, CudaValue>, CudaError> {
        let source = self.require(value, CudaDtype::U32)?;
        let updates = self.require(updates, CudaDtype::U32)?;
        self.scatter_values(source, indices, updates, op, axis, CudaDtype::U32)
    }
    pub fn scatter_low_f32(
        &mut self,
        value: CudaValue,
        indices: CudaValue,
        updates: CudaValue,
        op: ScatterOp,
        axis: usize,
    ) -> Result<Scattered<CudaValue, CudaValue>, CudaError> {
        let (source, updates) = self.low_pair(value, updates)?;
        self.scatter_values(source, indices, updates, op, axis, CudaDtype::F32)
    }
    pub fn scatter_low(
        &mut self,
        value: CudaValue,
        indices: CudaValue,
        updates: CudaValue,
        op: ScatterOp,
        axis: usize,
    ) -> Result<Scattered<CudaValue, CudaValue>, CudaError> {
        let (source, updates) = self.low_pair(value, updates)?;
        let dtype = source.dtype;
        if matches!(op, ScatterOp::Add | ScatterOp::Multiply) {
            self.transaction(|this| {
                let result =
                    this.scatter_values(source, indices, updates, op, axis, CudaDtype::F32)?;
                Ok(Scattered {
                    values: this.cast_to_low(result.values, dtype.low_dtype().unwrap())?,
                    invalid_count: result.invalid_count,
                })
            })
        } else {
            // The raw CAS writer touches complete aligned u32 words. Expansion
            // retains this padded capacity; logical outputs remain unpadded.
            let n = source.layout.shape().numel();
            crate::low_scatter::padded_slots(n)?
                .checked_mul(2)
                .ok_or(CudaError::InvalidInput(
                    "CUDA low scatter padded byte count overflows",
                ))?;
            self.scatter_values(source, indices, updates, op, axis, dtype)
        }
    }
    fn scatter_values(
        &mut self,
        source: PlannedValue,
        indices: CudaValue,
        mut updates: PlannedValue,
        op: ScatterOp,
        axis: usize,
        dtype: CudaDtype,
    ) -> Result<Scattered<CudaValue, CudaValue>, CudaError> {
        let indices = self.require(indices, CudaDtype::U32)?;
        let shape = scatter_updates_shape(
            source.layout.shape(),
            indices.layout.shape(),
            updates.layout.shape(),
            axis,
        )?;
        updates.broadcast_to(shape)?;
        validate_layout(&updates.layout, updates.dtype)?;
        let (values, invalid_count) = self.output_with_count(
            source.layout.shape().clone(),
            dtype,
            |output, invalid_count| Step::Scatter {
                source,
                indices,
                updates,
                output,
                invalid_count,
                op,
                axis,
            },
        )?;
        Ok(Scattered {
            values,
            invalid_count,
        })
    }

    /// Validate both allocations before recording either result. The one Step
    /// retains a scalar count even when its values output is logically empty.
    fn output_with_count(
        &mut self,
        shape: Shape,
        dtype: CudaDtype,
        step: impl FnOnce(usize, usize) -> Step,
    ) -> Result<(CudaValue, CudaValue), CudaError> {
        let layout = Layout::contiguous(shape)?;
        validate_layout(&layout, dtype)?;
        let count_layout = Layout::contiguous(Shape::new(vec![])?)?;
        let output = self.plan.scratch.len();
        let count = output + 1;
        self.plan.scratch.extend([
            TensorSpec {
                layout: layout.clone(),
                dtype,
            },
            TensorSpec {
                layout: count_layout.clone(),
                dtype: CudaDtype::U32,
            },
        ]);
        self.plan.steps.push(step(output, count));
        let values = self.push(PlannedValue {
            input_views: Vec::new(),
            buffer: BufferRef::Scratch(output),
            layout,
            dtype,
        });
        let count = self.push(PlannedValue {
            input_views: Vec::new(),
            buffer: BufferRef::Scratch(count),
            layout: count_layout,
            dtype: CudaDtype::U32,
        });
        Ok((values, count))
    }
}
