//! AppKit presentation policy for the Metal sublayer owned by this window.
//! wgpu's default resize gravity stretches the previous drawable during live
//! resize. Anchor its contents top-left and disable implicit geometry animations.
// objc 0.2 macros use a legacy cargo-clippy cfg; scope compatibility to this shim.
#![allow(unexpected_cfgs)]
use objc::{
    class, msg_send,
    runtime::{Object, BOOL, YES},
    sel, sel_impl,
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

#[link(name = "QuartzCore", kind = "framework")]
extern "C" {
    static kCAGravityTopLeft: *mut Object;
}

pub fn configure(window: &Window) -> Result<(), String> {
    let RawWindowHandle::AppKit(handle) =
        window.window_handle().map_err(|e| e.to_string())?.as_raw()
    else {
        return Ok(());
    };
    // SAFETY: called on winit's main thread; Window outlives the NSView and its
    // layers. We only touch its CAMetalLayer, never a global/root app policy.
    // All Objective-C objects are retained by their owning view/layer or by the
    // dictionary property before this autorelease pool drains.
    unsafe {
        let view = handle.ns_view.as_ptr().cast::<Object>();
        let root: *mut Object = msg_send![view, layer];
        if root.is_null() {
            return Err("Metal root layer missing".into());
        }
        let mut layer = root;
        let is_metal: BOOL = msg_send![root,isKindOfClass:class!(CAMetalLayer)];
        if is_metal != YES {
            layer = std::ptr::null_mut();
            let children: *mut Object = msg_send![root, sublayers];
            let count: usize = msg_send![children, count];
            for i in 0..count {
                let child: *mut Object = msg_send![children,objectAtIndex:i];
                let is_metal: BOOL = msg_send![child,isKindOfClass:class!(CAMetalLayer)];
                if is_metal == YES {
                    layer = child;
                    break;
                }
            }
        }
        if layer.is_null() {
            return Err("Window Metal layer missing".into());
        }
        let names = [
            c"bounds",
            c"position",
            c"contents",
            c"contentsScale",
            c"drawableSize",
        ];
        let keys: Vec<*mut Object> = names
            .iter()
            .map(|name| msg_send![class!(NSString),stringWithUTF8String:name.as_ptr()])
            .collect();
        let null: *mut Object = msg_send![class!(NSNull), null];
        let values = vec![null; keys.len()];
        let actions: *mut Object = msg_send![class!(NSDictionary),dictionaryWithObjects:values.as_ptr() forKeys:keys.as_ptr() count:keys.len()];
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction),setDisableActions:YES];
        let _: () = msg_send![layer,setActions:actions];
        let _: () = msg_send![layer,setContentsGravity:kCAGravityTopLeft];
        // wgpu 29 reads this flag when acquiring the drawable and uses the
        // transaction-aware present path (waitUntilScheduled + drawable.present).
        let _: () = msg_send![layer,setPresentsWithTransaction:YES];
        let _: () = msg_send![class!(CATransaction), commit];
        let gravity: *mut Object = msg_send![layer, contentsGravity];
        let correct: BOOL = msg_send![gravity,isEqual:kCAGravityTopLeft];
        if correct != YES {
            return Err("Metal contents gravity was not applied".into());
        }
        let transactional: BOOL = msg_send![layer, presentsWithTransaction];
        if transactional != YES {
            return Err("Metal transactional presentation was not applied".into());
        }
    }
    println!("Presentation: Metal topLeft + synchronized transaction; implicit resize animations disabled");
    Ok(())
}
