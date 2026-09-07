//! Small native host demonstrating generated bindings. F2 replaces the model.
use super::{CustomerForm, CustomerVm};
use std::{num::NonZeroU32, rc::Rc, sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, Ime, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

struct App {
    form: CustomerForm,
    customers: [Rc<CustomerVm>; 2],
    selected: usize,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    cursor: (f32, f32),
    shift: bool,
    command: bool,
    composing: bool,
    last_frame: Instant,
}
impl App {
    fn draw(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32() * 1000.;
        self.last_frame = now;
        self.form.update(|runtime| runtime.tick(dt.min(100.)))?;
        let window = self.window.as_ref().unwrap();
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(());
        };
        let surface = self.surface.as_mut().unwrap();
        surface.resize(width, height).map_err(|e| e.to_string())?;
        let mut buffer = surface.buffer_mut().map_err(|e| e.to_string())?;
        let runtime = self.form.runtime()?;
        runtime.paint_native(
            &mut buffer,
            size.width,
            size.height,
            window.scale_factor() as f32,
        )?;
        buffer.present().map_err(|e| e.to_string())?;
        if runtime.is_animating() {
            window.request_redraw();
        }
        Ok(())
    }
    fn event(&mut self, event: WindowEvent, event_loop: &ActiveEventLoop) -> Result<(), String> {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return Ok(());
            }
            WindowEvent::RedrawRequested => return self.draw(),
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.window.as_ref().unwrap().scale_factor() as f32;
                self.cursor = (position.x as f32 / scale, position.y as f32 / scale);
                let (x, y) = self.cursor;
                self.form.update(|r| r.pointer(x, y, 0))?;
            }
            WindowEvent::CursorLeft { .. } => {
                self.form.update(|r| r.pointer(-1., -1., 3))?;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = self.cursor;
                self.form.update(|r| {
                    r.pointer(x, y, if state == ElementState::Pressed { 1 } else { 2 })
                })?;
            }
            WindowEvent::Focused(false) => {
                self.form.update(|r| r.focus(false))?;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.shift = modifiers.state().shift_key();
                self.command = if cfg!(target_os = "macos") {
                    modifiers.state().super_key()
                } else {
                    modifiers.state().control_key()
                };
            }
            WindowEvent::Ime(Ime::Preedit(text, _)) => {
                self.composing = !text.is_empty();
                self.form.update(|r| r.text_preedit(&text))?;
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.composing = false;
                self.form.update(|r| r.text_insert(&text))?;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                match event.logical_key {
                    Key::Named(NamedKey::F2) if pressed && !event.repeat => {
                        self.selected = 1 - self.selected;
                        self.form
                            .set_context(self.customers[self.selected].clone())?;
                    }
                    Key::Named(NamedKey::Tab) if pressed => {
                        let shift = self.shift;
                        self.form.update(|r| r.focus_next(shift))?;
                    }
                    Key::Named(name) => {
                        let key = match name {
                            NamedKey::Backspace => "Backspace",
                            NamedKey::Delete => "Delete",
                            NamedKey::ArrowLeft => "ArrowLeft",
                            NamedKey::ArrowRight => "ArrowRight",
                            NamedKey::ArrowUp => "ArrowUp",
                            NamedKey::ArrowDown => "ArrowDown",
                            NamedKey::Home => "Home",
                            NamedKey::End => "End",
                            NamedKey::Enter => "Enter",
                            _ => "",
                        };
                        let shift = self.shift;
                        let command = self.command;
                        self.form.update(|r| {
                            if pressed && !key.is_empty() && r.text_editing() {
                                r.text_key(key, shift, command);
                            } else if name == NamedKey::Enter {
                                r.key_event(13, pressed, event.repeat);
                            } else if name == NamedKey::Space {
                                if r.text_editing() {
                                    if pressed {
                                        r.text_insert(" ");
                                    }
                                } else {
                                    r.key_event(32, pressed, event.repeat);
                                }
                            }
                        })?;
                    }
                    Key::Character(key) if pressed && !self.composing => {
                        let command = self.command;
                        let shift = self.shift;
                        self.form.update(|r| {
                            if command {
                                r.text_key(&key, shift, true);
                            } else if let Some(text) = event.text {
                                r.text_insert(&text);
                            }
                        })?;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        if let Some(window) = &self.window {
            window.set_ime_allowed(self.form.runtime()?.text_editing());
            window.request_redraw();
        }
        Ok(())
    }
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let setup = || -> Result<_, String> {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title("Forma · F2 — сменить клиента")
                            .with_inner_size(LogicalSize::new(360., 290.))
                            .with_resizable(false),
                    )
                    .map_err(|e| e.to_string())?,
            );
            let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
            let surface =
                softbuffer::Surface::new(&context, window.clone()).map_err(|e| e.to_string())?;
            Ok((window, surface))
        };
        match setup() {
            Ok((window, surface)) => {
                let redraw = window.clone();
                self.form.on_dirty(move || redraw.request_redraw());
                window.request_redraw();
                self.window = Some(window);
                self.surface = Some(surface);
            }
            Err(error) => {
                eprintln!("{error}");
                event_loop.exit();
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let Err(error) = self.event(event, event_loop) {
            eprintln!("{error}");
            event_loop.exit();
        }
    }
}
pub fn run() -> Result<(), String> {
    let customers = [
        CustomerVm::new("Анна", "Тбилиси"),
        CustomerVm::new("Мария", "Ереван"),
    ];
    let form = CustomerForm::new(customers[0].clone())?;
    form.on_save_requested(|event| event.context.save());
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop
        .run_app(&mut App {
            form,
            customers,
            selected: 0,
            window: None,
            surface: None,
            cursor: (0., 0.),
            shift: false,
            command: false,
            composing: false,
            last_frame: Instant::now(),
        })
        .map_err(|e| e.to_string())
}
