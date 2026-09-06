use std::{num::NonZeroU32, sync::Arc, time::Instant};
#[cfg(target_os = "macos")]
mod metal_resize;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

struct App {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    gpu: Option<GpuWindow>,
    surface_size: Option<(u32, u32)>,
    render_error: Option<&'static str>,
    button: forma_vector::Button,
    pointer: (f32, f32),
    last: Instant,
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        event_loop.set_control_flow(ControlFlow::Wait);
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Forma — векторный компонент")
                        .with_inner_size(LogicalSize::new(
                            self.button.render_width(),
                            self.button.render_height(),
                        )),
                )
                .unwrap(),
        );
        if std::env::var("FORMA_RENDERER").as_deref() != Ok("cpu") {
            match pollster::block_on(GpuWindow::new(window.clone())) {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(error) => eprintln!("GPU недоступен, CPU fallback: {error}"),
            }
        }
        if self.gpu.is_none() {
            let context = softbuffer::Context::new(window.clone()).unwrap();
            self.surface = Some(softbuffer::Surface::new(&context, window.clone()).unwrap());
            println!("Renderer: CPU fallback");
        }
        window.request_redraw();
        self.window = Some(window);
        println!("Векторное окно: кэш геометрии/контента; перерисовка только при изменениях");
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let window = self.window.as_ref().unwrap();
        let button = &mut self.button;
        let clicks = button.clicks();
        let revision = button.visual_revision();
        let was_animating = button.is_animating();
        let redraw = matches!(event, WindowEvent::RedrawRequested);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f32>(window.scale_factor());
                self.pointer = (p.x, p.y);
                button.pointer(p.x, p.y, 0);
            }
            WindowEvent::CursorLeft { .. } => button.pointer(-1., -1., 3),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                button.pointer(
                    self.pointer.0,
                    self.pointer.1,
                    if state == ElementState::Pressed { 1 } else { 2 },
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * 32., -y * 32.),
                    MouseScrollDelta::PixelDelta(p) => (
                        -p.x as f32 / window.scale_factor() as f32,
                        -p.y as f32 / window.scale_factor() as f32,
                    ),
                };
                button.scroll(dx, dy);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && matches!(
                        event.logical_key,
                        Key::Named(NamedKey::Enter | NamedKey::Space)
                    )
                {
                    button.activate();
                }
            }
            WindowEvent::Focused(focused) => {
                button.focus(focused);
                if !focused {
                    button.pointer(-1., -1., 3);
                }
            }
            WindowEvent::Resized(_) => {
                // AppKit live resize must present before its resize transaction
                // ends, not one event-loop iteration later in RedrawRequested.
                #[cfg(target_os = "macos")]
                if let Some(gpu) = &mut self.gpu {
                    if gpu.draw(button, window).is_ok() {
                        if button.is_animating() {
                            window.request_redraw();
                        }
                        return;
                    }
                }
                window.request_redraw()
            }
            WindowEvent::ScaleFactorChanged { .. } => window.request_redraw(),
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let animating = button.tick((now - self.last).as_secs_f32() * 1000.);
                self.last = now;
                let size = window.inner_size();
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    if let Some(gpu) = &mut self.gpu {
                        match gpu.draw(button, window) {
                            Ok(()) => {
                                if animating {
                                    window.request_redraw();
                                }
                                return;
                            }
                            Err(error) => {
                                eprintln!("GPU остановлен, CPU fallback: {error}");
                                self.gpu = None;
                                let context = softbuffer::Context::new(window.clone()).unwrap();
                                self.surface = Some(
                                    softbuffer::Surface::new(&context, window.clone()).unwrap(),
                                );
                                self.surface_size = None;
                            }
                        }
                    }
                    let surface = self.surface.as_mut().unwrap();
                    if self.surface_size != Some((size.width, size.height)) {
                        surface.resize(width, height).unwrap();
                        self.surface_size = Some((size.width, size.height));
                    }
                    let mut buffer = surface.buffer_mut().unwrap();
                    let render_error = button
                        .paint_native(
                            &mut buffer,
                            size.width,
                            size.height,
                            window.scale_factor() as f32,
                        )
                        .err();
                    if render_error != self.render_error {
                        if let Some(error) = render_error {
                            eprintln!("Ошибка отрисовки: {error}");
                            window.set_title(&format!("Forma — {error}"));
                        } else {
                            window.set_title("Forma — векторный компонент");
                        }
                        self.render_error = render_error;
                    }
                    if render_error.is_some() {
                        // Keep the window alive: shrinking or changing DPI can
                        // bring the scene back within the CPU raster budget.
                        buffer.fill(0x111319);
                    }
                    buffer.present().unwrap();
                }
                if animating {
                    window.request_redraw();
                }
            }
            _ => {}
        }
        if !redraw {
            // Do not reset the animation clock on every mouse-move event.
            let started = !was_animating && button.is_animating();
            if started {
                self.last = Instant::now();
            }
            if started || button.visual_revision() != revision {
                window.request_redraw();
            }
        }
        if button.clicks() != clicks {
            println!(
                "{}.clicked → {} (событие; обработчик приложения пока не подключён)",
                button.key(),
                button.action()
            );
            window.set_title(&format!(
                "Forma — {} · {} нажатий",
                button.label(),
                button.clicks()
            ));
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let source = if let Some(path) = args.get(1) {
        std::fs::read_to_string(path)?
    } else {
        forma_vector::EXAMPLE.into()
    };
    let template = if let Some(path) = args.get(2) {
        std::fs::read_to_string(path)?
    } else {
        forma_vector::BUTTON_COMPONENT.into()
    };
    let button =
        forma_vector::Button::from_sources(&source, &template).map_err(std::io::Error::other)?;
    EventLoop::new()?.run_app(&mut App {
        window: None,
        surface: None,
        gpu: None,
        surface_size: None,
        render_error: None,
        button,
        pointer: (-1., -1.),
        last: Instant::now(),
    })?;
    Ok(())
}

struct GpuWindow {
    surface: wgpu::Surface<'static>,
    renderer: forma_vector::gpu::Renderer,
    config: wgpu::SurfaceConfiguration,
}
impl GpuWindow {
    async fn new(window: Arc<Window>) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(window.clone()),
        ));
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("No GPU surface configuration")?;
        // Prefer encoded-color target, matching CPU compositing exactly.
        if let Some(format) = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(|f| !f.is_srgb())
        {
            config.format = format;
        }
        config.present_mode = wgpu::PresentMode::Fifo;
        let renderer = forma_vector::gpu::Renderer::new(&adapter, config.format).await?;
        surface.configure(&renderer.device, &config);
        #[cfg(target_os = "macos")]
        metal_resize::configure(&window)?;
        println!(
            "Renderer: GPU vector / {:?} / {}",
            adapter.get_info().backend,
            adapter.get_info().name
        );
        Ok(Self {
            surface,
            renderer,
            config,
        })
    }
    fn draw(&mut self, button: &forma_vector::Button, window: &Window) -> Result<(), String> {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        if self.config.width != size.width || self.config.height != size.height {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.renderer.device, &self.config);
        }
        let mut suboptimal = false;
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                suboptimal = true;
                frame
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
            wgpu::CurrentSurfaceTexture::Timeout => {
                window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.renderer.device, &self.config);
                window.request_redraw();
                return Ok(());
            }
            e => return Err(format!("GPU surface: {e:?}")),
        };
        // AppKit can resize while the next drawable is being acquired. Always
        // address the actual target, not an earlier Window::inner_size snapshot.
        let target_size = frame.texture.size();
        self.renderer.draw(
            button,
            &frame.texture.create_view(&Default::default()),
            target_size.width,
            target_size.height,
            window.scale_factor() as f32,
            true,
            true,
        )?;
        window.pre_present_notify();
        frame.present();
        if suboptimal {
            self.config.width = 0;
            window.request_redraw();
        }
        Ok(())
    }
}
