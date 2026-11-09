//! Instrumented native readonly input check; journal is synthetic fixture data only.
#[cfg(not(feature = "native"))]
fn main() {
    panic!("Use --features native");
}
#[cfg(feature = "native")]
mod probe {
    use forma::{
        Runtime,
        native_window::{NativeApp, NativeRenderer, NativeWindowConfig, TargetPlatform, UiBackend},
    };
    use std::{
        fs::{File, OpenOptions},
        io::Write,
    };
    struct Probe {
        runtime: Runtime,
        trace: File,
    }
    impl Probe {
        fn log(&self, value: serde_json::Value) -> Result<(), String> {
            writeln!(&self.trace, "{value}").map_err(|e| e.to_string())
        }
    }
    impl NativeApp for Probe {
        fn inspect(&self, observer: &mut dyn FnMut(&Runtime)) -> Result<(), String> {
            observer(&self.runtime);
            Ok(())
        }
        fn update(&mut self, input: &mut dyn FnMut(&mut Runtime)) -> Result<(), String> {
            input(&mut self.runtime);
            self.runtime.sync_ui_bindings_native()?;
            self.log(serde_json::json!({"event":"update","focus":self.runtime.focused_index(),"values":[self.runtime.text_value(0),self.runtime.text_value(1)],"selected":self.runtime.text_selected(),"readonly":[self.runtime.control_readonly(0),self.runtime.control_readonly(1)]}))
        }
        fn on_frame_presented(&self, renderer: NativeRenderer) -> Result<(), String> {
            self.log(serde_json::json!({"event":"present","renderer":format!("{renderer:?}")}))
        }
        fn on_gpu_fallback(&self, error: &str) -> Result<(), String> {
            self.log(serde_json::json!({"event":"gpu_error","error":error}))
        }
        fn on_ime_event(&self, event: &winit::event::Ime) -> Result<(), String> {
            self.log(serde_json::json!({"event":"ime","value":format!("{event:?}")}))
        }
    }
    pub fn run() -> Result<(), String> {
        let trace = std::env::var_os("FORMA_READONLY_LOG").ok_or("Set FORMA_READONLY_LOG")?;
        let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state reference: String = 'Readonly — 你好 мир🙂'; state query: String = 'Editable'; Column { width:520; height:180; padding:16; gap:16; TextInput { key:'reference'; label:'Reference'; readOnly:true; width:480; height:40; value <-> state.reference; background:#27334a; } TextInput { key:'query'; label:'Query'; width:480; height:40; value <-> state.query; background:#27334a; } } }".into())]);
        let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default())?;
        runtime.set_svet_document_scheduling(true);
        runtime.set_reduced_motion(true);
        runtime.focus_control(0);
        let probe = Probe {
            runtime,
            trace: OpenOptions::new()
                .create(true)
                .append(true)
                .open(trace)
                .map_err(|e| e.to_string())?,
        };
        probe.log(serde_json::json!({"event":"start","pid":std::process::id()}))?;
        forma::native_window::run(
            probe,
            NativeWindowConfig {
                title: "Svet readonly qualification".into(),
                width: 520.,
                height: 180.,
                backend: Some(UiBackend::Svet),
                platform: TargetPlatform::Desktop,
                ..Default::default()
            },
        )
    }
}
#[cfg(feature = "native")]
fn main() -> Result<(), String> {
    probe::run()
}
