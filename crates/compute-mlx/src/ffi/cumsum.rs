//! MLX-C v0.7 split axis scans from flattened scans and added a dtype option.
use super::Handle;
use libloading::Library;
use std::ffi::c_int;
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct OptionalDtype {
    value: c_int,
    has_value: bool,
}
type Legacy = unsafe extern "C" fn(*mut Handle, Handle, c_int, bool, bool, Handle) -> c_int;
type Axis =
    unsafe extern "C" fn(*mut Handle, Handle, c_int, bool, bool, OptionalDtype, Handle) -> c_int;
pub(crate) enum Cumsum {
    Legacy(Legacy),
    Axis(Axis),
}
impl Cumsum {
    pub(super) unsafe fn load(library: &Library) -> Result<Self, String> {
        match unsafe { library.get::<Axis>(b"mlx_cumsum_axis\0") } {
            Ok(f) => Ok(Self::Axis(*f)),
            Err(_) => Ok(Self::Legacy(
                *unsafe { library.get::<Legacy>(b"mlx_cumsum\0") }.map_err(|e| e.to_string())?,
            )),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn call(
        &self,
        out: *mut Handle,
        input: Handle,
        axis: c_int,
        reverse: bool,
        inclusive: bool,
        dtype: c_int,
        stream: Handle,
    ) -> c_int {
        match self {
            Self::Legacy(f) => unsafe { f(out, input, axis, reverse, inclusive, stream) },
            Self::Axis(f) => unsafe {
                f(
                    out,
                    input,
                    axis,
                    reverse,
                    inclusive,
                    OptionalDtype {
                        value: dtype,
                        has_value: true,
                    },
                    stream,
                )
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn legacy(
        out: *mut Handle,
        _input: Handle,
        axis: c_int,
        reverse: bool,
        inclusive: bool,
        stream: Handle,
    ) -> c_int {
        assert_eq!(axis, 2);
        assert!(reverse);
        assert!(!inclusive);
        unsafe { *out = stream };
        23
    }
    unsafe extern "C" fn current(
        out: *mut Handle,
        input: Handle,
        axis: c_int,
        reverse: bool,
        inclusive: bool,
        dtype: OptionalDtype,
        stream: Handle,
    ) -> c_int {
        assert!(dtype.has_value);
        assert_eq!(dtype.value, super::super::U32);
        unsafe { legacy(out, input, axis, reverse, inclusive, stream) }
    }
    #[test]
    fn both_scan_abis_preserve_axis_options_dtype_and_stream() {
        let empty = Handle {
            ctx: std::ptr::null_mut(),
        };
        let mut token = 0u8;
        let stream = Handle {
            ctx: (&mut token as *mut u8).cast(),
        };
        for abi in [Cumsum::Legacy(legacy), Cumsum::Axis(current)] {
            let mut out = empty;
            assert_eq!(
                unsafe { abi.call(&mut out, empty, 2, true, false, super::super::U32, stream) },
                23
            );
            assert_eq!(out.ctx, stream.ctx);
        }
    }
}
