use crate::{HasShape, Shape, TensorError};

/// Result of a `topk` selection along the last axis.
#[derive(Clone, Debug, PartialEq)]
pub struct TopKResult {
    pub values: Tensor,
    pub indices: Vec<u32>,
}

/// Host reference tensor with checked [`Shape`] and contiguous `f32` storage.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    shape: Shape,
    data: Vec<f32>,
}

impl HasShape for Tensor {
    fn shape(&self) -> &Shape {
        &self.shape
    }
}

mod activation;
mod attention;
mod channel_norm;
mod convolution;
mod normalization;
mod rope;
mod similarity;
mod softmax;

impl Tensor {
    pub fn new(
        dims: impl Into<Vec<usize>>,
        data: impl Into<Vec<f32>>,
    ) -> Result<Self, TensorError> {
        let shape = Shape::new(dims)?;
        let data = data.into();
        if data.len() != shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: data.len(),
            });
        }
        Ok(Self { shape, data })
    }

    pub fn from_shape(shape: Shape, data: Vec<f32>) -> Result<Self, TensorError> {
        if data.len() != shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: data.len(),
            });
        }
        Ok(Self { shape, data })
    }

    #[inline]
    pub fn shape(&self) -> &Shape {
        &self.shape
    }

    #[inline]
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    #[inline]
    pub fn into_data(self) -> Vec<f32> {
        self.data
    }
}

#[cfg(test)]
mod tests;
