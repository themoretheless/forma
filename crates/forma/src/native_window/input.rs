//! Keyboard, pointer, touch and focus input.

use super::*;

impl<A: NativeApp + 'static> NativeHost<A> {
    pub(super) fn modifiers(&self) -> NativeModifiers {
        NativeModifiers {
            shift: self.shift,
            control: self.control,
            alt: self.alt,
            command: self.command,
        }
    }

    /// Offers a key to the application ahead of the runtime; `true` means handled.
    pub(super) fn offer_key(
        &mut self,
        logical_key: &Key,
        physical_key: &PhysicalKey,
        text: Option<&str>,
        pressed: bool,
        repeat: bool,
    ) -> Result<bool, String> {
        let event = NativeKeyEvent {
            logical_key,
            physical_key,
            text,
            pressed,
            repeat,
            modifiers: self.modifiers(),
            composing: self.ime.composing,
        };
        self.app.on_key_event(&event)
    }

    pub(super) fn pointer_event(&mut self, kind: NativePointerKind) -> Result<(), String> {
        let (x, y) = self.pointer;
        let event = NativePointerEvent {
            kind,
            x,
            y,
            modifiers: self.modifiers(),
        };
        self.app.on_pointer_event(&event)
    }

    /// Primary button on the application (not the chooser): the runtime gets the
    /// press or release first, then a completed double click is reported.
    pub(super) fn primary_button(&mut self, pressed: bool, at: Instant) -> Result<(), String> {
        let (x, y) = self.pointer;
        if self.picks_elements() {
            return self.focus_sensitive_input(&mut |runtime| pick_control(runtime, x, y));
        }
        let kind = if pressed { 1 } else { 2 };
        self.focus_sensitive_input(&mut |runtime| runtime.pointer(x, y, kind))?;
        if pressed && self.clicks.press(at, x, y) {
            self.pointer_event(NativePointerKind::DoubleClick)?;
        }
        Ok(())
    }

    pub(super) fn focus_sensitive_input(
        &mut self,
        input: &mut dyn FnMut(&mut crate::Runtime),
    ) -> Result<(), String> {
        if !self.ime.pending() {
            return self.app.update(input);
        }
        let mut retired = false;
        let mut owner = self.ime.owner;
        let result = self.app.update(&mut |runtime| {
            let previous = runtime.native_editor_session();
            owner = owner.or(previous);
            input(runtime);
            let next = runtime.native_editor_session();
            if previous.is_none() || previous != next {
                runtime.text_preedit("");
                retired = true;
            }
        });
        self.ime.owner = owner;
        if retired {
            self.ime.composing = false;
            self.ime.cancelled = true;
            self.ime.owner = None;
        }
        result
    }

    pub(super) fn touch_input(
        &mut self,
        device: winit::event::DeviceId,
        id: u64,
        phase: TouchPhase,
        x: f32,
        y: f32,
    ) -> Result<(), String> {
        let owner = (device, id);
        if phase == TouchPhase::Started {
            if self.touch_owner.is_some() {
                return Ok(());
            }
            self.touch_owner = Some(owner);
        } else if self.touch_owner != Some(owner) {
            return Ok(());
        }
        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.touch_owner = None;
        }
        self.pointer = (x, y);
        let pick = self.picks_elements();
        match phase {
            TouchPhase::Started => {
                self.touch_last = Some((x, y));
                if let Some(chooser) = &mut self.shell.chooser {
                    chooser.pointer(x, y, 0);
                    chooser.pointer(x, y, 1);
                } else {
                    self.focus_sensitive_input(&mut |runtime| {
                        runtime.pointer(x, y, 0);
                        if pick {
                            pick_control(runtime, x, y);
                        } else {
                            runtime.pointer(x, y, 1);
                        }
                    })?;
                }
            }
            TouchPhase::Moved => {
                let scroll = touch_scroll_step(&mut self.touch_last, x, y);
                if let Some(chooser) = &mut self.shell.chooser {
                    chooser.pointer(x, y, 0);
                    if let Some((dx, dy)) = scroll {
                        chooser.scroll(dx, dy);
                    }
                } else {
                    self.focus_sensitive_input(&mut |runtime| {
                        runtime.pointer(x, y, 0);
                        if let Some((dx, dy)) = scroll {
                            runtime.scroll(dx, dy);
                        }
                    })?;
                }
            }
            TouchPhase::Ended => {
                self.touch_last = None;
                if let Some(chooser) = &mut self.shell.chooser {
                    let pick = chooser_pick(chooser, |chooser| {
                        chooser.pointer(x, y, 2);
                    });
                    if let Some(chosen) = pick {
                        self.select_backend(chosen);
                    }
                } else if !pick {
                    self.focus_sensitive_input(&mut |runtime| runtime.pointer(x, y, 2))?;
                }
            }
            TouchPhase::Cancelled => {
                self.touch_last = None;
                if let Some(chooser) = &mut self.shell.chooser {
                    chooser.pointer(-1., -1., 3);
                } else {
                    self.focus_sensitive_input(&mut |runtime| runtime.pointer(-1., -1., 3))?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn focus_changed(&mut self, focused: bool) -> Result<(), String> {
        if !focused {
            // OS focus loss may omit the matching IME/modifier/touch end events.
            self.ime.cancelled |= self.ime.pending();
            self.ime.composing = false;
            self.shift = false;
            self.command = false;
            self.control = false;
            self.alt = false;
            self.touch_last = None;
            self.touch_owner = None;
        }
        let mut remembered = self.blurred_editor.take();
        if let Some(chooser) = &mut self.shell.chooser {
            chooser.focus(focused);
            if !focused {
                chooser.pointer(-1., -1., 3);
            }
        } else {
            let result = self.app.update(&mut |runtime| {
                if !focused {
                    if runtime.text_editing() {
                        let idx = runtime.focused_index() as usize;
                        let key = runtime.control_key(idx);
                        let origin = runtime.control_origin(idx);
                        let origin = (!origin.is_empty()).then_some(origin);
                        remembered = Some((key, origin));
                    }
                    runtime.focus(false);
                    runtime.pointer(-1., -1., 3);
                } else {
                    // An activation click may select an editor before the OS
                    // Focused(true) event arrives. Its newer choice supersedes
                    // the editor remembered at blur.
                    if runtime.focused_index() >= 0 {
                        return;
                    }
                    // Resolve the stable key first (or origin for unkeyed editors): controls may
                    // reorder or receive inserted siblings while inactive.
                    fn is_unkeyed_synthesized_key(key: &str) -> bool {
                        let leaf = key.rsplit_once('/').map_or(key, |(_, tail)| tail);
                        let Some(rest) = leaf.strip_prefix('@') else {
                            return false;
                        };
                        let Some((id, tail)) = rest.split_once(':') else {
                            return false;
                        };
                        !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && tail == "0:"
                    }
                    let unique_match =
                        |rt: &crate::Runtime, pred: &dyn Fn(usize) -> bool| -> Option<usize> {
                            let mut matches = (0..rt.control_count()).filter(|i| pred(*i));
                            let first = matches.next();
                            if matches.next().is_some() {
                                None
                            } else {
                                first
                            }
                        };
                    let target = remembered.as_ref().and_then(|(key, origin)| {
                        let is_unkeyed = is_unkeyed_synthesized_key(key) && origin.is_some();
                        if !key.is_empty() && !is_unkeyed {
                            unique_match(runtime, &|i| runtime.control_key(i) == *key)
                        } else {
                            origin
                                .as_ref()
                                .and_then(|o| {
                                    unique_match(runtime, &|i| runtime.control_origin(i) == *o)
                                })
                                .or_else(|| {
                                    (!key.is_empty())
                                        .then(|| {
                                            unique_match(runtime, &|i| {
                                                runtime.control_key(i) == *key
                                            })
                                        })
                                        .flatten()
                                })
                        }
                    });
                    let restored = target.is_some_and(|index| {
                        runtime.control_editable(index) && runtime.focus_control(index)
                    });
                    if !restored {
                        runtime.focus(true);
                    }
                }
            });
            if !focused || result.is_err() {
                self.blurred_editor = remembered;
            }
            result?;
        }
        Ok(())
    }

    pub(super) fn keyboard_input(&mut self, event: winit::event::KeyEvent) -> Result<(), String> {
        let pressed = event.state == ElementState::Pressed;
        match host_shortcut(
            self.config.mode,
            &event.logical_key,
            pressed,
            event.repeat,
            cfg!(feature = "native-test-hooks"),
        ) {
            Some(HostShortcut::NextPlatform) => {
                let next = self.shell.platform.next();
                self.select_platform(next);
                return Ok(());
            }
            Some(HostShortcut::ToggleBackend) => {
                let next = self
                    .shell
                    .backend
                    .map_or(UiBackend::Svet, UiBackend::toggle);
                self.select_backend(next);
                return Ok(());
            }
            Some(HostShortcut::InjectDeviceLoss) => {
                #[cfg(feature = "native-test-hooks")]
                if let Some(gpu) = &self.gpu {
                    gpu.inject_device_loss_for_testing();
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                return Ok(());
            }
            None => {}
        }
        if let Some(chooser) = &mut self.shell.chooser {
            if pressed && !event.repeat {
                match &event.logical_key {
                    Key::Character(c) if c.as_str() == "1" || c.eq_ignore_ascii_case("e") => {
                        self.select_backend(UiBackend::Egui);
                        return Ok(());
                    }
                    Key::Character(c) if c.as_str() == "2" || c.eq_ignore_ascii_case("s") => {
                        self.select_backend(UiBackend::Svet);
                        return Ok(());
                    }
                    Key::Named(NamedKey::Tab) => {
                        if !chooser.focus_next(self.shift) {
                            chooser.focus(false);
                            chooser.focus_next(self.shift);
                        }
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let key = match &event.logical_key {
                Key::Named(NamedKey::Space) => 1,
                Key::Named(NamedKey::Enter) => 2,
                Key::Named(NamedKey::Escape) => 3,
                _ => 0,
            };
            let pick = chooser_pick(chooser, |chooser| {
                chooser.key_event(key, pressed, event.repeat);
            });
            if let Some(chosen) = pick {
                self.select_backend(chosen);
            }
            return Ok(());
        }
        if self.offer_key(
            &event.logical_key,
            &event.physical_key,
            event.text.as_deref(),
            pressed,
            event.repeat,
        )? {
            return Ok(());
        }
        let named = match &event.logical_key {
            Key::Named(key) => format!("{key:?}"),
            _ => String::new(),
        };
        if pressed {
            let mut handled = false;
            self.app
                .update(&mut |runtime| handled = runtime.range_key(&named))?;
            if handled {
                return Ok(());
            }
        }

        let mut editing = false;
        self.app
            .update(&mut |runtime| editing = runtime.text_editing())?;
        if editing
            && !matches!(
                &event.logical_key,
                Key::Named(NamedKey::Tab) | Key::Named(NamedKey::Escape)
            )
        {
            if pressed && !self.ime.composing {
                let modifiers = self.modifiers();
                if modifiers.accessibility_chord() {
                    return Ok(());
                }
                let press = TextKey {
                    logical_key: &event.logical_key,
                    physical_key: &event.physical_key,
                    text: event.text.as_deref(),
                    modifiers,
                };
                self.app
                    .update(&mut |runtime| text_edit_key(runtime, &press))?;
            }
            return Ok(());
        }
        self.navigation_key(&event.logical_key, pressed, event.repeat)
    }

    pub(super) fn navigation_key(
        &mut self,
        logical_key: &Key,
        pressed: bool,
        repeat: bool,
    ) -> Result<(), String> {
        let cancel_composition = self.ime.pending()
            && pressed
            && !repeat
            && matches!(logical_key, Key::Named(NamedKey::Tab | NamedKey::Escape));
        if cancel_composition {
            self.ime.composing = false;
            self.ime.cancelled = true;
        }
        let key = match logical_key {
            Key::Named(NamedKey::ArrowUp) => 4,
            Key::Named(NamedKey::ArrowDown) => 5,
            other => activation_key(other),
        };
        let shift = self.shift;
        self.app.update(&mut |runtime| {
            if cancel_composition {
                runtime.text_preedit("");
            }
            if *logical_key == Key::Named(NamedKey::Tab) && pressed && !repeat {
                focus_traversal(runtime, shift);
            }
            runtime.key_event(key, pressed, repeat);
        })?;
        Ok(())
    }
}
