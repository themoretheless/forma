//! Encoding recorded commands into Metal command encoders.

use super::*;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) unsafe fn encode_compute_dispatches(enc: *mut c_void, dispatches: &[ComputeDispatchOp]) {
    let s = metal_selectors();
    let f_set_pso: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) = msg_send();
    let f_set_buf: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize, usize) =
        msg_send();
    let f_set_bytes: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, usize, usize) =
        msg_send();
    let f_set_tex: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize) = msg_send();
    let f_set_samp: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize) = msg_send();
    let f_dispatch: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLSize, MTLSize) = msg_send();

    let mut last_pso: *mut c_void = ptr::null_mut();
    for d in dispatches {
        let Some(pso) = &d.pipeline.pso else {
            continue;
        };
        let pso_ptr = pso.as_ptr();
        if pso_ptr != last_pso {
            f_set_pso(enc, s.set_compute_pipeline_state, pso_ptr);
            last_pso = pso_ptr;
        }
        let mut buffer_sizes = [0u32; 32];
        let any_buf_sizes = !d.pipeline.bindings.array_length_slots.is_empty();

        for (group_idx, bg_opt, dyn_offsets) in &d.bind_groups {
            let Some(bg) = bg_opt else { continue };
            for (b_idx, (binding, buf, base_off, byte_size)) in bg.buffers.iter().enumerate() {
                let slot = msl_buffer_slot(*group_idx, *binding) as usize;
                let dyn_off = dyn_offsets.get(b_idx).unwrap_or(0) as u64;
                let final_off = (*base_off + dyn_off) as usize;
                f_set_buf(enc, s.set_buffer, buf.inner.raw.as_ptr(), final_off, slot);
                if slot < 32 {
                    let rem = (*byte_size).saturating_sub(dyn_off);
                    buffer_sizes[slot] = rem as u32;
                }
            }
            let map = &d.pipeline.bindings;
            for (binding, tex) in bg.textures.iter() {
                if let Some(&slot) = map.textures.get(&(*group_idx, *binding)) {
                    f_set_tex(enc, s.set_texture, tex.raw.as_ptr(), slot as usize);
                }
            }
            for (binding, samp) in bg.samplers.iter() {
                if let Some(&slot) = map.samplers.get(&(*group_idx, *binding)) {
                    f_set_samp(enc, s.set_sampler_state, samp.raw.as_ptr(), slot as usize);
                }
            }
        }

        if any_buf_sizes {
            f_set_bytes(
                enc,
                s.set_bytes,
                buffer_sizes.as_ptr() as *const c_void,
                std::mem::size_of_val(&buffer_sizes),
                MSL_ARRAY_LENGTHS_BUFFER_SLOT as usize,
            );
        }

        if !d.immediates.is_empty() {
            f_set_bytes(
                enc,
                s.set_bytes,
                d.immediates.as_ptr() as *const c_void,
                d.immediates.len(),
                MSL_IMMEDIATE_BUFFER_SLOT as usize,
            );
        }

        let grid = MTLSize {
            width: d.groups[0].max(1) as usize,
            height: d.groups[1].max(1) as usize,
            depth: d.groups[2].max(1) as usize,
        };
        let tg = MTLSize {
            width: d.pipeline.workgroup_size[0].max(1) as usize,
            height: d.pipeline.workgroup_size[1].max(1) as usize,
            depth: d.pipeline.workgroup_size[2].max(1) as usize,
        };
        f_dispatch(enc, s.dispatch_threadgroups, grid, tg);
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) unsafe fn encode_non_compute_op(
    _metal: &MetalDevice,
    cmd_buf: *mut c_void,
    op: RecordedOp,
) {
    match op {
        RecordedOp::CopyBufferToBuffer {
            src,
            src_offset,
            dst,
            dst_offset,
            size,
        } => encode_buffer_copy(cmd_buf, &src, src_offset, &dst, dst_offset, size),
        RecordedOp::ClearBuffer { dst, offset, size } => {
            encode_buffer_clear(cmd_buf, &dst, offset, size)
        }
        RecordedOp::CopyTextureToBuffer {
            src_tex,
            mip_level,
            origin,
            dst_buf,
            dst_offset,
            bytes_per_row,
            rows_per_image,
            size,
        } => encode_texture_to_buffer(
            cmd_buf,
            TextureRegion {
                texture: &src_tex,
                mip_level,
                origin,
            },
            BufferRows {
                buffer: &dst_buf,
                offset: dst_offset,
                bytes_per_row,
                rows_per_image,
            },
            size,
        ),
        RecordedOp::CopyBufferToTexture {
            src_buf,
            src_offset,
            bytes_per_row,
            rows_per_image,
            dst_tex,
            dst_mip,
            dst_origin,
            size,
        } => encode_buffer_to_texture(
            cmd_buf,
            BufferRows {
                buffer: &src_buf,
                offset: src_offset,
                bytes_per_row,
                rows_per_image,
            },
            TextureRegion {
                texture: &dst_tex,
                mip_level: dst_mip,
                origin: dst_origin,
            },
            size,
        ),
        RecordedOp::CopyTextureToTexture {
            src_tex,
            src_mip,
            src_origin,
            dst_tex,
            dst_mip,
            dst_origin,
            size,
        } => encode_texture_to_texture(
            cmd_buf,
            TextureRegion {
                texture: &src_tex,
                mip_level: src_mip,
                origin: src_origin,
            },
            TextureRegion {
                texture: &dst_tex,
                mip_level: dst_mip,
                origin: dst_origin,
            },
            size,
        ),
        RecordedOp::WriteTimestamp { query_set, index } => record_timestamp(&query_set, index),
        RecordedOp::ResolveQuerySet {
            query_set,
            first_query,
            query_count,
            destination,
            destination_offset,
        } => resolve_timestamps(
            &query_set,
            first_query,
            query_count,
            &destination,
            destination_offset,
        ),
        RecordedOp::RenderPass(rp) => {
            encode_render_pass(_metal, cmd_buf, rp);
        }
        RecordedOp::ComputePass { .. } => {}
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
/// The corner of a copy inside one mip level of a texture.
struct TextureRegion<'a> {
    texture: &'a Texture,
    mip_level: u32,
    origin: Origin3d,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
/// Where the rows of a texture copy live in a buffer.
struct BufferRows<'a> {
    buffer: &'a Buffer,
    offset: BufferAddress,
    bytes_per_row: u32,
    rows_per_image: Option<u32>,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn encode_buffer_copy(
    cmd_buf: *mut c_void,
    src: &Buffer,
    src_offset: BufferAddress,
    dst: &Buffer,
    dst_offset: BufferAddress,
    size: BufferAddress,
) {
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    if size == 0 {
        return;
    }
    let blit = f_obj(cmd_buf, s.blit_command_encoder);
    let f_copy: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        usize,
        *mut c_void,
        usize,
        usize,
    ) = msg_send();
    f_copy(
        blit,
        s.copy_from_buffer,
        src.inner.raw.as_ptr(),
        src_offset as usize,
        dst.inner.raw.as_ptr(),
        dst_offset as usize,
        size as usize,
    );
    f_void(blit, s.end_encoding);
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn encode_buffer_clear(
    cmd_buf: *mut c_void,
    dst: &Buffer,
    offset: BufferAddress,
    size: BufferAddress,
) {
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    if size == 0 {
        return;
    }
    let blit = f_obj(cmd_buf, s.blit_command_encoder);
    let f_fill: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, NSRange, u8) =
        msg_send();
    f_fill(
        blit,
        s.fill_buffer,
        dst.inner.raw.as_ptr(),
        NSRange {
            location: offset as usize,
            length: size as usize,
        },
        0,
    );
    f_void(blit, s.end_encoding);
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn encode_texture_to_buffer(
    cmd_buf: *mut c_void,
    source: TextureRegion<'_>,
    destination: BufferRows<'_>,
    size: Extent3d,
) {
    let TextureRegion {
        texture: src_tex,
        mip_level,
        origin,
    } = source;
    let BufferRows {
        buffer: dst_buf,
        offset: dst_offset,
        bytes_per_row,
        rows_per_image,
    } = destination;
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    let blit = f_obj(cmd_buf, s.blit_command_encoder);
    let f_t2b: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        usize,
        usize,
        MTLOrigin,
        MTLSize,
        *mut c_void,
        usize,
        usize,
        usize,
    ) = msg_send();
    let (bpr, bpi) = metal_copy_strides(bytes_per_row, rows_per_image, size, src_tex.format, false);
    f_t2b(
        blit,
        s.copy_from_texture_to_buffer,
        src_tex.raw.as_ptr(),
        origin.z as usize,
        mip_level as usize,
        MTLOrigin {
            x: origin.x as usize,
            y: origin.y as usize,
            z: 0,
        },
        MTLSize {
            width: size.width as usize,
            height: size.height as usize,
            depth: 1,
        },
        dst_buf.inner.raw.as_ptr(),
        dst_offset as usize,
        bpr,
        bpi,
    );
    f_void(blit, s.end_encoding);
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn encode_buffer_to_texture(
    cmd_buf: *mut c_void,
    source: BufferRows<'_>,
    destination: TextureRegion<'_>,
    size: Extent3d,
) {
    let BufferRows {
        buffer: src_buf,
        offset: src_offset,
        bytes_per_row,
        rows_per_image,
    } = source;
    let TextureRegion {
        texture: dst_tex,
        mip_level: dst_mip,
        origin: dst_origin,
    } = destination;
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    let blit = f_obj(cmd_buf, s.blit_command_encoder);
    let f_b2t: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        usize,
        usize,
        usize,
        MTLSize,
        *mut c_void,
        usize,
        usize,
        MTLOrigin,
    ) = msg_send();
    let slices = crate::texture::CopySlices::new(
        dst_tex.dimension,
        dst_origin.z,
        size.depth_or_array_layers,
    );
    let (bpr, bpi) = metal_copy_strides(
        bytes_per_row,
        rows_per_image,
        size,
        dst_tex.format,
        slices.is_3d() || size.depth_or_array_layers > 1,
    );
    for slice in 0..slices.count {
        f_b2t(
            blit,
            s.copy_from_buffer_to_texture,
            src_buf.inner.raw.as_ptr(),
            src_offset as usize + slice * bpi,
            bpr,
            bpi,
            MTLSize {
                width: size.width as usize,
                height: size.height as usize,
                depth: slices.depth,
            },
            dst_tex.raw.as_ptr(),
            slices.array_slice(slice),
            dst_mip as usize,
            MTLOrigin {
                x: dst_origin.x as usize,
                y: dst_origin.y as usize,
                z: slices.region_z(),
            },
        );
    }
    f_void(blit, s.end_encoding);
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn encode_texture_to_texture(
    cmd_buf: *mut c_void,
    source: TextureRegion<'_>,
    destination: TextureRegion<'_>,
    size: Extent3d,
) {
    let TextureRegion {
        texture: src_tex,
        mip_level: src_mip,
        origin: src_origin,
    } = source;
    let TextureRegion {
        texture: dst_tex,
        mip_level: dst_mip,
        origin: dst_origin,
    } = destination;
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    let blit = f_obj(cmd_buf, s.blit_command_encoder);
    let f_t2t: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        usize,
        usize,
        MTLOrigin,
        MTLSize,
        *mut c_void,
        usize,
        usize,
        MTLOrigin,
    ) = msg_send();
    let slices = size.depth_or_array_layers.max(1) as usize;
    for slice_i in 0..slices {
        f_t2t(
            blit,
            s.copy_from_texture_to_texture,
            src_tex.raw.as_ptr(),
            src_origin.z as usize + slice_i,
            src_mip as usize,
            MTLOrigin {
                x: src_origin.x as usize,
                y: src_origin.y as usize,
                z: 0,
            },
            MTLSize {
                width: size.width as usize,
                height: size.height as usize,
                depth: 1,
            },
            dst_tex.raw.as_ptr(),
            dst_origin.z as usize + slice_i,
            dst_mip as usize,
            MTLOrigin {
                x: dst_origin.x as usize,
                y: dst_origin.y as usize,
                z: 0,
            },
        );
    }
    f_void(blit, s.end_encoding);
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
/// Timestamps are taken on the CPU when the command is encoded.
fn record_timestamp(query_set: &QuerySet, index: u32) {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    if let Ok(mut ts) = query_set.timestamps_ns.lock() {
        if (index as usize) < ts.len() {
            ts[index as usize] = now_ns;
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn resolve_timestamps(
    query_set: &QuerySet,
    first_query: u32,
    query_count: u32,
    destination: &Buffer,
    destination_offset: BufferAddress,
) {
    if let Ok(ts) = query_set.timestamps_ns.lock() {
        let start = first_query as usize;
        let end = (start + query_count as usize).min(ts.len());
        if start < end && !destination.inner.ptr.is_null() {
            let slice = &ts[start..end];
            let bytes = slice.len() * 8;
            // `resolve_query_set` refuses a range that does not fit; a query set shorter
            // than the range only makes the write smaller.
            let fits = usize::try_from(destination_offset)
                .ok()
                .and_then(|offset| offset.checked_add(bytes).map(|end| (offset, end)))
                .filter(|&(_, end)| end as u64 <= destination.inner.size);
            if let Some((offset, _)) = fits {
                // SAFETY: `offset + bytes` is inside the destination allocation, and the
                // timestamps are a separate allocation.
                ptr::copy_nonoverlapping(
                    slice.as_ptr() as *const u8,
                    destination.inner.ptr.add(offset),
                    bytes,
                );
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
type MsgSetU64 = unsafe extern "C" fn(*mut c_void, *mut c_void, u64);
#[cfg(any(target_os = "macos", target_os = "ios"))]
type MsgSetObj = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void);
#[cfg(any(target_os = "macos", target_os = "ios"))]
type MsgSetAtIndex = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize);
#[cfg(any(target_os = "macos", target_os = "ios"))]
type MsgSetBuffer = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize, usize);

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) unsafe fn encode_render_pass(
    metal: &MetalDevice,
    cmd_buf: *mut c_void,
    mut rp: RenderPassOp,
) {
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();

    let rpd = f_obj(s.cls_mtl_render_pass_descriptor, s.render_pass_descriptor);
    let owned_color_atts = std::mem::take(&mut rp.color_attachments);
    configure_color_attachments(rpd, &owned_color_atts);
    metal.recycle_color_atts(owned_color_atts);
    if let Some(dsa) = &rp.depth_stencil_attachment {
        configure_depth_attachment(rpd, dsa);
    }

    let mut commands = std::mem::take(&mut rp.commands);
    let f_rce: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
        msg_send();
    let enc = f_rce(cmd_buf, s.render_command_encoder_with_desc, rpd);
    if enc.is_null() {
        metal.recycle_render_cmds(commands);
        return;
    }

    let mut pass = RenderPassEncoder {
        enc,
        s,
        primitive_type: 3, // MTLPrimitiveTypeTriangle
        index_buffer: None,
        pipeline: None,
        bound_groups: Default::default(),
        immediates: Vec::new(),
    };
    for cmd in commands.drain(..) {
        pass.encode(cmd);
    }

    metal.recycle_render_cmds(commands);
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    f_void(enc, s.end_encoding);
}

/// Texture, load action and store action of each colour attachment.
#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn configure_color_attachments(
    rpd: *mut c_void,
    attachments: &[Option<OwnedColorAttachment>],
) {
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_sub: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void = msg_send();
    let f_set_obj: MsgSetObj = msg_send();
    let f_set_u64: MsgSetU64 = msg_send();
    let f_set_cc: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLClearColor) = msg_send();
    let color_atts = f_obj(rpd, s.color_attachments);
    for (i, ca_opt) in attachments.iter().enumerate() {
        let Some(ca) = ca_opt else { continue };
        let att = f_sub(color_atts, s.object_at_indexed_subscript, i);
        f_set_obj(att, s.set_attachment_texture, ca.view.texture.raw.as_ptr());
        f_set_u64(att, s.set_level, ca.view.base_mip_level as u64);
        f_set_u64(att, s.set_slice, ca.view.base_array_layer as u64);
        match ca.ops.load {
            LoadOp::Clear(c) => {
                f_set_u64(att, s.set_load_action, 2); // MTLLoadActionClear
                f_set_cc(
                    att,
                    s.set_clear_color,
                    MTLClearColor {
                        red: c.r,
                        green: c.g,
                        blue: c.b,
                        alpha: c.a,
                    },
                );
            }
            LoadOp::Load => {
                f_set_u64(att, s.set_load_action, 1); // MTLLoadActionLoad
            }
        }
        f_set_u64(att, s.set_store_action, store_action(ca.ops.store));
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn configure_depth_attachment(rpd: *mut c_void, dsa: &OwnedDepthAttachment) {
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_set_obj: MsgSetObj = msg_send();
    let f_set_u64: MsgSetU64 = msg_send();
    let f_set_f64: unsafe extern "C" fn(*mut c_void, *mut c_void, f64) = msg_send();
    let d_att = f_obj(rpd, s.depth_attachment);
    f_set_obj(
        d_att,
        s.set_attachment_texture,
        dsa.view.texture.raw.as_ptr(),
    );
    f_set_u64(d_att, s.set_level, dsa.view.base_mip_level as u64);
    f_set_u64(d_att, s.set_slice, dsa.view.base_array_layer as u64);
    let Some(ops) = &dsa.depth_ops else { return };
    match ops.load {
        LoadOp::Clear(d) => {
            f_set_u64(d_att, s.set_load_action, 2);
            f_set_f64(d_att, s.set_clear_depth, d as f64);
        }
        LoadOp::Load => {
            f_set_u64(d_att, s.set_load_action, 1);
        }
    }
    f_set_u64(d_att, s.set_store_action, store_action(ops.store));
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn store_action(store: StoreOp) -> u64 {
    match store {
        StoreOp::Store => 1,   // MTLStoreActionStore
        StoreOp::Discard => 0, // MTLStoreActionDontCare
    }
}

/// The state a render pass carries from one command to the next.
#[cfg(any(target_os = "macos", target_os = "ios"))]
struct RenderPassEncoder {
    enc: *mut c_void,
    s: &'static crate::metal::Selectors,
    primitive_type: u64,
    index_buffer: Option<(Buffer, IndexFormat, u64)>,
    /// Texture and sampler slots belong to each stage's shader, so they are bound against the
    /// current pipeline; groups set before a pipeline are bound when it arrives.
    pipeline: Option<RenderPipeline>,
    bound_groups: std::collections::BTreeMap<u32, BindGroup>,
    /// The immediate data set so far in this pass.
    immediates: Vec<u8>,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl RenderPassEncoder {
    unsafe fn encode(&mut self, cmd: RenderCommand) {
        let (enc, s) = (self.enc, self.s);
        match cmd {
            RenderCommand::SetPipeline(pipe) => self.set_pipeline(pipe),
            RenderCommand::SetBindGroup {
                index,
                bind_group,
                offsets,
            } => self.set_bind_group(index, bind_group, &offsets),
            RenderCommand::SetVertexBuffer {
                slot,
                buffer,
                offset,
            } => {
                let f_set_buf: MsgSetBuffer = msg_send();
                let mtl_slot = (MSL_VERTEX_BUFFER_BASE_SLOT + slot) as usize;
                f_set_buf(
                    enc,
                    s.set_vertex_buffer,
                    buffer.inner.raw.as_ptr(),
                    offset as usize,
                    mtl_slot,
                );
            }
            RenderCommand::SetIndexBuffer {
                buffer,
                format,
                offset,
            } => {
                self.index_buffer = Some((buffer, format, offset));
            }
            RenderCommand::SetImmediates { offset, data } => {
                // Metal replaces the whole block, so the pieces set so far are kept here.
                let start = offset as usize;
                let end = start + data.len();
                if self.immediates.len() < end {
                    self.immediates.resize(end, 0);
                }
                self.immediates[start..end].copy_from_slice(&data);
                let data = &self.immediates;
                let f_set_bytes: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *const c_void,
                    usize,
                    usize,
                ) = msg_send();
                let slot = MSL_IMMEDIATE_BUFFER_SLOT as usize;
                for selector in [s.set_vertex_bytes, s.set_fragment_bytes] {
                    f_set_bytes(
                        enc,
                        selector,
                        data.as_ptr() as *const c_void,
                        data.len(),
                        slot,
                    );
                }
            }
            RenderCommand::SetViewport {
                x,
                y,
                w,
                h,
                min_depth,
                max_depth,
            } => {
                let f_set_vp: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLViewport) =
                    msg_send();
                f_set_vp(
                    enc,
                    s.set_viewport,
                    MTLViewport {
                        origin_x: x as f64,
                        origin_y: y as f64,
                        width: w as f64,
                        height: h as f64,
                        znear: min_depth as f64,
                        zfar: max_depth as f64,
                    },
                );
            }
            RenderCommand::SetScissorRect { x, y, w, h } => {
                let f_set_sc: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLScissorRect) =
                    msg_send();
                f_set_sc(
                    enc,
                    s.set_scissor_rect,
                    MTLScissorRect {
                        x: x as usize,
                        y: y as usize,
                        width: w as usize,
                        height: h as usize,
                    },
                );
            }
            RenderCommand::Draw {
                vertices,
                instances,
            } => self.draw(vertices, instances),
            RenderCommand::DrawIndexed {
                indices,
                base_vertex,
                instances,
            } => self.draw_indexed(indices, base_vertex, instances),
        }
    }

    /// Binds the textures and samplers of `bind_group` to the slots `pipe` gives them.
    unsafe fn bind_handles(&self, index: u32, bind_group: &BindGroup, pipe: &RenderPipeline) {
        let (enc, s) = (self.enc, self.s);
        let f_set: MsgSetAtIndex = msg_send();
        let fragment = pipe.fragment_bindings.as_ref();
        for (binding, tex) in bind_group.textures.iter() {
            if let Some(&slot) = pipe.vertex_bindings.textures.get(&(index, *binding)) {
                f_set(enc, s.set_vertex_texture, tex.raw.as_ptr(), slot as usize);
            }
            if let Some(&slot) = fragment.and_then(|map| map.textures.get(&(index, *binding))) {
                f_set(enc, s.set_fragment_texture, tex.raw.as_ptr(), slot as usize);
            }
        }
        for (binding, samp) in bind_group.samplers.iter() {
            if let Some(&slot) = pipe.vertex_bindings.samplers.get(&(index, *binding)) {
                f_set(
                    enc,
                    s.set_vertex_sampler_state,
                    samp.raw.as_ptr(),
                    slot as usize,
                );
            }
            if let Some(&slot) = fragment.and_then(|map| map.samplers.get(&(index, *binding))) {
                f_set(
                    enc,
                    s.set_fragment_sampler_state,
                    samp.raw.as_ptr(),
                    slot as usize,
                );
            }
        }
    }

    unsafe fn set_pipeline(&mut self, pipe: RenderPipeline) {
        let (enc, s) = (self.enc, self.s);
        let Some(pso) = &pipe.pso else {
            self.pipeline = None;
            return;
        };
        for (index, bind_group) in &self.bound_groups {
            self.bind_handles(*index, bind_group, &pipe);
        }
        let f_set_obj: MsgSetObj = msg_send();
        let f_set_u64: MsgSetU64 = msg_send();
        f_set_obj(enc, s.set_render_pipeline_state, pso.as_ptr());
        if let Some(dss) = &pipe.depth_stencil_state {
            f_set_obj(enc, s.set_depth_stencil_state, dss.as_ptr());
        }
        self.primitive_type = pipe.primitive.topology.to_mtl();
        let winding = match pipe.primitive.front_face {
            FrontFace::Cw => 0u64,  // MTLWindingClockwise
            FrontFace::Ccw => 1u64, // MTLWindingCounterClockwise
        };
        f_set_u64(enc, s.set_front_facing_winding, winding);
        let cull = match pipe.primitive.cull_mode {
            None => 0u64,              // MTLCullModeNone
            Some(Face::Front) => 1u64, // MTLCullModeFront
            Some(Face::Back) => 2u64,  // MTLCullModeBack
        };
        f_set_u64(enc, s.set_cull_mode, cull);
        if pipe.depth_bias.constant != 0 || pipe.depth_bias.slope_scale != 0.0 {
            let f_set_db: unsafe extern "C" fn(*mut c_void, *mut c_void, f32, f32, f32) =
                msg_send();
            f_set_db(
                enc,
                s.set_depth_bias,
                pipe.depth_bias.constant as f32,
                pipe.depth_bias.slope_scale,
                pipe.depth_bias.clamp,
            );
        }
        self.pipeline = Some(pipe);
    }

    unsafe fn set_bind_group(
        &mut self,
        index: u32,
        bind_group: BindGroup,
        offsets: &DynamicOffsets,
    ) {
        let (enc, s) = (self.enc, self.s);
        let f_set_buf: MsgSetBuffer = msg_send();
        for (b_i, (binding, buf, base_off, _)) in bind_group.buffers.iter().enumerate() {
            let slot = msl_buffer_slot(index, *binding) as usize;
            let dyn_off = offsets.get(b_i).unwrap_or(0) as u64;
            let final_off = (*base_off + dyn_off) as usize;
            for selector in [s.set_vertex_buffer, s.set_fragment_buffer] {
                f_set_buf(enc, selector, buf.inner.raw.as_ptr(), final_off, slot);
            }
        }
        if let Some(pipe) = &self.pipeline {
            self.bind_handles(index, &bind_group, pipe);
        }
        self.bound_groups.insert(index, bind_group);
    }

    unsafe fn draw(&self, vertices: std::ops::Range<u32>, instances: std::ops::Range<u32>) {
        let v_count = vertices.end.saturating_sub(vertices.start) as usize;
        let i_count = instances.end.saturating_sub(instances.start) as usize;
        if v_count == 0 || i_count == 0 || self.pipeline.is_none() {
            return;
        }
        let f_draw: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            u64,
            usize,
            usize,
            usize,
            usize,
        ) = msg_send();
        f_draw(
            self.enc,
            self.s.draw_primitives,
            self.primitive_type,
            vertices.start as usize,
            v_count,
            i_count,
            instances.start as usize,
        );
    }

    unsafe fn draw_indexed(
        &self,
        indices: std::ops::Range<u32>,
        base_vertex: i32,
        instances: std::ops::Range<u32>,
    ) {
        let idx_count = indices.end.saturating_sub(indices.start) as usize;
        let i_count = instances.end.saturating_sub(instances.start) as usize;
        if idx_count == 0 || i_count == 0 || self.pipeline.is_none() {
            return;
        }
        let Some((ibuf, fmt, base_off)) = &self.index_buffer else {
            return;
        };
        let (elem_size, mtl_idx_type) = match fmt {
            IndexFormat::Uint16 => (2u64, 0u64), // MTLIndexTypeUInt16
            IndexFormat::Uint32 => (4u64, 1u64), // MTLIndexTypeUInt32
        };
        let byte_off = (*base_off + indices.start as u64 * elem_size) as usize;
        let f_draw_idx: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            u64,
            usize,
            u64,
            *mut c_void,
            usize,
            usize,
            isize,
            usize,
        ) = msg_send();
        f_draw_idx(
            self.enc,
            self.s.draw_indexed_primitives,
            self.primitive_type,
            idx_count,
            mtl_idx_type,
            ibuf.inner.raw.as_ptr(),
            byte_off,
            i_count,
            base_vertex as isize,
            instances.start as usize,
        );
    }
}

/// Metal row and image strides for a buffer<->texture copy of one image.
///
/// WebGPU only requires the bytes a copy touches to be in bounds, while Metal validates
/// `offset + bytesPerImage` against the buffer length. A padded row stride on the last (or
/// only) row would therefore reject copies that WebGPU accepts, so the image stride is
/// trimmed to the bytes actually read, and a single row uses its packed width.
pub(super) fn metal_copy_strides(
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
