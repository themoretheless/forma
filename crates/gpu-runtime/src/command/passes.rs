//! Compute and render passes: recorded while open, handed to the encoder when dropped.

use super::*;

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
            entry.2 = DynamicOffsets::from_slice(offsets);
        } else {
            self.bind_groups.push((
                index,
                bind_group.cloned(),
                DynamicOffsets::from_slice(offsets),
            ));
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
        } else {
            self.encoder.device.recycle_compute_dispatches(dispatches);
        }
    }
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
                offsets: DynamicOffsets::from_slice(offsets),
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
    pub fn set_viewport(&mut self, x: f32, y: f32, w: f32, h: f32, min_depth: f32, max_depth: f32) {
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
