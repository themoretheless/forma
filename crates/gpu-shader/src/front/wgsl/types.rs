//! Type resolution and struct ordering.

use super::*;

/// The struct declarations of `items`, ordered so that a struct follows every struct that types
/// one of its members, and otherwise in source order.
pub(super) fn structs_in_dependency_order<'a>(
    clean: &str,
    items: &[&'a str],
) -> Result<Vec<&'a str>, ParseError> {
    let mut declared: Vec<(&str, &str)> = Vec::new();
    for &item in items {
        let (_, rest) = parse_leading_attributes(clean, item.trim())?;
        if let Some(after_struct) = strip_keyword(rest.trim(), "struct") {
            let name = after_struct.split('{').next().unwrap_or_default().trim();
            declared.push((name, item));
        }
    }
    let mut state = vec![StructVisit::Pending; declared.len()];
    let mut ordered = Vec::with_capacity(declared.len());
    for index in 0..declared.len() {
        visit_struct(clean, index, &declared, &mut state, &mut ordered)?;
    }
    Ok(ordered)
}

#[derive(Clone, Copy, PartialEq)]
enum StructVisit {
    Pending,
    Active,
    Done,
}

fn visit_struct<'a>(
    clean: &str,
    index: usize,
    declared: &[(&str, &'a str)],
    state: &mut [StructVisit],
    ordered: &mut Vec<&'a str>,
) -> Result<(), ParseError> {
    let (name, item) = declared[index];
    match state[index] {
        StructVisit::Done => return Ok(()),
        StructVisit::Active => {
            return Err(parse_error_at(
                clean,
                item,
                format!("struct `{name}` contains itself"),
            ));
        }
        StructVisit::Pending => state[index] = StructVisit::Active,
    }
    let body = item.split_once('{').map_or("", |(_, body)| body);
    for member in split_comma_top_level(body) {
        let Some((_, member_ty)) = member.split_once(':') else {
            continue;
        };
        for word in member_ty.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if let Some(dependency) = declared.iter().position(|(other, _)| *other == word) {
                visit_struct(clean, dependency, declared, state, ordered)?;
            }
        }
    }
    state[index] = StructVisit::Done;
    ordered.push(item);
    Ok(())
}

pub fn resolve_or_insert_type(
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    raw_ty: &str,
) -> Result<Handle<Type>, String> {
    let ty_str = raw_ty.trim();
    if let Some(&h) = type_map.get(ty_str) {
        return Ok(h);
    }
    if let Some(aliased) = module.aliases.get(ty_str).cloned() {
        let h = resolve_or_insert_type(module, type_map, const_values, &aliased)?;
        type_map.insert(ty_str.to_string(), h);
        return Ok(h);
    }

    let inner = match ty_str {
        "bool" => TypeInner::Scalar(Scalar::BOOL),
        "i32" => TypeInner::Scalar(Scalar::I32),
        "u32" => TypeInner::Scalar(Scalar::U32),
        "f32" => TypeInner::Scalar(Scalar::F32),
        "f16" => TypeInner::Scalar(Scalar::F16),
        "vec2f" | "vec2<f32>" => TypeInner::Vector {
            size: VectorSize::Bi,
            scalar: Scalar::F32,
        },
        "vec3f" | "vec3<f32>" => TypeInner::Vector {
            size: VectorSize::Tri,
            scalar: Scalar::F32,
        },
        "vec4f" | "vec4<f32>" => TypeInner::Vector {
            size: VectorSize::Quad,
            scalar: Scalar::F32,
        },
        "vec2u" | "vec2<u32>" => TypeInner::Vector {
            size: VectorSize::Bi,
            scalar: Scalar::U32,
        },
        "vec3u" | "vec3<u32>" => TypeInner::Vector {
            size: VectorSize::Tri,
            scalar: Scalar::U32,
        },
        "vec4u" | "vec4<u32>" => TypeInner::Vector {
            size: VectorSize::Quad,
            scalar: Scalar::U32,
        },
        "vec2i" | "vec2<i32>" => TypeInner::Vector {
            size: VectorSize::Bi,
            scalar: Scalar::I32,
        },
        "vec3i" | "vec3<i32>" => TypeInner::Vector {
            size: VectorSize::Tri,
            scalar: Scalar::I32,
        },
        "vec4i" | "vec4<i32>" => TypeInner::Vector {
            size: VectorSize::Quad,
            scalar: Scalar::I32,
        },
        "vec2<bool>" | "vec2b" => TypeInner::Vector {
            size: VectorSize::Bi,
            scalar: Scalar::BOOL,
        },
        "vec3<bool>" | "vec3b" => TypeInner::Vector {
            size: VectorSize::Tri,
            scalar: Scalar::BOOL,
        },
        "vec4<bool>" | "vec4b" => TypeInner::Vector {
            size: VectorSize::Quad,
            scalar: Scalar::BOOL,
        },
        "mat2x2f" | "mat2x2<f32>" => TypeInner::Matrix {
            columns: VectorSize::Bi,
            rows: VectorSize::Bi,
            scalar: Scalar::F32,
        },
        "mat3x3f" | "mat3x3<f32>" => TypeInner::Matrix {
            columns: VectorSize::Tri,
            rows: VectorSize::Tri,
            scalar: Scalar::F32,
        },
        "mat4x4f" | "mat4x4<f32>" => TypeInner::Matrix {
            columns: VectorSize::Quad,
            rows: VectorSize::Quad,
            scalar: Scalar::F32,
        },
        "sampler" => TypeInner::Sampler { comparison: false },
        "sampler_comparison" => TypeInner::Sampler { comparison: true },
        "texture_depth_2d" => TypeInner::Image {
            dim: ImageDimension::D2,
            arrayed: false,
            class: ImageClass::Depth { multi: false },
        },
        _ if ty_str.starts_with("texture_2d_array<") || ty_str.starts_with("texture_cube<") => {
            let kind = sampled_kind(ty_str);
            let cube = ty_str.starts_with("texture_cube<");
            TypeInner::Image {
                dim: if cube {
                    ImageDimension::Cube
                } else {
                    ImageDimension::D2
                },
                arrayed: !cube,
                class: ImageClass::Sampled { kind, multi: false },
            }
        }
        _ if ty_str.starts_with("texture_2d<") => {
            let kind = sampled_kind(ty_str);
            TypeInner::Image {
                dim: ImageDimension::D2,
                arrayed: false,
                class: ImageClass::Sampled { kind, multi: false },
            }
        }
        _ if ty_str.starts_with("texture_3d<") => {
            let kind = sampled_kind(ty_str);
            TypeInner::Image {
                dim: ImageDimension::D3,
                arrayed: false,
                class: ImageClass::Sampled { kind, multi: false },
            }
        }
        _ if ty_str.starts_with("atomic<") => {
            let inner_s = ty_str
                .strip_prefix("atomic<")
                .and_then(|s| s.strip_suffix('>'))
                .unwrap_or("u32")
                .trim();
            let scalar = if inner_s == "i32" {
                Scalar::I32
            } else {
                Scalar::U32
            };
            TypeInner::Atomic(scalar)
        }
        _ if ty_str.starts_with("array<") => {
            let inner_s = ty_str
                .strip_prefix("array<")
                .and_then(|s| s.strip_suffix('>'))
                .unwrap_or("u32")
                .trim();
            let parts = split_comma_top_level(inner_s);
            let elem_ty_str = parts.first().map(|s| s.trim()).unwrap_or("u32");
            let base = resolve_or_insert_type(module, type_map, const_values, elem_ty_str)?;
            let base_layout = compute_type_layout(&module.types, base);
            let stride = align_up(base_layout.size, base_layout.alignment);
            let size = if let Some(len_str) = parts.get(1) {
                let len_str = len_str.trim();
                let length = eval_u32_const_expr(len_str, const_values)
                    .and_then(NonZeroU32::new)
                    .ok_or_else(|| {
                        format!("array length `{len_str}` is not a positive constant")
                    })?;
                ArraySize::Constant(length)
            } else {
                ArraySize::Dynamic
            };
            TypeInner::Array { base, size, stride }
        }
        _ if ty_str.starts_with("ptr<") => {
            let inner_s = ty_str
                .strip_prefix("ptr<")
                .and_then(|s| s.strip_suffix('>'))
                .unwrap_or("function, u32")
                .trim();
            let parts = split_comma_top_level(inner_s);
            let space_str = parts.first().map(|s| s.trim()).unwrap_or("function");
            let base_str = parts.get(1).map(|s| s.trim()).unwrap_or("u32");
            let access_str = parts.get(2).map(|s| s.trim()).unwrap_or("read_write");
            let space = match space_str {
                "workgroup" => AddressSpace::WorkGroup,
                "private" => AddressSpace::Private,
                "uniform" => AddressSpace::Uniform,
                "storage" => AddressSpace::Storage {
                    access: if access_str == "read" {
                        StorageAccess::Read
                    } else {
                        StorageAccess::ReadWrite
                    },
                },
                _ => AddressSpace::Function,
            };
            let base = resolve_or_insert_type(module, type_map, const_values, base_str)?;
            TypeInner::Pointer { base, space }
        }
        _ => {
            return Err(format!("unknown type `{ty_str}`"));
        }
    };

    let h = module.types.append(Type { name: None, inner });
    type_map.insert(ty_str.to_string(), h);
    Ok(h)
}

/// The scalar kind a sampled texture type such as `texture_2d<u32>` reads.
fn sampled_kind(ty_str: &str) -> ScalarKind {
    if ty_str.contains("u32") {
        ScalarKind::Uint
    } else if ty_str.contains("i32") {
        ScalarKind::Sint
    } else {
        ScalarKind::Float
    }
}
