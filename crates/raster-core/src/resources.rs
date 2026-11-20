//! GPU resource creation and updates; frame ordering lives in rasterizer.
use crate::RasterError;
use crate::pipeline::RasterPipelines;
use crate::uniform::{
    MORPH_BYTE_OFFSET, OBJECT_UNIFORM_BYTES, OBJECT_UNIFORM_FLOATS, ObjectUniform,
    STYLE_BYTE_OFFSET,
};
use gpu_compute::{GpuContext, create_buffer_initialized, f32_le_bytes, u32_le_bytes, wgpu};

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

pub(crate) fn create_mesh(
    context: &GpuContext,
    pipelines: &RasterPipelines,
    vertices: &[f32],
    indices: &[u32],
    uniform: &ObjectUniform,
    morph_source: Option<&[f32]>,
) -> Result<DrawMesh, RasterError> {
    let (vertex_buffer, index_buffer, index_count) =
        upload_geometry(&context.device, vertices, indices)?;
    let (uniform_buffer, bind_group) =
        upload_object_uniform(&context.device, &pipelines.object_bgl, uniform)?;
    let morph_source = morph_source
        .map(|positions| upload_morph_source(&context.device, positions))
        .transpose()?;
    Ok(DrawMesh {
        vertex_buffer,
        index_buffer,
        index_count,
        uniform_buffer,
        bind_group,
        morph_source,
        transparent: uniform.style[0] < 1.0,
    })
}

pub(crate) fn create_edges(
    context: &GpuContext,
    pipelines: &RasterPipelines,
    vertices: &[f32],
    edge_indices: &[u32],
    uniform: &ObjectUniform,
    morph_source: Option<&[f32]>,
) -> Result<DrawEdges, RasterError> {
    let (vertex_buffer, index_buffer, index_count) =
        upload_geometry(&context.device, vertices, edge_indices)?;
    let (uniform_buffer, bind_group) =
        upload_object_uniform(&context.device, &pipelines.object_bgl, uniform)?;
    let morph_source = morph_source
        .map(|positions| upload_morph_source(&context.device, positions))
        .transpose()?;
    Ok(DrawEdges {
        vertex_buffer,
        index_buffer,
        index_count,
        uniform_buffer,
        bind_group,
        morph_source,
    })
}

pub(crate) fn create_line_batch(
    device: &wgpu::Device,
    vertices: &[f32],
) -> Result<LineBatch, RasterError> {
    Ok(LineBatch {
        vertex_buffer: upload_vertex_data(device, vertices, "raster lines")?,
        vertex_count: (vertices.len() / 7) as u32,
    })
}

pub(crate) fn create_overlay_batch(
    device: &wgpu::Device,
    vertices: &[f32],
) -> Result<OverlayBatch, RasterError> {
    Ok(OverlayBatch {
        vertex_buffer: upload_vertex_data(device, vertices, "raster overlay")?,
        vertex_count: (vertices.len() / 7) as u32,
    })
}

pub(crate) fn create_instance_pool(
    device: &wgpu::Device,
    pipelines: &RasterPipelines,
    records: &[ObjectUniform],
) -> Result<InstancePool, RasterError> {
    let buffer = create_buffer_initialized(
        device,
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
    )?;
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("raster instance bind group"),
        layout: &pipelines.instance_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    Ok(InstancePool {
        buffer,
        bind_group,
        count: records.len() as u32,
    })
}

pub(crate) fn create_instanced_geometry(
    device: &wgpu::Device,
    vertices: &[f32],
    indices: &[u32],
    morph_source: Option<&[f32]>,
) -> Result<InstancedGeometry, RasterError> {
    let (vertex_buffer, index_buffer, index_count) = upload_geometry(device, vertices, indices)?;
    let morph_source = morph_source
        .map(|positions| upload_morph_source(device, positions))
        .transpose()?;
    Ok(InstancedGeometry {
        vertex_buffer,
        index_buffer,
        index_count,
        morph_source,
    })
}

pub(crate) fn set_style(queue: &wgpu::Queue, buffer: &wgpu::Buffer, style: [f32; 4]) {
    write_uniform_field(queue, buffer, STYLE_BYTE_OFFSET, &style);
}

pub(crate) fn set_morph_weight(queue: &wgpu::Queue, buffer: &wgpu::Buffer, weight: f32) {
    write_uniform_field(queue, buffer, MORPH_BYTE_OFFSET, &[weight, 0.0, 0.0, 0.0]);
}

fn write_uniform_field(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    byte_offset: u64,
    floats: &[f32; 4],
) {
    queue.write_buffer(buffer, byte_offset, &f32_le_bytes(floats));
}

fn upload_vertex_data(
    device: &wgpu::Device,
    floats: &[f32],
    label: &str,
) -> Result<wgpu::Buffer, RasterError> {
    upload_bytes(
        device,
        &f32_le_bytes(floats),
        label,
        wgpu::BufferUsages::VERTEX,
    )
}

fn upload_bytes(
    device: &wgpu::Device,
    bytes: &[u8],
    label: &str,
    usage: wgpu::BufferUsages,
) -> Result<wgpu::Buffer, RasterError> {
    Ok(create_buffer_initialized(
        device,
        &wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes.len().max(4) as u64,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        },
        |mapping| mapping.into_slice(..bytes.len()).copy_from_slice(bytes),
    )?)
}

fn upload_geometry(
    device: &wgpu::Device,
    vertices: &[f32],
    indices: &[u32],
) -> Result<(wgpu::Buffer, wgpu::Buffer, u32), RasterError> {
    let vertex_buffer = upload_vertex_data(device, vertices, "raster mesh vertices")?;
    let index_buffer = upload_bytes(
        device,
        &u32_le_bytes(indices),
        "raster mesh indices",
        wgpu::BufferUsages::INDEX,
    )?;
    Ok((vertex_buffer, index_buffer, indices.len() as u32))
}

fn upload_morph_source(
    device: &wgpu::Device,
    positions: &[f32],
) -> Result<wgpu::Buffer, RasterError> {
    upload_vertex_data(device, positions, "raster morph source")
}

fn upload_object_uniform(
    device: &wgpu::Device,
    object_bgl: &wgpu::BindGroupLayout,
    uniform: &ObjectUniform,
) -> Result<(wgpu::Buffer, wgpu::BindGroup), RasterError> {
    let mut floats = [0.0f32; OBJECT_UNIFORM_FLOATS];
    uniform.write_f32(&mut floats);
    let uniform_buffer = upload_bytes(
        device,
        &f32_le_bytes(&floats),
        "raster object uniform",
        wgpu::BufferUsages::UNIFORM,
    )?;
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("raster object bind group"),
        layout: object_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });
    Ok((uniform_buffer, bind_group))
}
