use crate::TensorError;

/// Checked logical dimensions. Rank zero is a scalar containing one element.
/// A zero dimension makes the tensor empty; rank has no artificial upper bound.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shape {
    dims: Vec<usize>,
    numel: usize,
}
impl Shape {
    pub fn new(dims: impl Into<Vec<usize>>) -> Result<Self, TensorError> {
        let dims = dims.into();
        let numel = if dims.contains(&0) {
            0
        } else {
            dims.iter().try_fold(1_usize, |n, &d| {
                n.checked_mul(d).ok_or(TensorError::ShapeOverflow)
            })?
        };
        Ok(Self { dims, numel })
    }
    pub fn dims(&self) -> &[usize] {
        &self.dims
    }
    pub fn rank(&self) -> usize {
        self.dims.len()
    }
    pub fn numel(&self) -> usize {
        self.numel
    }
    pub fn is_empty(&self) -> bool {
        self.numel == 0
    }

    /// Align trailing axes; equal dimensions or a dimension of one can match.
    /// In particular, broadcasting dimensions zero and one produces zero.
    pub fn broadcast(&self, other: &Self) -> Result<Self, TensorError> {
        if self.dims == other.dims {
            return Ok(self.clone());
        }
        Self::new(broadcast_dims(&self.dims, &other.dims)?)
    }

    /// Broadcasts every operand before checking the final element count. A
    /// later zero axis can make the result empty even if an intermediate
    /// pairwise shape would have overflowed. No intermediate tensor is needed.
    pub fn broadcast_all(shapes: &[&Self]) -> Result<Self, TensorError> {
        let mut dims = Vec::new();
        for shape in shapes {
            dims = broadcast_dims(&dims, shape.dims())?;
        }
        Self::new(dims)
    }

    pub fn validate_axes(&self, axes: &[usize]) -> Result<(), TensorError> {
        let rank = self.rank();
        if rank <= 64 {
            let mut seen_mask = 0u64;
            for &axis in axes {
                if axis >= rank {
                    return Err(TensorError::AxisOutOfBounds { axis, rank });
                }
                let bit = 1u64 << axis;
                if seen_mask & bit != 0 {
                    return Err(TensorError::DuplicateAxis { axis });
                }
                seen_mask |= bit;
            }
            return Ok(());
        }
        let mut seen = vec![false; rank];
        for &axis in axes {
            let Some(present) = seen.get_mut(axis) else {
                return Err(TensorError::AxisOutOfBounds { axis, rank });
            };
            if *present {
                return Err(TensorError::DuplicateAxis { axis });
            }
            *present = true;
        }
        Ok(())
    }

    /// Empty axes are the identity. Reduced axes of zero length still produce
    /// the same shape as other reductions. Operation-specific empty identities
    /// and errors are validated by `reduction_shape` and `mean_shape`.
    pub fn reduce(&self, axes: &[usize], keep_dims: bool) -> Result<Self, TensorError> {
        self.validate_axes(axes)?;
        let mut dims = Vec::with_capacity(self.rank());
        for (axis, &dim) in self.dims.iter().enumerate() {
            if axes.contains(&axis) {
                if keep_dims {
                    dims.push(1);
                }
            } else {
                dims.push(dim);
            }
        }
        Self::new(dims)
    }

    pub fn permute(&self, axes: &[usize]) -> Result<Self, TensorError> {
        if axes.len() != self.rank() {
            return Err(TensorError::InvalidPermutation);
        }
        self.validate_axes(axes)?;
        let mut dims = Vec::with_capacity(axes.len());
        for &axis in axes {
            dims.push(self.dims[axis]);
        }
        Self::new(dims)
    }
}

fn broadcast_dims(left: &[usize], right: &[usize]) -> Result<Vec<usize>, TensorError> {
    let rank = left.len().max(right.len());
    let mut dims = Vec::with_capacity(rank);
    for axis in 0..rank {
        let a = axis.checked_sub(rank - left.len()).map_or(1, |i| left[i]);
        let b = axis.checked_sub(rank - right.len()).map_or(1, |i| right[i]);
        let dim = if a == b || b == 1 {
            a
        } else if a == 1 {
            b
        } else {
            return Err(TensorError::IncompatibleBroadcast {
                left: left.to_vec(),
                right: right.to_vec(),
            });
        };
        dims.push(dim);
    }
    Ok(dims)
}

/// Logical and promoted shapes for vector, matrix and batched matrix products.
/// A left vector becomes `[1, K]`, a right vector becomes `[K, 1]`; the inserted
/// dimensions are removed from `output`. `matrix_output` retains them for
/// matrix kernels. Leading batch dimensions broadcast, contraction axes do not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatmulPlan {
    pub left: Shape,
    pub right: Shape,
    pub matrix_output: Shape,
    pub output: Shape,
}

impl MatmulPlan {
    pub fn new(left: &Shape, right: &Shape) -> Result<Self, TensorError> {
        if left.rank() == 0 || right.rank() == 0 {
            return Err(TensorError::MatmulRank {
                left: left.rank(),
                right: right.rank(),
            });
        }
        let left_vector = left.rank() == 1;
        let right_vector = right.rank() == 1;
        let left = if left_vector {
            Shape::new(vec![1, left.dims()[0]])?
        } else {
            left.clone()
        };
        let right = if right_vector {
            Shape::new(vec![right.dims()[0], 1])?
        } else {
            right.clone()
        };
        let a = left.dims();
        let b = right.dims();
        if a[a.len() - 1] != b[b.len() - 2] {
            return Err(TensorError::MatmulInnerDimension {
                left: a[a.len() - 1],
                right: b[b.len() - 2],
            });
        }
        let mut dims = broadcast_dims(&a[..a.len() - 2], &b[..b.len() - 2])?;
        dims.extend([a[a.len() - 2], b[b.len() - 1]]);
        let matrix_output = Shape::new(dims.clone())?;
        if right_vector {
            dims.pop();
        }
        if left_vector {
            dims.remove(matrix_output.rank() - 2);
        }
        let output = Shape::new(dims)?;
        Ok(Self {
            left,
            right,
            matrix_output,
            output,
        })
    }
}

/// Shape of a vector, matrix or batched matrix product. Two vectors return a
/// scalar, including zero-length vectors (whose dot product is zero).
pub fn matmul_shape(left: &Shape, right: &Shape) -> Result<Shape, TensorError> {
    Ok(MatmulPlan::new(left, right)?.output)
}
