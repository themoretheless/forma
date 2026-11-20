//! Errors and the scopes that capture them.

use super::*;

#[derive(Debug, Clone)]
pub enum Error {
    OutOfMemory {
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
    Validation {
        source: Arc<dyn std::error::Error + Send + Sync>,
        description: String,
    },
    Internal {
        source: Arc<dyn std::error::Error + Send + Sync>,
        description: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfMemory { source } => write!(f, "Out of memory: {source}"),
            Self::Validation { description, .. } => write!(f, "Validation error: {description}"),
            Self::Internal { description, .. } => write!(f, "Internal error: {description}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OutOfMemory { source }
            | Self::Validation { source, .. }
            | Self::Internal { source, .. } => Some(source.as_ref()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorFilter {
    OutOfMemory,
    Validation,
    Internal,
}

#[derive(Default)]
pub(super) struct ErrorScopeValue {
    pub(super) error: Option<Error>,
    pub(super) pending: usize,
    pub(super) pop_waiting: bool,
    pub(super) waker: Option<std::task::Waker>,
}

#[derive(Clone)]
pub(crate) struct ErrorScopeEntry {
    pub(crate) filter: ErrorFilter,
    pub(super) value: Arc<Mutex<ErrorScopeValue>>,
}

impl ErrorScopeEntry {
    pub(crate) fn new(filter: ErrorFilter) -> Self {
        Self {
            filter,
            value: Arc::new(Mutex::new(ErrorScopeValue::default())),
        }
    }
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<Error> {
        self.value
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .error
            .clone()
    }
    pub(crate) fn record(&self, error: Error) {
        let mut value = self.value.lock().unwrap_or_else(|e| e.into_inner());
        if value.error.is_none() {
            value.error = Some(error);
        }
    }
}

pub(crate) struct SubmittedScopes(pub(crate) Vec<ErrorScopeEntry>);

impl SubmittedScopes {
    pub(crate) fn capture(entries: &[ErrorScopeEntry]) -> Self {
        for entry in entries {
            entry
                .value
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending += 1;
        }
        Self(entries.to_vec())
    }
}

impl Drop for SubmittedScopes {
    fn drop(&mut self) {
        for entry in &self.0 {
            let wake = {
                let mut value = entry.value.lock().unwrap_or_else(|e| e.into_inner());
                value.pending -= 1;
                if value.pending == 0 {
                    value.waker.take()
                } else {
                    None
                }
            };
            if let Some(waker) = wake {
                waker.wake();
            }
        }
    }
}

#[derive(Default)]
pub(super) struct ScopeServiceState {
    pub(super) waiting: Vec<std::sync::Weak<Mutex<ErrorScopeValue>>>,
    pub(super) running: bool,
    pub(super) epoch: u64,
    #[cfg(test)]
    pub(super) native_completions: usize,
    #[cfg(test)]
    pub(super) starts: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ScopePark {
    GenerationChanged,
    Awakened,
    TimedOut,
}

#[derive(Default)]
pub(crate) struct ScopeCompletionService(pub(super) Mutex<ScopeServiceState>, std::sync::Condvar);

impl ScopeCompletionService {
    pub(super) fn park_if_unchanged(
        &self,
        state: std::sync::MutexGuard<'_, ScopeServiceState>,
        observed: u64,
        timeout: std::time::Duration,
    ) -> ScopePark {
        if state.epoch != observed {
            return ScopePark::GenerationChanged;
        }
        let (_state, result) = self
            .1
            .wait_timeout(state, timeout)
            .unwrap_or_else(|e| e.into_inner());
        if result.timed_out() {
            ScopePark::TimedOut
        } else {
            ScopePark::Awakened
        }
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(crate) fn epoch(&self) -> u64 {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).epoch
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(crate) fn wait_for_epoch_change(&self, observed: u64, timeout: std::time::Duration) {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self.park_if_unchanged(state, observed, timeout);
    }

    pub(crate) fn notify(&self) {
        {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            state.epoch = state.epoch.wrapping_add(1);
        }
        self.1.notify_all();
    }
    #[cfg(any(test, target_os = "macos", target_os = "ios"))]
    pub(crate) fn native_completed(&self) {
        #[cfg(test)]
        {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .native_completions += 1;
        }
        self.notify();
    }
    #[cfg(test)]
    pub(crate) fn native_notifications(&self) -> usize {
        self.0.lock().unwrap().native_completions
    }
    pub(super) fn register(&self, entry: &ErrorScopeEntry, backend: crate::metal::MetalDevice) {
        let start = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            state.waiting.push(Arc::downgrade(&entry.value));
            state.epoch = state.epoch.wrapping_add(1);
            if state.running {
                false
            } else {
                state.running = true;
                #[cfg(test)]
                {
                    state.starts += 1;
                }
                true
            }
        };
        self.1.notify_all();
        if start {
            std::thread::spawn(move || {
                loop {
                    let observed = backend
                        .scope_completion_service
                        .0
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .epoch;
                    // Reap work that finished since the previous iteration
                    // before deciding whether any scope still needs service.
                    let _ = backend.device_loss_message();
                    let mut waiting = {
                        let mut state = backend
                            .scope_completion_service
                            .0
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        std::mem::take(&mut state.waiting)
                    };
                    // Upgraded scope/waker destructors run outside service locks.
                    waiting.retain(|entry| {
                        entry.upgrade().is_some_and(|entry| {
                            let value = entry.lock().unwrap_or_else(|e| e.into_inner());
                            value.pending != 0 && value.pop_waiting
                        })
                    });
                    {
                        let mut state = backend
                            .scope_completion_service
                            .0
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        if state.waiting.is_empty() && waiting.is_empty() {
                            state.running = false;
                            return;
                        }
                        if state.waiting.is_empty() {
                            state.waiting = waiting;
                        } else {
                            state.waiting.append(&mut waiting);
                        }
                        // Notifications are generation-counted to avoid losing
                        // completion/cancellation between scan and parking.
                        let _parked = backend.scope_completion_service.park_if_unchanged(
                            state,
                            observed,
                            std::time::Duration::from_millis(16),
                        );
                    }
                }
            });
        }
    }
    #[cfg(test)]
    pub(crate) fn stats(&self) -> (bool, usize, usize, usize) {
        let state = self.0.lock().unwrap();
        (
            state.running,
            state.starts,
            state.waiting.len(),
            state.waiting.capacity(),
        )
    }
}

pub struct ErrorScopeGuard {
    pub(crate) stack: Arc<Mutex<Vec<ErrorScopeEntry>>>,
    pub(super) entry: ErrorScopeEntry,
    pub(super) completion: Option<crate::metal::MetalDevice>,
}

impl ErrorScopeGuard {
    pub(crate) fn push(stack: Arc<Mutex<Vec<ErrorScopeEntry>>>, filter: ErrorFilter) -> Self {
        let entry = ErrorScopeEntry::new(filter);
        stack
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(entry.clone());
        Self {
            stack,
            entry,
            completion: None,
        }
    }
    pub(crate) fn with_completion(mut self, backend: crate::metal::MetalDevice) -> Self {
        self.completion = Some(backend);
        self
    }
    /// Remove this scope from future submissions and await its captured work.
    pub fn pop(self) -> impl Future<Output = Option<Error>> {
        self.stack
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|entry| !Arc::ptr_eq(&entry.value, &self.entry.value));
        let service = self
            .completion
            .as_ref()
            .map(|backend| backend.scope_completion_service.clone());
        ErrorScopePop {
            entry: self.entry,
            completion: self.completion,
            service,
            started: false,
        }
    }
}

pub(super) struct ErrorScopePop {
    pub(super) entry: ErrorScopeEntry,
    pub(super) completion: Option<crate::metal::MetalDevice>,
    pub(super) started: bool,
    pub(super) service: Option<Arc<ScopeCompletionService>>,
}

impl Drop for ErrorScopePop {
    fn drop(&mut self) {
        let waker = {
            let mut value = self.entry.value.lock().unwrap_or_else(|e| e.into_inner());
            value.pop_waiting = false;
            value.waker.take()
        };
        // Executor task destructors may reenter GPU APIs.
        drop(waker);
        if let Some(service) = &self.service {
            service.notify();
        }
    }
}

impl Future for ErrorScopePop {
    type Output = Option<Error>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let next_waker = cx.waker().clone();
        let old_waker = {
            let mut value = self.entry.value.lock().unwrap_or_else(|e| e.into_inner());
            if value.pending == 0 {
                return std::task::Poll::Ready(value.error.take());
            }
            value.pop_waiting = true;
            value.waker.replace(next_waker)
        };
        drop(old_waker);
        if !self.started {
            self.started = true;
            if let Some(backend) = self.completion.take() {
                backend
                    .scope_completion_service
                    .register(&self.entry, backend.clone());
            }
        }
        std::task::Poll::Pending
    }
}
