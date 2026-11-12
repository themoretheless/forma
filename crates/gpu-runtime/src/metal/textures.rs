//! Textures and samplers.

use super::*;

impl MetalDevice {
    pub fn create_texture(&self, desc: &crate::TextureDescriptor<'_>) -> crate::Texture {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        unsafe {
            let s = selectors();
            let f_td: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                u64,
                usize,
                usize,
                bool,
            ) -> *mut c_void = msg_send();
            let f_set_u64: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) = msg_send();
            let f_new_tex: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
            ) -> *mut c_void = msg_send();

            let td = f_td(
                s.cls_mtl_texture_descriptor,
                s.tex_desc_2d,
                desc.format.to_mtl_pixel_format(),
                desc.size.width.max(1) as usize,
                desc.size.height.max(1) as usize,
                desc.mip_level_count > 1,
            );
            f_set_u64(
                td,
                s.set_mipmap_level_count,
                desc.mip_level_count.max(1) as u64,
            );
            if desc.sample_count > 1 {
                f_set_u64(td, s.set_sample_count, desc.sample_count as u64);
                f_set_u64(td, s.set_texture_type, 4);
            } else if desc.dimension == crate::TextureDimension::D3 {
                f_set_u64(td, s.set_texture_type, 7);
                f_set_u64(
                    td,
                    s.set_depth,
                    desc.size.depth_or_array_layers.max(1) as u64,
                );
            } else if desc.size.depth_or_array_layers > 1 {
                f_set_u64(td, s.set_texture_type, 3);
                f_set_u64(
                    td,
                    s.set_array_length,
                    desc.size.depth_or_array_layers as u64,
                );
            }

            let mut mtl_usage = 0u64;
            if desc.usage.contains(crate::TextureUsages::TEXTURE_BINDING)
                || desc.usage.contains(crate::TextureUsages::COPY_SRC)
            {
                mtl_usage |= 1;
                // MTLTextureUsagePixelFormatView: lets a single-layer 2D texture be bound
                // through a 2D-array view (see `Texture::create_view`).
                if desc.dimension == crate::TextureDimension::D2
                    && desc.size.depth_or_array_layers == 1
                    && desc.sample_count <= 1
                {
                    mtl_usage |= 0x10;
                }
            }
            if desc.usage.contains(crate::TextureUsages::STORAGE_BINDING) {
                mtl_usage |= 2;
            }
            if desc.usage.contains(crate::TextureUsages::RENDER_ATTACHMENT) {
                mtl_usage |= 4;
            }
            f_set_u64(td, s.set_usage, mtl_usage.max(1));

            let is_depth = matches!(
                desc.format,
                crate::TextureFormat::Depth32Float | crate::TextureFormat::Depth24Plus
            );
            let storage_mode = if is_depth || desc.sample_count > 1 || !self.has_unified_memory {
                2u64
            } else {
                0u64
            };
            f_set_u64(td, s.set_storage_mode, storage_mode);

            let tex_ptr = f_new_tex(self.raw.as_ptr(), s.new_texture_with_desc, td);
            assert!(!tex_ptr.is_null(), "Failed to create MTLTexture");
            crate::Texture {
                raw: ObjCObject(tex_ptr),
                size: desc.size,
                mip_level_count: desc.mip_level_count,
                sample_count: desc.sample_count,
                dimension: desc.dimension,
                format: desc.format,
                usage: desc.usage,
                storage_mode,
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        unimplemented!("Metal textures are only available on macOS and iOS")
    }

    pub fn create_sampler(&self, desc: &crate::SamplerDescriptor<'_>) -> crate::Sampler {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        unsafe {
            let s = selectors();
            let f_alloc: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let f_init: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let f_set_u64: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) = msg_send();
            let f_new_samp: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
            ) -> *mut c_void = msg_send();

            let sd_ptr = f_init(f_alloc(s.cls_mtl_sampler_descriptor, s.alloc), s.init);
            let sd = ObjCObject(sd_ptr);
            f_set_u64(sd.as_ptr(), s.set_min_filter, desc.min_filter.to_mtl());
            f_set_u64(sd.as_ptr(), s.set_mag_filter, desc.mag_filter.to_mtl());
            let mip = match desc.mipmap_filter {
                crate::MipmapFilterMode::Nearest => 1u64,
                crate::MipmapFilterMode::Linear => 2u64,
            };
            f_set_u64(sd.as_ptr(), s.set_mip_filter, mip);
            f_set_u64(
                sd.as_ptr(),
                s.set_s_address_mode,
                desc.address_mode_u.to_mtl(),
            );
            f_set_u64(
                sd.as_ptr(),
                s.set_t_address_mode,
                desc.address_mode_v.to_mtl(),
            );
            f_set_u64(
                sd.as_ptr(),
                s.set_r_address_mode,
                desc.address_mode_w.to_mtl(),
            );
            if let Some(cmp) = desc.compare {
                f_set_u64(sd.as_ptr(), s.set_compare_function, cmp.to_mtl());
            }

            let samp_ptr = f_new_samp(self.raw.as_ptr(), s.new_sampler_state, sd.as_ptr());
            crate::Sampler {
                raw: Arc::new(ObjCObject(samp_ptr)),
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        unimplemented!("Metal samplers are only available on macOS and iOS")
    }

    pub fn write_texture(
        &self,
        texture: crate::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: crate::TexelCopyBufferLayout,
        size: crate::Extent3d,
    ) {
        if data.is_empty() || size.width == 0 || size.height == 0 {
            return;
        }
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        unsafe {
            let s = selectors();
            let bpr = data_layout.row_stride(size.width, texture.texture.format) as usize;
            let bpi = data_layout
                .rows_per_image
                .map(|r| r as usize * bpr)
                .unwrap_or(size.height as usize * bpr);
            let src_ptr = data.as_ptr().add(data_layout.offset as usize) as *const c_void;

            if texture.texture.storage_mode == 0 {
                let f_rep: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    MTLRegion,
                    usize,
                    usize,
                    *const c_void,
                    usize,
                    usize,
                ) = msg_send();
                let slices = crate::texture::CopySlices::new(
                    texture.texture.dimension,
                    texture.origin.z,
                    size.depth_or_array_layers,
                );
                for slice_idx in 0..slices.count {
                    let region = MTLRegion {
                        origin: MTLOrigin {
                            x: texture.origin.x as usize,
                            y: texture.origin.y as usize,
                            z: slices.region_z(),
                        },
                        size: MTLSize {
                            width: size.width as usize,
                            height: size.height as usize,
                            depth: slices.depth,
                        },
                    };
                    let slice_ptr = (src_ptr as *const u8).add(slice_idx * bpi) as *const c_void;
                    f_rep(
                        texture.texture.raw.as_ptr(),
                        s.replace_region,
                        region,
                        texture.mip_level as usize,
                        slices.array_slice(slice_idx),
                        slice_ptr,
                        bpr,
                        bpi,
                    );
                }
            }
        }
    }
}
