//! Checked statistics geometry and launches shared by eager and prepared execution.
use crate::{
    CudaError, CudaRuntime,
    runtime::{rank, validate_logical_size, validate_storage},
};
use gpu_compute::cuda::{CudaSlice, LaunchConfig, PushKernelArg, cudarc::driver::DeviceRepr};
use tensor_core::{Layout, Shape, statistics_shape};

pub(crate) struct StatsInput<'a, T> {
    pub values: &'a CudaSlice<T>,
    pub dtype: Option<u32>,
}
pub(crate) struct StatsRows<'a> {
    pub first: &'a CudaSlice<f64>,
    pub second: &'a CudaSlice<f64>,
}
pub(crate) struct StatsOutputs<'a> {
    pub mean: &'a mut CudaSlice<f32>,
    pub variance: &'a mut CudaSlice<f32>,
}
#[derive(Clone, Debug)]
pub(crate) struct RowPlan {
    pub metadata: Vec<u64>,
    pub rows: usize,
    pub reduce_count: usize,
    pub chunks: usize,
    pub outer_rank: u32,
    pub reduce_rank: u32,
    pub offset: u64,
    required: usize,
    groups: u32,
}
impl RowPlan {
    pub fn new(layout: &Layout, axes: &[usize], max_groups: usize) -> Result<Self, CudaError> {
        let outer = statistics_shape(layout.shape(), axes, false)?;
        let rows = outer.numel();
        if rows == 0 || layout.shape().is_empty() || max_groups == 0 {
            return Err(CudaError::InvalidInput(
                "statistics rows require nonempty geometry and launch groups",
            ));
        }
        let groups = u32::try_from(max_groups)
            .map_err(|_| CudaError::InvalidInput("statistics launch groups overflow"))?;
        let reduce_count = layout.shape().numel() / rows;
        let chunks = reduce_count
            .div_ceil(2048)
            .min((max_groups / rows).max(1))
            .max(1);
        let kept: Vec<usize> = (0..layout.shape().rank())
            .filter(|axis| !axes.contains(axis))
            .collect();
        let dense = Layout::contiguous(layout.shape().clone())?;
        let mut metadata = Vec::with_capacity(layout.shape().rank() * 3);
        for list in [&kept[..], axes] {
            metadata.extend(list.iter().map(|&axis| layout.shape().dims()[axis] as u64));
            metadata.extend(list.iter().map(|&axis| layout.strides()[axis] as u64));
            metadata.extend(list.iter().map(|&axis| dense.strides()[axis] as u64));
        }
        validate_logical_size::<f64>(&Shape::new(vec![rows, chunks])?)?;
        rank(layout.shape())?;
        Ok(Self {
            required: layout.required_storage_len()?,
            groups,
            metadata,
            rows,
            reduce_count,
            chunks,
            outer_rank: kept.len() as u32,
            reduce_rank: axes.len() as u32,
            offset: layout.offset() as u64,
        })
    }
}

impl RowPlan {
    fn config(&self, jobs: usize) -> LaunchConfig {
        LaunchConfig {
            grid_dim: (jobs.min(self.groups as usize) as u32, 1, 1),
            block_dim: (256, 1, 1),
            shared_mem_bytes: 0,
        }
    }
    fn check<T: DeviceRepr>(
        &self,
        input: &StatsInput<'_, T>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        let width = if input.dtype.is_some() { 2 } else { 4 };
        if std::mem::size_of::<T>() != width || input.dtype.is_some_and(|d| d > 1) {
            return Err(CudaError::Dtype);
        }
        validate_storage(&[
            (input.values.len(), self.required),
            (metadata.len(), self.metadata.len()),
        ])
    }
    pub fn partial<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        input: StatsInput<'_, T>,
        center: &CudaSlice<f64>,
        output: &mut CudaSlice<f64>,
        metadata: &CudaSlice<u64>,
        op: u32,
    ) -> Result<(), CudaError> {
        self.check(&input, metadata)?;
        validate_storage(&[
            (center.len(), self.rows),
            (output.len(), self.rows * self.chunks),
        ])?;
        if op > 3 {
            return Err(CudaError::InvalidInput(
                "invalid statistics partial operation",
            ));
        }
        let (rows, count, chunks) = (
            self.rows as u64,
            self.reduce_count as u64,
            self.chunks as u64,
        );
        unsafe {
            let mut launch = rt
                .device
                .stream
                .launch_builder(&rt.statistics.partial[usize::from(input.dtype.is_some())]);
            launch
                .arg(input.values)
                .arg(center)
                .arg(output)
                .arg(metadata)
                .arg(&rows)
                .arg(&count)
                .arg(&chunks)
                .arg(&self.outer_rank)
                .arg(&self.reduce_rank)
                .arg(&self.offset)
                .arg(&op);
            if let Some(dtype) = input.dtype.as_ref() {
                launch.arg(dtype);
            }
            launch.launch(self.config(self.rows * self.chunks))?;
        }
        Ok(())
    }
    pub fn merge(
        &self,
        rt: &CudaRuntime,
        partials: &CudaSlice<f64>,
        output: &mut CudaSlice<f64>,
        op: u32,
    ) -> Result<(), CudaError> {
        validate_storage(&[
            (partials.len(), self.rows * self.chunks),
            (output.len(), self.rows),
        ])?;
        if op > 3 {
            return Err(CudaError::InvalidInput(
                "invalid statistics merge operation",
            ));
        }
        let (rows, chunks, count) = (
            self.rows as u64,
            self.chunks as u64,
            self.reduce_count as u64,
        );
        unsafe {
            rt.device
                .stream
                .launch_builder(&rt.statistics.merge)
                .arg(partials)
                .arg(output)
                .arg(&rows)
                .arg(&chunks)
                .arg(&count)
                .arg(&op)
                .launch(self.config(self.rows))?;
        }
        Ok(())
    }
    pub fn emit<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        input: StatsInput<'_, T>,
        rows: StatsRows<'_>,
        output: &mut CudaSlice<f32>,
        metadata: &CudaSlice<u64>,
        mode: (u32, f32),
    ) -> Result<(), CudaError> {
        self.check(&input, metadata)?;
        let (op, epsilon) = mode;
        if op > 2 {
            return Err(CudaError::InvalidInput("invalid statistics emit operation"));
        }
        if op == 2 {
            tensor_core::validate_epsilon(epsilon)?;
        }
        let count = (self.rows * self.reduce_count) as u64;
        let reduce_count = self.reduce_count as u64;
        validate_storage(&[
            (rows.first.len(), self.rows),
            (rows.second.len(), self.rows),
            (output.len(), count as usize),
        ])?;
        unsafe {
            let mut launch = rt
                .device
                .stream
                .launch_builder(&rt.statistics.emit[usize::from(input.dtype.is_some())]);
            launch
                .arg(input.values)
                .arg(rows.first)
                .arg(rows.second)
                .arg(output)
                .arg(metadata)
                .arg(&count)
                .arg(&reduce_count)
                .arg(&self.outer_rank)
                .arg(&self.reduce_rank)
                .arg(&self.offset)
                .arg(&op)
                .arg(&epsilon);
            if let Some(dtype) = input.dtype.as_ref() {
                launch.arg(dtype);
            }
            launch.launch(rt.config(count as usize))?;
        }
        Ok(())
    }
    pub fn logsumexp(
        &self,
        rt: &CudaRuntime,
        rows: StatsRows<'_>,
        output: &mut CudaSlice<f32>,
    ) -> Result<(), CudaError> {
        validate_storage(&[
            (rows.first.len(), self.rows),
            (rows.second.len(), self.rows),
            (output.len(), self.rows),
        ])?;
        let count = self.rows as u64;
        unsafe {
            rt.device
                .stream
                .launch_builder(&rt.statistics.lse)
                .arg(rows.first)
                .arg(rows.second)
                .arg(output)
                .arg(&count)
                .launch(rt.config(self.rows))?;
        }
        Ok(())
    }
    pub fn moments<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        input: StatsInput<'_, T>,
        rows: StatsRows<'_>,
        output: StatsOutputs<'_>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        self.check(&input, metadata)?;
        validate_storage(&[
            (rows.first.len(), self.rows),
            (rows.second.len(), self.rows),
            (output.mean.len(), self.rows),
            (output.variance.len(), self.rows),
        ])?;
        let count = self.rows as u64;
        unsafe {
            let mut launch = rt
                .device
                .stream
                .launch_builder(&rt.statistics.moments[usize::from(input.dtype.is_some())]);
            launch
                .arg(input.values)
                .arg(rows.first)
                .arg(rows.second)
                .arg(output.mean)
                .arg(output.variance)
                .arg(metadata)
                .arg(&count)
                .arg(&self.outer_rank)
                .arg(&self.reduce_rank)
                .arg(&self.offset);
            if let Some(dtype) = input.dtype.as_ref() {
                launch.arg(dtype);
            }
            launch.launch(rt.config(self.rows))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dense(dims: &[usize]) -> Layout {
        Layout::contiguous(Shape::new(dims.to_vec()).unwrap()).unwrap()
    }
    #[test]
    fn row_geometry_rejects_empty_jobs_bad_groups_and_duplicate_axes() {
        assert!(RowPlan::new(&dense(&[0, 5]), &[1], 8).is_err());
        assert!(RowPlan::new(&dense(&[2, 0]), &[1], 8).is_err());
        assert!(RowPlan::new(&dense(&[2, 5]), &[1], 0).is_err());
        assert!(RowPlan::new(&dense(&[2, 5]), &[1], u32::MAX as usize + 1).is_err());
        assert!(RowPlan::new(&dense(&[2, 5]), &[1, 1], 8).is_err());
    }
    #[test]
    fn row_and_chunk_products_stay_bounded_for_many_rows_and_long_axes() {
        for (rows, count, groups) in [
            (8193, 2, 8),
            (3, 8193, 8),
            (1, 131077, 8),
            (1, 131077, 1024),
        ] {
            let p = RowPlan::new(&dense(&[rows, count]), &[1], groups).unwrap();
            assert_eq!(p.rows, rows);
            assert_eq!(p.reduce_count, count);
            assert!(p.chunks >= 1 && p.chunks <= count.div_ceil(2048));
            assert!(p.rows * p.chunks <= rows * count);
            assert!(p.config(p.rows * p.chunks).grid_dim.0 > 0);
        }
    }
}
