//! Error routing, command error sinks, and logical device loss state.

use crate::{DeviceLostReason, Error, ErrorFilter};
use std::sync::{Arc, Mutex};

pub(crate) type DeviceLostHandler = Arc<dyn Fn(DeviceLostReason, String) + Send + Sync + 'static>;

#[derive(Default)]
struct DeviceLossInner {
    callback: Option<DeviceLostHandler>,
    loss: Option<(DeviceLostReason, String)>,
    delivered: bool,
}

#[derive(Default)]
pub(crate) struct DeviceLossState(Mutex<DeviceLossInner>);

impl DeviceLossState {
    fn take_notification(
        state: &mut DeviceLossInner,
    ) -> Option<(DeviceLostHandler, DeviceLostReason, String)> {
        if state.delivered {
            return None;
        }
        let callback = state.callback.as_ref()?.clone();
        let (reason, message) = state.loss.as_ref()?;
        let notification = (callback, *reason, message.clone());
        state.delivered = true;
        Some(notification)
    }

    pub(crate) fn set_callback(&self, callback: DeviceLostHandler) {
        let notification = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            state.callback = Some(callback);
            Self::take_notification(&mut state)
        };
        if let Some((callback, reason, message)) = notification {
            callback(reason, message);
        }
    }

    pub(crate) fn notify(&self, reason: DeviceLostReason, message: String) {
        let notification = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if state.loss.is_none() {
                state.loss = Some((reason, message));
            }
            Self::take_notification(&mut state)
        };
        if let Some((callback, reason, message)) = notification {
            callback(reason, message);
        }
    }
}

pub(crate) type UncapturedErrorHandler = Arc<dyn Fn(Error) + Send + Sync + 'static>;

// Matching scopes consume errors even after their first slot is occupied.
// Invoke unscoped handlers outside both mutexes to permit handler reentry.
pub(crate) fn dispatch_error(
    stack: &Mutex<Vec<crate::types::ErrorScopeEntry>>,
    handler: &Mutex<Option<UncapturedErrorHandler>>,
    error: Error,
) -> bool {
    let filter = match &error {
        Error::OutOfMemory { .. } => ErrorFilter::OutOfMemory,
        Error::Validation { .. } => ErrorFilter::Validation,
        Error::Internal { .. } => ErrorFilter::Internal,
    };
    {
        let mut scopes = stack.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = scopes.iter_mut().rev().find(|entry| entry.filter == filter) {
            entry.record(error);
            return true;
        }
    }
    let callback = handler.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match callback {
        Some(callback) => {
            callback(error);
            true
        }
        None => false,
    }
}

/// Why a shader module or pipeline could not be built.
#[derive(Debug)]
struct ShaderError(String);

impl std::fmt::Display for ShaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ShaderError {}

pub(crate) fn validation_error(description: String) -> Error {
    Error::Validation {
        source: Arc::new(ShaderError(description.clone())),
        description,
    }
}

#[derive(Clone)]
pub(crate) struct CommandErrorSink {
    pub(crate) stack: Arc<Mutex<Vec<crate::types::ErrorScopeEntry>>>,
    pub(crate) handler: Arc<Mutex<Option<UncapturedErrorHandler>>>,
    pub(crate) scopes: Option<Arc<crate::types::SubmittedScopes>>,
    pub(crate) captured: bool,
}

impl CommandErrorSink {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(crate) fn has_scoped_work(&self) -> bool {
        self.scopes.is_some()
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(crate) fn same_device(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.stack, &other.stack)
    }

    pub(crate) fn capture(&self) -> Self {
        if self.captured {
            return self.clone();
        }
        let active = self.stack.lock().unwrap_or_else(|e| e.into_inner());
        let scopes = if active.is_empty() {
            None
        } else {
            Some(Arc::new(crate::types::SubmittedScopes::capture(&active)))
        };
        Self {
            stack: self.stack.clone(),
            handler: self.handler.clone(),
            scopes,
            captured: true,
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(crate) fn same_submission(&self, other: &Self) -> bool {
        match (&self.scopes, &other.scopes) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => self.captured && other.captured && self.same_device(other),
            _ => false,
        }
    }

    pub(crate) fn command_failure(&self, error: Error) {
        if self.captured {
            let filter = match &error {
                Error::OutOfMemory { .. } => ErrorFilter::OutOfMemory,
                Error::Validation { .. } => ErrorFilter::Validation,
                Error::Internal { .. } => ErrorFilter::Internal,
            };
            if let Some(entry) = self
                .scopes
                .as_ref()
                .and_then(|scopes| scopes.0.iter().rev().find(|entry| entry.filter == filter))
            {
                entry.record(error);
                return;
            }
            let handler = self
                .handler
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(handler) = handler {
                handler(error);
            }
        } else {
            dispatch_error(&self.stack, &self.handler, error);
        }
    }
}
