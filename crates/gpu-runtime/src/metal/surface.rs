//! The Metal layer of a view: configuration, drawables and presentation.

use super::*;

impl MetalDevice {
    /// Returns or attaches a `CAMetalLayer` on `view_ptr`.
    ///
    /// # Safety
    ///
    /// When non-null, `view_ptr` must be a valid AppKit `NSView` (`is_appkit == true`)
    /// or UIKit `UIView` (`is_appkit == false`) pointer accessed on an allowed thread.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub unsafe fn ensure_metal_layer_for_view(
        view_ptr: *mut c_void,
        is_appkit: bool,
    ) -> Option<Arc<ObjCObject>> {
        if view_ptr.is_null() {
            return None;
        }
        let s = selectors();
        let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
        let f_set_obj: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) = msg_send();
        let f_set_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) = msg_send();
        let f_is_kind: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> bool =
            msg_send();
        let f_usize: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize = msg_send();
        let f_at_idx: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
            msg_send();
        let f_set_u64: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) = msg_send();
        let f_set_rect: unsafe extern "C" fn(*mut c_void, *mut c_void, CGRect) = msg_send();

        if is_appkit {
            f_set_bool(view_ptr, s.set_wants_layer, true);
        }
        let root_layer = f_obj(view_ptr, s.view_layer);
        if !root_layer.is_null() && f_is_kind(root_layer, s.is_kind_of_class, s.cls_ca_metal_layer)
        {
            return ObjCObject::from_unretained(root_layer).map(Arc::new);
        }

        if !root_layer.is_null() {
            let sublayers = f_obj(root_layer, s.sublayers);
            if !sublayers.is_null() {
                let count = f_usize(sublayers, s.count);
                for i in 0..count {
                    let child = f_at_idx(sublayers, s.object_at_index, i);
                    if !child.is_null()
                        && f_is_kind(child, s.is_kind_of_class, s.cls_ca_metal_layer)
                    {
                        return ObjCObject::from_unretained(child).map(Arc::new);
                    }
                }
            }
        }

        let new_layer = f_obj(s.cls_ca_metal_layer, s.layer_creator);
        if new_layer.is_null() {
            return None;
        }
        if is_appkit && root_layer.is_null() {
            f_set_obj(view_ptr, s.set_layer, new_layer);
            f_set_bool(view_ptr, s.set_wants_layer, true);
        } else if !root_layer.is_null() {
            let b = layer_bounds(root_layer);
            f_set_rect(new_layer, s.set_frame, b);
            // kCALayerWidthSizable (2) | kCALayerHeightSizable (16) = 18
            f_set_u64(new_layer, s.set_autoresizing_mask, 18);
            f_set_obj(root_layer, s.add_sublayer, new_layer);
        } else if is_appkit {
            f_set_obj(view_ptr, s.set_layer, new_layer);
        }
        ObjCObject::from_unretained(new_layer).map(Arc::new)
    }

    /// Configures `layer_ptr` with the surface format, presentation mode, size, and color space.
    ///
    /// # Safety
    ///
    /// When non-null, `layer_ptr` must point to a valid `CAMetalLayer` and `view_ptr`
    /// (if non-null) must point to the hosting `NSView`/`UIView`.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub unsafe fn configure_metal_layer(
        &self,
        layer_ptr: *mut c_void,
        view_ptr: *mut c_void,
        config: &crate::SurfaceConfiguration,
    ) {
        if layer_ptr.is_null() {
            return;
        }
        let s = selectors();
        let f_set_obj: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) = msg_send();
        let f_set_u64: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) = msg_send();
        let f_set_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) = msg_send();
        let f_set_size: unsafe extern "C" fn(*mut c_void, *mut c_void, CGSize) = msg_send();
        let f_set_rect: unsafe extern "C" fn(*mut c_void, *mut c_void, CGRect) = msg_send();
        let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();

        f_set_obj(layer_ptr, s.set_device, self.raw.as_ptr());
        f_set_u64(
            layer_ptr,
            s.set_pixel_format,
            config.format.to_mtl_pixel_format(),
        );
        let fb_only = config.usage == crate::TextureUsages::RENDER_ATTACHMENT;
        f_set_bool(layer_ptr, s.set_framebuffer_only, fb_only);
        let opaque = !matches!(
            config.alpha_mode,
            crate::CompositeAlphaMode::PreMultiplied | crate::CompositeAlphaMode::PostMultiplied
        );
        f_set_bool(layer_ptr, s.set_opaque, opaque);
        let max_drawables = config.desired_maximum_frame_latency.clamp(2, 3) as u64;
        f_set_u64(layer_ptr, s.set_maximum_drawable_count, max_drawables);
        #[cfg(target_os = "macos")]
        {
            let vsync = matches!(
                config.present_mode,
                crate::PresentMode::AutoVsync
                    | crate::PresentMode::Fifo
                    | crate::PresentMode::FifoRelaxed
            );
            f_set_bool(layer_ptr, s.set_display_sync_enabled, vsync);
            let f_set_f64: unsafe extern "C" fn(*mut c_void, *mut c_void, f64) = msg_send();
            // Set nextDrawableTimeout to 16ms so occluded/background windows return None quickly
            // instead of blocking the event loop for 1.0 second.
            let sel_timeout = sel_registerName(c"setNextDrawableTimeout:".as_ptr());
            let responds_sel = sel_registerName(c"respondsToSelector:".as_ptr());
            let f_responds: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> bool =
                msg_send();
            if f_responds(layer_ptr, responds_sel, sel_timeout) {
                f_set_f64(layer_ptr, sel_timeout, 0.016);
            }
        }
        if !view_ptr.is_null() {
            let root_layer = f_obj(view_ptr, s.view_layer);
            if !root_layer.is_null() && root_layer != layer_ptr {
                let b = layer_bounds(root_layer);
                f_set_rect(layer_ptr, s.set_frame, b);
            }
        }
        f_set_size(
            layer_ptr,
            s.set_drawable_size,
            CGSize {
                width: config.width.max(1) as f64,
                height: config.height.max(1) as f64,
            },
        );
        Self::configure_layer_color_space(layer_ptr, config);
    }

    /// Applies the configured color space; HDR spaces also request extended range output.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(super) unsafe fn configure_layer_color_space(
        layer_ptr: *mut c_void,
        config: &crate::SurfaceConfiguration,
    ) {
        use crate::SurfaceColorSpace as C;
        let space = config.color_space.resolve(config.format);
        let name = match space {
            C::Auto | C::Srgb => kCGColorSpaceSRGB,
            C::DisplayP3 => kCGColorSpaceDisplayP3,
            C::ExtendedSrgbLinear => kCGColorSpaceExtendedLinearSRGB,
            C::ExtendedSrgb => kCGColorSpaceExtendedSRGB,
            C::ExtendedDisplayP3 => kCGColorSpaceExtendedDisplayP3,
            C::Bt2100Pq => kCGColorSpaceITUR_2100_PQ,
            C::Bt2100Hlg => kCGColorSpaceITUR_2100_HLG,
        };
        let f_set_obj: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) = msg_send();
        let f_set_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) = msg_send();
        let color_space = CGColorSpaceCreateWithName(name);
        if !color_space.is_null() {
            f_set_obj(
                layer_ptr,
                sel_registerName(c"setColorspace:".as_ptr()),
                color_space,
            );
            CGColorSpaceRelease(color_space);
        }
        #[cfg(target_os = "macos")]
        f_set_bool(
            layer_ptr,
            sel_registerName(c"setWantsExtendedDynamicRangeContent:".as_ptr()),
            space.is_hdr(),
        );
        #[cfg(not(target_os = "macos"))]
        let _ = f_set_bool;
    }

    /// Acquires the next `CAMetalDrawable` and wraps its texture.
    ///
    /// # Safety
    ///
    /// When non-null, `layer_ptr` must point to a valid configured `CAMetalLayer`.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub unsafe fn acquire_drawable(
        &self,
        layer_ptr: *mut c_void,
        config: &crate::SurfaceConfiguration,
    ) -> Option<(ObjCObject, crate::Texture)> {
        if layer_ptr.is_null() {
            return None;
        }
        let s = selectors();
        let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
        let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
        let f_usize: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize = msg_send();

        let pool = f_obj(f_obj(s.cls_ns_autorelease_pool, s.alloc), s.init);
        let drawable_ptr = f_obj(layer_ptr, s.next_drawable);
        let Some(drawable) = ObjCObject::from_unretained(drawable_ptr) else {
            if !pool.is_null() {
                f_void(pool, s.drain);
            }
            return None;
        };
        let tex_ptr = f_obj(drawable.as_ptr(), s.drawable_texture);
        let Some(tex_raw) = ObjCObject::from_unretained(tex_ptr) else {
            if !pool.is_null() {
                f_void(pool, s.drain);
            }
            return None;
        };
        if !pool.is_null() {
            f_void(pool, s.drain);
        }
        let w = (f_usize(tex_raw.as_ptr(), s.width) as u32).max(1);
        let h = (f_usize(tex_raw.as_ptr(), s.height) as u32).max(1);
        let tex = crate::Texture {
            raw: tex_raw,
            size: crate::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: crate::TextureDimension::D2,
            format: config.format,
            usage: config.usage | crate::TextureUsages::RENDER_ATTACHMENT,
            storage_mode: 2,
        };
        Some((drawable, tex))
    }

    /// Presents `drawable_ptr`, honoring transactional presentation on `layer_ptr` when enabled.
    ///
    /// # Safety
    ///
    /// When non-null, `drawable_ptr` must point to a valid `CAMetalDrawable` and `layer_ptr`
    /// (if non-null) must point to its owning `CAMetalLayer`.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub unsafe fn present_drawable(&self, layer_ptr: *mut c_void, drawable_ptr: *mut c_void) {
        if drawable_ptr.is_null() {
            return;
        }
        let s = selectors();
        let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
        let f_bool: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool = msg_send();

        self.flush_pending_compute();
        let transactional = !layer_ptr.is_null() && f_bool(layer_ptr, s.presents_with_transaction);
        if let Ok(guard) = self.last_cmd_buf.lock() {
            if let Some((cmd_buf, _)) = guard.as_ref() {
                if transactional {
                    f_void(cmd_buf.as_ptr(), s.wait_until_scheduled);
                }
            }
        }
        f_void(drawable_ptr, s.present_drawable);
        if transactional {
            let cls_ca_tx = objc_getClass(c"CATransaction".as_ptr());
            let sel_flush = sel_registerName(c"flush".as_ptr());
            if !cls_ca_tx.is_null() {
                f_void(cls_ca_tx, sel_flush);
            }
        }
    }
}
