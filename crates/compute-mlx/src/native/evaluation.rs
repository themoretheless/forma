use super::{Api, MlxBackend, MlxDtype, MlxError, MlxTensor, native};
use tensor_core::TensorEvalBackend;

impl TensorEvalBackend for MlxBackend {
    fn evaluate(&self, floats: &[&MlxTensor], integers: &[&MlxTensor]) -> Result<(), MlxError> {
        for tensor in floats {
            self.check(tensor, Some(MlxDtype::F32))?;
        }
        for tensor in integers {
            self.check(tensor, Some(MlxDtype::U32))?;
        }
        native(
            "evaluate",
            self.context.api.call(|api| unsafe {
                // MLX 0.32.1 eval_impl detaches non-tracer graph edges after
                // evaluation (mlx/transforms.cpp). Views can still share data
                // storage; this does not clear allocator or compiled caches.
                for tensor in floats.iter().chain(integers) {
                    Api::check((api.array_eval)(tensor.array.raw))?;
                }
                Api::check((api.synchronize)(self.context.stream))
            }),
        )
    }
}
