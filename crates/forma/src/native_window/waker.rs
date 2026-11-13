//! Waking the event loop from application code.

use super::*;

/// Events the host posts to its own loop.
pub(super) enum HostEvent {
    Accessibility(accesskit_winit::Event),
    Wake,
}

impl From<accesskit_winit::Event> for HostEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}

pub(super) struct WakerShared {
    // Whether the proxy is `Sync` differs between winit platforms; the lock makes the
    // handle `Sync` on all of them.
    pub(super) proxy: Mutex<EventLoopProxy<HostEvent>>,
    pub(super) pending: AtomicBool,
}

/// Thread-safe handle that makes the window loop update the application and redraw.
/// Calls made before the loop has served the previous one are merged into it.
#[derive(Clone)]
pub struct NativeWaker {
    pub(super) shared: Arc<WakerShared>,
}

impl NativeWaker {
    pub(super) fn new(proxy: EventLoopProxy<HostEvent>) -> Self {
        Self {
            shared: Arc::new(WakerShared {
                proxy: Mutex::new(proxy),
                pending: AtomicBool::new(false),
            }),
        }
    }

    pub(super) fn proxy(&self) -> EventLoopProxy<HostEvent> {
        self.shared
            .proxy
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Asks the loop to call `NativeApp::update` and redraw. Returns `false` once the
    /// loop has ended, so a worker can stop.
    pub fn wake(&self) -> bool {
        if !begin_wake(&self.shared.pending) {
            return true;
        }
        let sent = self.proxy().send_event(HostEvent::Wake).is_ok();
        if !sent {
            self.shared.pending.store(false, Ordering::Release);
        }
        sent
    }
}

/// Claims the pending wake; `false` means one is already queued and covers this call.
pub(super) fn begin_wake(pending: &AtomicBool) -> bool {
    !pending.swap(true, Ordering::AcqRel)
}

/// Releases the pending wake before the update it stands for, so a result published
/// during that update queues the next one.
pub(super) fn finish_wake(pending: &AtomicBool) {
    pending.store(false, Ordering::Release);
}
