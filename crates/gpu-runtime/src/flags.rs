//! Bitflags macro and GPU capability / usage bitflags (`Backends`, `InstanceFlags`, `Features`, `BufferUsages`, `TextureUsages`, `ShaderStages`, `ColorWrites`).

macro_rules! impl_bitflags {
    ($name:ident, $ty:ty) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name(pub $ty);

        impl $name {
            #[inline]
            pub const fn empty() -> Self {
                Self(0)
            }
            #[inline]
            pub const fn all() -> Self {
                Self(!0)
            }
            #[inline]
            pub const fn bits(self) -> $ty {
                self.0
            }
            #[inline]
            pub const fn from_bits_truncate(bits: $ty) -> Self {
                Self(bits)
            }
            #[inline]
            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }
            #[inline]
            pub const fn contains(self, other: Self) -> bool {
                (self.0 & other.0) == other.0
            }
            #[inline]
            pub const fn intersects(self, other: Self) -> bool {
                (self.0 & other.0) != 0
            }
            #[inline]
            pub const fn union(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}(0x{:x})", stringify!($name), self.0)
            }
        }

        impl std::ops::BitOr for $name {
            type Output = Self;
            #[inline]
            fn bitor(self, rhs: Self) -> Self {
                Self(self.0 | rhs.0)
            }
        }

        impl std::ops::BitOrAssign for $name {
            #[inline]
            fn bitor_assign(&mut self, rhs: Self) {
                self.0 |= rhs.0;
            }
        }

        impl std::ops::BitAnd for $name {
            type Output = Self;
            #[inline]
            fn bitand(self, rhs: Self) -> Self {
                Self(self.0 & rhs.0)
            }
        }

        impl std::ops::BitAndAssign for $name {
            #[inline]
            fn bitand_assign(&mut self, rhs: Self) {
                self.0 &= rhs.0;
            }
        }

        impl std::ops::Sub for $name {
            type Output = Self;
            #[inline]
            fn sub(self, rhs: Self) -> Self {
                Self(self.0 & !rhs.0)
            }
        }

        impl std::ops::Not for $name {
            type Output = Self;
            #[inline]
            fn not(self) -> Self {
                Self(!self.0)
            }
        }
    };
}

impl_bitflags!(Backends, u32);
impl Backends {
    pub const VULKAN: Self = Self(1 << 0);
    pub const GL: Self = Self(1 << 1);
    pub const METAL: Self = Self(1 << 2);
    pub const DX12: Self = Self(1 << 3);
    pub const BROWSER_WEBGPU: Self = Self(1 << 4);
    pub const CUDA: Self = Self(1 << 5);
    pub const PRIMARY: Self = Self(
        Self::VULKAN.0
            | Self::METAL.0
            | Self::DX12.0
            | Self::BROWSER_WEBGPU.0
            | Self::CUDA.0,
    );

    pub fn from_env() -> Option<Self> {
        let val = std::env::var("WGPU_BACKEND").ok()?;
        let mut out = Self::empty();
        for part in val.split(',') {
            match part.trim().to_ascii_lowercase().as_str() {
                "vulkan" | "vk" => out |= Self::VULKAN,
                "metal" | "mtl" => out |= Self::METAL,
                "dx12" | "d3d12" => out |= Self::DX12,
                "gl" | "opengl" | "gles" => out |= Self::GL,
                "webgpu" | "browser" => out |= Self::BROWSER_WEBGPU,
                "cuda" | "ptx" => out |= Self::CUDA,
                "primary" => out |= Self::PRIMARY,
                "all" => out |= Self::all(),
                _ => {}
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}

impl_bitflags!(InstanceFlags, u32);
impl InstanceFlags {
    pub const DEBUG: Self = Self(1 << 0);
    pub const VALIDATION: Self = Self(1 << 1);
    pub const DISCARD_HAL_LABELS: Self = Self(1 << 2);
}

impl_bitflags!(Features, u64);
impl Features {
    pub const TIMESTAMP_QUERY: Self = Self(1 << 0);
    pub const TIMESTAMP_QUERY_INSIDE_ENCODERS: Self = Self(1 << 1);
    pub const TIMESTAMP_QUERY_INSIDE_PASSES: Self = Self(1 << 2);
    pub const MAPPABLE_PRIMARY_BUFFERS: Self = Self(1 << 3);
    pub const TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES: Self = Self(1 << 4);
    pub const MULTI_DRAW_INDIRECT_COUNT: Self = Self(1 << 5);
    pub const SUBGROUP: Self = Self(1 << 6);
    pub const SHADER_F16: Self = Self(1 << 7);
    pub const FLOAT32_FILTERABLE: Self = Self(1 << 8);
    pub const IMMEDIATE_DATA: Self = Self(1 << 9);
    pub const PUSH_CONSTANTS: Self = Self::IMMEDIATE_DATA;
    pub const EXPERIMENTAL_RAY_QUERY: Self = Self(1 << 10);
    pub const EXPERIMENTAL_RAY_TRACING_ACCELERATION_STRUCTURE: Self = Self(1 << 11);
    pub const RAY_TRACING_PIPELINE: Self = Self(1 << 12);
}

impl_bitflags!(BufferUsages, u32);
impl BufferUsages {
    pub const MAP_READ: Self = Self(1 << 0);
    pub const MAP_WRITE: Self = Self(1 << 1);
    pub const COPY_SRC: Self = Self(1 << 2);
    pub const COPY_DST: Self = Self(1 << 3);
    pub const INDEX: Self = Self(1 << 4);
    pub const VERTEX: Self = Self(1 << 5);
    pub const UNIFORM: Self = Self(1 << 6);
    pub const STORAGE: Self = Self(1 << 7);
    pub const INDIRECT: Self = Self(1 << 8);
    pub const QUERY_RESOLVE: Self = Self(1 << 9);
    pub const BLAS_INPUT: Self = Self(1 << 10);
    pub const TLAS_INPUT: Self = Self(1 << 11);
}

impl_bitflags!(TextureUsages, u32);
impl TextureUsages {
    pub const COPY_SRC: Self = Self(1 << 0);
    pub const COPY_DST: Self = Self(1 << 1);
    pub const TEXTURE_BINDING: Self = Self(1 << 2);
    pub const STORAGE_BINDING: Self = Self(1 << 3);
    pub const RENDER_ATTACHMENT: Self = Self(1 << 4);
}

impl_bitflags!(ShaderStages, u32);
impl ShaderStages {
    pub const NONE: Self = Self(0);
    pub const VERTEX: Self = Self(1 << 0);
    pub const FRAGMENT: Self = Self(1 << 1);
    pub const COMPUTE: Self = Self(1 << 2);
    pub const VERTEX_FRAGMENT: Self = Self(Self::VERTEX.0 | Self::FRAGMENT.0);
}

impl_bitflags!(ColorWrites, u32);
impl ColorWrites {
    pub const RED: Self = Self(1 << 0);
    pub const GREEN: Self = Self(1 << 1);
    pub const BLUE: Self = Self(1 << 2);
    pub const ALPHA: Self = Self(1 << 3);
    pub const COLOR: Self = Self(Self::RED.0 | Self::GREEN.0 | Self::BLUE.0);
    pub const ALL: Self = Self(0xF);
}
