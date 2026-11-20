//! Portable `upstream-wgpu` surface creation, configuration, and frame presentation.

use crate::portable::convert;
use crate::{
    CurrentSurfaceTexture, RequestDeviceError, SurfaceCapabilities, SurfaceConfiguration,
    SurfaceTexture, Texture,
};
use std::sync::Arc;

pub(crate) fn create_surface<'a, W>(
    instance: &upstream_wgpu::Instance,
    target: W,
) -> Result<upstream_wgpu::Surface<'a>, RequestDeviceError>
where
    W: raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle + Send + Sync + 'a,
{
    instance
        .create_surface(target)
        .map_err(|err| RequestDeviceError(err.to_string()))
}

pub(crate) fn get_capabilities(
    surface: &upstream_wgpu::Surface<'_>,
    adapter: &upstream_wgpu::Adapter,
) -> SurfaceCapabilities {
    convert::from_wgpu_surface_capabilities(surface.get_capabilities(adapter))
}

pub(crate) fn get_default_config(
    surface: &upstream_wgpu::Surface<'_>,
    adapter: &upstream_wgpu::Adapter,
    width: u32,
    height: u32,
) -> Option<SurfaceConfiguration> {
    let caps = get_capabilities(surface, adapter);
    let format = *caps.formats.first()?;
    let present_mode = caps
        .present_modes
        .first()
        .copied()
        .unwrap_or(crate::PresentMode::Fifo);
    let alpha_mode = caps
        .alpha_modes
        .first()
        .copied()
        .unwrap_or(crate::CompositeAlphaMode::Auto);
    let wgpu_default = surface.get_default_config(adapter, width.max(1), height.max(1))?;
    Some(SurfaceConfiguration {
        usage: crate::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: wgpu_default.width,
        height: wgpu_default.height,
        present_mode,
        desired_maximum_frame_latency: wgpu_default.desired_maximum_frame_latency,
        alpha_mode,
        view_formats: Vec::new(),
        color_space: crate::SurfaceColorSpace::Auto,
    })
}

pub(crate) fn configure(
    surface: &upstream_wgpu::Surface<'_>,
    device: &upstream_wgpu::Device,
    config: &SurfaceConfiguration,
) {
    surface.configure(device, &convert::to_wgpu_surface_config(config));
}

pub(crate) fn get_current_texture(
    surface: &upstream_wgpu::Surface<'_>,
    device: &Arc<crate::portable::PortableDevice>,
    config: &SurfaceConfiguration,
) -> CurrentSurfaceTexture {
    match surface.get_current_texture() {
        upstream_wgpu::CurrentSurfaceTexture::Success(st) => {
            let texture = Texture::from_surface_portable(&st.texture, config);
            CurrentSurfaceTexture::Success(SurfaceTexture::from_portable(
                texture,
                st,
                false,
                device.clone(),
            ))
        }
        upstream_wgpu::CurrentSurfaceTexture::Suboptimal(st) => {
            let texture = Texture::from_surface_portable(&st.texture, config);
            CurrentSurfaceTexture::Suboptimal(SurfaceTexture::from_portable(
                texture,
                st,
                true,
                device.clone(),
            ))
        }
        upstream_wgpu::CurrentSurfaceTexture::Timeout => CurrentSurfaceTexture::Timeout,
        upstream_wgpu::CurrentSurfaceTexture::Occluded => CurrentSurfaceTexture::Occluded,
        upstream_wgpu::CurrentSurfaceTexture::Outdated => CurrentSurfaceTexture::Outdated,
        upstream_wgpu::CurrentSurfaceTexture::Lost => CurrentSurfaceTexture::Lost,
        upstream_wgpu::CurrentSurfaceTexture::Validation => CurrentSurfaceTexture::Validation,
    }
}

pub(crate) fn from_portable_texture_ref(
    raw: &upstream_wgpu::Texture,
    cfg: &SurfaceConfiguration,
) -> Texture {
    Texture::from_raw_portable(raw.clone(), cfg)
}
