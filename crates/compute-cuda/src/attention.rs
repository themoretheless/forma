use crate::attention_dispatch::{
    AttentionBuffers, AttentionDispatch, AttentionInput, AttentionMaskStorage, AttentionOutputs,
};
use crate::{CudaError, CudaRuntime, CudaTensor, indexing::output};
use gpu_compute::cuda::{CudaFunction, CudaModule, cudarc::driver::DeviceRepr};
use std::sync::Arc;
use tensor_core::{AttentionMask, AttentionOptions, AttentionPlan, TensorAttentionBackend};

#[derive(Clone)]
pub(crate) struct AttentionKernel(pub(crate) [CudaFunction; 2]);
impl AttentionKernel {
    pub(crate) fn load(module: &Arc<CudaModule>) -> Result<Self, CudaError> {
        Ok(Self([
            module.load_function("attention_f32")?,
            module.load_function("attention_low")?,
        ]))
    }
}

impl CudaRuntime {
    pub(crate) fn attention_loaded<T: DeviceRepr>(
        &self,
        query: &CudaTensor<T>,
        key: &CudaTensor<T>,
        value: &CudaTensor<T>,
        mask: AttentionMask<'_, CudaTensor, CudaTensor<u32>>,
        plan: AttentionPlan,
        dtype: Option<u32>,
    ) -> Result<CudaTensor, CudaError> {
        self.check(query)?;
        self.check(key)?;
        self.check(value)?;
        let (mask_layout, mask_flag) = match mask {
            AttentionMask::None => (None, 0u32),
            AttentionMask::Keep(tensor) => {
                self.check(tensor)?;
                (Some(tensor.layout()), 1)
            }
            AttentionMask::Additive(tensor) => {
                self.check(tensor)?;
                (Some(tensor.layout()), 2)
            }
        };
        let mut result = self.zeros(plan.output.clone())?;
        if result.shape().is_empty() || plan.keys == 0 {
            return Ok(result);
        }
        let pass = AttentionDispatch::new::<T>(
            &plan,
            query.layout(),
            key.layout(),
            value.layout(),
            mask_layout.map(|m| (m, mask_flag == 1)),
            self.device.multiprocessors,
        )?;
        let metadata = self.metadata(&pass.metadata)?;
        let mut scratch = self.zeros_typed::<f64>(plan.output.clone())?;
        let mask = match mask {
            AttentionMask::None => AttentionMaskStorage::None,
            AttentionMask::Keep(t) => AttentionMaskStorage::Keep(t.storage.as_ref()),
            AttentionMask::Additive(t) => AttentionMaskStorage::Additive(t.storage.as_ref()),
        };
        pass.launch(
            self,
            AttentionInput {
                buffers: AttentionBuffers {
                    query: query.storage.as_ref(),
                    key: key.storage.as_ref(),
                    value: value.storage.as_ref(),
                },
                dtype,
            },
            mask,
            AttentionOutputs {
                values: output(&mut result),
                accumulator: output(&mut scratch),
            },
            &metadata,
        )?;
        Ok(result)
    }
}

impl TensorAttentionBackend for CudaRuntime {
    fn attention(
        &self,
        query: &CudaTensor,
        key: &CudaTensor,
        value: &CudaTensor,
        mask: AttentionMask<'_, CudaTensor, CudaTensor<u32>>,
        options: AttentionOptions,
    ) -> Result<CudaTensor, CudaError> {
        let plan = AttentionPlan::new(
            query.shape(),
            key.shape(),
            value.shape(),
            mask.shape(),
            options,
        )?;
        self.attention_loaded(query, key, value, mask, plan, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention_dispatch::attention_metadata;
    use tensor_core::{Layout, Shape};
    fn shape(dims: &[usize]) -> Shape {
        Shape::new(dims.to_vec()).unwrap()
    }

    #[test]
    fn metadata_preserves_offsets_strides_broadcast_and_grouped_heads() {
        let query = Layout::new(shape(&[2, 4, 3, 5]), vec![60, 15, 1, 3], 7).unwrap();
        let key = Layout::new(shape(&[1, 2, 7, 5]), vec![70, 35, 5, 1], 11).unwrap();
        let value = Layout::new(shape(&[2, 2, 7, 9]), vec![126, 63, 9, 1], 13).unwrap();
        let mask = Layout::new(shape(&[3, 7]), vec![7, 1], 17).unwrap();
        let plan = AttentionPlan::new(
            query.shape(),
            key.shape(),
            value.shape(),
            Some(mask.shape()),
            AttentionOptions::default(),
        )
        .unwrap();
        let data = attention_metadata(&plan, &query, &key, &value, Some(&mask)).unwrap();
        assert_eq!(&data[..3], &[2, 4, 3]);
        assert_eq!(&data[3..6], &[60, 15, 1]);
        assert_eq!(&data[6..9], &[0, 35, 0]);
        assert_eq!(&data[9..12], &[126, 63, 0]);
        assert_eq!(&data[12..15], &[0, 0, 7]);
        assert_eq!(&data[15..], &[7, 11, 13, 17, 3, 5, 1, 9, 1, 1, 2]);
    }

    #[test]
    fn matrix_promotion_is_a_view_even_with_transposed_strides() {
        let q = Layout::new(shape(&[3, 5]), vec![1, 3], 2).unwrap();
        let k = Layout::contiguous(shape(&[7, 5])).unwrap();
        let v = Layout::contiguous(shape(&[7, 9])).unwrap();
        let plan = AttentionPlan::new(
            q.shape(),
            k.shape(),
            v.shape(),
            None,
            AttentionOptions::default(),
        )
        .unwrap();
        let data = attention_metadata(&plan, &q, &k, &v, None).unwrap();
        assert_eq!(&data[..4], &[1, 3, 0, 1]);
        assert_eq!(&data[10..], &[2, 0, 0, 0, 3, 5, 1, 9, 1, 0, 1]);
        assert_eq!(plan.output.dims(), &[3, 9]);
    }
}
