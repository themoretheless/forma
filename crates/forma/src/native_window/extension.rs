//! What the dev window adds to the host. An embedding application implements [`NativeApp`]
//! only; this interface stays inside the crate.

use super::*;
use crate::native_gpu::Presentation;

/// The state of the window an extension reports in its title.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct HostStatus {
    pub(super) backend: Option<UiBackend>,
    pub(super) platform: TargetPlatform,
    /// The backend chooser is shown instead of the application.
    pub(super) choosing: bool,
    pub(super) gpu: bool,
    /// `Presented` when nothing keeps the next frame from the surface.
    pub(super) surface: Presentation,
}

/// What an extension asks for when the loop is about to wait.
#[derive(Debug, Default, PartialEq)]
pub(super) struct IdleRequest {
    pub(super) redraw: bool,
    /// Wake the loop no later than this.
    pub(super) deadline: Option<Instant>,
    /// A new window title, when it changed.
    pub(super) title: Option<String>,
}

/// Behaviour of the window that belongs to one application rather than to every host.
pub(super) trait HostExtension {
    fn window_created(&mut self, window: &Window);

    /// `false` keeps the host on the software surface.
    fn allows_gpu(&self) -> bool;

    /// Sees a key ahead of the host's own shortcuts; `true` consumes it. Not offered while
    /// the backend chooser is shown.
    fn key(&mut self, key: &Key, pressed: bool, repeat: bool, event_loop: &ActiveEventLoop)
    -> bool;

    /// The primary button and a touch select the control under them instead of pressing it.
    fn picks_elements(&self) -> bool;

    /// Every presented frame is followed by the next one.
    fn redraws_continuously(&self) -> bool;

    fn backend_selected(&mut self, backend: UiBackend);

    fn platform_selected(&mut self, platform: TargetPlatform);

    /// The window title, replacing the host's own.
    fn title(&mut self, status: &HostStatus, now: Instant) -> String;

    fn idle(&mut self, status: &HostStatus, now: Instant) -> IdleRequest;

    /// An event of a window the host did not create.
    fn window_event(&mut self, window: WindowId, event: WindowEvent);
}

/// Finds the extension in an application, if it has one.
pub(super) type Extension<A> = fn(&mut A) -> Option<&mut dyn HostExtension>;

pub(super) fn no_extension<A>(_: &mut A) -> Option<&mut dyn HostExtension> {
    None
}

/// Focuses the control under the point without pressing it.
pub(super) fn pick_control(runtime: &mut crate::Runtime, x: f32, y: f32) {
    let index = runtime.hit_index(x, y);
    if index >= 0 {
        runtime.focus_control(index as usize);
    }
}

impl<A: NativeApp + 'static> NativeHost<A> {
    pub(super) fn extension(&mut self) -> Option<&mut dyn HostExtension> {
        (self.extension)(&mut self.app)
    }

    pub(super) fn picks_elements(&mut self) -> bool {
        self.extension().is_some_and(|ext| ext.picks_elements())
    }

    pub(super) fn status(&self) -> HostStatus {
        let zero_size = self.window.as_ref().is_some_and(|window| {
            let size = window.inner_size();
            size.width == 0 || size.height == 0
        });
        HostStatus {
            backend: self.shell.backend,
            platform: self.shell.platform,
            choosing: self.shell.chooser.is_some(),
            gpu: self.gpu.is_some(),
            surface: if self.occluded {
                Presentation::Occluded
            } else if zero_size {
                Presentation::ZeroSize
            } else if self.retry_at.is_some() {
                Presentation::Retry
            } else {
                Presentation::Presented
            },
        }
    }

    /// Offers a key to the extension ahead of the host's shortcuts.
    pub(super) fn extension_key(
        &mut self,
        event: &winit::event::KeyEvent,
        event_loop: &ActiveEventLoop,
    ) -> bool {
        if self.shell.chooser.is_some() {
            return false;
        }
        let pressed = event.state == ElementState::Pressed;
        self.extension()
            .is_some_and(|ext| ext.key(&event.logical_key, pressed, event.repeat, event_loop))
    }

    /// Lets the extension work while the loop is idle. Returns whether it wants a frame and
    /// when it wants to be woken.
    pub(super) fn extension_idle(&mut self, now: Instant) -> (bool, Option<Instant>) {
        let status = self.status();
        let Some(extension) = self.extension() else {
            return (false, None);
        };
        let request = extension.idle(&status, now);
        if let (Some(title), Some(window)) = (&request.title, &self.window) {
            window.set_title(title);
        }
        (request.redraw, request.deadline)
    }
}
