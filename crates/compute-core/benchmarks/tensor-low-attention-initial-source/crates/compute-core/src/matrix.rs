mod kernels;
pub(crate) use kernels::MatmulKernels;

use crate::{ComputeError, ComputeProgram, GpuArray, uniform_f32};

/// A dense row-major matrix using an existing f32 array. Shapes are exact:
/// padding, arbitrary strides and implicit transposes are not accepted.
#[derive(Clone, Copy)]
pub struct MatrixView<'a> {
    values: &'a GpuArray<f32>,
    rows: u32,
    columns: u32,
}

impl<'a> MatrixView<'a> {
    /// Checks dimension representation, multiplication overflow, and exact array
    /// length. Runtime ownership is checked when the view enters a program.
    pub fn new(
        values: &'a GpuArray<f32>,
        rows: usize,
        columns: usize,
    ) -> Result<Self, ComputeError> {
        let count = rows.checked_mul(columns).ok_or(ComputeError::OutOfBounds)?;
        let rows = u32::try_from(rows).map_err(|_| ComputeError::OutOfBounds)?;
        let columns = u32::try_from(columns).map_err(|_| ComputeError::OutOfBounds)?;
        if values.len() != count {
            return Err(ComputeError::LengthMismatch {
                expected: count,
                actual: values.len(),
            });
        }
        Ok(Self {
            values,
            rows,
            columns,
        })
    }
    pub fn values(self) -> &'a GpuArray<f32> {
        self.values
    }
    pub fn rows(self) -> usize {
        self.rows as usize
    }
    pub fn columns(self) -> usize {
        self.columns as usize
    }
}

/// Owned GPU matrix result. The array can feed ordinary elementwise, comparison,
/// selection or reduction operations without a CPU copy.
#[derive(Clone)]
pub struct GpuMatrix {
    values: GpuArray<f32>,
    rows: u32,
    columns: u32,
}

impl GpuMatrix {
    pub fn values(&self) -> &GpuArray<f32> {
        &self.values
    }
    pub fn rows(&self) -> usize {
        self.rows as usize
    }
    pub fn columns(&self) -> usize {
        self.columns as usize
    }
    pub fn view(&self) -> MatrixView<'_> {
        MatrixView {
            values: &self.values,
            rows: self.rows,
            columns: self.columns,
        }
    }
}

impl ComputeProgram<'_> {
    fn check_matrix_inputs(
        &self,
        a: MatrixView<'_>,
        b: MatrixView<'_>,
    ) -> Result<(), ComputeError> {
        self.runtime.check(a.values)?;
        self.runtime.check(b.values)?;
        if a.columns != b.rows {
            return Err(ComputeError::LengthMismatch {
                expected: a.columns(),
                actual: b.rows(),
            });
        }
        Ok(())
    }

    /// Dense f32 matrix product `(m × k) @ (k × n)`. Shared-memory 32×32 tiles
    /// form the portable baseline. Bounded Metal paths use aligned vec4 tiles,
    /// direct small products or cooperative inner-axis reduction. Arithmetic
    /// stays f32; inputs and intermediates must remain finite. Accumulation
    /// order varies between kernels, so compare results with a tolerance.
    /// All data stays on the GPU.
    pub fn matmul(
        &mut self,
        a: MatrixView<'_>,
        b: MatrixView<'_>,
    ) -> Result<GpuMatrix, ComputeError> {
        self.check_matrix_inputs(a, b)?;
        let count = a
            .rows()
            .checked_mul(b.columns())
            .ok_or(ComputeError::OutOfBounds)?;
        let values = self.runtime.zeros(count)?;
        let output = GpuMatrix {
            values,
            rows: a.rows,
            columns: b.columns,
        };
        self.matmul_into(a, b, output.view())?;
        Ok(output)
    }

    /// Records a product into distinct row-major output storage of shape m×n.
    /// Empty m/n has no work; k=0 actively writes zero output on each execution.
    /// Updating inputs and resubmitting reuses the pipeline, bindings and output.
    pub fn matmul_into(
        &mut self,
        a: MatrixView<'_>,
        b: MatrixView<'_>,
        output: MatrixView<'_>,
    ) -> Result<(), ComputeError> {
        self.check_matrix_inputs(a, b)?;
        self.runtime.check(output.values)?;
        if (output.rows, output.columns) != (a.rows, b.columns) {
            return Err(ComputeError::ShapeMismatch {
                expected: [a.rows(), b.columns()],
                actual: [output.rows(), output.columns()],
            });
        }
        if output.values.aliases(a.values) || output.values.aliases(b.values) {
            return Err(ComputeError::AliasedOutput);
        }
        if output.values.is_empty() {
            return Ok(());
        }
        let (kernel, dispatches) = self.runtime.matmul.select(
            self.runtime.context().backend,
            a.rows,
            a.columns,
            b.columns,
        );
        let groups = dispatches
            .min(
                self.runtime
                    .device
                    .limits()
                    .max_compute_workgroups_per_dimension,
            )
            .min(65535);
        let params = uniform_f32(
            &self.runtime.device,
            &self.runtime.queue,
            &[
                f32::from_bits(a.rows),
                f32::from_bits(a.columns),
                f32::from_bits(b.columns),
                f32::from_bits(groups),
            ],
        );
        let bindings = kernel.create_bind_group(
            &self.runtime.device,
            &[
                &params,
                a.values.buffer(),
                b.values.buffer(),
                output.values.buffer(),
            ],
        );
        self.batch.push(kernel, &bindings, groups);
        Ok(())
    }
}
