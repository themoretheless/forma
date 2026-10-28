//! Checked, allocation-free launches for each level of the scan hierarchy.
use crate::{
    CudaError, CudaRuntime,
    runtime::{rank, validate_logical_size},
};
use gpu_compute::cuda::{
    CudaFunction, CudaSlice, LaunchConfig, PushKernelArg, cudarc::driver::DeviceRepr,
};
use tensor_core::{Layout, ScanOptions, Shape};

#[derive(Clone, Debug)]
pub(crate) struct ScanPass {
    pub metadata: Vec<u64>,
    pub output_shape: Shape,
    pub totals_shape: Shape,
    rows: u64,
    length: u64,
    inner: u64,
    chunks: u64,
    row_rank: u32,
    offset: u64,
    axis_stride: u64,
    inclusive: u32,
    reverse: u32,
    groups: u32,
    input_required: usize,
}
pub(crate) struct LoadedScan<'a> {
    pub function: &'a CudaFunction,
    pub dtype: Option<u32>,
}
pub(crate) struct ScanOutputs<'a, T> {
    pub values: &'a mut CudaSlice<T>,
    pub totals: &'a mut CudaSlice<T>,
}
impl ScanPass {
    pub fn new<I: DeviceRepr, O: DeviceRepr>(
        input: &Layout,
        axis: usize,
        options: ScanOptions,
        multiprocessors: u32,
    ) -> Result<Self, CudaError> {
        input.shape().validate_axes(&[axis])?;
        validate_logical_size::<I>(input.shape())?;
        validate_logical_size::<O>(input.shape())?;
        let limit = multiprocessors
            .checked_mul(8)
            .filter(|&n| n != 0)
            .ok_or(CudaError::InvalidInput("invalid scan multiprocessor count"))?;
        let shape = input.shape();
        let length = shape.dims()[axis];
        let empty = shape.is_empty();
        let rows = if empty { 0 } else { shape.numel() / length };
        let chunks = if empty { 0 } else { length.div_ceil(256) };
        // Empty tensors may contain dimensions whose suffix product overflows;
        // no kernel reads that geometry, so it has a zero sentinel instead.
        let inner = if empty {
            0
        } else {
            shape.dims()[axis + 1..].iter().product()
        };
        let totals_shape = Shape::new(vec![rows, chunks])?;
        validate_logical_size::<O>(&totals_shape)?;
        let metadata = shape
            .dims()
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != axis)
            .map(|(_, &d)| d as u64)
            .chain(
                input
                    .strides()
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| i != axis)
                    .map(|(_, &s)| s as u64),
            )
            .collect();
        Ok(Self {
            metadata,
            output_shape: shape.clone(),
            groups: totals_shape.numel().min(limit as usize).max(1) as u32,
            totals_shape,
            rows: rows as u64,
            length: length as u64,
            inner: inner as u64,
            chunks: chunks as u64,
            row_rank: rank(shape)? - 1,
            offset: input.offset() as u64,
            axis_stride: input.strides()[axis] as u64,
            inclusive: u32::from(options.inclusive),
            reverse: u32::from(options.reverse),
            input_required: input.required_storage_len()?,
        })
    }
    pub fn chunks(&self) -> usize {
        self.chunks as usize
    }
    pub fn carry(&self) -> Option<ScanCarryDispatch> {
        (self.chunks() > 1).then(|| ScanCarryDispatch {
            count: self.output_shape.numel() as u64,
            offsets_required: self.totals_shape.numel(),
            length: self.length,
            inner: self.inner,
            chunks: self.chunks,
            reverse: self.reverse,
        })
    }
    fn validate_storage(
        &self,
        input: usize,
        values: usize,
        totals: usize,
        metadata: usize,
    ) -> Result<(), CudaError> {
        if input < self.input_required
            || values < self.output_shape.numel()
            || totals < self.totals_shape.numel()
            || metadata < self.metadata.len()
        {
            return Err(CudaError::InvalidInput(
                "scan dispatch storage is too small",
            ));
        }
        Ok(())
    }
    pub fn launch<I: DeviceRepr, O: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        kernel: LoadedScan<'_>,
        input: &CudaSlice<I>,
        output: ScanOutputs<'_, O>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        self.validate_storage(
            input.len(),
            output.values.len(),
            output.totals.len(),
            metadata.len(),
        )?;
        if self.output_shape.is_empty() {
            return Ok(());
        }
        // Each workgroup owns one row/chunk; only the input loader differs for
        // native low storage. Totals and values share the accumulator type O.
        unsafe {
            let mut launch = rt.device.stream.launch_builder(kernel.function);
            launch
                .arg(input)
                .arg(output.values)
                .arg(output.totals)
                .arg(metadata)
                .arg(&self.rows)
                .arg(&self.length)
                .arg(&self.inner)
                .arg(&self.chunks)
                .arg(&self.row_rank)
                .arg(&self.offset)
                .arg(&self.axis_stride)
                .arg(&self.inclusive)
                .arg(&self.reverse);
            if let Some(dtype) = kernel.dtype.as_ref() {
                launch.arg(dtype);
            }
            launch.launch(LaunchConfig {
                grid_dim: (self.groups, 1, 1),
                block_dim: (256, 1, 1),
                shared_mem_bytes: 0,
            })?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ScanCarryDispatch {
    count: u64,
    length: u64,
    inner: u64,
    chunks: u64,
    reverse: u32,
    offsets_required: usize,
}
impl ScanCarryDispatch {
    fn validate_storage(&self, output: usize, offsets: usize) -> Result<(), CudaError> {
        if output < self.count as usize || offsets < self.offsets_required {
            return Err(CudaError::InvalidInput("scan carry storage is too small"));
        }
        Ok(())
    }
    pub fn launch<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        kernel: &CudaFunction,
        output: &mut CudaSlice<T>,
        offsets: &CudaSlice<T>,
    ) -> Result<(), CudaError> {
        self.validate_storage(output.len(), offsets.len())?;
        unsafe {
            rt.device
                .stream
                .launch_builder(kernel)
                .arg(output)
                .arg(offsets)
                .arg(&self.count)
                .arg(&self.length)
                .arg(&self.inner)
                .arg(&self.chunks)
                .arg(&self.reverse)
                .launch(rt.config(self.count as usize))?;
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
    fn rows_and_carries_map_every_strided_axis_and_direction() {
        let layout = dense(&[3, 5, 517])
            .narrow(2, 3, 511)
            .unwrap()
            .permute(&[1, 2, 0])
            .unwrap();
        for axis in 0..3 {
            for inclusive in [false, true] {
                for reverse in [false, true] {
                    let pass = ScanPass::new::<u16, f32>(
                        &layout,
                        axis,
                        ScanOptions { inclusive, reverse },
                        40,
                    )
                    .unwrap();
                    let rows = Layout::new(
                        Shape::new(
                            pass.metadata[..2]
                                .iter()
                                .map(|&d| d as usize)
                                .collect::<Vec<_>>(),
                        )
                        .unwrap(),
                        pass.metadata[2..]
                            .iter()
                            .map(|&s| s as usize)
                            .collect::<Vec<_>>(),
                        layout.offset(),
                    )
                    .unwrap();
                    for row in 0..pass.rows as usize {
                        for coordinate in 0..pass.length as usize {
                            let inner = pass.inner as usize;
                            let length = pass.length as usize;
                            let logical =
                                ((row / inner) * length + coordinate) * inner + row % inner;
                            assert_eq!(
                                rows.element_offset(row).unwrap()
                                    + coordinate * layout.strides()[axis],
                                layout.element_offset(logical).unwrap()
                            );
                            if let Some(carry) = pass.carry() {
                                let position = if reverse {
                                    length - 1 - coordinate
                                } else {
                                    coordinate
                                };
                                let offset = row * pass.chunks() + position / 256;
                                assert!(offset < carry.offsets_required);
                            }
                        }
                    }
                    assert_eq!(
                        (pass.inclusive, pass.reverse),
                        (u32::from(inclusive), u32::from(reverse))
                    );
                }
            }
        }
    }
    #[test]
    fn chunks_and_accumulator_shapes_cover_boundary_and_multilevel_tails() {
        for (length, chunks) in [
            (1, 1),
            (255, 1),
            (256, 1),
            (257, 2),
            (65536, 256),
            (65537, 257),
            (131077, 513),
        ] {
            let pass =
                ScanPass::new::<u16, f32>(&dense(&[2, length, 3]), 1, ScanOptions::default(), 40)
                    .unwrap();
            assert_eq!(pass.totals_shape.dims(), [6, chunks]);
            assert_eq!(pass.chunks(), chunks);
            assert_eq!(pass.carry().is_some(), chunks > 1);
            assert_eq!(pass.groups, (6 * chunks).min(320) as u32);
            let totals = Layout::contiguous(pass.totals_shape.clone()).unwrap();
            let next = ScanPass::new::<f32, f32>(
                &totals,
                1,
                ScanOptions {
                    inclusive: false,
                    reverse: false,
                },
                40,
            )
            .unwrap();
            assert_eq!(next.totals_shape.dims(), [6, chunks.div_ceil(256)]);
        }
    }
    #[test]
    fn validation_rejects_invalid_axis_sizes_and_short_buffers_before_launch() {
        assert!(ScanPass::new::<u32, u32>(&dense(&[]), 0, ScanOptions::default(), 1).is_err());
        assert!(ScanPass::new::<u32, u32>(&dense(&[3]), 1, ScanOptions::default(), 1).is_err());
        for sm in [0, u32::MAX] {
            assert!(
                ScanPass::new::<f32, f32>(&dense(&[3]), 0, ScanOptions::default(), sm).is_err()
            );
        }
        let source =
            Layout::new(Shape::new(vec![usize::MAX / 4 + 1]).unwrap(), vec![0], 0).unwrap();
        assert!(ScanPass::new::<u16, f32>(&source, 0, ScanOptions::default(), 1).is_err());
        let layout = dense(&[259]).narrow(0, 2, 257).unwrap();
        let pass = ScanPass::new::<f32, f32>(&layout, 0, ScanOptions::default(), 1).unwrap();
        assert!(pass.validate_storage(259, 257, 2, 0).is_ok());
        for (input, values, totals) in [(258, 257, 2), (259, 256, 2), (259, 257, 1)] {
            assert!(pass.validate_storage(input, values, totals, 0).is_err());
        }
        let carry = pass.carry().unwrap();
        assert!(carry.validate_storage(257, 2).is_ok());
        assert!(carry.validate_storage(256, 2).is_err());
        assert!(carry.validate_storage(257, 1).is_err());
        let ranked =
            ScanPass::new::<u32, u32>(&dense(&[2, 3]), 1, ScanOptions::default(), 1).unwrap();
        assert!(ranked.validate_storage(6, 6, 2, 1).is_err());
    }
    #[test]
    fn empty_geometry_avoids_zero_division_and_unused_suffix_overflow() {
        let shape = Shape::new(vec![0, usize::MAX, usize::MAX]).unwrap();
        let layout = Layout::new(shape, vec![0, 0, 0], 0).unwrap();
        for axis in 0..3 {
            let pass = ScanPass::new::<u16, f32>(&layout, axis, ScanOptions::default(), 1).unwrap();
            assert!(pass.output_shape.is_empty());
            assert!(pass.totals_shape.is_empty());
            assert_eq!((pass.rows, pass.inner, pass.chunks), (0, 0, 0));
            assert!(pass.carry().is_none());
        }
    }
}
