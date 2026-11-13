//! Recording commands into an encoder.

use super::*;

impl CommandEncoder {
    /// Reports a command that is not recorded because it would reach outside its resources.
    fn refuse(&self, description: String) {
        self.record_error(Error::Validation {
            source: Arc::new(std::io::Error::other(description.clone())),
            description,
        });
    }

    /// Whether `offset..offset + size` lies inside `buffer`; reports a validation error if not.
    fn range_fits(
        &self,
        what: &str,
        buffer: &Buffer,
        offset: BufferAddress,
        size: BufferAddress,
    ) -> bool {
        let fits = offset
            .checked_add(size)
            .is_some_and(|end| end <= buffer.size());
        if !fits {
            self.refuse(format!(
                "{what}: {size} bytes at offset {offset} do not fit a buffer of {} bytes",
                buffer.size()
            ));
        }
        fits
    }

    /// Whether a copy region lies inside mip `mip_level` of `texture`; reports a validation error
    /// if not.
    fn region_fits(
        &self,
        what: &str,
        texture: &Texture,
        mip_level: u32,
        origin: Origin3d,
        size: Extent3d,
    ) -> bool {
        let mip_extent = |extent: u32| (extent >> mip_level.min(31)).max(1);
        let layers = if texture.dimension == crate::TextureDimension::D3 {
            mip_extent(texture.size.depth_or_array_layers)
        } else {
            texture.size.depth_or_array_layers.max(1)
        };
        let inside = |start: u32, length: u32, limit: u32| {
            start.checked_add(length).is_some_and(|end| end <= limit)
        };
        let fits = mip_level < texture.mip_level_count.max(1)
            && inside(origin.x, size.width, mip_extent(texture.size.width))
            && inside(origin.y, size.height, mip_extent(texture.size.height))
            && inside(origin.z, size.depth_or_array_layers.max(1), layers);
        if !fits {
            self.refuse(format!(
                "{what}: region {size:?} at {origin:?} of mip {mip_level} is outside a texture of {:?}",
                texture.size
            ));
        }
        fits
    }

    /// Whether the bytes a texture copy reads from or writes to `buffer` lie inside it; reports a
    /// validation error if not. `images` is how many images of `bytes_per_image` are addressed.
    fn copy_bytes_fit(
        &self,
        what: &str,
        buffer: &Buffer,
        offset: BufferAddress,
        bytes_per_image: usize,
        images: usize,
    ) -> bool {
        match (bytes_per_image as u64).checked_mul(images as u64) {
            Some(bytes) => self.range_fits(what, buffer, offset, bytes),
            None => {
                self.refuse(format!("{what}: the copy size overflows"));
                false
            }
        }
    }

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
        if Arc::ptr_eq(&source.inner, &destination.inner) {
            self.refuse("copy_buffer_to_buffer: source and destination are the same buffer".into());
            return;
        }
        if !self.range_fits(
            "copy_buffer_to_buffer source",
            source,
            source_offset,
            copy_size,
        ) || !self.range_fits(
            "copy_buffer_to_buffer destination",
            destination,
            destination_offset,
            copy_size,
        ) {
            return;
        }
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
        let sz = match size {
            Some(size) => size,
            // Clearing to the end from past the end is out of bounds, not an empty clear.
            None if offset > buffer.size() => 1,
            None => buffer.size() - offset,
        };
        if !self.range_fits("clear_buffer", buffer, offset, sz) {
            return;
        }
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
        let what = "copy_texture_to_buffer";
        let bytes_per_row = destination
            .layout
            .row_stride(copy_size.width, source.texture.format);
        // One image is copied, as the encoder does.
        let (_, bytes_per_image) = metal_encode::metal_copy_strides(
            bytes_per_row,
            destination.layout.rows_per_image,
            copy_size,
            source.texture.format,
            false,
        );
        if !self.region_fits(
            what,
            source.texture,
            source.mip_level,
            source.origin,
            copy_size,
        ) || !self.copy_bytes_fit(
            what,
            destination.buffer,
            destination.layout.offset,
            bytes_per_image,
            1,
        ) {
            return;
        }
        self.ops.push(RecordedOp::CopyTextureToBuffer {
            src_tex: source.texture.clone(),
            mip_level: source.mip_level,
            origin: source.origin,
            dst_buf: destination.buffer.clone(),
            dst_offset: destination.layout.offset,
            bytes_per_row,
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
        let what = "copy_buffer_to_texture";
        let bytes_per_row = source
            .layout
            .row_stride(copy_size.width, destination.texture.format);
        let slices = crate::texture::CopySlices::new(
            destination.texture.dimension,
            destination.origin.z,
            copy_size.depth_or_array_layers,
        );
        let (_, bytes_per_image) = metal_encode::metal_copy_strides(
            bytes_per_row,
            source.layout.rows_per_image,
            copy_size,
            destination.texture.format,
            slices.is_3d() || copy_size.depth_or_array_layers > 1,
        );
        if !self.region_fits(
            what,
            destination.texture,
            destination.mip_level,
            destination.origin,
            copy_size,
        ) || !self.copy_bytes_fit(
            what,
            source.buffer,
            source.layout.offset,
            bytes_per_image,
            slices.count.max(slices.depth),
        ) {
            return;
        }
        self.ops.push(RecordedOp::CopyBufferToTexture {
            src_buf: source.buffer.clone(),
            src_offset: source.layout.offset,
            bytes_per_row,
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
        if !self.region_fits(
            "copy_texture_to_texture source",
            source.texture,
            source.mip_level,
            source.origin,
            copy_size,
        ) || !self.region_fits(
            "copy_texture_to_texture destination",
            destination.texture,
            destination.mip_level,
            destination.origin,
            copy_size,
        ) {
            return;
        }
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
        let bytes = u64::from(query_count) * 8;
        if !self.range_fits("resolve_query_set", destination, destination_offset, bytes) {
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
