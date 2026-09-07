use forma::frame_stats::{FrameRate, FrameStats};
use std::{
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};
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

const TITLE_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Presentation {
    Presented,
    ZeroSize,
    Occluded,
    Retry,
}

struct App {
    inspect_mode: bool,
    inspect_report: String,
    shift: bool,
    command: bool,
    composing: bool,
    ime_allowed: bool,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    gpu: Option<GpuWindow>,
    surface_size: Option<(u32, u32)>,
    render_error: Option<String>,
    surface_state: Presentation,
    button: forma::Runtime,
    pointer: (f32, f32),
    last: Instant,
    frame_stats: FrameStats,
    continuous_redraw: bool,
    log_fps: bool,
    next_title_update: Instant,
    last_title: String,
    retry_at: Option<Instant>,
}

impl App {
    fn set_render_error(&mut self, error: Option<String>) {
        if self.render_error != error {
            if let Some(error) = &error {
                eprintln!("Ошибка отрисовки: {error}");
            }
            self.render_error = error;
            self.next_title_update = Instant::now();
        }
    }

    fn present_result(&mut self, result: Presentation, window: &Window) {
        let now = Instant::now();
        if self.surface_state != result {
            self.next_title_update = now;
        }
        self.surface_state = result;
        self.retry_at = None;
        match result {
            Presentation::Presented => {
                self.frame_stats.presented(now);
                if self.button.is_animating() || self.continuous_redraw {
                    window.request_redraw();
                }
            }
            Presentation::Retry => self.retry_at = Some(now + Duration::from_millis(16)),
            Presentation::Occluded if self.continuous_redraw => {
                self.retry_at = Some(now + Duration::from_millis(100));
            }
            _ => {}
        }
    }

    /// Shared by normal redraw and the synchronous AppKit live-resize path.
    fn draw(&mut self, window: &Arc<Window>) {
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            self.present_result(Presentation::ZeroSize, window);
            return;
        };
        if let Some(gpu) = &mut self.gpu {
            match gpu.draw(&self.button, window) {
                Ok(result) => {
                    self.set_render_error(None);
                    self.present_result(result, window);
                    return;
                }
                Err(error) => {
                    eprintln!("GPU остановлен, CPU fallback: {error}");
                    self.gpu = None;
                    self.frame_stats = FrameStats::default(); // Do not mix backend cadences.
                    self.next_title_update = Instant::now();
                    let context = softbuffer::Context::new(window.clone()).unwrap();
                    self.surface =
                        Some(softbuffer::Surface::new(&context, window.clone()).unwrap());
                    self.surface_size = None;
                }
            }
        }
        let surface = self.surface.as_mut().unwrap();
        if self.surface_size != Some((size.width, size.height)) {
            if let Err(error) = surface.resize(width, height) {
                self.set_render_error(Some(error.to_string()));
                self.present_result(Presentation::Retry, window);
                return;
            }
            self.surface_size = Some((size.width, size.height));
        }
        let mut buffer = match surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(error) => {
                self.set_render_error(Some(error.to_string()));
                self.present_result(Presentation::Retry, window);
                return;
            }
        };
        let error = self
            .button
            .paint_native(
                &mut buffer,
                size.width,
                size.height,
                window.scale_factor() as f32,
            )
            .err()
            .map(str::to_owned);
        if error.is_some() {
            buffer.fill(0x111319);
        }
        // As on GPU, count only successful presentation, not a render attempt.
        let presented = buffer.present();
        self.set_render_error(error);
        match presented {
            Ok(()) => self.present_result(Presentation::Presented, window),
            Err(error) => {
                self.set_render_error(Some(error.to_string()));
                self.present_result(Presentation::Retry, window);
            }
        }
    }

    fn update_title(&mut self, now: Instant) {
        let Some(window) = &self.window else {
            return;
        };
        let backend = if self.gpu.is_some() { "GPU" } else { "CPU" };
        let rate = if let Some(error) = &self.render_error {
            format!("ошибка: {error}")
        } else if self.surface_state == Presentation::Occluded {
            "occluded".into()
        } else if self.surface_state == Presentation::ZeroSize {
            "нет поверхности".into()
        } else if self.surface_state == Presentation::Retry {
            "ожидание поверхности".into()
        } else {
            match self.frame_stats.snapshot(now) {
                FrameRate::Waiting => "ожидание кадра".into(),
                FrameRate::Sampling => "замер…".into(),
                FrameRate::Idle => "idle".into(),
                FrameRate::Active { fps, frame_ms } => format!("{fps:.1} FPS · {frame_ms:.2} ms"),
            }
        };
        let mode = if self.continuous_redraw {
            "FPS-тест"
        } else {
            "F8: тест"
        };
        let mut title = format!("Forma · {backend} · {rate} · {mode}");
        if self.button.clicks() > 0 {
            title.push_str(&format!(" · кликов: {}", self.button.clicks()));
        }
        if title != self.last_title {
            window.set_title(&title);
            if self.log_fps {
                println!("Frame stats: {title}");
            }
            self.last_title = title;
        }
        self.next_title_update = now + TITLE_INTERVAL;
    }
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
                        .with_title("Forma · ожидание кадра · F8: FPS-тест")
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
        window.focus_window();
        window.request_redraw();
        self.window = Some(window);
        println!(
            "F8: непрерывный FPS-тест; обычный режим перерисовывает окно только при изменениях"
        );
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };
        let clicks = self.button.clicks();
        let revision = self.button.visual_revision();
        let was_animating = self.button.is_animating();
        let redraw = matches!(event, WindowEvent::RedrawRequested);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f32>(window.scale_factor());
                self.pointer = (p.x, p.y);
                self.button.pointer(p.x, p.y, 0);
                let index=self.button.hit_index(p.x,p.y);
                window.set_cursor(if index>=0 && self.button.control_editable(index as usize) {winit::window::CursorIcon::Text}else{winit::window::CursorIcon::Default});
            }
            WindowEvent::CursorLeft { .. } => self.button.pointer(-1., -1., 3),
            WindowEvent::MouseInput {button:MouseButton::Left,..} if self.inspect_mode => {
                let index=self.button.hit_index(self.pointer.0,self.pointer.1);
                if index>=0 {self.button.focus_control(index as usize);}
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.button.pointer(
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
                self.button.scroll(dx, dy);
            }
            WindowEvent::ModifiersChanged(modifiers) => {self.shift = modifiers.state().shift_key();self.command=if cfg!(target_os="macos"){modifiers.state().super_key()}else{modifiers.state().control_key()};},
            WindowEvent::Ime(winit::event::Ime::Preedit(text,_))=>{self.composing=!text.is_empty();self.button.text_preedit(&text);},
            WindowEvent::Ime(winit::event::Ime::Commit(text))=>{self.composing=false;self.button.text_insert(&text);},
            WindowEvent::Ime(winit::event::Ime::Disabled)=>{self.composing=false;self.button.text_preedit("");},
            WindowEvent::KeyboardInput { event, .. } => {
                if event.logical_key==Key::Named(NamedKey::F9) && event.state==ElementState::Pressed && !event.repeat {
                    self.inspect_mode=!self.inspect_mode;
                    self.button.focus(false);
                    self.inspect_report.clear();
                    println!("F9: выбор элемента {}",if self.inspect_mode {"включён"}else{"выключен"});
                } else if event.logical_key == Key::Named(NamedKey::F8)
                    && event.state == ElementState::Pressed
                    && !event.repeat
                {
                    self.continuous_redraw = !self.continuous_redraw;
                    self.frame_stats = FrameStats::default();
                    self.next_title_update = Instant::now();
                    self.retry_at = None;
                    window.request_redraw();
                } else if event.state==ElementState::Pressed && self.button.range_key(&match &event.logical_key {Key::Named(key)=>format!("{key:?}"),_=>String::new()}) {
                    // Range navigation belongs to the same runtime as pointer input.
                } else if self.button.text_editing() && event.logical_key != Key::Named(NamedKey::Tab) {
                    if event.state==ElementState::Pressed && !self.composing {
                        let key=match &event.logical_key {
                            Key::Character(text)=>text.to_string(),
                            Key::Named(name)=>format!("{name:?}"),_=>String::new(),
                        };
                        let lower=key.to_lowercase();
                        if self.command && ["c","x","v"].contains(&lower.as_str()) {
                            if lower=="v" {if let Ok(text)=clipboard_read(){self.button.text_insert(&text);}}
                            else {let text=self.button.text_selected();if !text.is_empty() && clipboard_write(&text).is_ok() && lower=="x" {self.button.text_insert("");}}
                        } else if !self.button.text_key(&key,self.shift,self.command) && !self.command {
                            if let Some(text)=event.text {if !text.chars().any(char::is_control){self.button.text_insert(&text);}}
                        }
                    }
                } else {
                    let key = match event.logical_key {
                        Key::Named(NamedKey::Space) => 1,
                        Key::Named(NamedKey::Enter) => 2,
                        _ => 0,
                    };
                    if event.logical_key == Key::Named(NamedKey::Tab)
                        && event.state == ElementState::Pressed
                        && !event.repeat
                    {
                        if !self.button.focus_next(self.shift) {
                            self.button.focus(false);
                            self.button.focus_next(self.shift);
                        }
                    }
                    self.button
                        .key_event(key, event.state == ElementState::Pressed, event.repeat);
                }
            }
            WindowEvent::Focused(focused) => {
                self.button.focus(focused);
                if !focused {
                    self.button.pointer(-1., -1., 3);
                }
            }
            WindowEvent::Occluded(false) => window.request_redraw(),
            WindowEvent::Occluded(true) => {
                self.surface_state = Presentation::Occluded;
                self.next_title_update = Instant::now();
            }
            WindowEvent::Resized(_) => {
                // Keep synchronous transaction-aware Metal presentation on live resize.
                #[cfg(target_os = "macos")]
                if self.gpu.is_some() {
                    self.draw(&window);
                    return;
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => window.request_redraw(),
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                self.button.tick((now - self.last).as_secs_f32() * 1000.);
                self.last = now;
                self.draw(&window);
            }
            _ => {}
        }
        if self.inspect_mode {
            let nodes=self.button.nodes().iter().map(|n|format!("{{\"id\":{},\"parent\":{},\"control\":{},\"kind\":\"{:?}\"}}",n.id,n.parent.map_or("null".into(),|v|v.to_string()),n.control.map_or("null".into(),|v|v.to_string()),n.kind)).collect::<Vec<_>>().join(",");
            let controls=(0..self.button.control_count()).map(|i|format!("{{\"index\":{},\"bounds\":{:?},\"disabled\":{}}}",i,self.button.control_bounds(i),self.button.control_disabled(i))).collect::<Vec<_>>().join(",");
            let report=format!("{{\"selected\":{},\"nodes\":[{}],\"controls\":[{}]}}",self.button.focused_index(),nodes,controls);
            if report!=self.inspect_report {println!("FORMA_INSPECT {}",report);self.inspect_report=report;}
        }
        let editing=self.button.text_editing();
        if editing!=self.ime_allowed {window.set_ime_allowed(editing);self.ime_allowed=editing;}
        let caret=self.button.text_caret_bounds();
        if caret.len()==4 {window.set_ime_cursor_area(winit::dpi::LogicalPosition::new(caret[0] as f64,caret[1] as f64),LogicalSize::new(caret[2] as f64,caret[3] as f64));}
        if !redraw {
            let started = !was_animating && self.button.is_animating();
            if started {
                self.last = Instant::now();
            }
            if started || self.button.visual_revision() != revision {
                window.request_redraw();
            }
        }
        if self.button.clicks() != clicks {
            println!(
                "{}.clicked → {} (событие; обработчик приложения пока не подключён)",
                self.button.key(),
                self.button.action()
            );
            self.next_title_update = Instant::now();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.retry_at.is_some_and(|time| time <= now) {
            self.retry_at = None;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        let idle_deadline = self.frame_stats.idle_deadline();
        if now >= self.next_title_update || idle_deadline.is_some_and(|deadline| deadline <= now) {
            self.update_title(now);
        }
        // Updating the title never causes a redraw. Stop all meter wakeups at idle.
        let meter_deadline = idle_deadline
            .filter(|deadline| *deadline > now)
            .map(|deadline| deadline.min(self.next_title_update));
        let deadline = self.retry_at.into_iter().chain(meter_deadline).min();
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let source = args
        .get(1)
        .map(std::fs::read_to_string)
        .transpose()?
        .unwrap_or(forma::EXAMPLE.into());
    let template = args
        .get(2)
        .map(std::fs::read_to_string)
        .transpose()?
        .unwrap_or(forma::BUTTON_COMPONENT.into());
    let button =
        forma::Runtime::from_sources(&source, &template).map_err(std::io::Error::other)?;
    EventLoop::new()?.run_app(&mut App {
        inspect_mode:false,
        inspect_report:String::new(),
        shift: false,
        command: false,
        composing: false,
        ime_allowed: false,
        window: None,
        surface: None,
        gpu: None,
        surface_size: None,
        render_error: None,
        surface_state: Presentation::Presented,
        button,
        pointer: (-1., -1.),
        last: Instant::now(),
        frame_stats: FrameStats::default(),
        continuous_redraw: std::env::var("FORMA_FPS_TEST").as_deref() == Ok("1"),
        log_fps: std::env::var("FORMA_FPS_LOG").as_deref() == Ok("1"),
        next_title_update: Instant::now(),
        last_title: String::new(),
        retry_at: None,
    })?;
    Ok(())
}

struct GpuWindow {
    surface: wgpu::Surface<'static>,
    renderer: forma::gpu::Renderer,
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
        let renderer = forma::gpu::Renderer::new(&adapter, config.format).await?;
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
    fn draw(
        &mut self,
        button: &forma::Runtime,
        window: &Window,
    ) -> Result<Presentation, String> {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(Presentation::ZeroSize);
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
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(Presentation::Occluded),
            wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(Presentation::Retry);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.renderer.device, &self.config);
                return Ok(Presentation::Retry);
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
        Ok(Presentation::Presented)
    }
}

#[cfg(target_os="macos")]
fn clipboard_read()->Result<String,std::io::Error> {
    let output=std::process::Command::new("/usr/bin/pbpaste").output()?;
    if !output.status.success(){return Err(std::io::Error::other("Clipboard read failed"));}
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
#[cfg(target_os="macos")]
fn clipboard_write(text:&str)->Result<(),std::io::Error> {
    use std::io::Write;
    let mut child=std::process::Command::new("/usr/bin/pbcopy").stdin(std::process::Stdio::piped()).spawn()?;
    child.stdin.take().unwrap().write_all(text.as_bytes())?;
    if child.wait()?.success(){Ok(())}else{Err(std::io::Error::other("Clipboard write failed"))}
}
#[cfg(not(target_os="macos"))]
fn clipboard_read()->Result<String,std::io::Error> {Err(std::io::Error::other("Native clipboard adapter unavailable"))}
#[cfg(not(target_os="macos"))]
fn clipboard_write(_: &str)->Result<(),std::io::Error> {Err(std::io::Error::other("Native clipboard adapter unavailable"))}
