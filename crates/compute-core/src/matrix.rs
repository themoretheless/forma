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
                    .context()
                    .limits
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

/// Cache-blocked 8x8 register-tiled CPU `f32` matrix multiplication `(rows × inner) @ (inner × cols)`
/// aligned with the 8x8 matrix tile primitive (`simdgroup_matrix` 8x8 on Apple Silicon / AVX/NEON
/// register blocking).
///
/// Uses an 8x8 register tile for interior blocks with zero inner-loop bounds checks, and clean
/// scalar cleanup for boundary rows/columns.
pub fn matmul_f32_tiled_8x8(
    a: &[f32],
    b: &[f32],
    rows: usize,
    inner: usize,
    cols: usize,
) -> Vec<f32> {
    let total_out = rows.saturating_mul(cols);
    let mut out = vec![0.0_f32; total_out];
    if rows == 0 || cols == 0 || inner == 0 {
        return out;
    }
    assert!(
        a.len() >= rows.saturating_mul(inner) && b.len() >= inner.saturating_mul(cols),
        "matmul_f32_tiled_8x8 input slices shorter than declared matrix dimensions"
    );

    const TILE: usize = 8;
    const KC_BLOCK: usize = 64;

    let full_rows = (rows / TILE) * TILE;
    let full_cols = (cols / TILE) * TILE;

    for kc in (0..inner).step_by(KC_BLOCK) {
        let k_end = (kc + KC_BLOCK).min(inner);

        // 8x8 register-tiled interior micro-kernel
        for i0 in (0..full_rows).step_by(TILE) {
            for j0 in (0..full_cols).step_by(TILE) {
                let mut acc = [[0.0_f32; TILE]; TILE];
                for r in 0..TILE {
                    let out_row = &out[(i0 + r) * cols + j0..(i0 + r) * cols + j0 + TILE];
                    acc[r].copy_from_slice(out_row);
                }

                for k in kc..k_end {
                    let b_row = &b[k * cols + j0..k * cols + j0 + TILE];
                    let b0 = b_row[0];
                    let b1 = b_row[1];
                    let b2 = b_row[2];
                    let b3 = b_row[3];
                    let b4 = b_row[4];
                    let b5 = b_row[5];
                    let b6 = b_row[6];
                    let b7 = b_row[7];

                    for (r, acc_row) in acc.iter_mut().enumerate() {
                        let a_val = a[(i0 + r) * inner + k];
                        acc_row[0] = a_val.mul_add(b0, acc_row[0]);
                        acc_row[1] = a_val.mul_add(b1, acc_row[1]);
                        acc_row[2] = a_val.mul_add(b2, acc_row[2]);
                        acc_row[3] = a_val.mul_add(b3, acc_row[3]);
                        acc_row[4] = a_val.mul_add(b4, acc_row[4]);
                        acc_row[5] = a_val.mul_add(b5, acc_row[5]);
                        acc_row[6] = a_val.mul_add(b6, acc_row[6]);
                        acc_row[7] = a_val.mul_add(b7, acc_row[7]);
                    }
                }

                for (r, acc_row) in acc.iter().enumerate() {
                    let out_row = &mut out[(i0 + r) * cols + j0..(i0 + r) * cols + j0 + TILE];
                    out_row.copy_from_slice(acc_row);
                }
            }

            // Right-edge column remainder for full row blocks
            if full_cols < cols {
                for r in 0..TILE {
                    let row_idx = i0 + r;
                    let a_row = &a[row_idx * inner + kc..row_idx * inner + k_end];
                    let out_rem = &mut out[row_idx * cols + full_cols..row_idx * cols + cols];
                    for (offset, &a_val) in a_row.iter().enumerate() {
                        let k = kc + offset;
                        let b_rem = &b[k * cols + full_cols..k * cols + cols];
                        for (o, &b_val) in out_rem.iter_mut().zip(b_rem.iter()) {
                            *o = a_val.mul_add(b_val, *o);
                        }
                    }
                }
            }
        }

        // Bottom-edge row remainder across all columns
        for row_idx in full_rows..rows {
            let a_row = &a[row_idx * inner + kc..row_idx * inner + k_end];
            let out_row = &mut out[row_idx * cols..(row_idx + 1) * cols];
            for (offset, &a_val) in a_row.iter().enumerate() {
                let k = kc + offset;
                let b_row = &b[k * cols..(k + 1) * cols];
                for (o, &b_val) in out_row.iter_mut().zip(b_row.iter()) {
                    *o = a_val.mul_add(b_val, *o);
                }
            }
        }
    }

    out
}

/// Batched strided `f32` matrix multiplication `C[b] = A[b] @ B[b]` for `b in 0..batch`, where each
/// batch slice `A[b]` has shape `(m × k)` and `B[b]` has shape `(k × n)`.
///
/// Delegates each batch slice to the 8×8 register-tiled FMA micro-kernel [`matmul_f32_tiled_8x8`].
/// Also supports broadcasting a single `B` matrix (`b.len() == k * n`) across all `batch` slices of `A`.
pub fn batched_matmul_f32(
    a: &[f32],
    b: &[f32],
    batch: usize,
    m: usize,
    k: usize,
    n: usize,
) -> Result<Vec<f32>, ComputeError> {
    let a_stride = m.checked_mul(k).ok_or(ComputeError::OutOfBounds)?;
    let b_stride = k.checked_mul(n).ok_or(ComputeError::OutOfBounds)?;
    let c_stride = m.checked_mul(n).ok_or(ComputeError::OutOfBounds)?;
    let expected_a = batch.checked_mul(a_stride).ok_or(ComputeError::OutOfBounds)?;
    let expected_b = batch.checked_mul(b_stride).ok_or(ComputeError::OutOfBounds)?;
    let total_c = batch.checked_mul(c_stride).ok_or(ComputeError::OutOfBounds)?;

    if a.len() != expected_a {
        return Err(ComputeError::LengthMismatch {
            expected: expected_a,
            actual: a.len(),
        });
    }
    let broadcast_b = batch > 1 && b.len() == b_stride;
    if !broadcast_b && b.len() != expected_b {
        return Err(ComputeError::LengthMismatch {
            expected: expected_b,
            actual: b.len(),
        });
    }
    if batch == 0 || m == 0 || n == 0 {
        return Ok(vec![0.0_f32; total_c]);
    }

    let mut out = Vec::with_capacity(total_c);
    for idx in 0..batch {
        let a_slice = &a[idx * a_stride..(idx + 1) * a_stride];
        let b_slice = if broadcast_b {
            &b[0..b_stride]
        } else {
            &b[idx * b_stride..(idx + 1) * b_stride]
        };
        let c_slice = matmul_f32_tiled_8x8(a_slice, b_slice, m, k, n);
        out.extend_from_slice(&c_slice);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matmul_f32_tiled_8x8_matches_naive_across_tile_and_remainder_shapes() {
        for (m, k, n) in [(8, 8, 8), (16, 16, 16), (11, 13, 17), (1, 65, 9), (9, 70, 15)] {
            let a: Vec<f32> = (0..(m * k)).map(|i| ((i % 17) as f32) * 0.25 - 1.5).collect();
            let b: Vec<f32> = (0..(k * n)).map(|i| ((i % 13) as f32) * 0.5 - 2.0).collect();
            let got = matmul_f32_tiled_8x8(&a, &b, m, k, n);
            assert_eq!(got.len(), m * n);
            for r in 0..m {
                for c in 0..n {
                    let mut expected = 0.0_f32;
                    for p in 0..k {
                        expected += a[r * k + p] * b[p * n + c];
                    }
                    assert!(
                        (got[r * n + c] - expected).abs() < 1e-4,
                        "mismatch at ({r}, {c}) for shape ({m}, {k}, {n}): got {}, expected {expected}",
                        got[r * n + c]
                    );
                }
            }
        }
        assert!(matmul_f32_tiled_8x8(&[], &[], 0, 0, 0).is_empty());

        // Batched matmul test (2 batches of 2x3 @ 3x2)
        let a_batch = [
            1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0, // batch 0
            2.0, 0.0, 1.0, 0.0, 3.0, 2.0, // batch 1
        ];
        let b_batch = [
            1.0_f32, 0.0, 0.0, 1.0, 1.0, 1.0, // batch 0
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, // batch 1
        ];
        let c_batch = batched_matmul_f32(&a_batch, &b_batch, 2, 2, 3, 2).unwrap();
        assert_eq!(
            c_batch,
            vec![
                4.0, 5.0, 10.0, 11.0, // batch 0
                7.0, 10.0, 19.0, 24.0, // batch 1
            ]
        );
        assert!(batched_matmul_f32(&a_batch[..5], &b_batch, 2, 2, 3, 2).is_err());
    }
}
