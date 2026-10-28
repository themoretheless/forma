use crate::{Shape, TensorError};

/// Element-addressed storage view. Stride zero broadcasts a dimension. Strides
/// are nonnegative; reverse views require materialization in this version.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Layout {
    shape: Shape,
    strides: Vec<usize>,
    offset: usize,
}
impl Layout {
    pub fn new(
        shape: Shape,
        strides: impl Into<Vec<usize>>,
        offset: usize,
    ) -> Result<Self, TensorError> {
        let strides = strides.into();
        if shape.rank() != strides.len() {
            return Err(TensorError::RankMismatch {
                expected: shape.rank(),
                actual: strides.len(),
            });
        }
        let result = Self {
            shape,
            strides,
            offset,
        };
        result.required_storage_len()?;
        Ok(result)
    }
    pub fn contiguous(shape: Shape) -> Result<Self, TensorError> {
        let mut strides = vec![0; shape.rank()];
        if !shape.is_empty() {
            let mut stride = 1_usize;
            for (axis, &dim) in shape.dims().iter().enumerate().rev() {
                strides[axis] = stride;
                stride = stride.checked_mul(dim).ok_or(TensorError::LayoutOverflow)?;
            }
        }
        Self::new(shape, strides, 0)
    }
    pub fn shape(&self) -> &Shape {
        &self.shape
    }
    pub fn strides(&self) -> &[usize] {
        &self.strides
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn is_contiguous(&self) -> bool {
        if self.shape.is_empty() {
            return true;
        }
        let mut expected = 1;
        for (&dim, &stride) in self.shape.dims().iter().zip(&self.strides).rev() {
            if dim > 1 && stride != expected {
                return false;
            }
            expected *= dim; // Checked by Shape::new for nonempty shapes.
        }
        true
    }
    pub fn required_storage_len(&self) -> Result<usize, TensorError> {
        if self.shape.is_empty() {
            return Ok(self.offset);
        }
        self.shape
            .dims()
            .iter()
            .zip(&self.strides)
            .try_fold(self.offset, |end, (&dim, &stride)| {
                stride
                    .checked_mul(dim - 1)
                    .and_then(|delta| end.checked_add(delta))
                    .ok_or(TensorError::LayoutOverflow)
            })?
            .checked_add(1)
            .ok_or(TensorError::LayoutOverflow)
    }
    pub fn validate_storage_len(&self, available: usize) -> Result<(), TensorError> {
        let required = self.required_storage_len()?;
        if required > available {
            return Err(TensorError::StorageOutOfBounds {
                required,
                actual: available,
            });
        }
        Ok(())
    }
    pub fn reshape(&self, shape: Shape) -> Result<Self, TensorError> {
        if shape.numel() != self.shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: self.shape.numel(),
                actual: shape.numel(),
            });
        }
        if !self.is_contiguous() {
            return Err(TensorError::NonContiguousReshape);
        }
        let mut result = Self::contiguous(shape)?;
        result.offset = self.offset;
        result.required_storage_len()?;
        Ok(result)
    }
    pub fn permute(&self, axes: &[usize]) -> Result<Self, TensorError> {
        let shape = self.shape.permute(axes)?;
        Self::new(
            shape,
            axes.iter()
                .map(|&axis| self.strides[axis])
                .collect::<Vec<_>>(),
            self.offset,
        )
    }
    pub fn broadcast_to(&self, shape: Shape) -> Result<Self, TensorError> {
        if self.shape.broadcast(&shape)? != shape {
            return Err(TensorError::IncompatibleBroadcast {
                left: self.shape.dims().to_vec(),
                right: shape.dims().to_vec(),
            });
        }
        let leading = shape.rank() - self.shape.rank();
        let mut strides = vec![0; shape.rank()];
        for (axis, &dim) in self.shape.dims().iter().enumerate() {
            if dim != 1 {
                strides[leading + axis] = self.strides[axis];
            }
        }
        Self::new(shape, strides, self.offset)
    }
    pub fn narrow(&self, axis: usize, start: usize, len: usize) -> Result<Self, TensorError> {
        self.shape.validate_axes(&[axis])?;
        let dimension = self.shape.dims()[axis];
        if start > dimension || len > dimension - start {
            return Err(TensorError::InvalidSlice {
                axis,
                start,
                len,
                dimension,
            });
        }
        let mut dims = self.shape.dims().to_vec();
        dims[axis] = len;
        let shape = Shape::new(dims)?;
        // Empty views address no elements. Advancing past a strided final
        // element can exceed the backing allocation or overflow needlessly.
        let offset = if shape.is_empty() {
            self.offset
        } else {
            start
                .checked_mul(self.strides[axis])
                .and_then(|delta| self.offset.checked_add(delta))
                .ok_or(TensorError::LayoutOverflow)?
        };
        Self::new(shape, self.strides.clone(), offset)
    }
    /// Converts a logical row-major index into the backing storage index.
    pub fn element_offset(&self, mut flat: usize) -> Result<usize, TensorError> {
        if flat >= self.shape.numel() {
            return Err(TensorError::StorageOutOfBounds {
                required: flat.saturating_add(1),
                actual: self.shape.numel(),
            });
        }
        let mut offset = self.offset;
        for (&dim, &stride) in self.shape.dims().iter().zip(&self.strides).rev() {
            offset += (flat % dim) * stride;
            flat /= dim;
        }
        Ok(offset)
    }
}
