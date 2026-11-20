//! Metal render pass command encoding.

use super::super::*;
use super::{MsgSetAtIndex, MsgSetBuffer, MsgSetObj, MsgSetU64};
use std::ffi::c_void;

/// # Safety
///
/// `cmd_buf` must be an uncommitted `MTLCommandBuffer` belonging to `metal` with no open encoder.
pub(crate) unsafe fn encode_render_pass(
    metal: &MetalDevice,
    cmd_buf: *mut c_void,
    mut rp: RenderPassOp,
) {
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_rce: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
        msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();

    // SAFETY: `s.cls_mtl_render_pass_descriptor` creates an autoreleased descriptor; attachments are
    // populated from valid `TextureView`s and recycled afterward.
    let (enc, mut commands) = unsafe {
        let rpd = f_obj(s.cls_mtl_render_pass_descriptor, s.render_pass_descriptor);
        let owned_color_atts = std::mem::take(&mut rp.color_attachments);
        configure_color_attachments(rpd, &owned_color_atts);
        metal.recycle_color_atts(owned_color_atts);
        if let Some(dsa) = &rp.depth_stencil_attachment {
            configure_depth_attachment(rpd, dsa);
        }

        let commands = std::mem::take(&mut rp.commands);
        let enc = f_rce(cmd_buf, s.render_command_encoder_with_desc, rpd);
        (enc, commands)
    };
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
        // SAFETY: `enc` stays open until `endEncoding` below.
        unsafe {
            pass.encode(cmd);
        }
    }

    metal.recycle_render_cmds(commands);
    // SAFETY: `enc` is non-null and open.
    unsafe {
        f_void(enc, s.end_encoding);
    }
}

/// Texture, load action and store action of each colour attachment.
///
/// # Safety
///
/// `rpd` must point to a valid `MTLRenderPassDescriptor`.
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
    // SAFETY: `rpd` is a valid `MTLRenderPassDescriptor` and attachment textures are live.
    unsafe {
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
}

/// # Safety
///
/// `rpd` must point to a valid `MTLRenderPassDescriptor`.
unsafe fn configure_depth_attachment(rpd: *mut c_void, dsa: &OwnedDepthAttachment) {
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_set_obj: MsgSetObj = msg_send();
    let f_set_u64: MsgSetU64 = msg_send();
    let f_set_f64: unsafe extern "C" fn(*mut c_void, *mut c_void, f64) = msg_send();
    // SAFETY: `rpd` is a valid `MTLRenderPassDescriptor` and `dsa.view.texture` is live.
    unsafe {
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
}

fn store_action(store: StoreOp) -> u64 {
    match store {
        StoreOp::Store => 1,   // MTLStoreActionStore
        StoreOp::Discard => 0, // MTLStoreActionDontCare
    }
}

/// The state a render pass carries from one command to the next.
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

impl RenderPassEncoder {
    /// # Safety
    ///
    /// `self.enc` must be an open `MTLRenderCommandEncoder`.
    unsafe fn encode(&mut self, cmd: RenderCommand) {
        let (enc, s) = (self.enc, self.s);
        match cmd {
            RenderCommand::SetPipeline(pipe) => {
                // SAFETY: `self.enc` is open.
                unsafe { self.set_pipeline(pipe) };
            }
            RenderCommand::SetBindGroup {
                index,
                bind_group,
                offsets,
            } => {
                // SAFETY: `self.enc` is open.
                unsafe { self.set_bind_group(index, bind_group, &offsets) };
            }
            RenderCommand::SetVertexBuffer {
                slot,
                buffer,
                offset,
            } => {
                let f_set_buf: MsgSetBuffer = msg_send();
                let mtl_slot = (MSL_VERTEX_BUFFER_BASE_SLOT + slot) as usize;
                // SAFETY: `buffer.inner.raw` is a live `MTLBuffer`.
                unsafe {
                    f_set_buf(
                        enc,
                        s.set_vertex_buffer,
                        buffer.inner.raw.as_ptr(),
                        offset as usize,
                        mtl_slot,
                    );
                }
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
                    // SAFETY: `data` is contiguous and valid for `data.len()` bytes.
                    unsafe {
                        f_set_bytes(
                            enc,
                            selector,
                            data.as_ptr() as *const c_void,
                            data.len(),
                            slot,
                        );
                    }
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
                // SAFETY: `enc` is an open `MTLRenderCommandEncoder`.
                unsafe {
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
            }
            RenderCommand::SetScissorRect { x, y, w, h } => {
                let f_set_sc: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLScissorRect) =
                    msg_send();
                // SAFETY: `enc` is an open `MTLRenderCommandEncoder`.
                unsafe {
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
            }
            RenderCommand::Draw {
                vertices,
                instances,
            } => {
                // SAFETY: `self.enc` is open.
                unsafe { self.draw(vertices, instances) };
            }
            RenderCommand::DrawIndexed {
                indices,
                base_vertex,
                instances,
            } => {
                // SAFETY: `self.enc` is open.
                unsafe { self.draw_indexed(indices, base_vertex, instances) };
            }
        }
    }

    /// Binds the textures and samplers of `bind_group` to the slots `pipe` gives them.
    ///
    /// # Safety
    ///
    /// `self.enc` must be an open `MTLRenderCommandEncoder` and resources must be live.
    unsafe fn bind_handles(&self, index: u32, bind_group: &BindGroup, pipe: &RenderPipeline) {
        let (enc, s) = (self.enc, self.s);
        let f_set: MsgSetAtIndex = msg_send();
        let fragment = pipe.fragment_bindings.as_ref();
        for (binding, tex) in bind_group.textures.iter() {
            if let Some(&slot) = pipe.vertex_bindings.textures.get(&(index, *binding)) {
                // SAFETY: `enc` and `tex.raw` are valid Metal objects.
                unsafe {
                    f_set(enc, s.set_vertex_texture, tex.raw.as_ptr(), slot as usize);
                }
            }
            if let Some(&slot) = fragment.and_then(|map| map.textures.get(&(index, *binding))) {
                // SAFETY: `enc` and `tex.raw` are valid Metal objects.
                unsafe {
                    f_set(enc, s.set_fragment_texture, tex.raw.as_ptr(), slot as usize);
                }
            }
        }
        for (binding, samp) in bind_group.samplers.iter() {
            if let Some(&slot) = pipe.vertex_bindings.samplers.get(&(index, *binding)) {
                // SAFETY: `enc` and `samp.raw` are valid Metal objects.
                unsafe {
                    f_set(
                        enc,
                        s.set_vertex_sampler_state,
                        samp.raw.as_ptr(),
                        slot as usize,
                    );
                }
            }
            if let Some(&slot) = fragment.and_then(|map| map.samplers.get(&(index, *binding))) {
                // SAFETY: `enc` and `samp.raw` are valid Metal objects.
                unsafe {
                    f_set(
                        enc,
                        s.set_fragment_sampler_state,
                        samp.raw.as_ptr(),
                        slot as usize,
                    );
                }
            }
        }
    }

    /// # Safety
    ///
    /// `self.enc` must be an open `MTLRenderCommandEncoder`.
    unsafe fn set_pipeline(&mut self, pipe: RenderPipeline) {
        let (enc, s) = (self.enc, self.s);
        let Some(pso) = &pipe.pso else {
            self.pipeline = None;
            return;
        };
        for (index, bind_group) in &self.bound_groups {
            // SAFETY: `self.enc` is open.
            unsafe {
                self.bind_handles(*index, bind_group, &pipe);
            }
        }
        let f_set_obj: MsgSetObj = msg_send();
        let f_set_u64: MsgSetU64 = msg_send();
        let winding = match pipe.primitive.front_face {
            FrontFace::Cw => 0u64,  // MTLWindingClockwise
            FrontFace::Ccw => 1u64, // MTLWindingCounterClockwise
        };
        let cull = match pipe.primitive.cull_mode {
            None => 0u64,              // MTLCullModeNone
            Some(Face::Front) => 1u64, // MTLCullModeFront
            Some(Face::Back) => 2u64,  // MTLCullModeBack
        };
        // SAFETY: `enc`, `pso`, and optional `dss` are valid Metal objects.
        unsafe {
            f_set_obj(enc, s.set_render_pipeline_state, pso.as_ptr());
            if let Some(dss) = &pipe.depth_stencil_state {
                f_set_obj(enc, s.set_depth_stencil_state, dss.as_ptr());
            }
            f_set_u64(enc, s.set_front_facing_winding, winding);
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
        }
        self.primitive_type = pipe.primitive.topology.to_mtl();
        self.pipeline = Some(pipe);
    }

    /// # Safety
    ///
    /// `self.enc` must be an open `MTLRenderCommandEncoder`.
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
                // SAFETY: `enc` and `buf.inner.raw` are valid Metal objects.
                unsafe {
                    f_set_buf(enc, selector, buf.inner.raw.as_ptr(), final_off, slot);
                }
            }
        }
        if let Some(pipe) = &self.pipeline {
            // SAFETY: `self.enc` is open.
            unsafe {
                self.bind_handles(index, &bind_group, pipe);
            }
        }
        self.bound_groups.insert(index, bind_group);
    }

    /// # Safety
    ///
    /// `self.enc` must be an open `MTLRenderCommandEncoder` with an active valid pipeline.
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
        // SAFETY: Pipeline and buffers for this draw are already bound on `self.enc`.
        unsafe {
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
    }

    /// # Safety
    ///
    /// `self.enc` must be an open `MTLRenderCommandEncoder` with an active valid pipeline.
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
        // SAFETY: `self.enc` and `ibuf.inner.raw` are valid Metal objects.
        unsafe {
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
}
