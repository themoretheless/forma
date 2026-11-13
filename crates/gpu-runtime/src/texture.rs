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
    R16Uint,
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Rgba8Snorm,
    Rgba8Uint,
    Rgba8Sint,
    Bgra8Unorm,
    Bgra8UnormSrgb,
    R32Uint,
    R32Sint,
    R32Float,
    Rg32Uint,
    Rg32Sint,
    Rg32Float,
    Rgba16Uint,
    Rgba16Sint,
    Rgba16Float,
    Rgba32Uint,
    Rgba32Sint,
    Rgba32Float,
    /// BC1 block compression, sRGB. Sampled only; never a render target.
    Bc1RgbaUnormSrgb,
    Depth24Plus,
    Depth32Float,
}

impl TextureFormat {
    pub(crate) const fn to_mtl_pixel_format(self) -> u64 {
        match self {
            Self::R8Unorm => 10,           // MTLPixelFormatR8Unorm
            Self::R16Uint => 23,           // MTLPixelFormatR16Uint
            Self::Rgba8Unorm => 70,        // MTLPixelFormatRGBA8Unorm
            Self::Rgba8UnormSrgb => 71,    // MTLPixelFormatRGBA8Unorm_sRGB
            Self::Rgba8Snorm => 72,        // MTLPixelFormatRGBA8Snorm
            Self::Rgba8Uint => 73,         // MTLPixelFormatRGBA8Uint
            Self::Rgba8Sint => 74,         // MTLPixelFormatRGBA8Sint
            Self::R32Uint => 53,           // MTLPixelFormatR32Uint
            Self::R32Sint => 54,           // MTLPixelFormatR32Sint
            Self::Rg32Uint => 103,         // MTLPixelFormatRG32Uint
            Self::Rg32Sint => 104,         // MTLPixelFormatRG32Sint
            Self::Rgba16Uint => 113,       // MTLPixelFormatRGBA16Uint
            Self::Rgba16Sint => 114,       // MTLPixelFormatRGBA16Sint
            Self::Rgba32Uint => 123,       // MTLPixelFormatRGBA32Uint
            Self::Rgba32Sint => 124,       // MTLPixelFormatRGBA32Sint
            Self::Bgra8Unorm => 80,        // MTLPixelFormatBGRA8Unorm
            Self::Bgra8UnormSrgb => 81,    // MTLPixelFormatBGRA8Unorm_sRGB
            Self::R32Float => 55,          // MTLPixelFormatR32Float
            Self::Rg32Float => 105,        // MTLPixelFormatRG32Float
            Self::Rgba16Float => 115,      // MTLPixelFormatRGBA16Float
            Self::Rgba32Float => 125,      // MTLPixelFormatRGBA32Float
            Self::Bc1RgbaUnormSrgb => 131, // MTLPixelFormatBC1_RGBA_sRGB
            Self::Depth24Plus => 252,      // MTLPixelFormatDepth32Float
            Self::Depth32Float => 252,     // MTLPixelFormatDepth32Float
        }
    }

    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::R8Unorm => 1,
            Self::R16Uint => 2,
            Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb
            | Self::Rgba8Snorm
            | Self::Rgba8Uint
            | Self::Rgba8Sint
            | Self::R32Uint
            | Self::R32Sint
            | Self::R32Float
            | Self::Depth24Plus
            | Self::Depth32Float => 4,
            Self::Rg32Uint
            | Self::Rg32Sint
            | Self::Rg32Float
            | Self::Rgba16Uint
            | Self::Rgba16Sint
            | Self::Rgba16Float => 8,
            Self::Rgba32Uint | Self::Rgba32Sint | Self::Rgba32Float => 16,
            // Block compressed: 8 bytes per 4x4 block, so there is no whole byte count per pixel.
            Self::Bc1RgbaUnormSrgb => 0,
        }
    }

    pub const fn is_srgb(&self) -> bool {
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
    pub(crate) raw: ObjCObject,
    pub(crate) size: Extent3d,
    pub(crate) mip_level_count: u32,
    pub(crate) sample_count: u32,
    pub(crate) dimension: TextureDimension,
    pub(crate) format: TextureFormat,
    pub(crate) usage: TextureUsages,
    pub(crate) storage_mode: u64,
}

impl Texture {
    /// Why a copy region is not inside mip `mip_level` of this texture, if it is not.
    pub(crate) fn region_error(
        &self,
        mip_level: u32,
        origin: Origin3d,
        size: Extent3d,
    ) -> Option<String> {
        let mip_extent = |extent: u32| (extent >> mip_level.min(31)).max(1);
        let layers = if self.dimension == TextureDimension::D3 {
            mip_extent(self.size.depth_or_array_layers)
        } else {
            self.size.depth_or_array_layers.max(1)
        };
        let inside = |start: u32, length: u32, limit: u32| {
            start.checked_add(length).is_some_and(|end| end <= limit)
        };
        let fits = mip_level < self.mip_level_count.max(1)
            && inside(origin.x, size.width, mip_extent(self.size.width))
            && inside(origin.y, size.height, mip_extent(self.size.height))
            && inside(origin.z, size.depth_or_array_layers.max(1), layers);
        (!fits).then(|| {
            format!(
                "region {size:?} at {origin:?} of mip {mip_level} is outside a texture of {:?}",
                self.size
            )
        })
    }

    /// Why `data_len` bytes laid out by `layout` do not hold a region of `size` texels, if they
    /// do not. The last row of the last image may stop at its last texel.
    pub(crate) fn data_error(
        &self,
        data_len: usize,
        layout: &TexelCopyBufferLayout,
        size: Extent3d,
    ) -> Option<String> {
        let texel = u64::from(self.format.bytes_per_pixel());
        let stride = u64::from(layout.row_stride(size.width, self.format));
        // A block-compressed format has no byte count per texel: its rows are rows of 4x4 blocks
        // and each takes the whole stride.
        let (rows, last_row) = if texel == 0 {
            (u64::from(size.height.div_ceil(4)), stride)
        } else {
            (u64::from(size.height), u64::from(size.width) * texel)
        };
        let images = u64::from(size.depth_or_array_layers.max(1));
        // `None` when the byte count does not fit in 64 bits, which no data can hold.
        let needed = (|| {
            let image = rows
                .saturating_sub(1)
                .checked_mul(stride)?
                .checked_add(last_row)?;
            let image_stride =
                u64::from(layout.rows_per_image.unwrap_or(size.height)).checked_mul(stride)?;
            (images - 1)
                .checked_mul(image_stride)?
                .checked_add(image)?
                .checked_add(layout.offset)
        })();
        match needed {
            Some(needed) if needed <= data_len as u64 => None,
            _ => Some(format!(
                "{data_len} bytes with layout {layout:?} do not hold a region of {size:?}"
            )),
        }
    }

    pub fn create_view(&self, desc: &TextureViewDescriptor<'_>) -> TextureView {
        TextureView {
            texture: self.reinterpreted_as(desc.dimension),
            base_mip_level: desc.base_mip_level,
            base_array_layer: desc.base_array_layer,
        }
    }

    /// The texture seen through a view of `dimension`.
    ///
    /// Metal binds textures by type, so a single-layer 2D texture cannot be bound to a
    /// `texture2d_array` parameter (WebGPU allows this through a `D2Array` view). Such views
    /// get a Metal texture view of the requested type; every other view shares the texture.
    fn reinterpreted_as(&self, dimension: Option<crate::TextureViewDimension>) -> Texture {
        use crate::TextureViewDimension as V;
        let mtl_type = match (
            dimension,
            self.dimension,
            self.size.depth_or_array_layers,
            self.sample_count,
        ) {
            // MTLTextureType2DArray
            (Some(V::D2Array), TextureDimension::D2, 1, 1) => 3u64,
            _ => return self.clone(),
        };
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        unsafe {
            use std::ffi::c_void;
            let s = crate::metal::selectors();
            #[repr(C)]
            struct NsRange {
                location: usize,
                length: usize,
            }
            let f: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                u64,
                u64,
                NsRange,
                NsRange,
            ) -> *mut c_void = crate::metal::msg_send();
            let view = f(
                self.raw.as_ptr(),
                s.new_texture_view_with_type,
                self.format.to_mtl_pixel_format(),
                mtl_type,
                NsRange {
                    location: 0,
                    length: self.mip_level_count.max(1) as usize,
                },
                NsRange {
                    location: 0,
                    length: 1,
                },
            );
            if let Some(raw) = ObjCObject::from_retained(view) {
                return Texture {
                    raw,
                    ..self.clone()
                };
            }
        }
        let _ = mtl_type;
        self.clone()
    }

    pub fn size(&self) -> Extent3d {
        self.size
    }

    pub fn width(&self) -> u32 {
        self.size.width
    }

    pub fn height(&self) -> u32 {
        self.size.height
    }

    pub fn depth_or_array_layers(&self) -> u32 {
        self.size.depth_or_array_layers
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

impl TexelCopyBufferLayout {
    /// The row stride, defaulting to tightly packed rows `width` texels wide.
    pub(crate) fn row_stride(&self, width: u32, format: TextureFormat) -> u32 {
        self.bytes_per_row
            .unwrap_or(width * format.bytes_per_pixel())
    }
}

/// How a buffer copy of `layers` images addresses a texture: a 3D texture takes one region
/// `layers` deep, any other texture one single-depth region per array layer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CopySlices {
    pub(crate) count: usize,
    pub(crate) depth: usize,
    is_3d: bool,
    origin_z: usize,
}

impl CopySlices {
    pub(crate) fn new(dimension: TextureDimension, origin_z: u32, layers: u32) -> Self {
        let is_3d = dimension == TextureDimension::D3;
        let layers = layers.max(1) as usize;
        let (count, depth) = if is_3d { (1, layers) } else { (layers, 1) };
        Self {
            count,
            depth,
            is_3d,
            origin_z: origin_z as usize,
        }
    }

    pub(crate) fn is_3d(&self) -> bool {
        self.is_3d
    }

    /// The array slice that copy slice `index` writes.
    pub(crate) fn array_slice(&self, index: usize) -> usize {
        if self.is_3d { 0 } else { self.origin_z + index }
    }

    /// The `z` of every region's origin.
    pub(crate) fn region_z(&self) -> usize {
        if self.is_3d { self.origin_z } else { 0 }
    }
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
    pub(crate) timestamps_ns: Arc<Mutex<Vec<u64>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_slices_address_3d_depth_and_array_layers() {
        let volume = CopySlices::new(TextureDimension::D3, 2, 4);
        assert_eq!((volume.count, volume.depth, volume.region_z()), (1, 4, 2));
        assert_eq!(volume.array_slice(0), 0);

        let layers = CopySlices::new(TextureDimension::D2, 2, 3);
        assert_eq!((layers.count, layers.depth, layers.region_z()), (3, 1, 0));
        assert_eq!(
            (0..3).map(|i| layers.array_slice(i)).collect::<Vec<_>>(),
            [2, 3, 4]
        );

        let empty = CopySlices::new(TextureDimension::D2, 0, 0);
        assert_eq!((empty.count, empty.depth), (1, 1));
    }

    #[test]
    fn row_stride_defaults_to_packed_rows() {
        let packed = TexelCopyBufferLayout::default();
        assert_eq!(packed.row_stride(3, TextureFormat::Rgba8Unorm), 12);
        let padded = TexelCopyBufferLayout {
            bytes_per_row: Some(256),
            ..Default::default()
        };
        assert_eq!(padded.row_stride(3, TextureFormat::Rgba8Unorm), 256);
    }
}
