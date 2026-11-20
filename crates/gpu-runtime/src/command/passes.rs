//! Compute and render passes: recorded on Metal or forwarded on portable backends.

use super::*;

impl ComputePass<'_> {
    #[inline]
    pub fn set_pipeline(&mut self, pipeline: &ComputePipeline) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            ComputePassBackend::Metal {
                current_pipeline, ..
            } => {
                *current_pipeline = Some(pipeline.clone());
            }
            ComputePassBackend::Portable(p) => p.set_pipeline(pipeline),
        }
    }

    #[inline]
    pub fn set_bind_group<'b>(
        &mut self,
        index: u32,
        bind_group: impl Into<Option<&'b BindGroup>>,
        offsets: &[DynamicOffset],
    ) {
        let bind_group = bind_group.into();
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            ComputePassBackend::Metal { bind_groups, .. } => {
                if let Some(entry) = bind_groups.iter_mut().find(|(i, _, _)| *i == index) {
                    entry.1 = bind_group.cloned();
                    entry.2 = DynamicOffsets::from_slice(offsets);
                } else {
                    bind_groups.push((
                        index,
                        bind_group.cloned(),
                        DynamicOffsets::from_slice(offsets),
                    ));
                }
            }
            ComputePassBackend::Portable(p) => p.set_bind_group(index, bind_group, offsets),
        }
    }

    #[inline]
    pub fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            ComputePassBackend::Metal { immediates, .. } => {
                let end = offset as usize + data.len();
                if immediates.len() < end {
                    immediates.resize(end, 0);
                }
                immediates[offset as usize..end].copy_from_slice(data);
            }
            ComputePassBackend::Portable(p) => p.set_immediates(offset, data),
        }
    }

    #[inline]
    pub fn dispatch_workgroups(&mut self, x: u32, y: u32, z: u32) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            ComputePassBackend::Metal {
                current_pipeline,
                bind_groups,
                immediates,
                dispatches,
                has_pipeline_error,
                uncaptured,
                ..
            } => {
                if let Some(pipe) = current_pipeline {
                    dispatches.push(ComputeDispatchOp {
                        pipeline: pipe.clone(),
                        bind_groups: bind_groups.clone(),
                        immediates: immediates.clone(),
                        groups: [x, y, z],
                    });
                } else {
                    let msg =
                        "dispatch_workgroups called without an active ComputePipeline".to_string();
                    crate::dispatch_error(
                        has_pipeline_error,
                        uncaptured,
                        Error::Validation {
                            source: Arc::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                msg.clone(),
                            )),
                            description: msg,
                        },
                    );
                }
            }
            ComputePassBackend::Portable(p) => p.dispatch_workgroups(x, y, z),
        }
    }
}

impl Drop for ComputePass<'_> {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let ComputePassBackend::Metal {
            device,
            ops,
            dispatches,
            start_timestamp,
            end_timestamp,
            ..
        } = &mut self.backend
        {
            let dispatches = std::mem::take(dispatches);
            let start_ts = start_timestamp.take();
            let end_ts = end_timestamp.take();
            if !dispatches.is_empty() || start_ts.is_some() || end_ts.is_some() {
                ops.push(RecordedOp::ComputePass {
                    dispatches,
                    start_ts,
                    end_ts,
                });
            } else {
                device.recycle_compute_dispatches(dispatches);
            }
        }
    }
}

impl RenderPass<'_> {
    #[inline]
    pub fn set_pipeline(&mut self, pipeline: &RenderPipeline) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RenderPassBackend::Metal { commands, .. } => {
                commands.push(RenderCommand::SetPipeline(pipeline.clone()));
            }
            RenderPassBackend::Portable(p) => p.set_pipeline(pipeline),
        }
    }

    #[inline]
    pub fn set_bind_group<'b>(
        &mut self,
        index: u32,
        bind_group: impl Into<Option<&'b BindGroup>>,
        offsets: &[DynamicOffset],
    ) {
        let bind_group = bind_group.into();
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RenderPassBackend::Metal { commands, .. } => {
                if let Some(bg) = bind_group {
                    commands.push(RenderCommand::SetBindGroup {
                        index,
                        bind_group: bg.clone(),
                        offsets: DynamicOffsets::from_slice(offsets),
                    });
                }
            }
            RenderPassBackend::Portable(p) => p.set_bind_group(index, bind_group, offsets),
        }
    }

    #[inline]
    pub fn set_vertex_buffer(&mut self, slot: u32, buffer_slice: BufferSlice<'_>) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RenderPassBackend::Metal { commands, .. } => {
                commands.push(RenderCommand::SetVertexBuffer {
                    slot,
                    buffer: buffer_slice.buffer.clone(),
                    offset: buffer_slice.offset,
                });
            }
            RenderPassBackend::Portable(p) => {
                p.set_vertex_buffer(slot, buffer_slice.buffer, buffer_slice.offset);
            }
        }
    }

    #[inline]
    pub fn set_index_buffer(&mut self, buffer_slice: BufferSlice<'_>, index_format: IndexFormat) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RenderPassBackend::Metal { commands, .. } => {
                commands.push(RenderCommand::SetIndexBuffer {
                    buffer: buffer_slice.buffer.clone(),
                    format: index_format,
                    offset: buffer_slice.offset,
                });
            }
            RenderPassBackend::Portable(p) => {
                p.set_index_buffer(buffer_slice.buffer, buffer_slice.offset, index_format);
            }
        }
    }

    #[inline]
    pub fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RenderPassBackend::Metal { commands, .. } => {
                commands.push(RenderCommand::SetImmediates {
                    offset,
                    data: data.to_vec(),
                });
            }
            RenderPassBackend::Portable(p) => p.set_immediates(offset, data),
        }
    }

    #[inline]
    pub fn set_viewport(&mut self, x: f32, y: f32, w: f32, h: f32, min_depth: f32, max_depth: f32) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RenderPassBackend::Metal { commands, .. } => {
                commands.push(RenderCommand::SetViewport {
                    x,
                    y,
                    w,
                    h,
                    min_depth,
                    max_depth,
                });
            }
            RenderPassBackend::Portable(p) => p.set_viewport(x, y, w, h, min_depth, max_depth),
        }
    }

    #[inline]
    pub fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RenderPassBackend::Metal { commands, .. } => {
                commands.push(RenderCommand::SetScissorRect {
                    x,
                    y,
                    w: width,
                    h: height,
                });
            }
            RenderPassBackend::Portable(p) => p.set_scissor_rect(x, y, width, height),
        }
    }

    #[inline]
    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RenderPassBackend::Metal { commands, .. } => {
                commands.push(RenderCommand::Draw {
                    vertices,
                    instances,
                });
            }
            RenderPassBackend::Portable(p) => p.draw(vertices, instances),
        }
    }

    #[inline]
    pub fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        match &mut self.backend {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RenderPassBackend::Metal { commands, .. } => {
                commands.push(RenderCommand::DrawIndexed {
                    indices,
                    base_vertex,
                    instances,
                });
            }
            RenderPassBackend::Portable(p) => p.draw_indexed(indices, base_vertex, instances),
        }
    }
}

impl Drop for RenderPass<'_> {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let RenderPassBackend::Metal {
            ops,
            color_attachments,
            depth_stencil_attachment,
            commands,
            start_timestamp,
            end_timestamp,
        } = &mut self.backend
        {
            ops.push(RecordedOp::RenderPass(RenderPassOp {
                color_attachments: std::mem::take(color_attachments),
                depth_stencil_attachment: depth_stencil_attachment.take().map(|d| *d),
                commands: std::mem::take(commands),
                start_ts: start_timestamp.take(),
                end_ts: end_timestamp.take(),
            }));
        }
    }
}
