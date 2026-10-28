//! GPU Texture, TextureView, TextureFormat, Sampler, Extent3d, Origin3d, and QuerySet types.

use crate::metal::ObjCObject;
use crate::{Buffer, BufferAddress, CompareFunction, Label, TextureUsages, TextureViewDimension};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureDimension {
    D1,
    #[default]
    D2,
    D3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    R8Unorm,
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Bgra8Unorm,
    Bgra8UnormSrgb,
    R32Float,
    Rg32Float,
    Rgba16Float,
    Rgba32Float,
    Depth24Plus,
    Depth32Float,
}

impl TextureFormat {
    pub(crate) const fn to_mtl_pixel_format(self) -> u64 {
        match self {
            Self::R8Unorm => 10,          // MTLPixelFormatR8Unorm
            Self::Rgba8Unorm => 70,       // MTLPixelFormatRGBA8Unorm
            Self::Rgba8UnormSrgb => 71,   // MTLPixelFormatRGBA8Unorm_sRGB
            Self::Bgra8Unorm => 80,       // MTLPixelFormatBGRA8Unorm
            Self::Bgra8UnormSrgb => 81,   // MTLPixelFormatBGRA8Unorm_sRGB
            Self::R32Float => 55,         // MTLPixelFormatR32Float
            Self::Rg32Float => 105,       // MTLPixelFormatRG32Float
            Self::Rgba16Float => 115,     // MTLPixelFormatRGBA16Float
            Self::Rgba32Float => 125,     // MTLPixelFormatRGBA32Float
            Self::Depth24Plus => 252,     // MTLPixelFormatDepth32Float
            Self::Depth32Float => 252,    // MTLPixelFormatDepth32Float
        }
    }

    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::R8Unorm => 1,
            Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb
            | Self::R32Float
            | Self::Depth24Plus
            | Self::Depth32Float => 4,
            Self::Rg32Float | Self::Rgba16Float => 8,
            Self::Rgba32Float => 16,
        }
    }

    pub const fn is_srgb(self) -> bool {
        matches!(self, Self::Rgba8UnormSrgb | Self::Bgra8UnormSrgb)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent3d {
    pub width: u32,
    pub height: u32,
    pub depth_or_array_layers: u32,
}

impl Default for Extent3d {
    fn default() -> Self {
        Self {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Origin3d {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

impl Origin3d {
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureAspect {
    #[default]
    All,
    StencilOnly,
    DepthOnly,
}

#[derive(Debug, Clone)]
pub struct TextureDescriptor<'a> {
    pub label: Label<'a>,
    pub size: Extent3d,
    pub mip_level_count: u32,
    pub sample_count: u32,
    pub dimension: TextureDimension,
    pub format: TextureFormat,
    pub usage: TextureUsages,
    pub view_formats: &'a [TextureFormat],
}

#[derive(Debug, Clone)]
pub struct Texture {
    pub(crate) raw: Arc<ObjCObject>,
    pub(crate) size: Extent3d,
    pub(crate) mip_level_count: u32,
    pub(crate) sample_count: u32,
    pub(crate) dimension: TextureDimension,
    pub(crate) format: TextureFormat,
    pub(crate) usage: TextureUsages,
    pub(crate) storage_mode: u64,
}

impl Texture {
    pub fn create_view(&self, desc: &TextureViewDescriptor<'_>) -> TextureView {
        TextureView {
            texture: self.clone(),
            base_mip_level: desc.base_mip_level,
            base_array_layer: desc.base_array_layer,
        }
    }

    pub fn size(&self) -> Extent3d {
        self.size
    }

    pub fn format(&self) -> TextureFormat {
        self.format
    }

    pub fn mip_level_count(&self) -> u32 {
        self.mip_level_count
    }

    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    pub fn dimension(&self) -> TextureDimension {
        self.dimension
    }

    pub fn usage(&self) -> TextureUsages {
        self.usage
    }

    pub fn destroy(&self) {}

    pub fn as_image_copy(&self) -> TexelCopyTextureInfo<'_> {
        TexelCopyTextureInfo {
            texture: self,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TextureViewDescriptor<'a> {
    pub label: Label<'a>,
    pub format: Option<TextureFormat>,
    pub dimension: Option<TextureViewDimension>,
    pub usage: Option<TextureUsages>,
    pub aspect: TextureAspect,
    pub base_mip_level: u32,
    pub mip_level_count: Option<u32>,
    pub base_array_layer: u32,
    pub array_layer_count: Option<u32>,
}

#[derive(Clone)]
pub struct TextureView {
    pub(crate) texture: Texture,
    pub(crate) base_mip_level: u32,
    pub(crate) base_array_layer: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AddressMode {
    #[default]
    ClampToEdge,
    Repeat,
    MirrorRepeat,
    ClampToBorder,
}

impl AddressMode {
    pub(crate) const fn to_mtl(self) -> u64 {
        match self {
            Self::ClampToEdge => 0,
            Self::MirrorRepeat => 3,
            Self::Repeat => 2,
            Self::ClampToBorder => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    #[default]
    Nearest,
    Linear,
}

impl FilterMode {
    pub(crate) const fn to_mtl(self) -> u64 {
        match self {
            Self::Nearest => 0,
            Self::Linear => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MipmapFilterMode {
    #[default]
    Nearest,
    Linear,
}

#[derive(Debug, Clone)]
pub struct SamplerDescriptor<'a> {
    pub label: Label<'a>,
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub address_mode_w: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_filter: MipmapFilterMode,
    pub lod_min_clamp: f32,
    pub lod_max_clamp: f32,
    pub compare: Option<CompareFunction>,
    pub anisotropy_clamp: u16,
    pub border_color: Option<()>,
}

impl Default for SamplerDescriptor<'_> {
    fn default() -> Self {
        Self {
            label: None,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 32.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        }
    }
}

#[derive(Clone)]
pub struct Sampler {
    pub(crate) raw: Arc<ObjCObject>,
}

#[derive(Debug, Clone, Copy)]
pub struct TexelCopyTextureInfo<'a> {
    pub texture: &'a Texture,
    pub mip_level: u32,
    pub origin: Origin3d,
    pub aspect: TextureAspect,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TexelCopyBufferLayout {
    pub offset: BufferAddress,
    pub bytes_per_row: Option<u32>,
    pub rows_per_image: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct TexelCopyBufferInfo<'a> {
    pub buffer: &'a Buffer,
    pub layout: TexelCopyBufferLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    Occlusion,
    Timestamp,
}

#[derive(Debug, Clone)]
pub struct QuerySetDescriptor<'a> {
    pub label: Label<'a>,
    pub ty: QueryType,
    pub count: u32,
}

#[derive(Clone)]
pub struct QuerySet {
    pub(crate) count: u32,
    pub(crate) timestamps_ns: Arc<Mutex<Vec<u64>>>,
}
