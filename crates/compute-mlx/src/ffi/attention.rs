//! MLX-C v0.6/v0.7 attention ABI compatibility. In v0.7 `force_fused`
//! precedes the stream, and `mlx_fast_cross_entropy` was added in that release.
use super::Handle;
use libloading::Library;
use std::ffi::{c_char, c_int};
type Legacy = unsafe extern "C" fn(
    *mut Handle,
    Handle,
    Handle,
    Handle,
    f32,
    *const c_char,
    Handle,
    Handle,
    Handle,
) -> c_int;
type ForceFused = unsafe extern "C" fn(
    *mut Handle,
    Handle,
    Handle,
    Handle,
    f32,
    *const c_char,
    Handle,
    Handle,
    bool,
    Handle,
) -> c_int;
pub(crate) enum Attention {
    Legacy(Legacy),
    ForceFused(ForceFused),
}
impl Attention {
    pub(super) unsafe fn load(library: &Library) -> Result<Self, String> {
        // The symbol is a release marker; it is never called through this probe.
        let current =
            unsafe { library.get::<unsafe extern "C" fn()>(b"mlx_fast_cross_entropy\0") }.is_ok();
        if current {
            Ok(Self::ForceFused(
                *unsafe { library.get::<ForceFused>(b"mlx_fast_scaled_dot_product_attention\0") }
                    .map_err(|e| e.to_string())?,
            ))
        } else {
            Ok(Self::Legacy(
                *unsafe { library.get::<Legacy>(b"mlx_fast_scaled_dot_product_attention\0") }
                    .map_err(|e| e.to_string())?,
            ))
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn call(
        &self,
        out: *mut Handle,
        q: Handle,
        k: Handle,
        v: Handle,
        scale: f32,
        mode: *const c_char,
        mask: Handle,
        sinks: Handle,
        stream: Handle,
    ) -> c_int {
        match self {
            Self::Legacy(f) => unsafe { f(out, q, k, v, scale, mode, mask, sinks, stream) },
            Self::ForceFused(f) => unsafe {
                f(out, q, k, v, scale, mode, mask, sinks, false, stream)
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn legacy(
        out: *mut Handle,
        _q: Handle,
        _k: Handle,
        _v: Handle,
        _s: f32,
        _m: *const c_char,
        _mask: Handle,
        _sinks: Handle,
        stream: Handle,
    ) -> c_int {
        unsafe { *out = stream };
        17
    }
    unsafe extern "C" fn current(
        out: *mut Handle,
        q: Handle,
        k: Handle,
        v: Handle,
        s: f32,
        m: *const c_char,
        mask: Handle,
        sinks: Handle,
        force: bool,
        stream: Handle,
    ) -> c_int {
        assert!(!force);
        unsafe { legacy(out, q, k, v, s, m, mask, sinks, stream) }
    }
    #[test]
    fn both_attention_abis_preserve_stream_and_default_fusion() {
        let empty = Handle {
            ctx: std::ptr::null_mut(),
        };
        let mut token = 0u8;
        let stream = Handle {
            ctx: (&mut token as *mut u8).cast(),
        };
        for abi in [Attention::Legacy(legacy), Attention::ForceFused(current)] {
            let mut out = empty;
            assert_eq!(
                unsafe {
                    abi.call(
                        &mut out,
                        empty,
                        empty,
                        empty,
                        1.,
                        c"".as_ptr(),
                        empty,
                        empty,
                        stream,
                    )
                },
                17
            );
            assert_eq!(out.ctx, stream.ctx);
        }
    }
}
