//! Immutable material draws, separate from scene decoding and visible publication.
use crate::pipeline::{PipelineSpec, color_target, depth_test, render_pipeline, wgsl_module};
use crate::wgpu;
use crate::{GpuResourceLease, MaterialTexture, MaterialTextureUploader};
use bytemuck::{Pod, Zeroable};
use memory_budget::MemoryBudget;
use std::sync::{Arc, OnceLock};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MaterialVertex {
    pub position: [f32; 3],
    /// World normal, or zero for a flat derivative normal.
    pub normal: [f32; 3],
    /// World tangent + handedness; w=0 requests a derivative UV frame.
    pub tangent: [f32; 4],
    pub color: [f32; 4],
    /// Base color, metallic/roughness, normal, occlusion, emissive.
    pub texcoords: [[f32; 2]; 5],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MaterialParameters {
    pub clip_from_world: [[f32; 4]; 4],
    /// Camera xyz; w is neutral diffuse environment intensity.
    pub eye: [f32; 4],
    /// Direction toward light; w scales radiance.
    pub light_direction: [f32; 4],
    pub light_color: [f32; 4],
    pub base_color: [f32; 4],
    /// RGB emission; w=1 selects orthographic eye direction, w=0 perspective eye position.
    pub emissive: [f32; 4],
    /// Metallic, roughness, normal scale, occlusion strength.
    pub factors: [f32; 4],
    /// Alpha cutoff, alpha mode (set by prepare), normal-map flag, sidedness (set by prepare).
    pub alpha: [f32; 4],
}
impl Default for MaterialParameters {
    fn default() -> Self {
        Self {
            clip_from_world: [
                [1., 0., 0., 0.],
                [0., 1., 0., 0.],
                [0., 0., 1., 0.],
                [0., 0., 0., 1.],
            ],
            eye: [0., 0., 2., 0.03],
            light_direction: [0., 0., 1., 1.],
            light_color: [1.; 4],
            base_color: [1.; 4],
            emissive: [0.; 4],
            factors: [1., 1., 1., 1.],
            alpha: [0.5, 0., 0., 0.],
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum MaterialAlpha {
    Opaque,
    Mask,
    Blend,
}
#[derive(Debug, thiserror::Error)]
pub enum MaterialRenderError {
    #[error("invalid material geometry, parameters or output target")]
    Invalid,
    #[error("material textures or draw belong to another upload context")]
    Owner,
    #[error("material preparation cancelled")]
    Cancelled,
    #[error(transparent)]
    Memory(#[from] memory_budget::BufferError),
}
/// Pipeline cache is bound to the upload context/device and output target.
#[derive(Debug)]
pub struct MaterialRenderer {
    uploader: MaterialTextureUploader,
    target: wgpu::TextureFormat,
    pipelines: [OnceLock<wgpu::RenderPipeline>; 12],
    owner: Arc<()>,
}
#[derive(Debug)]
pub struct MaterialDraw {
    vertices: wgpu::Buffer,
    _uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    count: u32,
    pipeline: usize,
    owner: Arc<()>,
    credit: Arc<memory_budget::Reservation>,
    textures: [MaterialTexture; 5],
}
impl MaterialDraw {
    /// Capture before submitting the encoded draw; retain through completion.
    pub fn resource_lease(&self) -> GpuResourceLease {
        let mut lease = GpuResourceLease::new([self.credit.clone()]);
        for texture in &self.textures {
            lease.extend(texture.resource_lease());
        }
        lease
    }
}
impl MaterialRenderer {
    pub fn new(
        uploader: MaterialTextureUploader,
        target: wgpu::TextureFormat,
    ) -> Result<Self, MaterialRenderError> {
        if !matches!(
            target,
            wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8UnormSrgb
                | wgpu::TextureFormat::Rgba16Float
        ) {
            return Err(MaterialRenderError::Invalid);
        }
        Ok(Self {
            uploader,
            target,
            pipelines: std::array::from_fn(|_| OnceLock::new()),
            owner: Arc::new(()),
        })
    }
    /// World-space expanded triangle vertices. No scene transform or UV guessing.
    /// Fit/rebase coordinates before packing: finite host values must have |v|<=1e15.
    /// Supply white neutral maps for unbound roles; normal map flag controls TBN use.
    pub fn prepare(
        &self,
        budget: &MemoryBudget,
        vertices: &[MaterialVertex],
        mut p: MaterialParameters,
        textures: [&MaterialTexture; 5],
        alpha: MaterialAlpha,
        double_sided: bool,
        clockwise: bool,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<MaterialDraw, MaterialRenderError> {
        if cancelled() {
            return Err(MaterialRenderError::Cancelled);
        }
        if textures.iter().any(|t| !self.uploader.owns(t)) {
            return Err(MaterialRenderError::Owner);
        }
        if textures
            .iter()
            .enumerate()
            .any(|(i, t)| t.is_color_role() != (i == 0 || i == 4))
        {
            return Err(MaterialRenderError::Invalid);
        }
        let device = self.uploader.device();
        let bytes = std::mem::size_of_val(vertices) as u64;
        if vertices.is_empty()
            || vertices.len() % 3 != 0
            || vertices.len() > u32::MAX as usize
            || bytes > device.limits().max_buffer_size
        {
            return Err(MaterialRenderError::Invalid);
        }
        for chunk in vertices.chunks(1024) {
            if cancelled() {
                return Err(MaterialRenderError::Cancelled);
            }
            if bytemuck::cast_slice::<MaterialVertex, f32>(chunk)
                .iter()
                .any(|&v| !crate::finite_bounded(v))
                || chunk.iter().any(|v| ![0., -1., 1.].contains(&v.tangent[3]))
            {
                return Err(MaterialRenderError::Invalid);
            }
        }
        if bytemuck::cast_slice::<MaterialParameters, f32>(&[p])
            .iter()
            .any(|&v| !crate::finite_bounded(v))
            || p.base_color
                .iter()
                .chain(p.emissive[..3].iter())
                .any(|v| !(0. ..=1.).contains(v))
            || [p.factors[0], p.factors[1], p.factors[3]]
                .iter()
                .any(|v| !(0. ..=1.).contains(v))
            || ![0., 1.].contains(&p.emissive[3])
            || p.alpha[0] < 0.
            || ![0., 1.].contains(&p.alpha[2])
            || p.eye[3] < 0.
            || p.light_direction[3] < 0.
            || p.light_color[..3].iter().any(|v| *v < 0.)
        {
            return Err(MaterialRenderError::Invalid);
        }
        let mode = match alpha {
            MaterialAlpha::Opaque => 0,
            MaterialAlpha::Mask => 1,
            MaterialAlpha::Blend => 2,
        };
        p.alpha[1] = mode as f32;
        p.alpha[3] = if double_sided { 1. } else { 0. };
        let index = mode + 3 * usize::from(double_sided) + 6 * usize::from(clockwise);
        let credit = Arc::new(budget.try_reserve(bytes + size_of::<MaterialParameters>() as u64)?);
        if cancelled() {
            return Err(MaterialRenderError::Cancelled);
        }
        let pipeline = self.pipelines[index]
            .get_or_init(|| pipeline(device, self.target, alpha, double_sided, clockwise));
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material triangle vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material parameters"),
            contents: bytemuck::bytes_of(&p),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let entries: [wgpu::BindGroupEntry<'_>; 11] = std::array::from_fn(|binding| {
            let resource = if binding == 0 {
                uniform.as_entire_binding()
            } else if binding % 2 == 1 {
                wgpu::BindingResource::TextureView(textures[(binding - 1) / 2].view())
            } else {
                wgpu::BindingResource::Sampler(textures[(binding - 1) / 2].sampler())
            };
            wgpu::BindGroupEntry {
                binding: binding as u32,
                resource,
            }
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material textures"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        if cancelled() {
            return Err(MaterialRenderError::Cancelled);
        }
        Ok(MaterialDraw {
            vertices: vertex_buffer,
            _uniform: uniform,
            bind,
            count: vertices.len() as u32,
            pipeline: index,
            owner: self.owner.clone(),
            credit,
            textures: textures.map(|t| t.clone()),
        })
    }
    /// Atomically admit one batch of view/eye uploads before changing any draw.
    /// Call before encoding; the context's own queue retains credits to completion.
    pub fn update_views<'a>(
        &self,
        budget: &MemoryBudget,
        draws: impl ExactSizeIterator<Item = &'a MaterialDraw> + Clone,
        clip: [[f32; 4]; 4],
        eye: [f32; 4],
    ) -> Result<(), MaterialRenderError> {
        #[repr(C)]
        #[derive(Clone, Copy, Pod, Zeroable)]
        struct Frame {
            clip: [[f32; 4]; 4],
            eye: [f32; 4],
        }
        let frame = Frame { clip, eye };
        if bytemuck::cast_slice::<Frame, f32>(&[frame])
            .iter()
            .any(|&v| !crate::finite_bounded(v))
            || eye[3] < 0.
        {
            return Err(MaterialRenderError::Invalid);
        }
        if draws
            .clone()
            .any(|draw| !Arc::ptr_eq(&self.owner, &draw.owner))
        {
            return Err(MaterialRenderError::Owner);
        }
        if draws.len() == 0 {
            return Ok(());
        }
        let bytes = (draws.len() as u64)
            .checked_mul(size_of::<Frame>() as u64)
            .ok_or(MaterialRenderError::Invalid)?;
        let queue = self.uploader.queue();
        let _queued = crate::QueuedUploadReservation {
            queue: queue.clone(),
            reservation: Some(budget.try_reserve(bytes)?),
            resources: GpuResourceLease::new(draws.clone().map(|draw| draw.credit.clone())),
        };
        for draw in draws {
            queue.write_buffer(&draw._uniform, 0, bytemuck::bytes_of(&frame));
        }
        Ok(())
    }
    /// Caller owns the pass (Depth32Float, compatible output), ordering and target.
    /// Blend draws disable depth writes and require caller-supplied sorting.
    pub fn encode(
        &self,
        draw: &MaterialDraw,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<(), MaterialRenderError> {
        if !Arc::ptr_eq(&self.owner, &draw.owner) {
            return Err(MaterialRenderError::Owner);
        }
        pass.set_pipeline(self.pipelines[draw.pipeline].get().unwrap());
        pass.set_bind_group(0, &draw.bind, &[]);
        pass.set_vertex_buffer(0, draw.vertices.slice(..));
        pass.draw(0..draw.count, 0..1);
        Ok(())
    }
}
fn pipeline(
    device: &wgpu::Device,
    target: wgpu::TextureFormat,
    alpha: MaterialAlpha,
    double_sided: bool,
    clockwise: bool,
) -> wgpu::RenderPipeline {
    let module = wgsl_module(
        device,
        "glTF material lighting",
        include_str!("../shaders/material_view.wgsl"),
    );
    let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x4,3=>Float32x4,4=>Float32x2,5=>Float32x2,6=>Float32x2,7=>Float32x2,8=>Float32x2];
    let blend = matches!(alpha, MaterialAlpha::Blend);
    render_pipeline(
        device,
        PipelineSpec {
            label: "material renderer",
            module: &module,
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<MaterialVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
            primitive: wgpu::PrimitiveState {
                front_face: if clockwise {
                    wgpu::FrontFace::Cw
                } else {
                    wgpu::FrontFace::Ccw
                },
                cull_mode: if double_sided {
                    None
                } else {
                    Some(wgpu::Face::Back)
                },
                ..Default::default()
            },
            depth: Some(depth_test(!blend)),
            target: color_target(target, blend.then_some(wgpu::BlendState::ALPHA_BLENDING)),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::validate_wgsl;
    #[cfg(target_os = "macos")]
    use crate::test_support::{Readback, clear_pass, metal_device, texture_2d};
    #[test]
    fn material_shader_and_layout_validate() {
        assert_eq!(size_of::<MaterialVertex>(), 96);
        assert_eq!(size_of::<MaterialParameters>(), 176);
        validate_wgsl(
            include_str!("../shaders/material_view.wgsl"),
            wgpu::naga::valid::Capabilities::empty(),
        );
    }
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "actual Metal material draw, alpha, culling, lighting and root leases"]
    fn material_draw_metal() {
        let (device, queue) = metal_device(wgpu::Features::FLOAT32_FILTERABLE);
        let budget = MemoryBudget::new(1024 * 1024);
        let uploader = MaterialTextureUploader::new(&device, &queue);
        let white = uploader
            .upload_rgba8(
                &budget,
                [1, 1],
                &[255; 4],
                false,
                Default::default(),
                || false,
            )
            .unwrap();
        let white_color = uploader
            .upload_rgba8(&budget, [1, 1], &[255; 4], true, Default::default(), || {
                false
            })
            .unwrap();
        let neutral = [&white_color, &white, &white, &white, &white_color];
        let renderer =
            MaterialRenderer::new(uploader.clone(), wgpu::TextureFormat::Rgba8UnormSrgb).unwrap();
        assert!(
            MaterialRenderer::new(uploader.clone(), wgpu::TextureFormat::Bc1RgbaUnormSrgb).is_err()
        );
        let target = texture_2d(
            &device,
            wgpu::Extent3d {
                width: 3,
                height: 3,
                depth_or_array_layers: 1,
            },
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture_2d(
            &device,
            target.size(),
            crate::pipeline::DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let vertex = MaterialVertex {
            position: [0.; 3],
            normal: [0., 0., 1.],
            tangent: [1., 0., 0., 1.],
            color: [1.; 4],
            texcoords: [[0.5; 2]; 5],
        };
        let vertices = [[-1., -1., 0.5], [3., -1., 0.5], [-1., 3., 0.5]]
            .map(|position| MaterialVertex { position, ..vertex });
        let render = |draw: &MaterialDraw| {
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let view = target.create_view(&Default::default());
                let dv = depth.create_view(&Default::default());
                let mut pass = clear_pass(&mut encoder, &view, &dv);
                renderer.encode(draw, &mut pass).unwrap();
            }
            let readback = Readback::record(&device, &mut encoder, &target, 0, 4);
            queue.submit([encoder.finish()]);
            draw.resource_lease().retain_until_complete(&queue);
            // The centre texel of the 3x3 target.
            let pixel: [u8; 4] = readback.read(&device)[16..20].try_into().unwrap();
            pixel
        };
        let mut p = MaterialParameters::default();
        p.light_direction[3] = 0.;
        p.eye[3] = 0.;
        p.emissive = [0.25, 0.5, 1., 0.];
        p.base_color[3] = 0.25;
        for (mode, expected) in [
            (MaterialAlpha::Opaque, [137, 188, 255, 255]),
            (MaterialAlpha::Mask, [0, 0, 0, 0]),
            (MaterialAlpha::Blend, [71, 99, 137, 64]),
        ] {
            let draw = renderer
                .prepare(&budget, &vertices, p, neutral, mode, false, false, || false)
                .unwrap();
            let actual = render(&draw);
            for c in 0..4 {
                assert!(
                    actual[c].abs_diff(expected[c]) <= 1,
                    "{mode:?}: {actual:?}/{expected:?}"
                );
            }
            eprintln!("material {mode:?}: {actual:?}");
        }
        let mut cutoff = p;
        cutoff.alpha[0] = 0.25;
        let equal = renderer
            .prepare(
                &budget,
                &vertices,
                cutoff,
                neutral,
                MaterialAlpha::Mask,
                false,
                false,
                || false,
            )
            .unwrap();
        assert_eq!(render(&equal), [137, 188, 255, 255]);
        drop(equal);
        cutoff.alpha[0] = 0.25001;
        let above = renderer
            .prepare(
                &budget,
                &vertices,
                cutoff,
                neutral,
                MaterialAlpha::Mask,
                false,
                false,
                || false,
            )
            .unwrap();
        assert_eq!(render(&above), [0; 4]);
        drop(above);
        p.emissive = [0.; 4];
        p.light_direction[3] = 1.;
        p.base_color = [0.5, 0.25, 0.125, 1.];
        p.factors[0] = 0.;
        let draw = renderer
            .prepare(
                &budget,
                &vertices,
                p,
                neutral,
                MaterialAlpha::Opaque,
                false,
                false,
                || false,
            )
            .unwrap();
        let actual = render(&draw);
        for (c, numerator) in [0.49f64, 0.25, 0.13].into_iter().enumerate() {
            let v = numerator / std::f64::consts::PI;
            let expected = ((1.055 * v.powf(1. / 2.4) - 0.055) * 255.).round() as u8;
            assert!(
                actual[c].abs_diff(expected) <= 1,
                "direct GGX channel {c}: {actual:?}/{expected}"
            );
        }
        eprintln!("material dielectric direct GGX: {actual:?}");
        drop(draw);
        let mut normal_p = p;
        normal_p.alpha[2] = 1.;
        normal_p.light_direction = [1., 1., 1., 1.];
        for frame in 0..3 {
            let mapped = vertices.map(|mut v| {
                v.texcoords[2] = [(v.position[0] + 1.) / 2., (v.position[1] + 1.) / 2.];
                if frame == 1 {
                    v.tangent = [0.; 4];
                }
                if frame == 2 {
                    v.normal = [0.; 3];
                    v.tangent = [0., 1., 0., 1.];
                }
                v
            });
            let draw = renderer
                .prepare(
                    &budget,
                    &mapped,
                    normal_p,
                    neutral,
                    MaterialAlpha::Opaque,
                    false,
                    false,
                    || false,
                )
                .unwrap();
            let actual = render(&draw);
            let expected = [110u8, 80, 58, 255];
            for c in 0..4 {
                assert!(
                    actual[c].abs_diff(expected[c]) <= 1,
                    "normal frame={frame}: {actual:?}"
                );
            }
            eprintln!("normal frame={frame}: {actual:?}");
        }
        let color16 = uploader
            .upload_rgba16(
                &budget,
                [1, 1],
                &[65535; 4],
                true,
                Default::default(),
                || false,
            )
            .unwrap();
        let data16 = uploader
            .upload_rgba16(
                &budget,
                [1, 1],
                &[65535; 4],
                false,
                Default::default(),
                || false,
            )
            .unwrap();
        let draw16 = renderer
            .prepare(
                &budget,
                &vertices,
                p,
                [&color16, &data16, &data16, &data16, &color16],
                MaterialAlpha::Opaque,
                false,
                false,
                || false,
            )
            .unwrap();
        assert_eq!(render(&draw16), actual);
        eprintln!("16-bit source float32 texture bindings reproduce the RGBA8 white-map draw");
        drop(draw16);
        drop(color16);
        drop(data16);
        let backwards = [vertices[0], vertices[2], vertices[1]];
        for (double, clockwise, visible) in [
            (false, false, false),
            (true, false, true),
            (false, true, true),
        ] {
            let draw = renderer
                .prepare(
                    &budget,
                    &backwards,
                    p,
                    neutral,
                    MaterialAlpha::Opaque,
                    double,
                    clockwise,
                    || false,
                )
                .unwrap();
            let pixel = render(&draw);
            assert_eq!(pixel[3] != 0, visible);
            if double {
                assert_eq!(pixel[..3], [0, 0, 0]);
            }
            if clockwise {
                assert_eq!(pixel, actual);
            }
        }
        // Linear HDR target preserves radiance above 1; no SDR transfer/tone map.
        let hdr_renderer =
            MaterialRenderer::new(uploader.clone(), wgpu::TextureFormat::Rgba16Float).unwrap();
        let hdr = texture_2d(
            &device,
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let hdr_depth = texture_2d(
            &device,
            hdr.size(),
            crate::pipeline::DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let mut hdr_p = p;
        hdr_p.light_direction[3] = 16.;
        let hdr_draw = hdr_renderer
            .prepare(
                &budget,
                &vertices,
                hdr_p,
                neutral,
                MaterialAlpha::Opaque,
                false,
                false,
                || false,
            )
            .unwrap();
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let view = hdr.create_view(&Default::default());
            let depth_view = hdr_depth.create_view(&Default::default());
            let mut pass = clear_pass(&mut encoder, &view, &depth_view);
            hdr_renderer.encode(&hdr_draw, &mut pass).unwrap();
        }
        let readback = Readback::record(&device, &mut encoder, &hdr, 0, 8);
        queue.submit([encoder.finish()]);
        let lease = hdr_draw.resource_lease();
        drop(hdr_draw);
        lease.retain_until_complete(&queue);
        let mapped = readback.read(&device);
        let mut radiance = [0f64; 4];
        for (c, value) in radiance.iter_mut().enumerate() {
            let bits = u16::from_ne_bytes(mapped[2 * c..2 * c + 2].try_into().unwrap());
            let e = (bits >> 10) & 31;
            let f = bits & 1023;
            assert_eq!(bits >> 15, 0);
            assert!(e < 31);
            *value = if e == 0 {
                f64::from(f) * 2f64.powi(-24)
            } else {
                (1. + f64::from(f) / 1024.) * 2f64.powi(i32::from(e) - 15)
            };
        }
        for (c, numerator) in [0.49, 0.25, 0.13].into_iter().enumerate() {
            assert!((radiance[c] - 16. * numerator / std::f64::consts::PI).abs() < 0.002);
        }
        assert_eq!(radiance[3], 1.);
        assert!(radiance[0] > 2.);
        eprintln!("linear HDR material radiance: {radiance:?}");
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let resident = budget.used();
        let tiny = budget.child(463); // 3*96 + 176 = 464 bytes required.
        assert!(matches!(
            renderer.prepare(
                &tiny,
                &vertices,
                p,
                neutral,
                MaterialAlpha::Opaque,
                false,
                false,
                || false
            ),
            Err(MaterialRenderError::Memory(_))
        ));
        assert_eq!(budget.used(), resident);
        let exact = budget.child(464);
        let exact_draw = renderer
            .prepare(
                &exact,
                &vertices,
                p,
                neutral,
                MaterialAlpha::Opaque,
                false,
                false,
                || false,
            )
            .unwrap();
        assert_eq!(exact.used(), 464);
        let held = exact_draw.resource_lease();
        drop(exact_draw);
        assert_eq!(exact.used(), 464);
        drop(held);
        assert_eq!(exact.used(), 0);
        assert_eq!(budget.used(), resident);
        let mut checks = 0;
        assert!(matches!(
            renderer.prepare(
                &budget,
                &vertices,
                p,
                neutral,
                MaterialAlpha::Opaque,
                false,
                false,
                || {
                    checks += 1;
                    checks == 4
                }
            ),
            Err(MaterialRenderError::Cancelled)
        ));
        assert_eq!(budget.used(), resident);
        assert!(matches!(
            renderer.prepare(
                &budget,
                &vertices,
                p,
                [&white; 5],
                MaterialAlpha::Opaque,
                false,
                false,
                || false
            ),
            Err(MaterialRenderError::Invalid)
        ));
        let mut invalid = vertices;
        invalid[0].position[0] = f32::NAN;
        assert!(matches!(
            renderer.prepare(
                &budget,
                &invalid,
                p,
                neutral,
                MaterialAlpha::Opaque,
                false,
                false,
                || false
            ),
            Err(MaterialRenderError::Invalid)
        ));
        invalid[0].position[0] = 1e20;
        assert!(matches!(
            renderer.prepare(
                &budget,
                &invalid,
                p,
                neutral,
                MaterialAlpha::Opaque,
                false,
                false,
                || false
            ),
            Err(MaterialRenderError::Invalid)
        ));
        assert_eq!(budget.used(), resident);
        let foreign = MaterialRenderer::new(
            MaterialTextureUploader::new(&device, &queue),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        )
        .unwrap();
        assert!(matches!(
            foreign.prepare(
                &budget,
                &vertices,
                p,
                neutral,
                MaterialAlpha::Opaque,
                false,
                false,
                || false
            ),
            Err(MaterialRenderError::Owner)
        ));
        drop(white);
        drop(white_color);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(budget.used(), 0);
    }
}
