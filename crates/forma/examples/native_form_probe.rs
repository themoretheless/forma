//! Instrumented OS-window qualification with synthetic 8/32/96/256-editor forms.
//! Journaling/getrusage add overhead; present callbacks are not GPU completion.
#[cfg(not(feature = "native"))]
fn main() {
    panic!("Run with --features native");
}

#[cfg(feature = "native")]
mod probe {
    use forma::{
        Runtime,
        form_document::{Properties, Value},
        native_window::{NativeApp, NativeRenderer, NativeWindowConfig, TargetPlatform, UiBackend},
    };
    use std::{cell::Cell, fs::File, io::Write, path::PathBuf, time::Instant};
    pub fn runtime(count: usize) -> Result<Runtime, String> {
        if ![8, 32, 96, 256].contains(&count) {
            return Err("Expected 8,32,96 or256 controls".into());
        }
        let rows = Value::List(
            (0..count)
                .map(|i| {
                    Value::Object(Properties::from([
                        ("id".into(), Value::Text(format!("row-{i}"))),
                        (
                            "title".into(),
                            Value::Text(format!("row-{i:03} — 你好 мир🙂")),
                        ),
                    ]))
                })
                .collect(),
        );
        let files = std::collections::BTreeMap::from([(
            "App.ui".into(),
            include_str!("../tests/fixtures/svet-native-large/App.ui").into(),
        )]);
        let mut runtime =
            Runtime::from_ui_files(&files, "App.ui", &Properties::from([("rows".into(), rows)]))?;
        runtime.set_svet_document_scheduling(true);
        if runtime.control_count() != count {
            return Err(format!(
                "Expected {count} editors, got {}",
                runtime.control_count()
            ));
        }
        Ok(runtime)
    }
    fn peak_rss_bytes() -> Option<u64> {
        #[cfg(unix)]
        {
            let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
            if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
                return None;
            }
            let value = unsafe { usage.assume_init() }.ru_maxrss as u64;
            #[cfg(target_os = "macos")]
            {
                Some(value)
            }
            #[cfg(not(target_os = "macos"))]
            {
                Some(value * 1024)
            }
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
    struct Probe {
        runtime: Runtime,
        trace: File,
        start: Instant,
        last: Option<(i32, Vec<f32>)>,
        previous_present_us: Cell<Option<u64>>,
        last_update_us: Cell<u64>,
        no_focus: bool,
    }
    impl Probe {
        fn log(&self, value: serde_json::Value) -> Result<(), String> {
            let mut file = &self.trace;
            writeln!(file, "{value}").map_err(|e| e.to_string())
        }
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            let _ = self.log(
                serde_json::json!({"event":"stop","at_us":self.start.elapsed().as_micros() as u64}),
            );
        }
    }
    impl NativeApp for Probe {
        fn inspect(&self, observer: &mut dyn FnMut(&Runtime)) -> Result<(), String> {
            observer(&self.runtime);
            Ok(())
        }
        fn update(&mut self, input: &mut dyn FnMut(&mut Runtime)) -> Result<(), String> {
            let begin = Instant::now();
            input(&mut self.runtime);
            if self.no_focus {
                self.runtime.focus(false);
            }
            let changed = self.runtime.sync_ui_bindings_native()?;
            self.last_update_us.set(begin.elapsed().as_micros() as u64);
            let current = (self.runtime.focused_index(), self.runtime.scroll_offset());
            if changed || self.last.as_ref() != Some(&current) {
                self.log(serde_json::json!({"event":"model","at_us":self.start.elapsed().as_micros() as u64,
                    "focused_index":current.0,"scroll":current.1,"sync_changed":changed,
                    "update_us":self.last_update_us.get(),"state":serde_json::from_str::<serde_json::Value>(&self.runtime.ui_state_json()).map_err(|e|e.to_string())?}))?;
                self.last = Some(current);
            }
            Ok(())
        }
        fn on_frame_presented(&self, renderer: NativeRenderer) -> Result<(), String> {
            let now = self.start.elapsed().as_micros() as u64;
            let previous = self.previous_present_us.replace(Some(now));
            self.log(serde_json::json!({"event":"present","at_us":now,"interval_us":previous.map(|p|now-p),
                "renderer":format!("{renderer:?}"),"focused_index":self.runtime.focused_index(),
                "scroll":self.runtime.scroll_offset(),"animating":self.runtime.is_animating(),
                "last_update_us":self.last_update_us.get(),"raster_stats":self.runtime.raster_stats(),
                "peak_rss_bytes":peak_rss_bytes()}))
        }
        fn on_gpu_health_checked(&self, idle: bool) -> Result<(), String> {
            self.log(serde_json::json!({"event":"gpu_health","idle":idle,"at_us":self.start.elapsed().as_micros() as u64}))
        }
        fn on_gpu_fallback(&self, error: &str) -> Result<(), String> {
            self.log(serde_json::json!({"event":"gpu_error","error":error}))
        }
        fn on_occlusion_changed(&self, occluded: bool) -> Result<(), String> {
            self.log(serde_json::json!({"event":"occlusion","occluded":occluded,"at_us":self.start.elapsed().as_micros() as u64}))
        }
    }
    pub fn run() -> Result<(), String> {
        let count = std::env::var("FORMA_NATIVE_FORM_CONTROLS")
            .map_err(|e| e.to_string())?
            .parse::<usize>()
            .map_err(|e| e.to_string())?;
        let trace = PathBuf::from(
            std::env::var_os("FORMA_NATIVE_FORM_LOG").ok_or("Missing FORMA_NATIVE_FORM_LOG")?,
        );
        let no_focus = std::env::var("FORMA_NATIVE_FORM_NO_FOCUS").as_deref() == Ok("1");
        let start = Instant::now();
        let mut runtime = runtime(count)?;
        if no_focus {
            runtime.focus(false);
        }
        let load_us = start.elapsed().as_micros() as u64;
        let probe = Probe {
            runtime,
            trace: File::create(trace).map_err(|e| e.to_string())?,
            start,
            last: None,
            previous_present_us: Cell::new(None),
            last_update_us: Cell::new(0),
            no_focus,
        };
        probe.log(serde_json::json!({"event":"start","controls":count,"load_us":load_us,"pid":std::process::id(),"peak_rss_bytes":peak_rss_bytes(),"scope":"instrumented actual NativeHost; presentation callback is not GPU completion or OS input latency"}))?;
        forma::native_window::run(
            probe,
            NativeWindowConfig {
                title: format!("Svet large form — {count} editors"),
                width: 640.,
                height: 420.,
                backend: Some(UiBackend::Svet),
                platform: TargetPlatform::Desktop,
                ..Default::default()
            },
        )
    }
    #[cfg(test)]
    mod tests {
        #[test]
        fn fixture_keeps_all_rows_and_binding_writes_through_after_scroll() {
            for count in [8, 32, 96, 256] {
                let mut runtime = super::runtime(count).unwrap();
                assert_eq!(runtime.control_count(), count);
                runtime.scroll(0., 100000.);
                runtime
                    .set_binding_value(count - 1, "last — 你好 мир🙂", true)
                    .unwrap();
                assert!(runtime.sync_ui_bindings_native().unwrap());
                let state: serde_json::Value =
                    serde_json::from_str(&runtime.ui_state_json()).unwrap();
                assert_eq!(state["rows"].as_array().unwrap().len(), count);
                assert_eq!(state["rows"][count - 1]["title"], "last — 你好 мир🙂");
                assert_eq!(state["rows"][0]["title"], "row-000 — 你好 мир🙂");
                let mut pixels = vec![0; 1280 * 840];
                runtime.paint_native(&mut pixels, 1280, 840, 2.).unwrap();
                assert!(pixels.iter().any(|p| *p != pixels[0]));
            }
        }
    }
}
#[cfg(feature = "native")]
fn main() -> Result<(), String> {
    probe::run()
}
