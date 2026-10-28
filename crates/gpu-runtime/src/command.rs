//! GPU CommandEncoder, CommandBuffer, ComputePass, RenderPass, and hardware Metal command encoding.

use crate::metal::{
    metal_msg_send, metal_selectors, MTLClearColor, MTLOrigin, MTLScissorRect, MTLSize,
    MTLViewport, MetalDevice, NSRange,
};
use crate::{
    BindGroup, Buffer, BufferAddress, BufferAsyncError, BufferSlice, ComputePipeline,
    DynamicOffset, Error, ErrorFilter, Extent3d, Face, FrontFace, IndexFormat, Label, MapMode,
    Origin3d, QuerySet, RenderPipeline, TexelCopyBufferInfo, TexelCopyTextureInfo, Texture,
    TextureView,
};
use gpu_shader::back::msl::{
    msl_buffer_slot, msl_sampler_slot, msl_texture_slot, MSL_ARRAY_LENGTHS_BUFFER_SLOT,
    MSL_IMMEDIATE_BUFFER_SLOT, MSL_VERTEX_BUFFER_BASE_SLOT,
};
use std::ffi::c_void;
use std::num::NonZeroU32;
use std::ops::{Range, RangeBounds};
use std::ptr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, Default)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
}

#[derive(Debug, Clone, Copy)]
pub enum LoadOp<V> {
    Clear(V),
    Load,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOp {
    Store,
    Discard,
}

#[derive(Debug, Clone, Copy)]
pub struct Operations<V> {
    pub load: LoadOp<V>,
    pub store: StoreOp,
}

#[derive(Clone)]
pub struct RenderPassColorAttachment<'a> {
    pub view: &'a TextureView,
    pub resolve_target: Option<&'a TextureView>,
    pub ops: Operations<Color>,
    pub depth_slice: Option<u32>,
}

#[derive(Clone)]
pub struct RenderPassDepthStencilAttachment<'a> {
    pub view: &'a TextureView,
    pub depth_ops: Option<Operations<f32>>,
    pub stencil_ops: Option<Operations<u32>>,
}

#[derive(Clone)]
pub struct RenderPassTimestampWrites<'a> {
    pub query_set: &'a QuerySet,
    pub beginning_of_pass_write_index: Option<u32>,
    pub end_of_pass_write_index: Option<u32>,
}

#[derive(Clone, Default)]
pub struct RenderPassDescriptor<'a> {
    pub label: Label<'a>,
    pub color_attachments: &'a [Option<RenderPassColorAttachment<'a>>],
    pub depth_stencil_attachment: Option<RenderPassDepthStencilAttachment<'a>>,
    pub timestamp_writes: Option<RenderPassTimestampWrites<'a>>,
    pub occlusion_query_set: Option<&'a QuerySet>,
    pub multiview_mask: Option<NonZeroU32>,
}

#[derive(Clone)]
pub struct ComputePassTimestampWrites<'a> {
    pub query_set: &'a QuerySet,
    pub beginning_of_pass_write_index: Option<u32>,
    pub end_of_pass_write_index: Option<u32>,
}

#[derive(Clone, Default)]
pub struct ComputePassDescriptor<'a> {
    pub label: Label<'a>,
    pub timestamp_writes: Option<ComputePassTimestampWrites<'a>>,
}

#[derive(Debug, Clone, Default)]
pub struct CommandEncoderDescriptor<'a> {
    pub label: Label<'a>,
}

pub(crate) struct ComputeDispatchOp {
    pub(crate) pipeline: ComputePipeline,
    pub(crate) bind_groups: Vec<(u32, Option<BindGroup>, Vec<u32>)>,
    pub(crate) immediates: Vec<u8>,
    pub(crate) groups: [u32; 3],
}

#[derive(Clone)]
pub(crate) struct OwnedColorAttachment {
    pub(crate) view: TextureView,
    pub(crate) ops: Operations<Color>,
}

#[derive(Clone)]
pub(crate) struct OwnedDepthAttachment {
    pub(crate) view: TextureView,
    pub(crate) depth_ops: Option<Operations<f32>>,
}

pub(crate) enum RenderCommand {
    SetPipeline(RenderPipeline),
    SetBindGroup {
        index: u32,
        bind_group: BindGroup,
        offsets: Vec<u32>,
    },
    SetVertexBuffer {
        slot: u32,
        buffer: Buffer,
        offset: u64,
    },
    SetIndexBuffer {
        buffer: Buffer,
        format: IndexFormat,
        offset: u64,
    },
    SetImmediates {
        offset: u32,
        data: Vec<u8>,
    },
    SetViewport {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        min_depth: f32,
        max_depth: f32,
    },
    SetScissorRect {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    },
    Draw {
        vertices: Range<u32>,
        instances: Range<u32>,
    },
    DrawIndexed {
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    },
}

pub(crate) struct RenderPassOp {
    pub(crate) color_attachments: Vec<Option<OwnedColorAttachment>>,
    pub(crate) depth_stencil_attachment: Option<OwnedDepthAttachment>,
    pub(crate) commands: Vec<RenderCommand>,
    pub(crate) start_ts: Option<(QuerySet, u32)>,
    pub(crate) end_ts: Option<(QuerySet, u32)>,
}

pub(crate) enum RecordedOp {
    ComputePass {
        dispatches: Vec<ComputeDispatchOp>,
        start_ts: Option<(QuerySet, u32)>,
        end_ts: Option<(QuerySet, u32)>,
    },
    RenderPass(RenderPassOp),
    CopyBufferToBuffer {
        src: Buffer,
        src_offset: u64,
        dst: Buffer,
        dst_offset: u64,
        size: u64,
    },
    ClearBuffer {
        dst: Buffer,
        offset: u64,
        size: u64,
    },
    CopyTextureToBuffer {
        src_tex: Texture,
        mip_level: u32,
        origin: Origin3d,
        dst_buf: Buffer,
        dst_offset: u64,
        bytes_per_row: u32,
        rows_per_image: Option<u32>,
        size: Extent3d,
    },
    CopyTextureToTexture {
        src_tex: Texture,
        src_mip: u32,
        src_origin: Origin3d,
        dst_tex: Texture,
        dst_mip: u32,
        dst_origin: Origin3d,
        size: Extent3d,
    },
    WriteTimestamp {
        query_set: QuerySet,
        index: u32,
    },
    ResolveQuerySet {
        query_set: QuerySet,
        first_query: u32,
        query_count: u32,
        destination: Buffer,
        destination_offset: u64,
    },
}

pub struct CommandEncoder {
    pub(crate) device: MetalDevice,
    pub(crate) error_stack: Arc<Mutex<Vec<(ErrorFilter, Option<Error>)>>>,
    pub(crate) ops: Vec<RecordedOp>,
    pub(crate) on_submit_callbacks: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}

impl CommandEncoder {
    pub(crate) fn record_error(&self, err: Error) {
        let err_filter = match &err {
            Error::OutOfMemory { .. } => ErrorFilter::OutOfMemory,
            Error::Validation { .. } => ErrorFilter::Validation,
            Error::Internal { .. } => ErrorFilter::Internal,
        };
        if let Ok(mut stack) = self.error_stack.lock() {
            if let Some((_, slot)) = stack.iter_mut().rev().find(|(f, _)| *f == err_filter) {
                if slot.is_none() {
                    *slot = Some(err);
                }
            }
        }
    }

    pub fn map_buffer_on_submit<S, F>(
        &self,
        buffer: &Buffer,
        _mode: MapMode,
        _bounds: S,
        callback: F,
    ) where
        S: RangeBounds<BufferAddress>,
        F: FnOnce(Result<(), BufferAsyncError>) + Send + 'static,
    {
        let buf = buffer.clone();
        if let Ok(mut cbs) = self.on_submit_callbacks.lock() {
            cbs.push(Box::new(move || {
                buf.inner.mapped_ready.store(true, Ordering::Release);
                callback(Ok(()));
            }));
        }
    }

    pub fn begin_compute_pass(&mut self, desc: &ComputePassDescriptor<'_>) -> ComputePass<'_> {
        let start_ts = desc.timestamp_writes.as_ref().and_then(|tw| {
            tw.beginning_of_pass_write_index
                .map(|i| (tw.query_set.clone(), i))
        });
        let end_ts = desc
            .timestamp_writes
            .as_ref()
            .and_then(|tw| tw.end_of_pass_write_index.map(|i| (tw.query_set.clone(), i)));
        ComputePass {
            encoder: self,
            current_pipeline: None,
            bind_groups: Vec::with_capacity(4),
            immediates: Vec::new(),
            dispatches: Vec::with_capacity(8),
            start_timestamp: start_ts,
            end_timestamp: end_ts,
        }
    }

    pub fn begin_render_pass(&mut self, desc: &RenderPassDescriptor<'_>) -> RenderPass<'_> {
        let color_attachments = desc
            .color_attachments
            .iter()
            .map(|ca| {
                ca.as_ref().map(|c| OwnedColorAttachment {
                    view: c.view.clone(),
                    ops: c.ops,
                })
            })
            .collect();
        let depth_stencil_attachment =
            desc.depth_stencil_attachment
                .as_ref()
                .map(|d| OwnedDepthAttachment {
                    view: d.view.clone(),
                    depth_ops: d.depth_ops,
                });
        let start_timestamp = desc.timestamp_writes.as_ref().and_then(|tw| {
            tw.beginning_of_pass_write_index
                .map(|i| (tw.query_set.clone(), i))
        });
        let end_timestamp = desc
            .timestamp_writes
            .as_ref()
            .and_then(|tw| tw.end_of_pass_write_index.map(|i| (tw.query_set.clone(), i)));
        RenderPass {
            encoder: self,
            color_attachments,
            depth_stencil_attachment,
            commands: Vec::with_capacity(32),
            start_timestamp,
            end_timestamp,
        }
    }

    pub fn copy_buffer_to_buffer(
        &mut self,
        source: &Buffer,
        source_offset: BufferAddress,
        destination: &Buffer,
        destination_offset: BufferAddress,
        copy_size: BufferAddress,
    ) {
        self.ops.push(RecordedOp::CopyBufferToBuffer {
            src: source.clone(),
            src_offset: source_offset,
            dst: destination.clone(),
            dst_offset: destination_offset,
            size: copy_size,
        });
    }

    pub fn clear_buffer(
        &mut self,
        buffer: &Buffer,
        offset: BufferAddress,
        size: Option<BufferAddress>,
    ) {
        let sz = size.unwrap_or_else(|| buffer.size().saturating_sub(offset));
        self.ops.push(RecordedOp::ClearBuffer {
            dst: buffer.clone(),
            offset,
            size: sz,
        });
    }

    pub fn copy_texture_to_buffer(
        &mut self,
        source: TexelCopyTextureInfo<'_>,
        destination: TexelCopyBufferInfo<'_>,
        copy_size: Extent3d,
    ) {
        self.ops.push(RecordedOp::CopyTextureToBuffer {
            src_tex: source.texture.clone(),
            mip_level: source.mip_level,
            origin: source.origin,
            dst_buf: destination.buffer.clone(),
            dst_offset: destination.layout.offset,
            bytes_per_row: destination
                .layout
                .bytes_per_row
                .unwrap_or(copy_size.width * source.texture.format.bytes_per_pixel()),
            rows_per_image: destination.layout.rows_per_image,
            size: copy_size,
        });
    }

    pub fn copy_texture_to_texture(
        &mut self,
        source: TexelCopyTextureInfo<'_>,
        destination: TexelCopyTextureInfo<'_>,
        copy_size: Extent3d,
    ) {
        self.ops.push(RecordedOp::CopyTextureToTexture {
            src_tex: source.texture.clone(),
            src_mip: source.mip_level,
            src_origin: source.origin,
            dst_tex: destination.texture.clone(),
            dst_mip: destination.mip_level,
            dst_origin: destination.origin,
            size: copy_size,
        });
    }

    pub fn write_timestamp(&mut self, query_set: &QuerySet, query_index: u32) {
        self.ops.push(RecordedOp::WriteTimestamp {
            query_set: query_set.clone(),
            index: query_index,
        });
    }

    pub fn resolve_query_set(
        &mut self,
        query_set: &QuerySet,
        query_range: Range<u32>,
        destination: &Buffer,
        destination_offset: BufferAddress,
    ) {
        self.ops.push(RecordedOp::ResolveQuerySet {
            query_set: query_set.clone(),
            first_query: query_range.start,
            query_count: query_range.end.saturating_sub(query_range.start),
            destination: destination.clone(),
            destination_offset,
        });
    }

    pub fn finish(self) -> CommandBuffer {
        let on_submit_callbacks = self
            .on_submit_callbacks
            .into_inner()
            .unwrap_or_default();
        CommandBuffer {
            ops: self.ops,
            on_submit_callbacks,
        }
    }
}

pub struct CommandBuffer {
    pub(crate) ops: Vec<RecordedOp>,
    pub(crate) on_submit_callbacks: Vec<Box<dyn FnOnce() + Send>>,
}

pub struct ComputePass<'a> {
    encoder: &'a mut CommandEncoder,
    current_pipeline: Option<ComputePipeline>,
    bind_groups: Vec<(u32, Option<BindGroup>, Vec<u32>)>,
    immediates: Vec<u8>,
    dispatches: Vec<ComputeDispatchOp>,
    start_timestamp: Option<(QuerySet, u32)>,
    end_timestamp: Option<(QuerySet, u32)>,
}

impl<'a> ComputePass<'a> {
    #[inline]
    pub fn set_pipeline(&mut self, pipeline: &ComputePipeline) {
        self.current_pipeline = Some(pipeline.clone());
    }

    #[inline]
    pub fn set_bind_group<'b>(
        &mut self,
        index: u32,
        bind_group: impl Into<Option<&'b BindGroup>>,
        offsets: &[DynamicOffset],
    ) {
        let bind_group = bind_group.into();
        if let Some(entry) = self.bind_groups.iter_mut().find(|(i, _, _)| *i == index) {
            entry.1 = bind_group.cloned();
            entry.2.clear();
            entry.2.extend_from_slice(offsets);
        } else {
            self.bind_groups
                .push((index, bind_group.cloned(), offsets.to_vec()));
        }
    }

    #[inline]
    pub fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        let end = offset as usize + data.len();
        if self.immediates.len() < end {
            self.immediates.resize(end, 0);
        }
        self.immediates[offset as usize..end].copy_from_slice(data);
    }

    #[inline]
    pub fn dispatch_workgroups(&mut self, x: u32, y: u32, z: u32) {
        if let Some(pipe) = &self.current_pipeline {
            self.dispatches.push(ComputeDispatchOp {
                pipeline: pipe.clone(),
                bind_groups: self.bind_groups.clone(),
                immediates: self.immediates.clone(),
                groups: [x, y, z],
            });
        } else {
            let msg = "dispatch_workgroups called without an active ComputePipeline".to_string();
            self.encoder.record_error(Error::Validation {
                source: Arc::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    msg.clone(),
                )),
                description: msg,
            });
        }
    }
}

impl<'a> Drop for ComputePass<'a> {
    fn drop(&mut self) {
        let dispatches = std::mem::take(&mut self.dispatches);
        let start_ts = self.start_timestamp.take();
        let end_ts = self.end_timestamp.take();
        if !dispatches.is_empty() || start_ts.is_some() || end_ts.is_some() {
            self.encoder.ops.push(RecordedOp::ComputePass {
                dispatches,
                start_ts,
                end_ts,
            });
        }
    }
}

pub struct RenderPass<'a> {
    encoder: &'a mut CommandEncoder,
    color_attachments: Vec<Option<OwnedColorAttachment>>,
    depth_stencil_attachment: Option<OwnedDepthAttachment>,
    commands: Vec<RenderCommand>,
    start_timestamp: Option<(QuerySet, u32)>,
    end_timestamp: Option<(QuerySet, u32)>,
}

impl<'a> RenderPass<'a> {
    #[inline]
    pub fn set_pipeline(&mut self, pipeline: &RenderPipeline) {
        self.commands
            .push(RenderCommand::SetPipeline(pipeline.clone()));
    }

    #[inline]
    pub fn set_bind_group<'b>(
        &mut self,
        index: u32,
        bind_group: impl Into<Option<&'b BindGroup>>,
        offsets: &[DynamicOffset],
    ) {
        if let Some(bg) = bind_group.into() {
            self.commands.push(RenderCommand::SetBindGroup {
                index,
                bind_group: bg.clone(),
                offsets: offsets.to_vec(),
            });
        }
    }

    #[inline]
    pub fn set_vertex_buffer(&mut self, slot: u32, buffer_slice: BufferSlice<'_>) {
        self.commands.push(RenderCommand::SetVertexBuffer {
            slot,
            buffer: buffer_slice.buffer.clone(),
            offset: buffer_slice.offset,
        });
    }

    #[inline]
    pub fn set_index_buffer(&mut self, buffer_slice: BufferSlice<'_>, index_format: IndexFormat) {
        self.commands.push(RenderCommand::SetIndexBuffer {
            buffer: buffer_slice.buffer.clone(),
            format: index_format,
            offset: buffer_slice.offset,
        });
    }

    #[inline]
    pub fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        self.commands.push(RenderCommand::SetImmediates {
            offset,
            data: data.to_vec(),
        });
    }

    #[inline]
    pub fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        self.commands.push(RenderCommand::SetViewport {
            x,
            y,
            w,
            h,
            min_depth,
            max_depth,
        });
    }

    #[inline]
    pub fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.commands.push(RenderCommand::SetScissorRect {
            x,
            y,
            w: width,
            h: height,
        });
    }

    #[inline]
    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.commands.push(RenderCommand::Draw {
            vertices,
            instances,
        });
    }

    #[inline]
    pub fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        self.commands.push(RenderCommand::DrawIndexed {
            indices,
            base_vertex,
            instances,
        });
    }
}

impl<'a> Drop for RenderPass<'a> {
    fn drop(&mut self) {
        let color_attachments = std::mem::take(&mut self.color_attachments);
        let depth_stencil_attachment = self.depth_stencil_attachment.take();
        let commands = std::mem::take(&mut self.commands);
        let start_ts = self.start_timestamp.take();
        let end_ts = self.end_timestamp.take();
        self.encoder.ops.push(RecordedOp::RenderPass(RenderPassOp {
            color_attachments,
            depth_stencil_attachment,
            commands,
            start_ts,
            end_ts,
        }));
    }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn encode_compute_dispatches(enc: *mut c_void, dispatches: &[ComputeDispatchOp]) {
    let s = metal_selectors();
    let f_set_pso: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_buf: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize, usize) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_bytes: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, usize, usize) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_tex: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_samp: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_dispatch: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLSize, MTLSize) =
        std::mem::transmute(metal_msg_send as *const c_void);

    let mut last_pso: *mut c_void = ptr::null_mut();
    for d in dispatches {
        let pso_ptr = d.pipeline.pso.as_ptr();
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
                let dyn_off = dyn_offsets.get(b_idx).copied().unwrap_or(0) as u64;
                let final_off = (*base_off + dyn_off) as usize;
                f_set_buf(
                    enc,
                    s.set_buffer,
                    buf.inner.raw.as_ptr(),
                    final_off,
                    slot,
                );
                if slot < 32 {
                    let rem = (*byte_size).saturating_sub(dyn_off);
                    buffer_sizes[slot] = rem as u32;
                }
            }
            for (binding, tex) in &bg.textures {
                let slot = msl_texture_slot(*group_idx, *binding) as usize;
                f_set_tex(enc, s.set_texture, tex.raw.as_ptr(), slot);
            }
            for (binding, samp) in &bg.samplers {
                let slot = msl_sampler_slot(*group_idx, *binding) as usize;
                f_set_samp(enc, s.set_sampler_state, samp.raw.as_ptr(), slot);
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

#[cfg(target_os = "macos")]
pub(crate) unsafe fn encode_non_compute_op(_metal: &MetalDevice, cmd_buf: *mut c_void, op: RecordedOp) {
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        std::mem::transmute(metal_msg_send as *const c_void);

    match op {
        RecordedOp::CopyBufferToBuffer {
            src,
            src_offset,
            dst,
            dst_offset,
            size,
        } => {
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
            ) = std::mem::transmute(metal_msg_send as *const c_void);
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
        RecordedOp::ClearBuffer {
            dst,
            offset,
            size,
        } => {
            if size == 0 {
                return;
            }
            let blit = f_obj(cmd_buf, s.blit_command_encoder);
            let f_fill: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, NSRange, u8) =
                std::mem::transmute(metal_msg_send as *const c_void);
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
        RecordedOp::CopyTextureToBuffer {
            src_tex,
            mip_level,
            origin,
            dst_buf,
            dst_offset,
            bytes_per_row,
            rows_per_image,
            size,
        } => {
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
            ) = std::mem::transmute(metal_msg_send as *const c_void);
            let bpr = bytes_per_row as usize;
            let bpi = rows_per_image.unwrap_or(size.height) as usize * bpr;
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
        RecordedOp::CopyTextureToTexture {
            src_tex,
            src_mip,
            src_origin,
            dst_tex,
            dst_mip,
            dst_origin,
            size,
        } => {
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
            ) = std::mem::transmute(metal_msg_send as *const c_void);
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
        RecordedOp::WriteTimestamp { query_set, index } => {
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
        RecordedOp::ResolveQuerySet {
            query_set,
            first_query,
            query_count,
            destination,
            destination_offset,
        } => {
            if let Ok(ts) = query_set.timestamps_ns.lock() {
                let start = first_query as usize;
                let end = (start + query_count as usize).min(ts.len());
                if start < end && !destination.inner.ptr.is_null() {
                    let slice = &ts[start..end];
                    ptr::copy_nonoverlapping(
                        slice.as_ptr() as *const u8,
                        destination.inner.ptr.add(destination_offset as usize),
                        slice.len() * 8,
                    );
                }
            }
        }
        RecordedOp::RenderPass(rp) => {
            encode_render_pass(cmd_buf, rp);
        }
        RecordedOp::ComputePass { .. } => {}
    }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn encode_render_pass(cmd_buf: *mut c_void, rp: RenderPassOp) {
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_sub: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_obj: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_u64: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_f64: unsafe extern "C" fn(*mut c_void, *mut c_void, f64) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_cc: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLClearColor) =
        std::mem::transmute(metal_msg_send as *const c_void);

    let rpd = f_obj(s.cls_mtl_render_pass_descriptor, s.render_pass_descriptor);
    let color_atts = f_obj(rpd, s.color_attachments);

    for (i, ca_opt) in rp.color_attachments.iter().enumerate() {
        if let Some(ca) = ca_opt {
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
            let store_act = match ca.ops.store {
                StoreOp::Store => 1u64,   // MTLStoreActionStore
                StoreOp::Discard => 0u64, // MTLStoreActionDontCare
            };
            f_set_u64(att, s.set_store_action, store_act);
        }
    }

    if let Some(dsa) = &rp.depth_stencil_attachment {
        let d_att = f_obj(rpd, s.depth_attachment);
        f_set_obj(d_att, s.set_attachment_texture, dsa.view.texture.raw.as_ptr());
        f_set_u64(d_att, s.set_level, dsa.view.base_mip_level as u64);
        f_set_u64(d_att, s.set_slice, dsa.view.base_array_layer as u64);
        if let Some(ops) = &dsa.depth_ops {
            match ops.load {
                LoadOp::Clear(d) => {
                    f_set_u64(d_att, s.set_load_action, 2);
                    f_set_f64(d_att, s.set_clear_depth, d as f64);
                }
                LoadOp::Load => {
                    f_set_u64(d_att, s.set_load_action, 1);
                }
            }
            let store_act = match ops.store {
                StoreOp::Store => 1u64,
                StoreOp::Discard => 0u64,
            };
            f_set_u64(d_att, s.set_store_action, store_act);
        }
    }

    let f_rce: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(metal_msg_send as *const c_void);
    let enc = f_rce(cmd_buf, s.render_command_encoder_with_desc, rpd);
    if enc.is_null() {
        return;
    }

    let f_set_pso: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_dss: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_buf: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize, usize) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_bytes: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, usize, usize) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_tex: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_samp: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_db: unsafe extern "C" fn(*mut c_void, *mut c_void, f32, f32, f32) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_vp: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLViewport) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_set_sc: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLScissorRect) =
        std::mem::transmute(metal_msg_send as *const c_void);
    let f_draw: unsafe extern "C" fn(*mut c_void, *mut c_void, u64, usize, usize, usize, usize) =
        std::mem::transmute(metal_msg_send as *const c_void);
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
    ) = std::mem::transmute(metal_msg_send as *const c_void);
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        std::mem::transmute(metal_msg_send as *const c_void);

    let mut current_primitive_type = 3u64; // MTLPrimitiveTypeTriangle
    let mut active_index_buffer: Option<(Buffer, IndexFormat, u64)> = None;

    for cmd in rp.commands {
        match cmd {
            RenderCommand::SetPipeline(pipe) => {
                f_set_pso(enc, s.set_render_pipeline_state, pipe.pso.as_ptr());
                if let Some(dss) = &pipe.depth_stencil_state {
                    f_set_dss(enc, s.set_depth_stencil_state, dss.as_ptr());
                }
                current_primitive_type = pipe.primitive.topology.to_mtl();
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
                    f_set_db(
                        enc,
                        s.set_depth_bias,
                        pipe.depth_bias.constant as f32,
                        pipe.depth_bias.slope_scale,
                        pipe.depth_bias.clamp,
                    );
                }
            }
            RenderCommand::SetBindGroup {
                index,
                bind_group,
                offsets,
            } => {
                for (b_i, (binding, buf, base_off, _)) in bind_group.buffers.iter().enumerate() {
                    let slot = msl_buffer_slot(index, *binding) as usize;
                    let dyn_off = offsets.get(b_i).copied().unwrap_or(0) as u64;
                    let final_off = (*base_off + dyn_off) as usize;
                    f_set_buf(
                        enc,
                        s.set_vertex_buffer,
                        buf.inner.raw.as_ptr(),
                        final_off,
                        slot,
                    );
                    f_set_buf(
                        enc,
                        s.set_fragment_buffer,
                        buf.inner.raw.as_ptr(),
                        final_off,
                        slot,
                    );
                }
                for (binding, tex) in &bind_group.textures {
                    let slot = msl_texture_slot(index, *binding) as usize;
                    f_set_tex(enc, s.set_vertex_texture, tex.raw.as_ptr(), slot);
                    f_set_tex(enc, s.set_fragment_texture, tex.raw.as_ptr(), slot);
                }
                for (binding, samp) in &bind_group.samplers {
                    let slot = msl_sampler_slot(index, *binding) as usize;
                    f_set_samp(enc, s.set_vertex_sampler_state, samp.raw.as_ptr(), slot);
                    f_set_samp(enc, s.set_fragment_sampler_state, samp.raw.as_ptr(), slot);
                }
            }
            RenderCommand::SetVertexBuffer {
                slot,
                buffer,
                offset,
            } => {
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
                active_index_buffer = Some((buffer, format, offset));
            }
            RenderCommand::SetImmediates { offset: _, data } => {
                let slot = MSL_IMMEDIATE_BUFFER_SLOT as usize;
                f_set_bytes(
                    enc,
                    s.set_vertex_bytes,
                    data.as_ptr() as *const c_void,
                    data.len(),
                    slot,
                );
                f_set_bytes(
                    enc,
                    s.set_fragment_bytes,
                    data.as_ptr() as *const c_void,
                    data.len(),
                    slot,
                );
            }
            RenderCommand::SetViewport {
                x,
                y,
                w,
                h,
                min_depth,
                max_depth,
            } => {
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
            } => {
                let v_count = vertices.end.saturating_sub(vertices.start) as usize;
                let i_count = instances.end.saturating_sub(instances.start) as usize;
                if v_count > 0 && i_count > 0 {
                    f_draw(
                        enc,
                        s.draw_primitives,
                        current_primitive_type,
                        vertices.start as usize,
                        v_count,
                        i_count,
                        instances.start as usize,
                    );
                }
            }
            RenderCommand::DrawIndexed {
                indices,
                base_vertex,
                instances,
            } => {
                let idx_count = indices.end.saturating_sub(indices.start) as usize;
                let i_count = instances.end.saturating_sub(instances.start) as usize;
                if idx_count > 0 && i_count > 0 {
                    if let Some((ibuf, fmt, base_off)) = &active_index_buffer {
                        let elem_size = match fmt {
                            IndexFormat::Uint16 => 2u64,
                            IndexFormat::Uint32 => 4u64,
                        };
                        let mtl_idx_type = match fmt {
                            IndexFormat::Uint16 => 0u64, // MTLIndexTypeUInt16
                            IndexFormat::Uint32 => 1u64, // MTLIndexTypeUInt32
                        };
                        let byte_off = (*base_off + indices.start as u64 * elem_size) as usize;
                        f_draw_idx(
                            enc,
                            s.draw_indexed_primitives,
                            current_primitive_type,
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
        }
    }

    f_void(enc, s.end_encoding);
}
