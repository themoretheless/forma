//! Encoding recorded commands into Metal command encoders.

use crate::Extent3d;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod blit;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod compute;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod render;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use blit::encode_non_compute_op;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use compute::encode_compute_dispatches;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use render::encode_render_pass;

#[cfg(any(target_os = "macos", target_os = "ios"))]
type MsgSetU64 = unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, u64);
#[cfg(any(target_os = "macos", target_os = "ios"))]
type MsgSetObj =
    unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void);
#[cfg(any(target_os = "macos", target_os = "ios"))]
type MsgSetAtIndex = unsafe extern "C" fn(
    *mut std::ffi::c_void,
    *mut std::ffi::c_void,
    *mut std::ffi::c_void,
    usize,
);
#[cfg(any(target_os = "macos", target_os = "ios"))]
type MsgSetBuffer = unsafe extern "C" fn(
    *mut std::ffi::c_void,
    *mut std::ffi::c_void,
    *mut std::ffi::c_void,
    usize,
    usize,
);

/// Metal row and image strides for a buffer<->texture copy of one image.
///
/// WebGPU only requires the bytes a copy touches to be in bounds, while Metal validates
/// `offset + bytesPerImage` against the buffer length. A padded row stride on the last (or
/// only) row would therefore reject copies that WebGPU accepts, so the image stride is
/// trimmed to the bytes actually read, and a single row uses its packed width.
pub(in crate::command) fn metal_copy_strides(
    bytes_per_row: u32,
    rows_per_image: Option<u32>,
    size: Extent3d,
    format: crate::TextureFormat,
    multi_image: bool,
) -> (usize, usize) {
    let packed_row = size.width as usize * format.bytes_per_pixel() as usize;
    let mut bpr = bytes_per_row as usize;
    if size.height <= 1 && packed_row != 0 {
        bpr = bpr.min(packed_row);
    }
    let rows = size.height.max(1) as usize;
    let bpi = if multi_image {
        rows_per_image.unwrap_or(size.height) as usize * bpr
    } else {
        (rows - 1) * bpr + packed_row.max(1).min(bpr)
    };
    (bpr, bpi)
}
