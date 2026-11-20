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
    /// # Safety
    ///
    /// `library` must outlive `Self` and match either MLX-C v0.7 (`mlx_cumsum_axis`)
    /// or MLX-C v0.6 (`mlx_cumsum`) ABI signatures.
    pub(super) unsafe fn load(library: &Library) -> Result<Self, String> {
        // SAFETY: `library` remains loaded inside `Api::global()`; tries v0.7 symbol before falling back to v0.6.
        match unsafe { library.get::<Axis>(b"mlx_cumsum_axis\0") } {
            Ok(f) => Ok(Self::Axis(*f)),
            Err(_) => Ok(Self::Legacy(
                // SAFETY: Fallback matches MLX-C v0.6 `mlx_cumsum` ABI signature.
                *unsafe { library.get::<Legacy>(b"mlx_cumsum\0") }.map_err(|e| e.to_string())?,
            )),
        }
    }
    /// # Safety
    ///
    /// `out` must point to writable `Handle` storage, and `input` and `stream`
    /// must be valid live handles under the active synchronized `CALLS` gate.
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
            // SAFETY: Caller upholds all handle and pointer preconditions for the v0.6 ABI.
            Self::Legacy(f) => unsafe { f(out, input, axis, reverse, inclusive, stream) },
            // SAFETY: Caller upholds all preconditions for the v0.7 axis ABI with explicit dtype.
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
    /// # Safety
    ///
    /// `out` must be a valid aligned pointer to writable `Handle`.
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
        // SAFETY: Test passes stack `&mut out`.
        unsafe { *out = stream };
        23
    }
    /// # Safety
    ///
    /// `out` must be a valid aligned pointer to writable `Handle`.
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
        // SAFETY: Forwards stack `&mut out` to `legacy`.
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
                // SAFETY: `&mut out` is valid stack storage for the fake ABI callbacks.
                unsafe { abi.call(&mut out, empty, 2, true, false, super::super::U32, stream) },
                23
            );
            assert_eq!(out.ctx, stream.ctx);
        }
    }
}
