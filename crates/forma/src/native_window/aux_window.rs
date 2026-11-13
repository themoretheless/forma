//! Windows the dev host opens besides its main one.

use super::*;
use crate::native_gpu::GpuWindow;
use std::collections::HashMap;

/// Independent native window surface managed by the multi-window host (primary scene window,
/// detached popups, tooltips, or secondary inspector windows).
struct AuxiliaryWindow {
    window: Arc<Window>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    gpu: Option<GpuWindow>,
    surface_size: Option<(u32, u32)>,
    runtime: crate::Runtime,
    pointer: (f32, f32),
}

/// The secondary windows of one event loop, by window id.
#[derive(Default)]
pub struct AuxiliaryWindows {
    windows: HashMap<WindowId, AuxiliaryWindow>,
}

impl AuxiliaryWindows {
    pub fn contains(&self, window_id: WindowId) -> bool {
        self.windows.contains_key(&window_id)
    }

    /// Opens the detached popup, or closes every secondary window when one is open.
    pub fn toggle_popup(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            self.windows.clear();
            println!("F10: вторичное окно попапа/инспектора закрыто");
            return;
        }
        let popup_source = "component Popup { Frame { width:260; height:110; padding:12; gap:8; Button { key:'popup-action'; width:236; height:38; text:'Нативный попап (F10)'; clicked -> actions.confirm_popup(); } } }";
        let Ok(popup_runtime) = crate::Runtime::from_sources(popup_source, crate::BUTTON_COMPONENT)
        else {
            return;
        };
        let Ok(raw_win) = event_loop.create_window(
            Window::default_attributes()
                .with_title("Forma · Detached Popup / Inspector")
                .with_inner_size(LogicalSize::new(
                    popup_runtime.render_width(),
                    popup_runtime.render_height(),
                )),
        ) else {
            return;
        };
        let win = Arc::new(raw_win);
        let id = win.id();
        let mut gpu = None;
        if std::env::var("FORMA_RENDERER").as_deref() != Ok("cpu") {
            gpu = pollster::block_on(GpuWindow::new(win.clone())).ok();
        }
        let surface = if gpu.is_none() {
            let context = softbuffer::Context::new(win.clone()).unwrap();
            Some(softbuffer::Surface::new(&context, win.clone()).unwrap())
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
                runtime: popup_runtime,
                pointer: (-1.0, -1.0),
            },
        );
        println!("F10: открыто вторичное нативное окно (Multi-Window popup)");
    }

    pub fn window_event(&mut self, window_id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.windows.remove(&window_id);
            return;
        }
        let Some(aux) = self.windows.get_mut(&window_id) else {
            return;
        };
        let rev = aux.runtime.visual_revision();
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f32>(aux.window.scale_factor());
                aux.pointer = (p.x, p.y);
                aux.runtime.pointer(p.x, p.y, 0);
            }
            WindowEvent::CursorLeft { .. } => aux.runtime.pointer(-1.0, -1.0, 3),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                aux.runtime.pointer(
                    aux.pointer.0,
                    aux.pointer.1,
                    if state == ElementState::Pressed { 1 } else { 2 },
                );
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                aux.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                aux.runtime.tick(16.0);
                let size = aux.window.inner_size();
                if let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    if let Some(gpu) = &mut aux.gpu {
                        let _ = gpu.draw(&aux.runtime, &aux.window);
                    } else if let Some(surface) = &mut aux.surface {
                        if aux.surface_size != Some((size.width, size.height)) {
                            let _ = surface.resize(w, h);
                            aux.surface_size = Some((size.width, size.height));
                        }
                        if let Ok(mut buf) = surface.buffer_mut() {
                            let _ = aux.runtime.paint_native(
                                &mut buf,
                                size.width,
                                size.height,
                                aux.window.scale_factor() as f32,
                            );
                            let _ = buf.present();
                        }
                    }
                }
                return;
            }
            _ => {}
        }
        if aux.runtime.visual_revision() != rev || aux.runtime.is_animating() {
            aux.window.request_redraw();
        }
    }
}
