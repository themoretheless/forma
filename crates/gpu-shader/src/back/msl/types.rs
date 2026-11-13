//! WGSL types as MSL types.

use super::*;

pub fn msl_type_for_handle(module: &Module, handle: Handle<Type>, packed_vec3: bool) -> String {
    if let Some(name) = &module.types[handle].name {
        return name.clone();
    }
    match &module.types[handle].inner {
        TypeInner::Scalar(s) => msl_scalar(*s).to_string(),
        TypeInner::Vector { size, scalar } => {
            let prefix = match (scalar.kind, scalar.width) {
                (ScalarKind::Float, 2) => "half",
                (ScalarKind::Float, _) => "float",
                (ScalarKind::Sint, _) => "int",
                (ScalarKind::Uint, _) => "uint",
                (ScalarKind::Bool, _) => "bool",
            };
            let n = size.as_u32();
            if packed_vec3 && n == 3 {
                format!("packed_{prefix}3")
            } else {
                format!("{prefix}{n}")
            }
        }
        TypeInner::Matrix {
            columns,
            rows,
            scalar,
        } => {
            let prefix = if scalar.width == 2 { "half" } else { "float" };
            format!("{prefix}{}x{}", columns.as_u32(), rows.as_u32())
        }
        TypeInner::Atomic(s) => {
            if s.kind == ScalarKind::Sint {
                "atomic_int".to_string()
            } else {
                "atomic_uint".to_string()
            }
        }
        TypeInner::Array { base, size, .. } => {
            let elem = msl_type_for_handle(module, *base, false);
            match size {
                ArraySize::Constant(n) => format!("_wgpu_arr<{elem}, {}>", n.get()),
                ArraySize::Dynamic => elem,
            }
        }
        TypeInner::Pointer { base, space } => {
            let elem = msl_type_for_handle(module, *base, false);
            let qual = match space {
                AddressSpace::WorkGroup => "threadgroup",
                AddressSpace::Storage {
                    access: StorageAccess::Read,
                } => "const device",
                AddressSpace::Storage { .. } => "device",
                AddressSpace::Uniform | AddressSpace::Immediate | AddressSpace::PushConstant => {
                    "constant"
                }
                _ => "thread",
            };
            format!("{qual} {elem}*")
        }
        TypeInner::Image {
            dim,
            arrayed,
            class,
        } => {
            let dim_str = match (dim, arrayed) {
                (crate::ImageDimension::D1, false) => "1d",
                (crate::ImageDimension::D1, true) => "1d_array",
                (crate::ImageDimension::D2, false) => "2d",
                (crate::ImageDimension::D2, true) => "2d_array",
                (crate::ImageDimension::D3, _) => "3d",
                (crate::ImageDimension::Cube, false) => "cube",
                (crate::ImageDimension::Cube, true) => "cube_array",
            };
            match class {
                ImageClass::Depth { .. } => format!("depth{dim_str}<float>"),
                ImageClass::Sampled { kind, .. } => {
                    let s = match kind {
                        ScalarKind::Sint => "int",
                        ScalarKind::Uint => "uint",
                        _ => "float",
                    };
                    format!("texture{dim_str}<{s}>")
                }
                ImageClass::Storage { format, access } => {
                    let acc = match access {
                        StorageAccess::Read => "access::read",
                        StorageAccess::Write => "access::write",
                        StorageAccess::ReadWrite => "access::read_write",
                    };
                    let component = match format {
                        crate::StorageFormat::R32Uint => "uint",
                        crate::StorageFormat::Rgba8Unorm
                        | crate::StorageFormat::Rgba32Float
                        | crate::StorageFormat::R32Float => "float",
                    };
                    format!("texture{dim_str}<{component}, {acc}>")
                }
            }
        }
        TypeInner::Sampler { .. } => "sampler".to_string(),
        TypeInner::Struct { .. } => module.types[handle]
            .name
            .clone()
            .unwrap_or_else(|| "uint".to_string()),
    }
}

pub(super) fn msl_param_type(module: &Module, handle: Handle<Type>) -> String {
    match &module.types[handle].inner {
        TypeInner::Pointer { base, space } => {
            let elem = msl_type_for_handle(module, *base, false);
            let qual = match space {
                AddressSpace::WorkGroup => "threadgroup",
                AddressSpace::Storage {
                    access: StorageAccess::Read,
                } => "const device",
                AddressSpace::Storage { .. } => "device",
                AddressSpace::Uniform | AddressSpace::Immediate | AddressSpace::PushConstant => {
                    "constant"
                }
                _ => "thread",
            };
            format!("{qual} {elem}*")
        }
        _ => msl_type_for_handle(module, handle, false),
    }
}

pub(super) fn msl_scalar(s: Scalar) -> &'static str {
    match (s.kind, s.width) {
        (ScalarKind::Float, 2) => "half",
        (ScalarKind::Float, _) => "float",
        (ScalarKind::Sint, _) => "int",
        (ScalarKind::Uint, _) => "uint",
        (ScalarKind::Bool, _) => "bool",
    }
}

pub fn wgsl_type_str_to_msl(module: &Module, raw: &str) -> String {
    let s = raw.trim();
    if let Some(aliased) = module.aliases.get(s) {
        return wgsl_type_str_to_msl(module, aliased);
    }
    match s {
        "bool" => "bool".to_string(),
        "i32" => "int".to_string(),
        "u32" => "uint".to_string(),
        "f32" => "float".to_string(),
        "f16" => "half".to_string(),
        "vec2f" | "vec2<f32>" => "float2".to_string(),
        "vec3f" | "vec3<f32>" => "float3".to_string(),
        "vec4f" | "vec4<f32>" => "float4".to_string(),
        "vec2u" | "vec2<u32>" => "uint2".to_string(),
        "vec3u" | "vec3<u32>" => "uint3".to_string(),
        "vec4u" | "vec4<u32>" => "uint4".to_string(),
        "vec2i" | "vec2<i32>" => "int2".to_string(),
        "vec3i" | "vec3<i32>" => "int3".to_string(),
        "vec4i" | "vec4<i32>" => "int4".to_string(),
        "vec2<bool>" | "vec2b" => "bool2".to_string(),
        "vec3<bool>" | "vec3b" => "bool3".to_string(),
        "vec4<bool>" | "vec4b" => "bool4".to_string(),
        "mat2x2f" | "mat2x2<f32>" => "float2x2".to_string(),
        "mat3x3f" | "mat3x3<f32>" => "float3x3".to_string(),
        "mat4x4f" | "mat4x4<f32>" => "float4x4".to_string(),
        "atomic<u32>" => "atomic_uint".to_string(),
        "atomic<i32>" => "atomic_int".to_string(),
        _ if s.starts_with("array<") => {
            let inner = s
                .strip_prefix("array<")
                .and_then(|x| x.strip_suffix('>'))
                .unwrap_or("u32");
            let parts = split_comma_top_level(inner);
            let elem = wgsl_type_str_to_msl(module, parts.first().copied().unwrap_or("u32"));
            if let Some(cnt) = parts.get(1) {
                format!("_wgpu_arr<{elem}, {}>", cnt.trim())
            } else {
                elem
            }
        }
        _ => s.to_string(),
    }
}
