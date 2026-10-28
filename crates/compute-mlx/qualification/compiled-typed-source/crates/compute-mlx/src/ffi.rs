//! Small, audited subset of the MLX-C 0.6 ABI. No Python or link-time dependency.
use libloading::Library;
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Handle {
    pub ctx: *mut c_void,
}
pub(crate) type Unary = unsafe extern "C" fn(*mut Handle, Handle, Handle) -> c_int;
pub(crate) type Binary = unsafe extern "C" fn(*mut Handle, Handle, Handle, Handle) -> c_int;
pub(crate) type ShapeOp =
    unsafe extern "C" fn(*mut Handle, Handle, *const c_int, usize, Handle) -> c_int;
pub(crate) type Reduce =
    unsafe extern "C" fn(*mut Handle, Handle, *const c_int, usize, bool, Handle) -> c_int;
pub(crate) type Scatter =
    unsafe extern "C" fn(*mut Handle, Handle, Handle, Handle, c_int, Handle) -> c_int;
// These functions return pointers, not half values through the calling ABI.
// Reading their storage as u16 preserves every floating-point bit pattern.
pub(crate) type HalfData = unsafe extern "C" fn(Handle) -> *const u16;
pub(crate) const F16: c_int = 9;
pub(crate) const F32: c_int = 10;
pub(crate) const BF16: c_int = 12;
pub(crate) const U32: c_int = 3;
pub(crate) const U16: c_int = 2;
pub(crate) const GPU: c_int = 1;

pub(crate) type TraceCallback = unsafe extern "C" fn(*mut Handle, Handle, *mut c_void) -> c_int;

// Compilation is independent of the optional custom Metal ABI. Keep the group
// complete: an older runtime must never expose a partially callable program.
pub(crate) struct CompileApi {
    pub compile: unsafe extern "C" fn(*mut Handle, Handle, bool) -> c_int,
    pub closure_new: unsafe extern "C" fn() -> Handle,
    pub closure_new_func_payload: unsafe extern "C" fn(
        TraceCallback,
        *mut c_void,
        unsafe extern "C" fn(*mut c_void),
    ) -> Handle,
    pub closure_free: unsafe extern "C" fn(Handle) -> c_int,
    pub closure_apply: unsafe extern "C" fn(*mut Handle, Handle, Handle) -> c_int,
    pub vector_array_new: unsafe extern "C" fn() -> Handle,
    pub vector_array_new_data: unsafe extern "C" fn(*const Handle, usize) -> Handle,
    pub vector_array_free: unsafe extern "C" fn(Handle) -> c_int,
    pub vector_array_set_data: unsafe extern "C" fn(*mut Handle, *const Handle, usize) -> c_int,
    pub vector_array_size: unsafe extern "C" fn(Handle) -> usize,
    pub vector_array_get: unsafe extern "C" fn(*mut Handle, Handle, usize) -> c_int,
}
impl CompileApi {
    unsafe fn load(library: &Library) -> Option<Self> {
        macro_rules! symbol {
            ($name:ident) => {
                *unsafe { library.get(concat!("mlx_", stringify!($name), "\0").as_bytes()) }.ok()?
            };
        }
        Some(Self {
            compile: symbol!(compile),
            closure_new: symbol!(closure_new),
            closure_new_func_payload: symbol!(closure_new_func_payload),
            closure_free: symbol!(closure_free),
            closure_apply: symbol!(closure_apply),
            vector_array_new: symbol!(vector_array_new),
            vector_array_new_data: symbol!(vector_array_new_data),
            vector_array_free: symbol!(vector_array_free),
            vector_array_set_data: symbol!(vector_array_set_data),
            vector_array_size: symbol!(vector_array_size),
            vector_array_get: symbol!(vector_array_get),
        })
    }
}

// Optional group: older MLX-C builds can still provide the existing tensor API.
// Custom kernels are available only when every symbol in this ABI is present.
macro_rules! metal_api {
    ($($name:ident:$ty:ty),* $(,)?) => {
        pub(crate) struct MetalApi { $(pub $name:$ty,)* }
        impl MetalApi {
            unsafe fn load(library: &Library) -> Option<Self> {
                $(let $name = *unsafe { library.get::<$ty>(concat!("mlx_",stringify!($name),"\0").as_bytes()) }.ok()?;)*
                Some(Self { $($name,)* })
            }
        }
    }
}
metal_api! {
    vector_string_new_data:unsafe extern "C" fn(*const *const c_char,usize)->Handle,
    vector_string_free:unsafe extern "C" fn(Handle)->c_int,
    vector_array_new:unsafe extern "C" fn()->Handle,
    vector_array_new_data:unsafe extern "C" fn(*const Handle,usize)->Handle,
    vector_array_free:unsafe extern "C" fn(Handle)->c_int,
    vector_array_get:unsafe extern "C" fn(*mut Handle,Handle,usize)->c_int,
    fast_metal_kernel_new:unsafe extern "C" fn(*const c_char,Handle,Handle,*const c_char,*const c_char,bool,bool)->Handle,
    fast_metal_kernel_free:unsafe extern "C" fn(Handle),
    fast_metal_kernel_config_new:unsafe extern "C" fn()->Handle,
    fast_metal_kernel_config_free:unsafe extern "C" fn(Handle),
    fast_metal_kernel_config_add_output_arg:unsafe extern "C" fn(Handle,*const c_int,usize,c_int)->c_int,
    fast_metal_kernel_config_set_grid:unsafe extern "C" fn(Handle,c_int,c_int,c_int)->c_int,
    fast_metal_kernel_config_set_thread_group:unsafe extern "C" fn(Handle,c_int,c_int,c_int)->c_int,
    fast_metal_kernel_config_set_init_value:unsafe extern "C" fn(Handle,f32)->c_int,
    fast_metal_kernel_config_add_template_arg_dtype:unsafe extern "C" fn(Handle,*const c_char,c_int)->c_int,
    fast_metal_kernel_config_add_template_arg_bool:unsafe extern "C" fn(Handle,*const c_char,bool)->c_int,
    fast_metal_kernel_apply:unsafe extern "C" fn(*mut Handle,Handle,Handle,Handle,Handle)->c_int,
}

static CALLS: Mutex<()> = Mutex::new(());
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
unsafe extern "C" fn error_handler(message: *const c_char, _data: *mut c_void) {
    if message.is_null() {
        return;
    }
    // MLX guarantees a valid NUL-terminated message for the callback duration.
    let message = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
}

macro_rules! api {
    ($($name:ident:$ty:ty),* $(,)?)=>{
        pub(crate) struct Api {
            $(pub $name:$ty,)*
            pub array_data_float16: Option<HalfData>,
            pub array_data_bfloat16: Option<HalfData>,
            pub metal: Option<MetalApi>,
            pub compile: Option<CompileApi>,
            _library:Library,
        }
        impl Api {
            unsafe fn load(path:&std::ffi::OsStr)->Result<Self,String> {
                // Caller-selected native libraries are trusted executable code.
                let library=unsafe { Library::new(path) }.map_err(|e|e.to_string())?;
                $(let $name=*unsafe { library.get::<$ty>(format!("mlx_{}\0",stringify!($name).trim_start_matches("r#")).as_bytes()) }.map_err(|e|e.to_string())?;)*
                // MLX-C only exports these accessors when its host compiler
                // supports the corresponding native half type. Their absence
                // must not make the existing f32/u32 backend unavailable.
                let array_data_float16 = unsafe { library.get::<HalfData>(b"mlx_array_data_float16\0") }.ok().map(|p| *p);
                let array_data_bfloat16 = unsafe { library.get::<HalfData>(b"mlx_array_data_bfloat16\0") }.ok().map(|p| *p);
                let metal = unsafe { MetalApi::load(&library) };
                let compile = unsafe { CompileApi::load(&library) };
                Ok(Self { $($name,)* array_data_float16, array_data_bfloat16, metal, compile, _library:library })
            }
        }
    }
}
api! {
    set_error_handler:unsafe extern "C" fn(unsafe extern "C" fn(*const c_char,*mut c_void),*mut c_void,Option<unsafe extern "C" fn(*mut c_void)>),
    version:unsafe extern "C" fn(*mut Handle)->c_int,
    string_new:unsafe extern "C" fn()->Handle,
    string_free:unsafe extern "C" fn(Handle)->c_int,
    string_data:unsafe extern "C" fn(Handle)->*const c_char,
    device_new_type:unsafe extern "C" fn(c_int,c_int)->Handle,
    device_free:unsafe extern "C" fn(Handle)->c_int,
    device_is_available:unsafe extern "C" fn(*mut bool,Handle)->c_int,
    stream_new_device:unsafe extern "C" fn(Handle)->Handle,
    stream_free:unsafe extern "C" fn(Handle)->c_int,
    synchronize:unsafe extern "C" fn(Handle)->c_int,
    metal_is_available:unsafe extern "C" fn(*mut bool)->c_int,
    array_new:unsafe extern "C" fn()->Handle,
    array_set_data:unsafe extern "C" fn(*mut Handle,*const c_void,*const c_int,c_int,c_int)->c_int,
    array_free:unsafe extern "C" fn(Handle)->c_int,
    array_eval:unsafe extern "C" fn(Handle)->c_int,
    array_dtype:unsafe extern "C" fn(Handle)->c_int,
    array_size:unsafe extern "C" fn(Handle)->usize,
    array_ndim:unsafe extern "C" fn(Handle)->usize,
    array_shape:unsafe extern "C" fn(Handle)->*const c_int,
    array_data_float32:unsafe extern "C" fn(Handle)->*const f32,
    array_data_uint32:unsafe extern "C" fn(Handle)->*const u32,
    contiguous:unsafe extern "C" fn(*mut Handle,Handle,bool,Handle)->c_int,
    reshape:ShapeOp,
    transpose_axes:ShapeOp,
    broadcast_to:ShapeOp,
    negative:Unary, abs:Unary, sqrt:Unary, exp:Unary, log:Unary, sin:Unary, cos:Unary, reciprocal:Unary, square:Unary,
    add:Binary, subtract:Binary, multiply:Binary, divide:Binary, minimum:Binary, maximum:Binary,
    equal:Binary, not_equal:Binary, less:Binary, less_equal:Binary, greater:Binary, greater_equal:Binary,
    matmul:Binary,
    astype:unsafe extern "C" fn(*mut Handle,Handle,c_int,Handle)->c_int,
    view:unsafe extern "C" fn(*mut Handle,Handle,c_int,Handle)->c_int,
    bitwise_and:Binary, right_shift:Binary,
    sum_axes:Reduce, prod_axes:Reduce, min_axes:Reduce, max_axes:Reduce, mean_axes:Reduce,
    softmax_axes:Reduce, logsumexp_axes:Reduce,
    fast_scaled_dot_product_attention:unsafe extern "C" fn(*mut Handle,Handle,Handle,Handle,f32,*const c_char,Handle,Handle,Handle)->c_int,
    cumsum:unsafe extern "C" fn(*mut Handle,Handle,c_int,bool,bool,Handle)->c_int,
    take_axis:unsafe extern "C" fn(*mut Handle,Handle,Handle,c_int,Handle)->c_int,
    r#where:unsafe extern "C" fn(*mut Handle,Handle,Handle,Handle,Handle)->c_int,
    zeros:unsafe extern "C" fn(*mut Handle,*const c_int,usize,c_int,Handle)->c_int,
    ones:unsafe extern "C" fn(*mut Handle,*const c_int,usize,c_int,Handle)->c_int,
    arange:unsafe extern "C" fn(*mut Handle,f64,f64,f64,c_int,Handle)->c_int,
    put_along_axis:unsafe extern "C" fn(*mut Handle,Handle,Handle,Handle,c_int,Handle)->c_int,
    scatter_add_single:Scatter, scatter_prod_single:Scatter,
    scatter_min_single:Scatter, scatter_max_single:Scatter,
}
impl Api {
    pub fn global() -> Result<Arc<Self>, String> {
        // Keep the library loaded for the process lifetime: MLX owns background
        // workers, global streams and the callback, beyond a single Rust handle.
        static API: OnceLock<Mutex<Option<Arc<Api>>>> = OnceLock::new();
        let mut shared = API
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(api) = shared.as_ref() {
            return Ok(api.clone());
        }
        let paths: Vec<std::ffi::OsString> =
            if let Some(path) = std::env::var_os("COMPUTE_MLX_LIBRARY") {
                vec![path]
            } else if cfg!(target_os = "macos") {
                [
                    "/opt/homebrew/opt/mlx-c/lib/libmlxc.dylib",
                    "/usr/local/lib/libmlxc.dylib",
                    "libmlxc.dylib",
                ]
                .into_iter()
                .map(Into::into)
                .collect()
            } else if cfg!(target_os = "windows") {
                vec!["mlxc.dll".into()]
            } else {
                vec!["libmlxc.so".into()]
            };
        let mut failures = Vec::new();
        for path in paths {
            match unsafe { Self::load(&path) } {
                Ok(api) => {
                    let api = Arc::new(api);
                    let _gate = CALLS.lock().unwrap_or_else(|e| e.into_inner());
                    unsafe { (api.set_error_handler)(error_handler, std::ptr::null_mut(), None) };
                    *shared = Some(api.clone());
                    return Ok(api);
                }
                Err(error) => failures.push(format!("{}: {error}", path.to_string_lossy())),
            }
        }
        Err(failures.join("; "))
    }
    pub fn call<T>(&self, f: impl FnOnce(&Self) -> Result<T, String>) -> Result<T, String> {
        let _guard = CALLS.lock().unwrap_or_else(|e| e.into_inner());
        *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let result = f(self);
        let message = LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(message) = message {
            Err(message)
        } else {
            result
        }
    }
    pub fn check(code: c_int) -> Result<(), String> {
        if code == 0 {
            Ok(())
        } else {
            Err(format!("MLX C returned status {code}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_library_is_an_error_without_native_initialization() {
        assert!(
            unsafe { Api::load(std::ffi::OsStr::new("/__compute_mlx_missing__/libmlxc")) }.is_err()
        );
    }
    #[test]
    fn native_shape_error_returns_message_and_keeps_c_api_usable() {
        let api = match Api::global() {
            Ok(a) => a,
            Err(e) => {
                assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{e}");
                return;
            }
        };
        // All operations here use a CPU stream and only validate shapes; no GPU
        // execution is needed to prove the C handler does not terminate Rust.
        let result = api.call(|a| unsafe {
            let device = (a.device_new_type)(0, 0);
            let stream = (a.stream_new_device)(device);
            (a.device_free)(device);
            let mut input = (a.array_new)();
            let values = [1.0_f32, 2.0];
            let shape = [2];
            let upload = Api::check((a.array_set_data)(
                &mut input,
                values.as_ptr().cast(),
                shape.as_ptr(),
                1,
                F32,
            ));
            if let Err(e) = upload {
                (a.array_free)(input);
                (a.stream_free)(stream);
                return Err(e);
            }
            let mut output = (a.array_new)();
            let invalid = [3];
            let rejected = (a.reshape)(&mut output, input, invalid.as_ptr(), 1, stream);
            // A subsequent valid operation still succeeds before cleanup.
            let valid = (a.reshape)(&mut output, input, shape.as_ptr(), 1, stream);
            (a.array_free)(output);
            (a.array_free)(input);
            (a.stream_free)(stream);
            assert_ne!(rejected, 0);
            assert_eq!(valid, 0);
            Api::check(rejected)
        });
        let message = result.unwrap_err();
        assert!(message.contains("reshape"), "{message}");
    }
}
