use wasm_bindgen::prelude::*;
mod blend;
pub mod frame_stats;
mod geometry;
pub mod markup;
pub mod runtime;
pub use runtime::Runtime;
pub mod binding;
pub mod canvas;
pub mod control_models_wasm;
pub mod control_state;
pub mod display_list;
pub mod form_document;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod mcp;
mod raster_cache;
pub mod reveal;
mod shape;
pub mod template;
pub mod text;
pub mod text_input;
#[cfg(feature = "gpu")]
pub use gpu_compute;
#[cfg(feature = "gpu")]
pub use gpu_shader;
#[cfg(all(feature = "gpu", target_os = "linux"))]
extern crate wgpu_linux as wgpu;
#[cfg(all(feature = "gpu", not(target_os = "linux")))]
pub use wgpu as gpu_runtime;
#[cfg(all(feature = "gpu", target_os = "linux"))]
pub use wgpu_linux as gpu_runtime;
#[cfg(feature = "native")]
pub mod native_accessibility;
#[cfg(feature = "native")]
pub mod native_clipboard;
#[cfg(feature = "native")]
#[doc(hidden)]
pub mod native_gpu;
#[cfg(feature = "native")]
pub mod native_window;

pub const EXAMPLE: &str = include_str!("../examples/Button.ui");
pub const TWO_BUTTONS_EXAMPLE: &str = include_str!("../examples/TwoButtons.ui");
pub const BUTTON_COMPONENT: &str = include_str!("../examples/Button.component.ui");

/// Logical single-line metrics from the same font as the vector text renderer.
/// `family` is `sans` (the default when omitted or unknown) or `mono`.
#[wasm_bindgen]
pub fn text_metrics(value: &str, font_size: f32, family: Option<String>) -> Vec<f32> {
    let family = family
        .as_deref()
        .and_then(text::FontFamily::from_name)
        .unwrap_or_default();
    text::measure_line_in(family, value, font_size).to_vec()
}

/// Shaped single-line metrics (`[width, height, glyph_count]`) using ligature/BiDi/fallback shaping.
#[wasm_bindgen]
pub fn shaped_text_metrics(value: &str, font_size: f32) -> Vec<f32> {
    let w = text::measure_shaped_text(value, font_size);
    let [_, h] = text::measure_line(value, font_size);
    let glyphs = text::shape_text(value, font_size).len() as f32;
    vec![w, h, glyphs]
}

/// The extent a scroll container moves over: the scene's content without its padding.
pub(crate) fn scroll_content_size(scene: &markup::Scene) -> [f32; 2] {
    [
        scene.content_width - scene.padding[1] - scene.padding[3],
        scene.content_height - scene.padding[0] - scene.padding[2],
    ]
}

mod control;
pub use control::Button;

#[cfg(test)]
mod tests;

#[cfg(target_os = "android")]
pub mod android_jni {
    use std::sync::{Mutex, OnceLock};

    struct AndroidState {
        runtime: crate::Runtime,
        backend: Option<&'static str>,
        chooser: Option<crate::Runtime>,
        last_action: String,
    }

    static ANDROID_STATE: OnceLock<Mutex<AndroidState>> = OnceLock::new();

    fn chooser_ui_for_width(vw: f32, vh: f32) -> String {
        let w = vw.max(320.0).round() as i32;
        let h = vh.max(360.0).round() as i32;
        let pad_x = 24;
        let btn_w = (w - pad_x * 2).max(200);
        format!(
            r#"component BackendLauncher {{
    Frame {{
        width: {w};
        height: {h};
        padding: 88 {pad_x} 24 {pad_x};
        gap: 20;
        background: #141923;
        radius: 0;
        clip: true;
        Button {{
            key: 'choose_egui';
            width: {btn_w};
            height: 76;
            text: '[1] Запустить на egui (Immediate-Mode)';
            background: #5b8af5;
            hoverBackground: #7ba3ff;
            pressedBackground: #426cd4;
            clicked -> actions.choose_egui();
        }}
        Button {{
            key: 'choose_svet';
            width: {btn_w};
            height: 76;
            text: '[2] Запустить на svet (Reactive GPU/UMA)';
            background: #58c496;
            hoverBackground: #76dfb1;
            pressedBackground: #3fa177;
            clicked -> actions.choose_svet();
        }}
    }}
}}"#
        )
    }

    fn two_buttons_ui_for_width(vw: f32, vh: f32) -> String {
        let w = vw.max(320.0).round() as i32;
        let h = vh.max(360.0).round() as i32;
        let pad_x = 24;
        let btn_w = (w - pad_x * 2).max(200);
        format!(
            r#"component TwoButtons {{
    Frame {{
        width: {w};
        height: {h};
        padding: 88 {pad_x} 24 {pad_x};
        gap: 20;
        background: #181e2a;
        radius: 0;
        clip: true;
        Button {{
            key: 'first';
            width: {btn_w};
            height: 76;
            text: 'Первая кнопка';
            clicked -> actions.first();
        }}
        Button {{
            key: 'second';
            width: {btn_w};
            height: 76;
            text: 'Вторая кнопка';
            background: #aad9bd;
            hoverBackground: #c7eed7;
            pressedBackground: #78b493;
            clicked -> actions.second();
        }}
    }}
}}"#
        )
    }

    fn get_state() -> &'static Mutex<AndroidState> {
        ANDROID_STATE.get_or_init(|| {
            let src = two_buttons_ui_for_width(412.0, 915.0);
            let runtime = crate::Runtime::from_sources(&src, crate::BUTTON_COMPONENT)
                .expect("compile TwoButtons.ui for Android");
            let csrc = chooser_ui_for_width(412.0, 915.0);
            let chooser = crate::Runtime::from_sources(&csrc, crate::BUTTON_COMPONENT).ok();
            Mutex::new(AndroidState {
                runtime,
                backend: None,
                chooser,
                last_action: String::from("Выберите бэкенд: [1] egui / [2] svet"),
            })
        })
    }

    #[repr(C)]
    pub struct JNINativeInterface {
        _reserved: [*const core::ffi::c_void; 230],
        pub get_direct_buffer_address:
            unsafe extern "C" fn(*mut JNIEnv, *mut core::ffi::c_void) -> *mut u8,
        pub get_direct_buffer_capacity:
            unsafe extern "C" fn(*mut JNIEnv, *mut core::ffi::c_void) -> i64,
    }

    pub type JNIEnv = *const JNINativeInterface;

    #[no_mangle]
    pub unsafe extern "C" fn Java_dev_forma_app_MainActivity_nativeInit(
        _env: *mut JNIEnv,
        _this: *mut core::ffi::c_void,
        backend_mode: i32,
    ) {
        let mut st = get_state().lock().unwrap();
        let src = two_buttons_ui_for_width(412.0, 915.0);
        st.runtime = crate::Runtime::from_sources(&src, crate::BUTTON_COMPONENT).unwrap();
        match backend_mode {
            1 => {
                st.backend = Some("egui");
                st.chooser = None;
                st.last_action = "Активен бэкенд: egui (Immediate-Mode)".into();
            }
            2 => {
                st.backend = Some("svet");
                st.chooser = None;
                st.last_action = "Активен бэкенд: svet (Reactive GPU/UMA)".into();
            }
            _ => {
                st.backend = None;
                let csrc = chooser_ui_for_width(412.0, 915.0);
                st.chooser = crate::Runtime::from_sources(&csrc, crate::BUTTON_COMPONENT).ok();
                st.last_action = "Выберите бэкенд: [1] egui / [2] svet".into();
            }
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_dev_forma_app_MainActivity_nativePointer(
        _env: *mut JNIEnv,
        _this: *mut core::ffi::c_void,
        x: f32,
        y: f32,
        kind: i32,
    ) -> i32 {
        let mut st = get_state().lock().unwrap();
        if let Some(chooser) = &mut st.chooser {
            let before = chooser.clicks();
            chooser.pointer(x, y, kind as u8);
            if chooser.clicks() != before {
                let chosen = if chooser.key() == "choose_egui" {
                    "egui"
                } else {
                    "svet"
                };
                st.backend = Some(chosen);
                st.chooser = None;
                st.last_action = format!("Активен бэкенд: {chosen}");
                return if chosen == "egui" { 101 } else { 102 };
            }
            return 0;
        }
        let before = st.runtime.clicks();
        st.runtime.pointer(x, y, kind as u8);
        if st.runtime.clicks() != before {
            st.last_action = format!("{} → {}", st.runtime.key(), st.runtime.action());
            return st.runtime.clicks() as i32;
        }
        0
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_dev_forma_app_MainActivity_nativeToggleBackend(
        _env: *mut JNIEnv,
        _this: *mut core::ffi::c_void,
    ) -> i32 {
        let mut st = get_state().lock().unwrap();
        let next = match st.backend {
            Some("egui") => "svet",
            _ => "egui",
        };
        st.backend = Some(next);
        st.chooser = None;
        if next == "egui" { 1 } else { 2 }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_dev_forma_app_MainActivity_nativeRender(
        env: *mut JNIEnv,
        _this: *mut core::ffi::c_void,
        buffer_obj: *mut core::ffi::c_void,
        width: i32,
        height: i32,
        scale: f32,
        dt_ms: f32,
    ) {
        if env.is_null() || buffer_obj.is_null() || width <= 0 || height <= 0 {
            return;
        }
        let iface = &*(*env);
        let ptr = (iface.get_direct_buffer_address)(env, buffer_obj) as *mut u32;
        let cap = (iface.get_direct_buffer_capacity)(env, buffer_obj);
        let needed = (width as usize) * (height as usize);
        if ptr.is_null() || (cap as usize) < needed * 4 {
            return;
        }
        let slice = core::slice::from_raw_parts_mut(ptr, needed);
        let mut st = get_state().lock().unwrap();
        let logical_w = (width as f32 / scale.max(1.0)).round();
        let logical_h = (height as f32 / scale.max(1.0)).round();
        if (st.runtime.render_width() - logical_w).abs() > 2.0 {
            let next_src = two_buttons_ui_for_width(logical_w, logical_h);
            if let Ok(mut next_rt) =
                crate::Runtime::from_sources(&next_src, crate::BUTTON_COMPONENT)
            {
                next_rt.preserve_interaction(&st.runtime);
                st.runtime = next_rt;
            }
        }
        if let Some(chooser) = &st.chooser {
            if (chooser.render_width() - logical_w).abs() > 2.0 {
                let next_csrc = chooser_ui_for_width(logical_w, logical_h);
                if let Ok(mut next_c) =
                    crate::Runtime::from_sources(&next_csrc, crate::BUTTON_COMPONENT)
                {
                    next_c.preserve_interaction(chooser);
                    st.chooser = Some(next_c);
                }
            }
        }
        let is_egui = st.backend == Some("egui");
        let rt = if let Some(chooser) = &mut st.chooser {
            chooser
        } else {
            &mut st.runtime
        };
        rt.tick(dt_ms.clamp(0.0, 100.0));
        let _ = rt.paint_native(slice, width as u32, height as u32, scale.max(1.0));
        // Convert 0x00RRGGBB (softbuffer ARGB8888) to 0xFFRRGGBB for Android ARGB_8888 Bitmap
        for px in slice.iter_mut() {
            let r = (*px >> 16) & 0xFF;
            let g = (*px >> 8) & 0xFF;
            let b = *px & 0xFF;
            // Android Bitmap.copyPixelsFromBuffer with ARGB_8888 on little-endian expects ABGR (0xAABBGGRR)
            *px = 0xFF00_0000 | (b << 16) | (g << 8) | r;
        }
        if is_egui && height >= 6 {
            let bar_h = (4.0 * scale.max(1.0)) as usize;
            for y in 0..bar_h.min(height as usize) {
                let row = y * (width as usize);
                for x in 0..(width as usize) {
                    slice[row + x] = 0xFFF5_8A5B; // #5b8af5 in ABGR
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native_document_graph;
