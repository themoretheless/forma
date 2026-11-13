//! Surfaces: presentation modes, colour spaces, configuration and frames.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresentMode {
    /// Vsync where available (`Fifo` on Metal).
    AutoVsync,
    /// No vsync where available (`Immediate` on Metal).
    AutoNoVsync,
    #[default]
    Fifo,
    FifoRelaxed,
    Immediate,
    Mailbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompositeAlphaMode {
    #[default]
    Auto,
    Opaque,
    PreMultiplied,
    PostMultiplied,
    Inherit,
}

/// Color space a surface presents in. Mirrors `wgpu::SurfaceColorSpace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SurfaceColorSpace {
    /// Extended linear sRGB for `Rgba16Float`, sRGB otherwise.
    #[default]
    Auto,
    Srgb,
    ExtendedSrgbLinear,
    DisplayP3,
    Bt2100Pq,
    Bt2100Hlg,
    ExtendedSrgb,
    ExtendedDisplayP3,
}

impl SurfaceColorSpace {
    pub const fn to_color_spaces(self) -> Option<crate::SurfaceColorSpaces> {
        use crate::SurfaceColorSpaces as S;
        match self {
            Self::Auto => None,
            Self::Srgb => Some(S::SRGB),
            Self::ExtendedSrgbLinear => Some(S::EXTENDED_SRGB_LINEAR),
            Self::DisplayP3 => Some(S::DISPLAY_P3),
            Self::Bt2100Pq => Some(S::BT2100_PQ),
            Self::Bt2100Hlg => Some(S::BT2100_HLG),
            Self::ExtendedSrgb => Some(S::EXTENDED_SRGB),
            Self::ExtendedDisplayP3 => Some(S::EXTENDED_DISPLAY_P3),
        }
    }

    pub const fn is_hdr(self) -> bool {
        matches!(
            self,
            Self::ExtendedSrgbLinear
                | Self::ExtendedSrgb
                | Self::ExtendedDisplayP3
                | Self::Bt2100Pq
                | Self::Bt2100Hlg
        )
    }

    /// `Auto` resolved for `format`.
    pub const fn resolve(self, format: crate::TextureFormat) -> Self {
        match (self, format) {
            (Self::Auto, crate::TextureFormat::Rgba16Float) => Self::ExtendedSrgbLinear,
            (Self::Auto, _) => Self::Srgb,
            (other, _) => other,
        }
    }
}

/// HDR state of the display showing a surface, as Extended Dynamic Range headroom: the largest
/// linear value above 1.0 (SDR white) the display shows now, and the most it could show.
/// `None` when it cannot be read (no window, or not on the main thread).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DisplayHdrInfo {
    pub headroom: Option<f32>,
    pub potential_headroom: Option<f32>,
}

/// Color spaces a surface can present one format in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceFormatCapabilities {
    pub format: crate::TextureFormat,
    pub color_spaces: crate::SurfaceColorSpaces,
}

#[derive(Debug, Clone)]
pub struct SurfaceCapabilities {
    pub formats: Vec<crate::TextureFormat>,
    pub format_capabilities: Vec<SurfaceFormatCapabilities>,
    pub present_modes: Vec<PresentMode>,
    pub alpha_modes: Vec<CompositeAlphaMode>,
    pub usages: crate::TextureUsages,
}

#[derive(Debug, Clone)]
pub struct SurfaceConfiguration {
    pub usage: crate::TextureUsages,
    pub format: crate::TextureFormat,
    pub width: u32,
    pub height: u32,
    pub present_mode: PresentMode,
    pub desired_maximum_frame_latency: u32,
    pub alpha_mode: CompositeAlphaMode,
    pub view_formats: Vec<crate::TextureFormat>,
    pub color_space: SurfaceColorSpace,
}

pub struct SurfaceTexture {
    pub texture: crate::Texture,
    pub suboptimal: bool,
    pub(crate) drawable: Option<crate::metal::ObjCObject>,
    pub(crate) layer: Option<Arc<crate::metal::ObjCObject>>,
    pub(crate) device: Option<crate::Device>,
}

impl SurfaceTexture {
    pub fn present(self) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let (Some(device), Some(drawable)) = (&self.device, &self.drawable) {
            let layer_ptr = self
                .layer
                .as_ref()
                .map_or(std::ptr::null_mut(), |l| l.as_ptr());
            unsafe {
                device.metal.present_drawable(layer_ptr, drawable.as_ptr());
            }
        }
    }
}

pub enum CurrentSurfaceTexture {
    Success(SurfaceTexture),
    Suboptimal(SurfaceTexture),
    Timeout,
    Occluded,
    Outdated,
    Lost,
    Validation,
}

impl std::fmt::Debug for CurrentSurfaceTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success(_) => write!(f, "Success"),
            Self::Suboptimal(_) => write!(f, "Suboptimal"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Occluded => write!(f, "Occluded"),
            Self::Outdated => write!(f, "Outdated"),
            Self::Lost => write!(f, "Lost"),
            Self::Validation => write!(f, "Validation"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Surface<'a> {
    pub(crate) config: Arc<Mutex<Option<(crate::Device, SurfaceConfiguration)>>>,
    pub(crate) metal_layer: Option<Arc<crate::metal::ObjCObject>>,
    pub(crate) metal_view: Option<Arc<crate::metal::ObjCObject>>,
    pub(crate) _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Surface<'a> {
    pub fn get_capabilities(&self, _adapter: &crate::Adapter) -> SurfaceCapabilities {
        use crate::SurfaceColorSpaces as S;
        let formats = vec![
            crate::TextureFormat::Bgra8Unorm,
            crate::TextureFormat::Rgba8Unorm,
            crate::TextureFormat::Bgra8UnormSrgb,
            crate::TextureFormat::Rgba8UnormSrgb,
            crate::TextureFormat::Rgba16Float,
        ];
        let format_capabilities = formats
            .iter()
            .map(|&format| SurfaceFormatCapabilities {
                format,
                color_spaces: if format == crate::TextureFormat::Rgba16Float {
                    S::SRGB
                        | S::DISPLAY_P3
                        | S::EXTENDED_SRGB_LINEAR
                        | S::EXTENDED_SRGB
                        | S::EXTENDED_DISPLAY_P3
                        | S::BT2100_PQ
                        | S::BT2100_HLG
                } else {
                    S::SRGB | S::DISPLAY_P3
                },
            })
            .collect();
        SurfaceCapabilities {
            formats,
            format_capabilities,
            present_modes: vec![
                PresentMode::AutoVsync,
                PresentMode::AutoNoVsync,
                PresentMode::Fifo,
                PresentMode::Immediate,
                PresentMode::Mailbox,
            ],
            alpha_modes: vec![
                CompositeAlphaMode::Auto,
                CompositeAlphaMode::Opaque,
                CompositeAlphaMode::PostMultiplied,
            ],
            usages: crate::TextureUsages::RENDER_ATTACHMENT
                | crate::TextureUsages::COPY_SRC
                | crate::TextureUsages::COPY_DST,
        }
    }

    /// Reads the HDR headroom of the screen showing this surface. Call on the main thread.
    pub fn display_hdr_info(&self, _adapter: &crate::Adapter) -> DisplayHdrInfo {
        #[cfg(target_os = "macos")]
        if let Some(view) = &self.metal_view {
            return unsafe { crate::metal::display_hdr_info(view.as_ptr()) };
        }
        DisplayHdrInfo::default()
    }

    pub fn get_default_config(
        &self,
        _adapter: &crate::Adapter,
        width: u32,
        height: u32,
    ) -> Option<SurfaceConfiguration> {
        Some(SurfaceConfiguration {
            usage: crate::TextureUsages::RENDER_ATTACHMENT,
            format: crate::TextureFormat::Bgra8Unorm,
            width: width.max(1),
            height: height.max(1),
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
            color_space: SurfaceColorSpace::Auto,
        })
    }

    pub fn configure(&self, device: &crate::Device, config: &SurfaceConfiguration) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let Some(layer) = &self.metal_layer {
            let view_ptr = self
                .metal_view
                .as_ref()
                .map_or(std::ptr::null_mut(), |v| v.as_ptr());
            unsafe {
                device
                    .metal
                    .configure_metal_layer(layer.as_ptr(), view_ptr, config);
            }
        }
        if let Ok(mut guard) = self.config.lock() {
            *guard = Some((device.clone(), config.clone()));
        }
    }

    pub fn get_current_texture(&self) -> CurrentSurfaceTexture {
        let Some((device, cfg)) = self.config.lock().ok().and_then(|g| g.clone()) else {
            return CurrentSurfaceTexture::Outdated;
        };
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let Some(layer) = &self.metal_layer {
            let acquired = unsafe { device.metal.acquire_drawable(layer.as_ptr(), &cfg) };
            match acquired {
                Some((drawable, texture)) => {
                    let suboptimal = texture.size.width != cfg.width.max(1)
                        || texture.size.height != cfg.height.max(1);
                    let surface_tex = SurfaceTexture {
                        texture,
                        suboptimal,
                        drawable: Some(drawable),
                        layer: Some(layer.clone()),
                        device: Some(device),
                    };
                    return if suboptimal {
                        CurrentSurfaceTexture::Suboptimal(surface_tex)
                    } else {
                        CurrentSurfaceTexture::Success(surface_tex)
                    };
                }
                None => return CurrentSurfaceTexture::Timeout,
            }
        }
        let texture = device.create_texture(&crate::TextureDescriptor {
            label: Some("Surface drawable texture"),
            size: crate::Extent3d {
                width: cfg.width.max(1),
                height: cfg.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: crate::TextureDimension::D2,
            format: cfg.format,
            usage: cfg.usage | crate::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        CurrentSurfaceTexture::Success(SurfaceTexture {
            texture,
            suboptimal: false,
            drawable: None,
            layer: None,
            device: Some(device),
        })
    }
}
