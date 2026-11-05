//! Manual OS input qualification of the reusable NativeHost, with fixture-only state traces.
#[cfg(not(feature = "native"))]
fn main() { panic!("Run with --features native"); }

#[cfg(feature = "native")]
fn main() -> Result<(), String> {
    use forma::{native_window::{NativeApp, NativeWindowConfig, TargetPlatform, UiBackend}, Runtime};
    struct Probe { runtime: Runtime, last: Option<(String, i32)>, trace: Option<std::path::PathBuf> }
    impl NativeApp for Probe {
        fn on_gpu_fallback(&self,error:&str)->Result<(),String> {
            if let Some(path)=&self.trace {
                use std::io::Write;
                let mut file=std::fs::OpenOptions::new().create(true).append(true).open(path.with_extension("gpu-errors.jsonl")).map_err(|e|e.to_string())?;
                writeln!(file,"{}",serde_json::json!({"gpu_fallback":error,"at_ns":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64})).map_err(|e|e.to_string())?;
            }
            Ok(())
        }
        fn on_occlusion_changed(&self, occluded: bool) -> Result<(),String> {
            if let Some(path)=&self.trace {
                use std::io::Write;
                let mut file=std::fs::OpenOptions::new().create(true).append(true).open(path.with_extension("window.jsonl")).map_err(|e|e.to_string())?;
                writeln!(file,"{}",serde_json::json!({"occluded":occluded,"at_ns":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64})).map_err(|e|e.to_string())?;
            }
            Ok(())
        }
        fn on_frame_presented(&self, renderer: forma::native_window::NativeRenderer) -> Result<(),String> {
            if let Some(path)=&self.trace {
                use std::io::Write;
                let mut file=std::fs::OpenOptions::new().create(true).append(true).open(path.with_extension("presentation.jsonl")).map_err(|e|e.to_string())?;
                writeln!(file,"{}",serde_json::json!({"renderer":format!("{renderer:?}"),"focused_index":self.runtime.focused_index(),"at_ns":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64})).map_err(|e|e.to_string())?;
            }
            Ok(())
        }
        fn on_ime_event(&self, event: &winit::event::Ime) -> Result<(), String> {
            if let Some(path) = &self.trace {
                use std::io::Write;
                let mut file=std::fs::OpenOptions::new().create(true).append(true)
                    .open(path.with_extension("ime.jsonl")).map_err(|e|e.to_string())?;
                let record=serde_json::json!({"ime":format!("{event:?}"),"focused_index":self.runtime.focused_index()});
                writeln!(file,"{record}").map_err(|e|e.to_string())?;
            }
            Ok(())
        }
        fn inspect(&self, observer: &mut dyn FnMut(&Runtime)) -> Result<(), String> {
            observer(&self.runtime);
            Ok(())
        }
        fn update(&mut self, input: &mut dyn FnMut(&mut Runtime)) -> Result<(), String> {
            input(&mut self.runtime);
            self.runtime.sync_ui_bindings_native()?;
            let current = (self.runtime.ui_state_json(), self.runtime.focused_index());
            if self.last.as_ref() != Some(&current) {
                let record = serde_json::json!({"state":serde_json::from_str::<serde_json::Value>(&current.0).map_err(|e|e.to_string())?,"focused_index":current.1});
                println!("NATIVE_INPUT_PROBE {record}");
                if let Some(path) = &self.trace {
                    use std::io::Write;
                    let mut file=std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e|e.to_string())?;
                    writeln!(file,"{record}").map_err(|e|e.to_string())?;
                }
                self.last = Some(current);
            }
            Ok(())
        }
    }
    let files = std::collections::BTreeMap::from([("App.ui".into(), include_str!("../tests/fixtures/svet-native-ime/App.ui").into())]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default())?;
    runtime.set_svet_document_scheduling(true);
    forma::native_window::run(Probe { runtime, last: None, trace: std::env::var_os("FORMA_NATIVE_INPUT_PROBE_LOG").map(Into::into) }, NativeWindowConfig {
        title:"Svet native input probe".into(),width:640.,height:360.,backend:Some(UiBackend::Svet),platform:TargetPlatform::Desktop,..Default::default()
    })
}
