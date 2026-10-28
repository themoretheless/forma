use crate::TensorIndexBackend;

/// Completes resident f32/u32 results without transferring their values to the
/// host. Lazy backends evaluate every supplied result and its dependencies;
/// eager backends wait for the relevant submitted work to finish.
///
/// All arguments must pass ownership and dtype validation before evaluation or
/// synchronization starts, including empty tensors and later list entries.
/// Duplicate tensors and read-only views are allowed. Empty lists still fence
/// previously submitted work, but do not evaluate unspecified lazy results.
///
/// This is a completion boundary for bounded, tiled computation graphs. It
/// does not materialize views, clear allocator caches, promise a peak-memory
/// bound, or submit commands retained in a caller's unsubmitted recorder.
/// The supplied WGSL implementation is native-only: browser event loops need
/// an asynchronous completion API instead of this blocking contract.
pub trait TensorEvalBackend: TensorIndexBackend {
    fn evaluate(
        &self,
        floats: &[&Self::Tensor],
        integers: &[&Self::UIntTensor],
    ) -> Result<(), Self::Error>;
}
