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

    let inner = if let Some(inner) = numeric_type(ty_str).or_else(|| handle_type(ty_str)) {
        inner
    } else if ty_str.starts_with("texture_storage_") {
        storage_texture_type(ty_str)?
    } else if ty_str.starts_with("array<") {
        array_type(module, type_map, const_values, ty_str)?
    } else if ty_str.starts_with("ptr<") {
        pointer_type(module, type_map, const_values, ty_str)?
    } else {
        return Err(format!("unknown type `{ty_str}`"));
    };

    let h = module.types.append(Type { name: None, inner });
    type_map.insert(ty_str.to_string(), h);
    Ok(h)
}

/// The scalar a short suffix (`vec3f`) or a type argument (`vec3<f32>`) names.
fn scalar_of(suffix: &str) -> Option<Scalar> {
    match suffix {
        "f" | "<f32>" => Some(Scalar::F32),
        "u" | "<u32>" => Some(Scalar::U32),
        "i" | "<i32>" => Some(Scalar::I32),
        "b" | "<bool>" => Some(Scalar::BOOL),
        _ => None,
    }
}

fn vector_size(digit: &str) -> Option<VectorSize> {
    match digit {
        "2" => Some(VectorSize::Bi),
        "3" => Some(VectorSize::Tri),
        "4" => Some(VectorSize::Quad),
        _ => None,
    }
}

/// Scalars, vectors, square float matrices and atomics.
fn numeric_type(ty_str: &str) -> Option<TypeInner> {
    let scalar = match ty_str {
        "bool" => Some(Scalar::BOOL),
        "i32" => Some(Scalar::I32),
        "u32" => Some(Scalar::U32),
        "f32" => Some(Scalar::F32),
        "f16" => Some(Scalar::F16),
        _ => None,
    };
    if let Some(scalar) = scalar {
        return Some(TypeInner::Scalar(scalar));
    }
    if let Some(rest) = ty_str.strip_prefix("vec") {
        let (digit, suffix) = rest.split_at_checked(1)?;
        return Some(TypeInner::Vector {
            size: vector_size(digit)?,
            scalar: scalar_of(suffix)?,
        });
    }
    if let Some(rest) = ty_str.strip_prefix("mat") {
        // Only square float matrices are supported: `mat3x3f` and `mat3x3<f32>`.
        let (columns, rest) = rest.split_at_checked(1)?;
        let (rows, suffix) = rest.strip_prefix('x')?.split_at_checked(1)?;
        if columns != rows || scalar_of(suffix)? != Scalar::F32 {
            return None;
        }
        let size = vector_size(columns)?;
        return Some(TypeInner::Matrix {
            columns: size,
            rows: size,
            scalar: Scalar::F32,
        });
    }
    if ty_str.starts_with("atomic<") {
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
        return Some(TypeInner::Atomic(scalar));
    }
    None
}

/// Samplers and textures.
fn handle_type(ty_str: &str) -> Option<TypeInner> {
    let sampled = |dim, arrayed| TypeInner::Image {
        dim,
        arrayed,
        class: ImageClass::Sampled {
            kind: sampled_kind(ty_str),
            multi: false,
        },
    };
    Some(match ty_str {
        "sampler" => TypeInner::Sampler { comparison: false },
        "sampler_comparison" => TypeInner::Sampler { comparison: true },
        "texture_depth_2d" => TypeInner::Image {
            dim: ImageDimension::D2,
            arrayed: false,
            class: ImageClass::Depth { multi: false },
        },
        _ if ty_str.starts_with("texture_2d_array<") => sampled(ImageDimension::D2, true),
        _ if ty_str.starts_with("texture_cube<") => sampled(ImageDimension::Cube, false),
        _ if ty_str.starts_with("texture_2d<") => sampled(ImageDimension::D2, false),
        _ if ty_str.starts_with("texture_3d<") => sampled(ImageDimension::D3, false),
        _ => return None,
    })
}

/// `texture_storage_2d<rgba8unorm, write>` and its 1D, 3D and array forms.
fn storage_texture_type(ty_str: &str) -> Result<TypeInner, String> {
    let unknown = || format!("unknown type `{ty_str}`");
    let (shape, arguments) = ty_str
        .strip_prefix("texture_storage_")
        .and_then(|rest| rest.strip_suffix('>'))
        .and_then(|rest| rest.split_once('<'))
        .ok_or_else(unknown)?;
    let (dim, arrayed) = match shape {
        "1d" => (ImageDimension::D1, false),
        "2d" => (ImageDimension::D2, false),
        "2d_array" => (ImageDimension::D2, true),
        "3d" => (ImageDimension::D3, false),
        _ => return Err(unknown()),
    };
    let (format_src, access_src) = arguments.split_once(',').ok_or_else(unknown)?;
    let format = match format_src.trim() {
        "rgba8unorm" => crate::StorageFormat::Rgba8Unorm,
        "rgba32float" => crate::StorageFormat::Rgba32Float,
        "r32float" => crate::StorageFormat::R32Float,
        "r32uint" => crate::StorageFormat::R32Uint,
        other => return Err(format!("unsupported storage texture format `{other}`")),
    };
    let access = match access_src.trim() {
        "read" => StorageAccess::Read,
        "write" => StorageAccess::Write,
        "read_write" => StorageAccess::ReadWrite,
        other => return Err(format!("unknown storage texture access `{other}`")),
    };
    Ok(TypeInner::Image {
        dim,
        arrayed,
        class: ImageClass::Storage { format, access },
    })
}

fn array_type(
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    ty_str: &str,
) -> Result<TypeInner, String> {
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
            .ok_or_else(|| format!("array length `{len_str}` is not a positive constant"))?;
        ArraySize::Constant(length)
    } else {
        ArraySize::Dynamic
    };
    Ok(TypeInner::Array { base, size, stride })
}

fn pointer_type(
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    ty_str: &str,
) -> Result<TypeInner, String> {
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
    Ok(TypeInner::Pointer { base, space })
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
