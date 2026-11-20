//! Windows the dev host opens besides its main one.

use super::*;
use crate::native_gpu::GpuWindow;
use std::collections::HashMap;

/// Windowless scene and pointer routing for a secondary window so multi-window logic can be
/// exercised deterministically in unit/integration tests as well as real OS event loops.
pub(super) struct AuxiliaryScene {
    pub(super) runtime: crate::Runtime,
    pub(super) pointer: (f32, f32),
}

impl AuxiliaryScene {
    pub(super) fn popup() -> Result<Self, String> {
        let popup_source = "component Popup { Frame { width:260; height:110; padding:12; gap:8; Button { key:'popup-action'; width:236; height:38; text:'Нативный попап (F10)'; clicked -> actions.confirm_popup(); } } }";
        let runtime = crate::Runtime::from_sources(popup_source, crate::BUTTON_COMPONENT)?;
        Ok(Self {
            runtime,
            pointer: (-1.0, -1.0),
        })
    }

    pub(super) fn handle_pointer_move(
        &mut self,
        physical_x: f64,
        physical_y: f64,
        scale: f64,
    ) -> bool {
        let rev = self.runtime.visual_revision();
        let scale = if scale.is_finite() && scale > 0. {
            scale
        } else {
            1.
        };
        let (x, y) = ((physical_x / scale) as f32, (physical_y / scale) as f32);
        self.pointer = (x, y);
        self.runtime.pointer(x, y, 0);
        self.runtime.visual_revision() != rev || self.runtime.is_animating()
    }

    pub(super) fn handle_pointer_leave(&mut self) -> bool {
        let rev = self.runtime.visual_revision();
        self.runtime.pointer(-1.0, -1.0, 3);
        self.runtime.visual_revision() != rev || self.runtime.is_animating()
    }

    pub(super) fn handle_primary_button(&mut self, pressed: bool) -> bool {
        let rev = self.runtime.visual_revision();
        self.runtime
            .pointer(self.pointer.0, self.pointer.1, if pressed { 1 } else { 2 });
        self.runtime.visual_revision() != rev || self.runtime.is_animating()
    }
}

/// Independent native window surface managed by the multi-window host (primary scene window,
/// detached popups, tooltips, or secondary inspector windows).
struct AuxiliaryWindow {
    window: Arc<Window>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    gpu: Option<GpuWindow>,
    surface_size: Option<(u32, u32)>,
    scene: AuxiliaryScene,
}

/// The secondary windows of one event loop, by window id.
#[derive(Default)]
pub(super) struct AuxiliaryWindows {
    windows: HashMap<WindowId, AuxiliaryWindow>,
}

impl AuxiliaryWindows {
    /// Opens the detached popup, or closes every secondary window when one is open.
    pub(super) fn toggle_popup(&mut self, event_loop: &ActiveEventLoop, allow_gpu: bool) {
        if !self.windows.is_empty() {
            self.windows.clear();
            println!("F10: вторичное окно попапа/инспектора закрыто");
            return;
        }
        let Ok(scene) = AuxiliaryScene::popup() else {
            return;
        };
        let Ok(raw_win) = event_loop.create_window(
            Window::default_attributes()
                .with_title("Forma · Detached Popup / Inspector")
                .with_inner_size(LogicalSize::new(
                    scene.runtime.render_width(),
                    scene.runtime.render_height(),
                )),
        ) else {
            return;
        };
        let win = Arc::new(raw_win);
        let id = win.id();
        let mut gpu = None;
        if allow_gpu {
            gpu = pollster::block_on(GpuWindow::new(win.clone())).ok();
        }
        let surface = if gpu.is_none() {
            softbuffer::Context::new(win.clone())
                .ok()
                .and_then(|context| softbuffer::Surface::new(&context, win.clone()).ok())
        } else {
            None
        };
        win.request_redraw();
        self.windows.insert(
            id,
            AuxiliaryWindow {
                window: win,
                surface,
                gpu,
                surface_size: None,
                scene,
            },
        );
        println!("F10: открыто вторичное нативное окно (Multi-Window popup)");
    }

    pub(super) fn window_event(&mut self, window_id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.windows.remove(&window_id);
            return;
        }
        let Some(aux) = self.windows.get_mut(&window_id) else {
            return;
        };
        let needs_redraw = match event {
            WindowEvent::CursorMoved { position, .. } => {
                aux.scene
                    .handle_pointer_move(position.x, position.y, aux.window.scale_factor())
            }
            WindowEvent::CursorLeft { .. } => aux.scene.handle_pointer_leave(),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => aux
                .scene
                .handle_primary_button(state == ElementState::Pressed),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => true,
            WindowEvent::RedrawRequested => {
                aux.scene.runtime.tick(16.0);
                let size = aux.window.inner_size();
                if let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    if let Some(gpu) = &mut aux.gpu {
                        let _ = gpu.draw(&aux.scene.runtime, &aux.window);
                    } else if let Some(surface) = &mut aux.surface {
                        if aux.surface_size != Some((size.width, size.height)) {
                            let _ = surface.resize(w, h);
                            aux.surface_size = Some((size.width, size.height));
                        }
                        if let Ok(mut buf) = surface.buffer_mut() {
                            let _ = aux.scene.runtime.paint_native(
                                &mut buf,
                                size.width,
                                size.height,
                                aux.window.scale_factor() as f32,
                            );
                            let _ = buf.present();
                        }
                    }
                }
                false
            }
            _ => false,
        };
        if needs_redraw {
            aux.window.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auxiliary_scene_handles_popup_pointer_activation_and_fractional_scale() {
        let mut scene = AuxiliaryScene::popup().expect("popup scene loads");
        assert!(scene.handle_pointer_move(24.0 * 1.5, 24.0 * 1.5, 1.5));
        assert!(scene.handle_primary_button(true));
        scene.handle_primary_button(false);
        assert_eq!(scene.runtime.clicks(), 1);
        assert_eq!(scene.runtime.action(), "actions.confirm_popup");
        scene.handle_pointer_leave();
    }
}
