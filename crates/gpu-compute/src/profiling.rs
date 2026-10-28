//! Optional GPU pass timing. This measures timestamp-query intervals on the
//! device; it excludes host encoding, submission, transfers and readback waits.
use crate::{BufferError, ByteReadback, GpuBuffer, GpuContext, ReadbackError, block_on, wgpu};
use std::{sync::mpsc, time::Duration};

#[derive(Clone, Debug, PartialEq)]
pub enum TimestampError {
    FeatureDisabled,
    InvalidPeriod(f64),
    InvalidPayload { bytes: usize },
    UnavailableSample { index: usize },
    NonMonotonic { start: u64, end: u64 },
    Buffer(BufferError),
    Readback(ReadbackError),
    Device(String),
}
impl std::fmt::Display for TimestampError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FeatureDisabled => write!(
                f,
                "GPU timestamp queries are not enabled; create a context with timestamps"
            ),
            Self::InvalidPeriod(period) => write!(f, "invalid GPU timestamp period {period}"),
            Self::InvalidPayload { bytes } => {
                write!(f, "expected 16 timestamp bytes, received {bytes}")
            }
            Self::UnavailableSample { index } => {
                write!(f, "GPU timestamp sample {index} is unavailable")
            }
            Self::NonMonotonic { start, end } => write!(
                f,
                "GPU timestamps moved backwards or wrapped: {start} -> {end}"
            ),
            Self::Buffer(error) => error.fmt(f),
            Self::Readback(error) => error.fmt(f),
            Self::Device(error) => write!(f, "GPU timestamp operation failed: {error}"),
        }
    }
}
impl std::error::Error for TimestampError {}
impl From<BufferError> for TimestampError {
    fn from(error: BufferError) -> Self {
        Self::Buffer(error)
    }
}
impl From<ReadbackError> for TimestampError {
    fn from(error: ReadbackError) -> Self {
        Self::Readback(error)
    }
}

/// Device timestamps for one compute pass. Absolute ticks have no wall-clock
/// meaning. Equal ticks yield zero at the device's available timing resolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuTimestamp {
    pub start_ticks: u64,
    pub end_ticks: u64,
    pub elapsed_ns: f64,
}

/// Reusable profiler with independent query storage for every recorded ticket.
/// Resolve/readback starts after the measured submission completes. Metal can
/// otherwise return stale counters when resolve is encoded in the same command
/// buffer as the timestamp writes. Uses only TIMESTAMP_QUERY.
pub struct GpuTimer {
    context: GpuContext,
    period_ns: f64,
}
impl GpuTimer {
    pub fn new(context: &GpuContext) -> Result<Self, TimestampError> {
        if !context
            .enabled_features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            return Err(TimestampError::FeatureDisabled);
        }
        let period_ns = f64::from(context.queue.get_timestamp_period());
        validate_period(period_ns)?;
        Ok(Self {
            context: context.clone(),
            period_ns,
        })
    }

    pub fn timestamp_period_ns(&self) -> f64 {
        self.period_ns
    }

    /// Records a timestamped pass without submitting. Invoke `submitted` on
    /// the ticket after queue submit, then poll or wait for its result. Resolving
    /// samples requires another submission after measured work completes; that
    /// profiling overhead is outside the measured GPU interval.
    ///
    /// The raw encoder and all resources used by the closure must belong to
    /// this timer's context. Raw encoder ownership cannot be checked here.
    /// wgpu can defer command validation until encoder finish; use `finish` to
    /// capture those errors. On an immediate backend validation error, discard
    /// the encoder: recording cannot be rolled back. No application-error
    /// Result is accepted by the closure.
    pub fn record_compute(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        record: impl FnOnce(&mut wgpu::ComputePass<'_>),
    ) -> Result<TimestampReadback, TimestampError> {
        let queries = capture_errors(&self.context.device, || {
            self.context
                .device
                .create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("compute pass timer"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 2,
                })
        })?;
        capture_errors(&self.context.device, || {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(label),
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &queries,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            });
            record(&mut pass);
        })?;
        Ok(TimestampReadback {
            context: self.context.clone(),
            period_ns: self.period_ns,
            queries,
            state: TimestampState::Recorded,
        })
    }

    /// Finishes an encoder on this timer's device, capturing deferred command
    /// validation errors before its command buffer can be submitted. On error,
    /// discard the readback tickets recorded on that encoder as well.
    pub fn finish(
        &self,
        encoder: wgpu::CommandEncoder,
    ) -> Result<wgpu::CommandBuffer, TimestampError> {
        capture_errors(&self.context.device, || encoder.finish())
    }
}

enum TimestampState {
    Recorded,
    Submitted {
        // Native blocking waits use the fence; browsers use `completed`.
        #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
        index: wgpu::SubmissionIndex,
        completed: mpsc::Receiver<()>,
    },
    Resolving {
        bytes: ByteReadback,
        _resolve: GpuBuffer,
    },
    Terminal(TimestampError),
}

/// A single-consumption timestamp result. Each ticket keeps its query storage
/// isolated through completion, resolution and mapping. `submitted` is required
/// before `try_read` or `wait`; cancellation applies to every pending stage.
pub struct TimestampReadback {
    context: GpuContext,
    period_ns: f64,
    queries: wgpu::QuerySet,
    state: TimestampState,
}
impl TimestampReadback {
    /// Call after submitting the encoder that contains this ticket's pass.
    /// Later calls do not replace its original completion fence.
    pub fn submitted(&mut self, index: wgpu::SubmissionIndex) {
        if !matches!(self.state, TimestampState::Recorded) {
            return;
        }
        let (sender, completed) = mpsc::channel();
        self.context.queue.on_submitted_work_done(move || {
            let _ = sender.send(());
        });
        self.state = TimestampState::Submitted { index, completed };
    }
    pub fn cancel(&mut self) {
        if !matches!(self.state, TimestampState::Terminal(_)) {
            // Dropping a resolving ByteReadback releases or cancels its mapping.
            self.state = TimestampState::Terminal(ReadbackError::Cancelled.into());
        }
    }
    pub fn try_read(&mut self) -> Result<Option<GpuTimestamp>, TimestampError> {
        if !self.resolve_if_complete()? {
            return Ok(None);
        }
        let TimestampState::Resolving { bytes, .. } = &mut self.state else {
            unreachable!()
        };
        match bytes.try_read()? {
            Some(bytes) => self.consume(&bytes).map(Some),
            None => Ok(None),
        }
    }
    /// A timeout in either completion or mapping keeps the result available.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_mut(&mut self, timeout: Duration) -> Result<GpuTimestamp, TimestampError> {
        let started = std::time::Instant::now();
        match &self.state {
            TimestampState::Recorded => return Err(ReadbackError::NotSubmitted.into()),
            TimestampState::Terminal(error) => return Err(error.clone()),
            TimestampState::Submitted { index, .. } => {
                self.context
                    .device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(index.clone()),
                        timeout: Some(timeout),
                    })
                    .map_err(poll_error)?;
            }
            TimestampState::Resolving { .. } => {}
        }
        if !self.resolve_if_complete()? {
            return Err(ReadbackError::Timeout.into());
        }
        let TimestampState::Resolving { bytes, .. } = &mut self.state else {
            unreachable!()
        };
        let bytes = bytes.wait(timeout.saturating_sub(started.elapsed()))?;
        self.consume(&bytes)
    }
    /// Browser callers advance both stages by polling from their event loop.
    #[cfg(target_arch = "wasm32")]
    pub fn wait_mut(&mut self, _timeout: Duration) -> Result<GpuTimestamp, TimestampError> {
        Err(ReadbackError::BlockingUnavailable.into())
    }
    /// Consumes the ticket, including on timeout; use wait_mut to allow retry.
    pub fn wait(mut self, timeout: Duration) -> Result<GpuTimestamp, TimestampError> {
        self.wait_mut(timeout)
    }

    fn resolve_if_complete(&mut self) -> Result<bool, TimestampError> {
        let completed = match &self.state {
            TimestampState::Recorded => return Err(ReadbackError::NotSubmitted.into()),
            TimestampState::Terminal(error) => return Err(error.clone()),
            TimestampState::Resolving { .. } => return Ok(true),
            TimestampState::Submitted { completed, .. } => completed,
        };
        self.context
            .device
            .poll(wgpu::PollType::Poll)
            .map_err(poll_error)?;
        match completed.try_recv() {
            Ok(()) => {}
            Err(mpsc::TryRecvError::Empty) => return Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => {
                let error = TimestampError::Device("submission completion callback dropped".into());
                self.state = TimestampState::Terminal(error.clone());
                return Err(error);
            }
        }
        match self.resolve() {
            Ok(state) => {
                self.state = state;
                Ok(true)
            }
            Err(error) => {
                self.state = TimestampState::Terminal(error.clone());
                Err(error)
            }
        }
    }

    fn resolve(&self) -> Result<TimestampState, TimestampError> {
        let resolve = GpuBuffer::new(
            &self.context,
            16,
            wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        )?;
        let bytes = capture_errors(&self.context.device, || {
            let mut encoder =
                self.context
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("completed timestamp readback"),
                    });
            // Offset zero meets QUERY_RESOLVE_BUFFER_ALIGNMENT; two u64 results
            // occupy 16 bytes, independently of that offset alignment.
            encoder.resolve_query_set(&self.queries, 0..2, resolve.raw(), 0);
            let mut bytes = ByteReadback::copy_buffer(
                &self.context.device,
                &mut encoder,
                resolve.raw(),
                0,
                16,
            )?;
            bytes.submitted(self.context.queue.submit([encoder.finish()]));
            Ok::<_, TimestampError>(bytes)
        })??;
        Ok(TimestampState::Resolving {
            bytes,
            _resolve: resolve,
        })
    }

    fn consume(&mut self, bytes: &[u8]) -> Result<GpuTimestamp, TimestampError> {
        let result = decode(bytes, self.period_ns);
        self.state = TimestampState::Terminal(match &result {
            Ok(_) => ReadbackError::Consumed.into(),
            Err(error) => error.clone(),
        });
        result
    }
}

fn poll_error(error: wgpu::PollError) -> TimestampError {
    match error {
        wgpu::PollError::Timeout => ReadbackError::Timeout.into(),
        other => TimestampError::Device(other.to_string()),
    }
}

fn validate_period(period: f64) -> Result<(), TimestampError> {
    if !period.is_finite() || period <= 0.0 {
        return Err(TimestampError::InvalidPeriod(period));
    }
    Ok(())
}
fn decode(bytes: &[u8], period_ns: f64) -> Result<GpuTimestamp, TimestampError> {
    validate_period(period_ns)?;
    let pair: &[u8; 16] = bytes
        .try_into()
        .map_err(|_| TimestampError::InvalidPayload { bytes: bytes.len() })?;
    let start_ticks = u64::from_le_bytes(pair[..8].try_into().unwrap());
    let end_ticks = u64::from_le_bytes(pair[8..].try_into().unwrap());
    // Metal reserves all ones as MTLCounterErrorValue. Never reinterpret a pair
    // of unavailable counters as a successful zero-duration measurement.
    for (index, ticks) in [start_ticks, end_ticks].into_iter().enumerate() {
        if ticks == u64::MAX {
            return Err(TimestampError::UnavailableSample { index });
        }
    }
    // Subtract integer ticks before converting: absolute GPU counters can exceed
    // f64's exact-integer range even when a short elapsed interval does not.
    let elapsed = end_ticks
        .checked_sub(start_ticks)
        .ok_or(TimestampError::NonMonotonic {
            start: start_ticks,
            end: end_ticks,
        })?;
    Ok(GpuTimestamp {
        start_ticks,
        end_ticks,
        elapsed_ns: elapsed as f64 * period_ns,
    })
}
fn capture_errors<T>(device: &wgpu::Device, run: impl FnOnce() -> T) -> Result<T, TimestampError> {
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = run();
    let errors = [
        block_on(validation.pop()),
        block_on(internal.pop()),
        block_on(oom.pop()),
    ];
    if let Some(error) = errors.into_iter().flatten().next() {
        return Err(TimestampError::Device(error.to_string()));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pair(start: u64, end: u64) -> Vec<u8> {
        [start.to_le_bytes(), end.to_le_bytes()].concat()
    }
    #[test]
    fn timestamp_conversion_preserves_short_deltas_and_rejects_unusable_samples() {
        let start = (1u64 << 60) + 7;
        let result = decode(&pair(start, start + 3), 2.5).unwrap();
        assert_eq!(result.elapsed_ns, 7.5);
        assert_eq!(result.start_ticks, start);
        assert_eq!(decode(&pair(99, 99), 1.0).unwrap().elapsed_ns, 0.0);
        assert!(matches!(
            decode(&pair(u64::MAX - 1, 0), 1.0),
            Err(TimestampError::NonMonotonic { .. })
        ));
        assert!(matches!(
            decode(&pair(u64::MAX, u64::MAX), 1.0),
            Err(TimestampError::UnavailableSample { index: 0 })
        ));
        assert!(matches!(
            decode(&pair(1, u64::MAX), 1.0),
            Err(TimestampError::UnavailableSample { index: 1 })
        ));
        assert!(matches!(
            decode(&[0; 8], 1.0),
            Err(TimestampError::InvalidPayload { bytes: 8 })
        ));
        for period in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                decode(&pair(0, 1), period),
                Err(TimestampError::InvalidPeriod(_))
            ));
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn completion_and_mapping_timeouts_keep_pending_tickets() {
        let context = match GpuContext::with_timestamps() {
            Ok(context) => context,
            Err(crate::GpuContextError::UnsupportedFeatures { .. }) => {
                assert!(std::env::var_os("COMPUTE_REQUIRE_TIMESTAMPS").is_none());
                return;
            }
            Err(error) => {
                assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(), "{error}");
                return;
            }
        };
        let timer = GpuTimer::new(&context).unwrap();
        let mut unused_encoder = context.device.create_command_encoder(&Default::default());
        let mut pending = timer
            .record_compute(&mut unused_encoder, "timeout fixture", |_| {})
            .unwrap();
        // Hold the completion signal independently of GPU speed. A completed
        // dummy fence plus a pending channel exercises timeout deterministically.
        let (_sender, completed) = mpsc::channel();
        pending.state = TimestampState::Submitted {
            index: context.queue.submit([]),
            completed,
        };
        assert_eq!(
            pending.wait_mut(Duration::from_secs(1)),
            Err(ReadbackError::Timeout.into())
        );
        pending.cancel();
        assert_eq!(pending.try_read(), Err(ReadbackError::Cancelled.into()));

        // Hold mapping commands unsubmitted, then release them. The synthetic
        // known sample tests the mapping stage without GPU scheduling races.
        let resolve = GpuBuffer::new(
            &context,
            16,
            wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        )
        .unwrap();
        context
            .queue
            .write_buffer(resolve.raw(), 0, &pair(123, 126));
        let mut map_encoder = context.device.create_command_encoder(&Default::default());
        let mut bytes =
            ByteReadback::copy_buffer(&context.device, &mut map_encoder, resolve.raw(), 0, 16)
                .unwrap();
        bytes.submitted(context.queue.submit([]));
        let mut pending = timer
            .record_compute(&mut unused_encoder, "mapping fixture", |_| {})
            .unwrap();
        pending.state = TimestampState::Resolving {
            bytes,
            _resolve: resolve,
        };
        assert_eq!(
            pending.wait_mut(Duration::from_secs(1)),
            Err(ReadbackError::Timeout.into())
        );
        let submitted = context.queue.submit([map_encoder.finish()]);
        if let TimestampState::Resolving { bytes, .. } = &mut pending.state {
            bytes.submitted(submitted);
        }
        let sample = pending.wait_mut(Duration::from_secs(1)).unwrap();
        assert_eq!(sample.elapsed_ns, 3.0 * timer.timestamp_period_ns());
        assert_eq!(pending.try_read(), Err(ReadbackError::Consumed.into()));
    }
}
