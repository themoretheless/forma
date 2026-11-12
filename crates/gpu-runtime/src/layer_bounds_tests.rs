//! A layer's bounds come back intact on every architecture.
use crate::metal::{CGPoint, CGRect, CGSize, ObjCObject, layer_bounds, objc_msgSend, selectors};
use std::ffi::c_void;

#[test]
fn layer_bounds_round_trip_through_the_objective_c_runtime() {
    // SAFETY: `alloc`/`init` on the CAMetalLayer class return an owned layer, which `ObjCObject`
    // releases; `setFrame:` takes a `CGRect` by value.
    unsafe {
        let s = selectors();
        let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const c_void);
        let set_frame: unsafe extern "C" fn(*mut c_void, *mut c_void, CGRect) =
            std::mem::transmute(objc_msgSend as *const c_void);
        let layer = ObjCObject(send(send(s.cls_ca_metal_layer, s.alloc), s.init));
        assert!(!layer.as_ptr().is_null());
        set_frame(
            layer.as_ptr(),
            s.set_frame,
            CGRect {
                origin: CGPoint { x: 11.0, y: 22.0 },
                size: CGSize {
                    width: 640.0,
                    height: 360.0,
                },
            },
        );
        let bounds = layer_bounds(layer.as_ptr());
        assert_eq!(
            (
                bounds.origin.x,
                bounds.origin.y,
                bounds.size.width,
                bounds.size.height
            ),
            (0.0, 0.0, 640.0, 360.0)
        );
    }
}
