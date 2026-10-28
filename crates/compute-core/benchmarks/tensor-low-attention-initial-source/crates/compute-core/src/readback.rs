use crate::{ComputeError, GpuArray, GpuElement, wgpu};
use gpu_compute::ByteReadback;
use std::marker::PhantomData;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

/// Typed decoding over the shared GPU byte transport. A result is consumed once.
/// Dropping/cancelling a ticket releases its mapping without waiting for the GPU.
pub struct Readback<T: GpuElement> {
    bytes: ByteReadback,
    marker: PhantomData<T>,
}
impl<T: GpuElement> Readback<T> {
    pub(crate) fn record(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        array: &GpuArray<T>,
    ) -> Result<Self, ComputeError> {
        Ok(Self {
            bytes: ByteReadback::copy_buffer(
                device,
                encoder,
                array.buffer(),
                0,
                array.len() as u64 * 4,
            )?,
            marker: PhantomData,
        })
    }
    pub fn submitted(&mut self, index: wgpu::SubmissionIndex) {
        self.bytes.submitted(index);
    }
    pub fn cancel(&mut self) {
        self.bytes.cancel();
    }
    pub fn try_read(&mut self) -> Result<Option<Vec<T>>, ComputeError> {
        Ok(self.bytes.try_read()?.map(decode::<T>))
    }
    /// Waits without taking ownership of the ticket. On timeout, the pending
    /// result can be retried with `wait_mut` or polled with `try_read`.
    /// Successful reads consume the result exactly once.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_mut(&mut self, timeout: Duration) -> Result<Vec<T>, ComputeError> {
        Ok(decode(self.bytes.wait(timeout)?))
    }

    /// Consumes the ticket, including on timeout. Use `wait_mut` to retain a
    /// pending readback for retry.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(mut self, timeout: Duration) -> Result<Vec<T>, ComputeError> {
        self.wait_mut(timeout)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn typed_wait_preserves_pending_results_after_timeout() {
        let Some(context) = gpu_compute::GpuContext::new() else {
            assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
            return;
        };
        let source = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("typed readback retry input"),
            size: 8,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        context
            .queue
            .write_buffer(&source, 0, &gpu_compute::pack_u32(&[17, u32::MAX]));
        let mut encoder = context.device.create_command_encoder(&Default::default());
        let bytes =
            ByteReadback::copy_buffer(&context.device, &mut encoder, &source, 0, 8).unwrap();
        let mut ticket = Readback::<u32> {
            bytes,
            marker: PhantomData,
        };
        assert!(matches!(
            ticket.wait_mut(Duration::ZERO),
            Err(ComputeError::Transfer(
                gpu_compute::ReadbackError::NotSubmitted
            ))
        ));

        // Deliberately hold the mapping commands outside the queue while using
        // an already completed fence. This simulates a delayed callback without
        // relying on GPU speed or exposing a production test hook.
        let completed = context.queue.submit([]);
        context
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(completed.clone()),
                timeout: Some(Duration::from_secs(5)),
            })
            .unwrap();
        ticket.submitted(completed);
        assert!(matches!(
            ticket.wait_mut(Duration::ZERO),
            Err(ComputeError::Transfer(gpu_compute::ReadbackError::Timeout))
        ));

        ticket.submitted(context.queue.submit([encoder.finish()]));
        assert_eq!(
            ticket.wait_mut(Duration::from_secs(5)).unwrap(),
            [17, u32::MAX]
        );
        assert!(matches!(
            ticket.wait_mut(Duration::ZERO),
            Err(ComputeError::ReadbackConsumed)
        ));
    }
}
fn decode<T: GpuElement>(bytes: Vec<u8>) -> Vec<T> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| T::from_bytes(*bytes))
        .collect()
}
