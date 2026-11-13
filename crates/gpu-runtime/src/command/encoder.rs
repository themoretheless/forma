//! Recording commands into an encoder.

use super::*;

impl CommandEncoder {
    pub(crate) fn record_error(&self, err: Error) {
        crate::dispatch_error(&self.error_stack, &self.uncaptured_error_handler, err);
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
        let end_ts = desc.timestamp_writes.as_ref().and_then(|tw| {
            tw.end_of_pass_write_index
                .map(|i| (tw.query_set.clone(), i))
        });
        let dispatches = self.device.take_compute_dispatches();
        ComputePass {
            encoder: self,
            current_pipeline: None,
            bind_groups: Vec::new(),
            immediates: Vec::new(),
            dispatches,
            start_timestamp: start_ts,
            end_timestamp: end_ts,
        }
    }

    pub fn begin_render_pass(&mut self, desc: &RenderPassDescriptor<'_>) -> RenderPass<'_> {
        let mut color_attachments = self.device.take_color_atts();
        color_attachments.extend(desc.color_attachments.iter().map(|ca| {
            ca.as_ref().map(|c| OwnedColorAttachment {
                view: c.view.clone(),
                ops: c.ops,
            })
        }));
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
        let end_timestamp = desc.timestamp_writes.as_ref().and_then(|tw| {
            tw.end_of_pass_write_index
                .map(|i| (tw.query_set.clone(), i))
        });
        let commands = self.device.take_render_cmds();
        RenderPass {
            encoder: self,
            color_attachments,
            depth_stencil_attachment,
            commands,
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
                .row_stride(copy_size.width, source.texture.format),
            rows_per_image: destination.layout.rows_per_image,
            size: copy_size,
        });
    }

    pub fn copy_buffer_to_texture(
        &mut self,
        source: TexelCopyBufferInfo<'_>,
        destination: TexelCopyTextureInfo<'_>,
        copy_size: Extent3d,
    ) {
        self.ops.push(RecordedOp::CopyBufferToTexture {
            src_buf: source.buffer.clone(),
            src_offset: source.layout.offset,
            bytes_per_row: source
                .layout
                .row_stride(copy_size.width, destination.texture.format),
            rows_per_image: source.layout.rows_per_image,
            dst_tex: destination.texture.clone(),
            dst_mip: destination.mip_level,
            dst_origin: destination.origin,
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
        let query_count = query_range.end.saturating_sub(query_range.start);
        // Each query resolves to one 64-bit value.
        let fits = u64::from(query_count)
            .checked_mul(8)
            .and_then(|bytes| destination_offset.checked_add(bytes))
            .is_some_and(|end| end <= destination.size());
        if !fits {
            self.record_error(Error::Validation {
                source: Arc::new(std::io::Error::other("query resolve out of bounds")),
                description: format!(
                    "resolve_query_set: {query_count} queries at offset {destination_offset} do not fit a buffer of {} bytes",
                    destination.size()
                ),
            });
            return;
        }
        self.ops.push(RecordedOp::ResolveQuerySet {
            query_set: query_set.clone(),
            first_query: query_range.start,
            query_count,
            destination: destination.clone(),
            destination_offset,
        });
    }

    pub fn finish(self) -> CommandBuffer {
        let on_submit_callbacks = self.on_submit_callbacks.into_inner().unwrap_or_default();
        CommandBuffer {
            ops: self.ops,
            on_submit_callbacks,
        }
    }
}
