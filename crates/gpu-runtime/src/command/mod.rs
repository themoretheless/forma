//! GPU CommandEncoder, CommandBuffer, ComputePass, RenderPass, and hardware Metal command encoding.

use crate::metal::MetalDevice;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::metal::{
    MTLClearColor, MTLOrigin, MTLScissorRect, MTLSize, MTLViewport, NSRange, metal_selectors,
    msg_send,
};
use crate::{
    BindGroup, Buffer, BufferAddress, BufferAsyncError, BufferSlice, ComputePipeline,
    DynamicOffset, Error, Extent3d, IndexFormat, Label, MapMode, Origin3d, QuerySet,
    RenderPipeline, TexelCopyBufferInfo, TexelCopyTextureInfo, Texture, TextureView,
};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::{Face, FrontFace};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use gpu_shader::back::msl::{
    MSL_ARRAY_LENGTHS_BUFFER_SLOT, MSL_IMMEDIATE_BUFFER_SLOT, MSL_VERTEX_BUFFER_BASE_SLOT,
    msl_buffer_slot,
};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use std::ffi::c_void;
use std::num::NonZeroU32;
use std::ops::{Range, RangeBounds};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use std::ptr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

mod encoder;
mod metal_encode;
mod passes;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use metal_encode::*;

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

#[derive(Clone, Default)]
pub enum DynamicOffsets {
    #[default]
    Empty,
    Inline {
        len: u8,
        vals: [u32; 4],
    },
    Heap(Vec<u32>),
}

impl DynamicOffsets {
    #[inline]
    pub fn from_slice(offsets: &[u32]) -> Self {
        if offsets.is_empty() {
            Self::Empty
        } else if offsets.len() <= 4 {
            let mut vals = [0u32; 4];
            vals[..offsets.len()].copy_from_slice(offsets);
            Self::Inline {
                len: offsets.len() as u8,
                vals,
            }
        } else {
            Self::Heap(offsets.to_vec())
        }
    }

    #[inline]
    pub fn get(&self, idx: usize) -> Option<u32> {
        match self {
            Self::Empty => None,
            Self::Inline { len, vals } => {
                if idx < *len as usize {
                    Some(vals[idx])
                } else {
                    None
                }
            }
            Self::Heap(v) => v.get(idx).copied(),
        }
    }
}

pub(crate) struct ComputeDispatchOp {
    pub(crate) pipeline: ComputePipeline,
    pub(crate) bind_groups: Vec<(u32, Option<BindGroup>, DynamicOffsets)>,
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
        offsets: DynamicOffsets,
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
    CopyBufferToTexture {
        src_buf: Buffer,
        src_offset: u64,
        bytes_per_row: u32,
        rows_per_image: Option<u32>,
        dst_tex: Texture,
        dst_mip: u32,
        dst_origin: Origin3d,
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
    pub(crate) error_stack: Arc<Mutex<Vec<crate::types::ErrorScopeEntry>>>,
    pub(crate) uncaptured_error_handler: Arc<Mutex<Option<crate::UncapturedErrorHandler>>>,
    pub(crate) ops: Vec<RecordedOp>,
    pub(crate) on_submit_callbacks: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}

pub struct CommandBuffer {
    pub(crate) ops: Vec<RecordedOp>,
    pub(crate) on_submit_callbacks: Vec<Box<dyn FnOnce() + Send>>,
}

pub struct ComputePass<'a> {
    encoder: &'a mut CommandEncoder,
    current_pipeline: Option<ComputePipeline>,
    bind_groups: Vec<(u32, Option<BindGroup>, DynamicOffsets)>,
    immediates: Vec<u8>,
    dispatches: Vec<ComputeDispatchOp>,
    start_timestamp: Option<(QuerySet, u32)>,
    end_timestamp: Option<(QuerySet, u32)>,
}

pub struct RenderPass<'a> {
    encoder: &'a mut CommandEncoder,
    color_attachments: Vec<Option<OwnedColorAttachment>>,
    depth_stencil_attachment: Option<OwnedDepthAttachment>,
    commands: Vec<RenderCommand>,
    start_timestamp: Option<(QuerySet, u32)>,
    end_timestamp: Option<(QuerySet, u32)>,
}
