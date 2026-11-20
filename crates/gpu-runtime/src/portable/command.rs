//! Portable command encoder, compute/render pass adapters, and queue submission.

use crate::portable::{PortableDevice, convert};
use crate::{
    BindGroup, Buffer, BufferAddress, CommandBuffer, ComputePassDescriptor, ComputePipeline,
    DynamicOffset, Error, Extent3d, IndexFormat, LoadOp, QuerySet, RenderPassDescriptor,
    RenderPipeline, SubmissionIndex, TexelCopyBufferInfo, TexelCopyTextureInfo,
};
use std::ops::Range;
use std::sync::{Arc, Mutex};

pub(crate) fn begin_compute_pass<'a>(
    encoder: &'a mut upstream_wgpu::CommandEncoder,
    desc: &ComputePassDescriptor<'_>,
) -> upstream_wgpu::ComputePass<'a> {
    let ts_writes = desc.timestamp_writes.as_ref().and_then(|tw| {
        tw.query_set
            .portable
            .as_ref()
            .map(|qs| upstream_wgpu::ComputePassTimestampWrites {
                query_set: qs,
                beginning_of_pass_write_index: tw.beginning_of_pass_write_index,
                end_of_pass_write_index: tw.end_of_pass_write_index,
            })
    });
    encoder.begin_compute_pass(&upstream_wgpu::ComputePassDescriptor {
        label: desc.label,
        timestamp_writes: ts_writes,
    })
}

pub(crate) fn begin_render_pass<'a>(
    encoder: &'a mut upstream_wgpu::CommandEncoder,
    desc: &RenderPassDescriptor<'_>,
) -> upstream_wgpu::RenderPass<'a> {
    let color_atts: Vec<Option<upstream_wgpu::RenderPassColorAttachment<'_>>> = desc
        .color_attachments
        .iter()
        .map(|slot| {
            slot.as_ref().and_then(|ca| {
                ca.view
                    .portable
                    .as_ref()
                    .map(|view| upstream_wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: ca.depth_slice,
                        resolve_target: ca.resolve_target.and_then(|rv| rv.portable.as_ref()),
                        ops: upstream_wgpu::Operations {
                            load: match ca.ops.load {
                                LoadOp::Clear(c) => {
                                    upstream_wgpu::LoadOp::Clear(convert::to_wgpu_color(c))
                                }
                                LoadOp::Load => upstream_wgpu::LoadOp::Load,
                            },
                            store: match ca.ops.store {
                                crate::StoreOp::Store => upstream_wgpu::StoreOp::Store,
                                crate::StoreOp::Discard => upstream_wgpu::StoreOp::Discard,
                            },
                        },
                    })
            })
        })
        .collect();

    let depth_att = desc.depth_stencil_attachment.as_ref().and_then(|da| {
        da.view
            .portable
            .as_ref()
            .map(|view| upstream_wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: da.depth_ops.map(|ops| upstream_wgpu::Operations {
                    load: match ops.load {
                        LoadOp::Clear(v) => upstream_wgpu::LoadOp::Clear(v),
                        LoadOp::Load => upstream_wgpu::LoadOp::Load,
                    },
                    store: match ops.store {
                        crate::StoreOp::Store => upstream_wgpu::StoreOp::Store,
                        crate::StoreOp::Discard => upstream_wgpu::StoreOp::Discard,
                    },
                }),
                stencil_ops: da.stencil_ops.map(|ops| upstream_wgpu::Operations {
                    load: match ops.load {
                        LoadOp::Clear(v) => upstream_wgpu::LoadOp::Clear(v),
                        LoadOp::Load => upstream_wgpu::LoadOp::Load,
                    },
                    store: match ops.store {
                        crate::StoreOp::Store => upstream_wgpu::StoreOp::Store,
                        crate::StoreOp::Discard => upstream_wgpu::StoreOp::Discard,
                    },
                }),
            })
    });

    let ts_writes = desc.timestamp_writes.as_ref().and_then(|tw| {
        tw.query_set
            .portable
            .as_ref()
            .map(|qs| upstream_wgpu::RenderPassTimestampWrites {
                query_set: qs,
                beginning_of_pass_write_index: tw.beginning_of_pass_write_index,
                end_of_pass_write_index: tw.end_of_pass_write_index,
            })
    });

    encoder.begin_render_pass(&upstream_wgpu::RenderPassDescriptor {
        label: desc.label,
        color_attachments: &color_atts,
        depth_stencil_attachment: depth_att,
        timestamp_writes: ts_writes,
        occlusion_query_set: desc.occlusion_query_set.and_then(|q| q.portable.as_ref()),
        multiview_mask: desc.multiview_mask,
    })
}

pub(crate) fn copy_buffer_to_buffer(
    encoder: &mut upstream_wgpu::CommandEncoder,
    source: &Buffer,
    source_offset: BufferAddress,
    destination: &Buffer,
    destination_offset: BufferAddress,
    copy_size: BufferAddress,
) {
    if let (Some(src), Some(dst)) = (
        source.inner.portable.as_ref(),
        destination.inner.portable.as_ref(),
    ) {
        encoder.copy_buffer_to_buffer(
            &src.raw,
            source_offset,
            &dst.raw,
            destination_offset,
            copy_size,
        );
    }
}

pub(crate) fn clear_buffer(
    encoder: &mut upstream_wgpu::CommandEncoder,
    buffer: &Buffer,
    offset: BufferAddress,
    size: Option<BufferAddress>,
) {
    if let Some(buf) = buffer.inner.portable.as_ref() {
        encoder.clear_buffer(&buf.raw, offset, size);
    }
}

pub(crate) fn copy_texture_to_buffer(
    encoder: &mut upstream_wgpu::CommandEncoder,
    source: TexelCopyTextureInfo<'_>,
    destination: TexelCopyBufferInfo<'_>,
    copy_size: Extent3d,
) {
    if let (Some(src), Some(dst)) = (
        source.texture.portable.as_ref(),
        destination.buffer.inner.portable.as_ref(),
    ) {
        encoder.copy_texture_to_buffer(
            upstream_wgpu::TexelCopyTextureInfo {
                texture: src,
                mip_level: source.mip_level,
                origin: upstream_wgpu::Origin3d {
                    x: source.origin.x,
                    y: source.origin.y,
                    z: source.origin.z,
                },
                aspect: convert::to_wgpu_texture_aspect(source.aspect),
            },
            upstream_wgpu::TexelCopyBufferInfo {
                buffer: &dst.raw,
                layout: upstream_wgpu::TexelCopyBufferLayout {
                    offset: destination.layout.offset,
                    bytes_per_row: destination.layout.bytes_per_row,
                    rows_per_image: destination.layout.rows_per_image,
                },
            },
            upstream_wgpu::Extent3d {
                width: copy_size.width,
                height: copy_size.height,
                depth_or_array_layers: copy_size.depth_or_array_layers,
            },
        );
    }
}

pub(crate) fn copy_buffer_to_texture(
    encoder: &mut upstream_wgpu::CommandEncoder,
    source: TexelCopyBufferInfo<'_>,
    destination: TexelCopyTextureInfo<'_>,
    copy_size: Extent3d,
) {
    if let (Some(src), Some(dst)) = (
        source.buffer.inner.portable.as_ref(),
        destination.texture.portable.as_ref(),
    ) {
        encoder.copy_buffer_to_texture(
            upstream_wgpu::TexelCopyBufferInfo {
                buffer: &src.raw,
                layout: upstream_wgpu::TexelCopyBufferLayout {
                    offset: source.layout.offset,
                    bytes_per_row: source.layout.bytes_per_row,
                    rows_per_image: source.layout.rows_per_image,
                },
            },
            upstream_wgpu::TexelCopyTextureInfo {
                texture: dst,
                mip_level: destination.mip_level,
                origin: upstream_wgpu::Origin3d {
                    x: destination.origin.x,
                    y: destination.origin.y,
                    z: destination.origin.z,
                },
                aspect: convert::to_wgpu_texture_aspect(destination.aspect),
            },
            upstream_wgpu::Extent3d {
                width: copy_size.width,
                height: copy_size.height,
                depth_or_array_layers: copy_size.depth_or_array_layers,
            },
        );
    }
}

pub(crate) fn copy_texture_to_texture(
    encoder: &mut upstream_wgpu::CommandEncoder,
    source: TexelCopyTextureInfo<'_>,
    destination: TexelCopyTextureInfo<'_>,
    copy_size: Extent3d,
) {
    if let (Some(src), Some(dst)) = (
        source.texture.portable.as_ref(),
        destination.texture.portable.as_ref(),
    ) {
        encoder.copy_texture_to_texture(
            upstream_wgpu::TexelCopyTextureInfo {
                texture: src,
                mip_level: source.mip_level,
                origin: upstream_wgpu::Origin3d {
                    x: source.origin.x,
                    y: source.origin.y,
                    z: source.origin.z,
                },
                aspect: convert::to_wgpu_texture_aspect(source.aspect),
            },
            upstream_wgpu::TexelCopyTextureInfo {
                texture: dst,
                mip_level: destination.mip_level,
                origin: upstream_wgpu::Origin3d {
                    x: destination.origin.x,
                    y: destination.origin.y,
                    z: destination.origin.z,
                },
                aspect: convert::to_wgpu_texture_aspect(destination.aspect),
            },
            upstream_wgpu::Extent3d {
                width: copy_size.width,
                height: copy_size.height,
                depth_or_array_layers: copy_size.depth_or_array_layers,
            },
        );
    }
}

pub(crate) fn write_timestamp(
    encoder: &mut upstream_wgpu::CommandEncoder,
    query_set: &QuerySet,
    query_index: u32,
) {
    if let Some(qs) = query_set.portable.as_ref() {
        encoder.write_timestamp(qs, query_index);
    }
}

pub(crate) fn resolve_query_set(
    encoder: &mut upstream_wgpu::CommandEncoder,
    query_set: &QuerySet,
    query_range: Range<u32>,
    destination: &Buffer,
    destination_offset: BufferAddress,
) {
    if let (Some(qs), Some(dst)) = (
        query_set.portable.as_ref(),
        destination.inner.portable.as_ref(),
    ) {
        encoder.resolve_query_set(qs, query_range, &dst.raw, destination_offset);
    }
}

pub(crate) fn submit<I: IntoIterator<Item = CommandBuffer>>(
    portable: &PortableDevice,
    command_buffers: I,
    _error_sink: Option<crate::CommandErrorSink>,
) -> SubmissionIndex {
    let mut wgpu_buffers = Vec::new();
    let mut callbacks = Vec::new();
    for cb in command_buffers {
        if let Some((raw, mut cbs)) = cb.take_portable() {
            wgpu_buffers.push(raw);
            callbacks.append(&mut cbs);
        }
    }
    let idx = portable.queue.submit(wgpu_buffers);
    for cb in callbacks {
        cb();
    }
    portable.record_submission(idx)
}

pub(crate) struct PortableComputePass<'a> {
    pub(crate) pass: upstream_wgpu::ComputePass<'a>,
    pub(crate) has_pipeline: bool,
    pub(crate) invalid_pipeline: bool,
    pub(crate) error_stack: &'a Arc<Mutex<Vec<crate::types::ErrorScopeEntry>>>,
    pub(crate) uncaptured_error_handler: &'a Arc<Mutex<Option<crate::UncapturedErrorHandler>>>,
}

impl PortableComputePass<'_> {
    pub(crate) fn set_pipeline(&mut self, pipeline: &ComputePipeline) {
        self.has_pipeline = true;
        if let Some(raw) = pipeline.portable.as_ref() {
            self.invalid_pipeline = false;
            self.pass.set_pipeline(raw);
        } else {
            self.invalid_pipeline = true;
        }
    }

    pub(crate) fn set_bind_group(
        &mut self,
        index: u32,
        bind_group: Option<&BindGroup>,
        offsets: &[DynamicOffset],
    ) {
        if self.invalid_pipeline {
            return;
        }
        let raw = bind_group.and_then(|bg| bg.portable.as_ref());
        self.pass.set_bind_group(index, raw, offsets);
    }

    pub(crate) fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        if !self.invalid_pipeline {
            self.pass.set_immediates(offset, data);
        }
    }

    pub(crate) fn dispatch_workgroups(&mut self, x: u32, y: u32, z: u32) {
        if !self.has_pipeline {
            let msg = "dispatch_workgroups called without an active ComputePipeline".to_string();
            crate::dispatch_error(
                self.error_stack,
                self.uncaptured_error_handler,
                Error::Validation {
                    source: Arc::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        msg.clone(),
                    )),
                    description: msg,
                },
            );
            return;
        }
        if !self.invalid_pipeline {
            self.pass.dispatch_workgroups(x, y, z);
        }
    }
}

pub(crate) struct PortableRenderPass<'a> {
    pub(crate) pass: upstream_wgpu::RenderPass<'a>,
    pub(crate) invalid_pipeline: bool,
}

impl PortableRenderPass<'_> {
    pub(crate) fn set_pipeline(&mut self, pipeline: &RenderPipeline) {
        if let Some(raw) = pipeline.portable.as_ref() {
            self.invalid_pipeline = false;
            self.pass.set_pipeline(raw);
        } else {
            self.invalid_pipeline = true;
        }
    }

    pub(crate) fn set_bind_group(
        &mut self,
        index: u32,
        bind_group: Option<&BindGroup>,
        offsets: &[DynamicOffset],
    ) {
        if self.invalid_pipeline {
            return;
        }
        let raw = bind_group.and_then(|bg| bg.portable.as_ref());
        self.pass.set_bind_group(index, raw, offsets);
    }

    pub(crate) fn set_vertex_buffer(&mut self, slot: u32, buffer: &Buffer, offset: BufferAddress) {
        if self.invalid_pipeline {
            return;
        }
        if let Some(buf) = buffer.inner.portable.as_ref() {
            self.pass.set_vertex_buffer(slot, buf.raw.slice(offset..));
        }
    }

    pub(crate) fn set_index_buffer(
        &mut self,
        buffer: &Buffer,
        offset: BufferAddress,
        format: IndexFormat,
    ) {
        if self.invalid_pipeline {
            return;
        }
        let fmt = match format {
            IndexFormat::Uint16 => upstream_wgpu::IndexFormat::Uint16,
            IndexFormat::Uint32 => upstream_wgpu::IndexFormat::Uint32,
        };
        if let Some(buf) = buffer.inner.portable.as_ref() {
            self.pass.set_index_buffer(buf.raw.slice(offset..), fmt);
        }
    }

    pub(crate) fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        if !self.invalid_pipeline {
            self.pass.set_immediates(offset, data);
        }
    }

    pub(crate) fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        self.pass.set_viewport(x, y, w, h, min_depth, max_depth);
    }

    pub(crate) fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.pass.set_scissor_rect(x, y, width, height);
    }

    pub(crate) fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        if !self.invalid_pipeline {
            self.pass.draw(vertices, instances);
        }
    }

    pub(crate) fn draw_indexed(
        &mut self,
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    ) {
        if !self.invalid_pipeline {
            self.pass.draw_indexed(indices, base_vertex, instances);
        }
    }
}
