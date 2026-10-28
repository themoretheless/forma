//! GPU resource creation and updates; frame ordering lives in rasterizer.
use crate::rasterizer::Rasterizer;
use crate::uniform::{
    MORPH_BYTE_OFFSET, OBJECT_UNIFORM_BYTES, OBJECT_UNIFORM_FLOATS, ObjectUniform,
    STYLE_BYTE_OFFSET,
};
use gpu_compute::{create_buffer_initialized, f32_le_bytes, u32_le_bytes, wgpu};

/// A drawable mesh: interleaved position+normal vertices, u32 triangle
/// indices, an object uniform buffer and an optional morph source buffer
/// (positions only, stride 3 floats).
pub struct DrawMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub uniform_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    /// Positions blended from when `morph.x` drops below the rest weight 1;
    /// `None` binds a zero dummy (inert while the weight stays at rest).
    pub morph_source: Option<wgpu::Buffer>,
    pub transparent: bool,
}

/// Edges of one mesh: same vertex buffer as the mesh, a separate index buffer
/// with u32 line pairs, and its own object uniform (edge style lives in
/// `style.z` = edge alpha).
pub struct DrawEdges {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub uniform_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub morph_source: Option<wgpu::Buffer>,
}

/// Per-vertex colored line segments (pos3 + color4 per vertex, line list).
pub struct LineBatch {
    pub vertex_buffer: wgpu::Buffer,
    pub vertex_count: u32,
}

/// Per-vertex colored overlay triangles (pos3 + color4 per vertex).
pub struct OverlayBatch {
    pub vertex_buffer: wgpu::Buffer,
    pub vertex_count: u32,
}

/// GPU instance records (56 floats per instance, [`ObjectUniform`] layout),
/// bound as a read-only storage buffer at group 1.
pub struct InstancePool {
    pub buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub count: u32,
}

/// Geometry shared by the instances of one [`InstancePool`] draw.
pub struct InstancedGeometry {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub morph_source: Option<wgpu::Buffer>,
}

impl Rasterizer {
    /// Creates a mesh with its own object uniform buffer and bind group.
    /// `vertices` is the interleaved position+normal layout (6 floats per
    /// vertex); `morph_source` is positions only (3 floats per vertex).
    pub fn create_mesh(
        &self,
        vertices: &[f32],
        indices: &[u32],
        uniform: &ObjectUniform,
        morph_source: Option<&[f32]>,
    ) -> DrawMesh {
        let (vertex_buffer, index_buffer, index_count) = self.upload_geometry(vertices, indices);
        let (uniform_buffer, bind_group) = self.upload_object_uniform(uniform);
        DrawMesh {
            vertex_buffer,
            index_buffer,
            index_count,
            uniform_buffer,
            bind_group,
            morph_source: morph_source.map(|positions| self.upload_morph_source(positions)),
            transparent: uniform.style[0] < 1.0,
        }
    }

    /// Creates an edge draw over the same interleaved vertex layout; edge
    /// indices are u32 pairs into `vertices`.
    pub fn create_edges(
        &self,
        vertices: &[f32],
        edge_indices: &[u32],
        uniform: &ObjectUniform,
        morph_source: Option<&[f32]>,
    ) -> DrawEdges {
        let (vertex_buffer, index_buffer, index_count) =
            self.upload_geometry(vertices, edge_indices);
        let (uniform_buffer, bind_group) = self.upload_object_uniform(uniform);
        DrawEdges {
            vertex_buffer,
            index_buffer,
            index_count,
            uniform_buffer,
            bind_group,
            morph_source: morph_source.map(|positions| self.upload_morph_source(positions)),
        }
    }

    /// Line segments, 7 floats per vertex (pos3 + color4).
    pub fn create_line_batch(&self, vertices: &[f32]) -> LineBatch {
        LineBatch {
            vertex_buffer: self.upload_vertex_data(vertices, "raster lines"),
            vertex_count: (vertices.len() / 7) as u32,
        }
    }

    /// Overlay triangles, 7 floats per vertex (pos3 + color4).
    pub fn create_overlay_batch(&self, vertices: &[f32]) -> OverlayBatch {
        OverlayBatch {
            vertex_buffer: self.upload_vertex_data(vertices, "raster overlay"),
            vertex_count: (vertices.len() / 7) as u32,
        }
    }

    /// Uploads per-instance records (one [`ObjectUniform`] per instance).
    pub fn create_instance_pool(&self, records: &[ObjectUniform]) -> InstancePool {
        let buffer = create_buffer_initialized(
            &self.context.device,
            &wgpu::BufferDescriptor {
                label: Some("raster instance pool"),
                size: (records.len() as u64 * OBJECT_UNIFORM_BYTES).max(4),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: true,
            },
            |mut mapping| {
                for (i, record) in records.iter().enumerate() {
                    let mut floats = [0.0; OBJECT_UNIFORM_FLOATS];
                    record.write_f32(&mut floats);
                    let offset = i * OBJECT_UNIFORM_BYTES as usize;
                    mapping
                        .slice(offset..offset + OBJECT_UNIFORM_BYTES as usize)
                        .copy_from_slice(&f32_le_bytes(&floats));
                }
            },
        )
        .expect("raster instance initialization failed");
        let bind_group = self
            .context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("raster instance bind group"),
                layout: &self.pipelines.instance_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
        InstancePool {
            buffer,
            bind_group,
            count: records.len() as u32,
        }
    }

    /// Geometry shared by instances (same interleaved layout as [`create_mesh`]).
    pub fn create_instanced_geometry(
        &self,
        vertices: &[f32],
        indices: &[u32],
        morph_source: Option<&[f32]>,
    ) -> InstancedGeometry {
        let (vertex_buffer, index_buffer, index_count) = self.upload_geometry(vertices, indices);
        InstancedGeometry {
            vertex_buffer,
            index_buffer,
            index_count,
            morph_source: morph_source.map(|positions| self.upload_morph_source(positions)),
        }
    }

    /// Updates the 4-float style vector (alpha, selected, edge, hovered).
    pub fn set_style(&self, mesh: &DrawMesh, style: [f32; 4]) {
        self.write_uniform_field(&mesh.uniform_buffer, STYLE_BYTE_OFFSET, &style);
    }

    /// Edge variant of [`set_style`] (edge alpha lives in `style.z`).
    pub fn set_edge_style(&self, edges: &DrawEdges, style: [f32; 4]) {
        self.write_uniform_field(&edges.uniform_buffer, STYLE_BYTE_OFFSET, &style);
    }

    /// Updates the morph blend weight (`morph.x`, 0 = source, 1 = target/rest).
    pub fn set_morph_weight(&self, mesh: &DrawMesh, weight: f32) {
        self.write_uniform_field(
            &mesh.uniform_buffer,
            MORPH_BYTE_OFFSET,
            &[weight, 0.0, 0.0, 0.0],
        );
    }

    /// Edge variant of [`set_morph_weight`].
    pub fn set_edge_morph_weight(&self, edges: &DrawEdges, weight: f32) {
        self.write_uniform_field(
            &edges.uniform_buffer,
            MORPH_BYTE_OFFSET,
            &[weight, 0.0, 0.0, 0.0],
        );
    }

    fn write_uniform_field(&self, buffer: &wgpu::Buffer, byte_offset: u64, floats: &[f32; 4]) {
        self.context
            .queue
            .write_buffer(buffer, byte_offset, &f32_le_bytes(floats));
    }

    fn upload_vertex_data(&self, floats: &[f32], label: &str) -> wgpu::Buffer {
        self.upload_bytes(&f32_le_bytes(floats), label, wgpu::BufferUsages::VERTEX)
    }

    fn upload_bytes(&self, bytes: &[u8], label: &str, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        create_buffer_initialized(
            &self.context.device,
            &wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes.len().max(4) as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: true,
            },
            |mapping| mapping.into_slice(..bytes.len()).copy_from_slice(bytes),
        )
        .expect("raster buffer initialization failed")
    }

    fn upload_geometry(
        &self,
        vertices: &[f32],
        indices: &[u32],
    ) -> (wgpu::Buffer, wgpu::Buffer, u32) {
        let vertex_buffer = self.upload_vertex_data(vertices, "raster mesh vertices");
        let index_buffer = self.upload_bytes(
            &u32_le_bytes(indices),
            "raster mesh indices",
            wgpu::BufferUsages::INDEX,
        );
        (vertex_buffer, index_buffer, indices.len() as u32)
    }

    fn upload_morph_source(&self, positions: &[f32]) -> wgpu::Buffer {
        self.upload_vertex_data(positions, "raster morph source")
    }

    fn upload_object_uniform(&self, uniform: &ObjectUniform) -> (wgpu::Buffer, wgpu::BindGroup) {
        let mut floats = [0.0f32; OBJECT_UNIFORM_FLOATS];
        uniform.write_f32(&mut floats);
        let uniform_buffer = self.upload_bytes(
            &f32_le_bytes(&floats),
            "raster object uniform",
            wgpu::BufferUsages::UNIFORM,
        );
        let bind_group = self
            .context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("raster object bind group"),
                layout: &self.pipelines.object_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                }],
            });
        (uniform_buffer, bind_group)
    }
}
