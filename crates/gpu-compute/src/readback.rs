use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadbackError {
    InvalidRange,
    BlockingUnavailable,
    MissingUsage,
    NotSubmitted,
    Consumed,
    Cancelled,
    Timeout,
    Mapping(String),
    Device(String),
}
impl std::fmt::Display for ReadbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlockingUnavailable => write!(
                f,
                "blocking GPU readback is unavailable in a browser; use try_read"
            ),
            Self::InvalidRange => write!(f, "invalid or unaligned GPU readback range"),
            Self::MissingUsage => write!(f, "buffer lacks the required readback usage"),
            Self::NotSubmitted => write!(f, "readback commands have not been submitted"),
            Self::Consumed => write!(f, "readback result already consumed"),
            Self::Cancelled => write!(f, "readback cancelled"),
            Self::Timeout => write!(f, "GPU readback timed out"),
            Self::Mapping(e) => write!(f, "GPU mapping failed: {e}"),
            Self::Device(e) => write!(f, "GPU polling failed: {e}"),
        }
    }
}
impl std::error::Error for ReadbackError {}

/// Single owner of a byte readback and its mapping lifecycle. Record copies and
/// mapping on an encoder, submit it, then call `submitted` with that submission.
/// The source/staging and encoder must belong to the supplied device. The typed
/// buffer-view API validates ownership before callers reach this low-level API.
pub struct ByteReadback {
    device: wgpu::Device,
    buffer: Option<wgpu::Buffer>,
    size: u64,
    receiver: Option<mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
    submission: Option<wgpu::SubmissionIndex>,
    submitted: bool,
    terminal: Option<ReadbackError>,
    cancelled: Arc<Mutex<bool>>,
}

impl ByteReadback {
    fn range(buffer: &wgpu::Buffer, offset: u64, size: u64) -> Result<(), ReadbackError> {
        if !offset.is_multiple_of(4)
            || !size.is_multiple_of(4)
            || offset
                .checked_add(size)
                .is_none_or(|end| end > buffer.size())
        {
            return Err(ReadbackError::InvalidRange);
        }
        Ok(())
    }

    /// Records a readback from `source`:
    /// - Under [`crate::MemoryArchitecture::Uma`] (Apple Silicon Metal `StorageModeShared`),
    ///   if `source` already carries `MAP_READ` (a mappable primary storage buffer) and `offset == 0`,
    ///   maps `source` directly on submission with zero staging buffer allocation or DMA copy.
    /// - Under [`crate::MemoryArchitecture::Duma`] (or when `source` is device-private),
    ///   allocates a `COPY_DST | MAP_READ` staging buffer and records a DMA `copy_buffer_to_buffer`.
    pub fn copy_buffer(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        offset: u64,
        size: u64,
    ) -> Result<Self, ReadbackError> {
        Self::range(source, offset, size)?;
        if size == 0 {
            if !source.usage().intersects(wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::MAP_READ) {
                return Err(ReadbackError::MissingUsage);
            }
            return Ok(Self::empty(device));
        }
        if crate::device_memory_architecture(device) == crate::MemoryArchitecture::Uma
            && offset == 0
            && source.usage().contains(wgpu::BufferUsages::MAP_READ)
        {
            return Self::map_on_submit(device, encoder, source, size);
        }
        if !source.usage().contains(wgpu::BufferUsages::COPY_SRC) {
            return Err(ReadbackError::MissingUsage);
        }
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(source, offset, &staging, 0, size);
        Self::map_on_submit(device, encoder, &staging, size)
    }

    /// Maps an unmapped MAP_READ staging buffer after previously recorded
    /// copies finish. Used by texture adapters, which own row/format policy.
    pub fn map_on_submit(
        device: &wgpu::Device,
        encoder: &wgpu::CommandEncoder,
        staging: &wgpu::Buffer,
        size: u64,
    ) -> Result<Self, ReadbackError> {
        Self::range(staging, 0, size)?;
        if !staging.usage().contains(wgpu::BufferUsages::MAP_READ) {
            return Err(ReadbackError::MissingUsage);
        }
        if size == 0 {
            return Ok(Self::empty(device));
        }
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(Mutex::new(false));
        encoder.map_buffer_on_submit(
            staging,
            wgpu::MapMode::Read,
            ..size,
            mapping_callback(staging.clone(), cancelled.clone(), sender),
        );
        Ok(Self {
            device: device.clone(),
            buffer: Some(staging.clone()),
            size,
            receiver: Some(receiver),
            submission: None,
            submitted: false,
            terminal: None,
            cancelled,
        })
    }

    fn empty(device: &wgpu::Device) -> Self {
        Self {
            device: device.clone(),
            buffer: None,
            size: 0,
            receiver: None,
            submission: None,
            submitted: false,
            terminal: None,
            cancelled: Arc::new(Mutex::new(false)),
        }
    }

    pub fn submitted(&mut self, index: wgpu::SubmissionIndex) {
        self.submission = Some(index);
        self.submitted = true;
    }

    pub fn cancel(&mut self) {
        if self.terminal.is_none() {
            self.release_mapping();
            self.terminal = Some(ReadbackError::Cancelled);
        }
    }

    fn release_mapping(&mut self) {
        let mut cancelled = self.cancelled.lock().expect("readback cancellation lock");
        *cancelled = true;
        if let Some(receiver) = self.receiver.take() {
            if let Ok(Ok(())) = receiver.try_recv() {
                if let Some(buffer) = &self.buffer {
                    buffer.unmap();
                }
            }
        }
        // A later callback sees the disconnected channel and unmaps itself.
    }

    pub fn try_read(&mut self) -> Result<Option<Vec<u8>>, ReadbackError> {
        self.try_read_with(<[u8]>::to_vec)
    }

    /// Consume mapped bytes in place, without an intermediate host byte Vec.
    /// The result cannot borrow the mapping. It is released before return,
    /// including if the consumer unwinds; pending/failed reads do not call it.
    pub fn try_read_with<R>(
        &mut self,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Result<Option<R>, ReadbackError> {
        if let Some(error) = &self.terminal {
            return Err(error.clone());
        }
        if !self.submitted {
            return Err(ReadbackError::NotSubmitted);
        }
        self.device.poll(wgpu::PollType::Poll).map_err(poll_error)?;
        if self.size == 0 {
            self.terminal = Some(ReadbackError::Consumed);
            return Ok(Some(consume(&[])));
        }
        match self
            .receiver
            .as_ref()
            .expect("pending readback receiver")
            .try_recv()
        {
            Ok(result) => {
                self.terminal = Some(ReadbackError::Consumed);
                if let Err(error) = result {
                    let error = ReadbackError::Mapping(error.to_string());
                    self.terminal = Some(error.clone());
                    return Err(error);
                }
                let buffer = self.buffer.as_ref().expect("nonempty staging");
                let unmap = UnmapOnDrop(buffer);
                let result = buffer
                    .get_mapped_range(..self.size)
                    .map(|view| consume(&view))
                    .map_err(|e| ReadbackError::Mapping(e.to_string()));
                drop(unmap);
                result.map(Some)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                let error = ReadbackError::Mapping("mapping callback dropped".into());
                self.terminal = Some(error.clone());
                Err(error)
            }
        }
    }

    /// Browser callers must poll `try_read` from their event loop.
    #[cfg(target_arch = "wasm32")]
    pub fn wait(&mut self, _timeout: Duration) -> Result<Vec<u8>, ReadbackError> {
        Err(ReadbackError::BlockingUnavailable)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn wait_with<R>(
        &mut self,
        _timeout: Duration,
        _consume: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, ReadbackError> {
        Err(ReadbackError::BlockingUnavailable)
    }

    /// A timeout leaves the ticket pending, so callers can retry or cancel it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(&mut self, timeout: Duration) -> Result<Vec<u8>, ReadbackError> {
        self.wait_with(timeout, <[u8]>::to_vec)
    }

    /// Blocking variant of `try_read_with`; a timeout retains the pending read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_with<R>(
        &mut self,
        timeout: Duration,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, ReadbackError> {
        if let Some(error) = &self.terminal {
            return Err(error.clone());
        }
        if !self.submitted {
            return Err(ReadbackError::NotSubmitted);
        }
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: self.submission.clone(),
                timeout: Some(timeout),
            })
            .map_err(poll_error)?;
        self.try_read_with(consume)?.ok_or(ReadbackError::Timeout)
    }
}
struct UnmapOnDrop<'a>(&'a wgpu::Buffer);
impl Drop for UnmapOnDrop<'_> {
    fn drop(&mut self) {
        self.0.unmap();
    }
}
impl Drop for ByteReadback {
    fn drop(&mut self) {
        self.release_mapping();
    }
}
fn poll_error(error: wgpu::PollError) -> ReadbackError {
    match error {
        wgpu::PollError::Timeout => ReadbackError::Timeout,
        other => ReadbackError::Device(other.to_string()),
    }
}

fn mapping_callback(
    retained: wgpu::Buffer,
    cancelled: Arc<Mutex<bool>>,
    sender: mpsc::Sender<Result<(), wgpu::BufferAsyncError>>,
) -> impl FnOnce(Result<(), wgpu::BufferAsyncError>) + 'static {
    move |result| {
        // Serialize delivery with cancellation: a successful map must have one
        // owner responsible for unmapping, even when the ticket is dropped.
        let abandoned = cancelled.lock().expect("readback cancellation lock");
        let mapped = result.is_ok();
        if (*abandoned || sender.send(result).is_err()) && mapped {
            retained.unmap();
        }
    }
}

/// Reads an already-submitted, unmapped staging buffer. The empty range succeeds
/// without mapping. Mapping is always released before returning a result.
pub fn try_read_buffer(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    size: usize,
) -> Result<Vec<u8>, ReadbackError> {
    try_read_buffer_with(device, buffer, size, <[u8]>::to_vec)
}

/// Decode an already-submitted staging buffer directly from its mapping.
pub fn try_read_buffer_with<R>(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    size: usize,
    consume: impl FnOnce(&[u8]) -> R,
) -> Result<R, ReadbackError> {
    ByteReadback::range(buffer, 0, size as u64)?;
    if !buffer.usage().contains(wgpu::BufferUsages::MAP_READ) {
        return Err(ReadbackError::MissingUsage);
    }
    if size == 0 {
        return Ok(consume(&[]));
    }
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::new(Mutex::new(false));
    buffer.map_async(
        wgpu::MapMode::Read,
        ..size as u64,
        mapping_callback(buffer.clone(), cancelled.clone(), sender),
    );
    let mut ticket = ByteReadback {
        device: device.clone(),
        buffer: Some(buffer.clone()),
        size: size as u64,
        receiver: Some(receiver),
        submission: None,
        submitted: true,
        terminal: None,
        cancelled,
    };
    ticket.wait_with(Duration::from_secs(30), consume)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn stalled_callback_can_timeout_then_fail_without_an_empty_success() {
        let Some(context) = crate::GpuContext::new() else {
            assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
            return;
        };
        // Hold the callback channel pending independently of GPU speed, then
        // inject its failure. This verifies timeout recovery deterministically.
        let (sender, receiver) = mpsc::channel();
        let mut ticket = ByteReadback::empty(&context.device);
        ticket.size = 4;
        ticket.receiver = Some(receiver);
        ticket.submitted(context.queue.submit([]));
        assert_eq!(
            ticket.wait(Duration::from_secs(1)),
            Err(ReadbackError::Timeout)
        );
        sender.send(Err(wgpu::BufferAsyncError)).unwrap();
        assert!(matches!(ticket.try_read(), Err(ReadbackError::Mapping(_))));
        assert!(matches!(ticket.try_read(), Err(ReadbackError::Mapping(_))));
    }
}
