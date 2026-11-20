//! Static WGSL type representation, coercion, and layout/signature lookup for validation.

use super::ModuleEnv;
use crate::{
    AddressSpace, ArraySize, Handle, ImageClass, ImageDimension, Module, Scalar, ScalarKind,
    StorageAccess, TypeInner, VectorSize,
};

/// Element scalar in the validator, distinguishing unsuffixed abstract numeric literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ElemScalar {
    AbstractInt,
    AbstractFloat,
    Bool,
    I32,
    U32,
    F32,
    F16,
}

impl ElemScalar {
    pub(super) fn from_ir(s: Scalar) -> Self {
        match (s.kind, s.width) {
            (ScalarKind::Bool, _) => Self::Bool,
            (ScalarKind::Sint, _) => Self::I32,
            (ScalarKind::Uint, _) => Self::U32,
            (ScalarKind::Float, 2) => Self::F16,
            (ScalarKind::Float, _) => Self::F32,
        }
    }

    pub(super) const fn is_numeric(self) -> bool {
        !matches!(self, Self::Bool)
    }

    pub(super) const fn is_int(self) -> bool {
        matches!(self, Self::AbstractInt | Self::I32 | Self::U32)
    }

    pub(super) const fn is_float(self) -> bool {
        matches!(self, Self::AbstractFloat | Self::F32 | Self::F16)
    }

    pub(super) const fn is_signed_or_float(self) -> bool {
        matches!(
            self,
            Self::AbstractInt | Self::AbstractFloat | Self::I32 | Self::F32 | Self::F16
        )
    }

    pub(super) const fn concretize(self) -> Self {
        match self {
            Self::AbstractInt => Self::I32,
            Self::AbstractFloat => Self::F32,
            other => other,
        }
    }

    pub(super) const fn bit_width(self) -> Option<u32> {
        match self {
            Self::AbstractInt | Self::AbstractFloat | Self::I32 | Self::U32 | Self::F32 => Some(32),
            Self::F16 => Some(16),
            Self::Bool => None,
        }
    }

    pub(super) const fn coerces_to(self, target: Self) -> bool {
        if matches!(
            (self, target),
            (Self::AbstractInt, Self::AbstractInt)
                | (Self::AbstractFloat, Self::AbstractFloat)
                | (Self::Bool, Self::Bool)
                | (Self::I32, Self::I32)
                | (Self::U32, Self::U32)
                | (Self::F32, Self::F32)
                | (Self::F16, Self::F16)
        ) {
            return true;
        }
        matches!(
            (self, target),
            (
                Self::AbstractInt,
                Self::AbstractFloat | Self::I32 | Self::U32 | Self::F32 | Self::F16
            ) | (Self::AbstractFloat, Self::F32 | Self::F16)
        )
    }

    pub(super) fn common(a: Self, b: Self) -> Option<Self> {
        if a.coerces_to(b) {
            Some(b)
        } else if b.coerces_to(a) {
            Some(a)
        } else {
            None
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::AbstractInt => "abstract-int",
            Self::AbstractFloat => "abstract-float",
            Self::Bool => "bool",
            Self::I32 => "i32",
            Self::U32 => "u32",
            Self::F32 => "f32",
            Self::F16 => "f16",
        }
    }
}

/// Resolved WGSL type for validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ValType {
    Scalar(ElemScalar),
    Vector(VectorSize, ElemScalar),
    Matrix(VectorSize, VectorSize, ElemScalar),
    Atomic(Scalar),
    Array(Box<ValType>, ArraySize),
    Struct(Handle<crate::Type>),
    Pointer {
        base: Box<ValType>,
        space: AddressSpace,
        access: StorageAccess,
    },
    Image {
        dim: ImageDimension,
        arrayed: bool,
        class: ImageClass,
    },
    Sampler {
        comparison: bool,
    },
    FrexpResult(ElemScalar, Option<VectorSize>),
    ModfResult(ElemScalar, Option<VectorSize>),
    AtomicCmpExchResult(Scalar),
    Void,
}

impl ValType {
    pub(super) fn from_ir(module: &Module, handle: Handle<crate::Type>) -> Self {
        match &module.types[handle].inner {
            TypeInner::Scalar(s) => Self::Scalar(ElemScalar::from_ir(*s)),
            TypeInner::Vector { size, scalar } => Self::Vector(*size, ElemScalar::from_ir(*scalar)),
            TypeInner::Matrix {
                columns,
                rows,
                scalar,
            } => Self::Matrix(*columns, *rows, ElemScalar::from_ir(*scalar)),
            TypeInner::Atomic(s) => Self::Atomic(*s),
            TypeInner::Pointer { base, space } => {
                let access = match space {
                    AddressSpace::Storage { access } => *access,
                    AddressSpace::Uniform
                    | AddressSpace::Immediate
                    | AddressSpace::PushConstant => StorageAccess::Read,
                    _ => StorageAccess::ReadWrite,
                };
                Self::Pointer {
                    base: Box::new(Self::from_ir(module, *base)),
                    space: *space,
                    access,
                }
            }
            TypeInner::Array { base, size, .. } => {
                Self::Array(Box::new(Self::from_ir(module, *base)), *size)
            }
            TypeInner::Struct { .. } => Self::Struct(handle),
            TypeInner::Image {
                dim,
                arrayed,
                class,
            } => Self::Image {
                dim: *dim,
                arrayed: *arrayed,
                class: *class,
            },
            TypeInner::Sampler { comparison } => Self::Sampler {
                comparison: *comparison,
            },
        }
    }

    pub(super) fn concretize(&self) -> Self {
        match self {
            Self::Scalar(s) => Self::Scalar(s.concretize()),
            Self::Vector(sz, s) => Self::Vector(*sz, s.concretize()),
            Self::Matrix(c, r, s) => Self::Matrix(*c, *r, s.concretize()),
            Self::Array(base, sz) => Self::Array(Box::new(base.concretize()), *sz),
            other => other.clone(),
        }
    }

    pub(super) fn bit_width(&self) -> Option<u32> {
        match self {
            Self::Scalar(s) => s.bit_width(),
            Self::Vector(sz, s) => s.bit_width().map(|w| w * sz.as_u32()),
            _ => None,
        }
    }

    pub(super) fn coerces_to(&self, target: &Self) -> bool {
        if self == target {
            return true;
        }
        match (self, target) {
            (Self::Scalar(a), Self::Scalar(b)) => a.coerces_to(*b),
            (Self::Vector(s1, e1), Self::Vector(s2, e2)) => s1 == s2 && e1.coerces_to(*e2),
            (Self::Matrix(c1, r1, e1), Self::Matrix(c2, r2, e2)) => {
                c1 == c2 && r1 == r2 && e1.coerces_to(*e2)
            }
            (Self::Array(b1, sz1), Self::Array(b2, sz2)) => sz1 == sz2 && b1.coerces_to(b2),
            (
                Self::Pointer {
                    base: b1,
                    space: sp1,
                    access: a1,
                },
                Self::Pointer {
                    base: b2,
                    space: sp2,
                    access: a2,
                },
            ) => b1 == b2 && pointer_space_compatible(*sp1, *a1, *sp2, *a2),
            _ => false,
        }
    }

    pub(super) fn common(a: &Self, b: &Self) -> Option<Self> {
        if a.coerces_to(b) {
            Some(b.clone())
        } else if b.coerces_to(a) {
            Some(a.clone())
        } else if let (Self::Vector(s1, e1), Self::Vector(s2, e2)) = (a, b) {
            (s1 == s2)
                .then(|| ElemScalar::common(*e1, *e2))
                .flatten()
                .map(|e| Self::Vector(*s1, e))
        } else if let (Self::Matrix(c1, r1, e1), Self::Matrix(c2, r2, e2)) = (a, b) {
            (c1 == c2 && r1 == r2)
                .then(|| ElemScalar::common(*e1, *e2))
                .flatten()
                .map(|e| Self::Matrix(*c1, *r1, e))
        } else {
            None
        }
    }

    pub(super) fn describe(&self, module: &Module) -> String {
        match self {
            Self::Scalar(s) => s.label().to_string(),
            Self::Vector(sz, s) => format!("vec{}<{}>", sz.as_u32(), s.label()),
            Self::Matrix(c, r, s) => format!("mat{}x{}<{}>", c.as_u32(), r.as_u32(), s.label()),
            Self::Atomic(s) => format!("atomic<{}>", ElemScalar::from_ir(*s).label()),
            Self::Array(base, ArraySize::Constant(n)) => {
                format!("array<{}, {}>", base.describe(module), n.get())
            }
            Self::Array(base, ArraySize::Dynamic) => format!("array<{}>", base.describe(module)),
            Self::Struct(h) => module.types[*h]
                .name
                .clone()
                .unwrap_or_else(|| "struct".to_string()),
            Self::Pointer { base, .. } => format!("ptr<..., {}>", base.describe(module)),
            Self::Image { .. } => "texture".to_string(),
            Self::Sampler { comparison } => {
                if *comparison {
                    "sampler_comparison".to_string()
                } else {
                    "sampler".to_string()
                }
            }
            Self::FrexpResult(..) => "__frexp_result".to_string(),
            Self::ModfResult(..) => "__modf_result".to_string(),
            Self::AtomicCmpExchResult(..) => "__atomic_compare_exchange_result".to_string(),
            Self::Void => "void".to_string(),
        }
    }

    pub(super) fn is_host_shareable_constructible(&self, module: &Module) -> bool {
        match self {
            Self::Scalar(s) => !matches!(s, ElemScalar::Bool),
            Self::Vector(_, s) => !matches!(s, ElemScalar::Bool),
            Self::Matrix(_, _, s) => s.is_float(),
            Self::Array(base, ArraySize::Constant(_)) => {
                base.is_host_shareable_constructible(module)
            }
            Self::Struct(h) => match &module.types[*h].inner {
                TypeInner::Struct { members, .. } => members
                    .iter()
                    .all(|m| Self::from_ir(module, m.ty).is_host_shareable_constructible(module)),
                _ => false,
            },
            _ => false,
        }
    }

    pub(super) fn contains_dynamic_array(&self, module: &Module) -> bool {
        match self {
            Self::Array(_, ArraySize::Dynamic) => true,
            Self::Array(base, ArraySize::Constant(_)) => base.contains_dynamic_array(module),
            Self::Struct(h) => match &module.types[*h].inner {
                TypeInner::Struct { members, .. } => members
                    .iter()
                    .any(|m| Self::from_ir(module, m.ty).contains_dynamic_array(module)),
                _ => false,
            },
            _ => false,
        }
    }
}

fn pointer_space_compatible(
    s1: AddressSpace,
    a1: StorageAccess,
    s2: AddressSpace,
    a2: StorageAccess,
) -> bool {
    match (s1, s2) {
        (AddressSpace::Storage { .. }, AddressSpace::Storage { .. }) => a1.contains(a2),
        (x, y) => x == y,
    }
}

pub(super) fn resolve_type_str(env: &ModuleEnv<'_>, raw: &str) -> Result<ValType, String> {
    resolve_type_str_depth(env, raw, 0)
}

fn resolve_type_str_depth(env: &ModuleEnv<'_>, raw: &str, depth: usize) -> Result<ValType, String> {
    if depth > 32 {
        return Err(format!("cyclic type alias `{}`", raw.trim()));
    }
    let s = raw.trim();
    if let Some(target) = env.module.aliases.get(s) {
        return resolve_type_str_depth(env, target, depth + 1);
    }
    if let Some(&h) = env.structs.get(s) {
        return Ok(ValType::Struct(h));
    }
    if let Some(ty) = parse_scalar_or_vector_str(env, s, depth)? {
        return Ok(ty);
    }
    if let Some(ty) = parse_matrix_or_atomic_str(env, s, depth)? {
        return Ok(ty);
    }
    if let Some(inner) = s.strip_prefix("array<").and_then(|r| r.strip_suffix('>')) {
        return parse_array_str(env, inner, depth);
    }
    if let Some(inner) = s.strip_prefix("ptr<").and_then(|r| r.strip_suffix('>')) {
        return parse_pointer_str(env, inner, depth);
    }
    Err(format!("unknown type `{s}`"))
}

fn parse_scalar_or_vector_str(
    env: &ModuleEnv<'_>,
    s: &str,
    depth: usize,
) -> Result<Option<ValType>, String> {
    let scalar = match s {
        "bool" => Some(ElemScalar::Bool),
        "i32" => Some(ElemScalar::I32),
        "u32" => Some(ElemScalar::U32),
        "f32" => Some(ElemScalar::F32),
        "f16" => Some(ElemScalar::F16),
        _ => None,
    };
    if let Some(sc) = scalar {
        return Ok(Some(ValType::Scalar(sc)));
    }
    let Some(rest) = s.strip_prefix("vec") else {
        return Ok(None);
    };
    let Some((digit, suffix)) = rest.split_at_checked(1) else {
        return Ok(None);
    };
    let size = match digit {
        "2" => VectorSize::Bi,
        "3" => VectorSize::Tri,
        "4" => VectorSize::Quad,
        _ => return Ok(None),
    };
    let elem = match suffix {
        "f" => Some(ElemScalar::F32),
        "h" => Some(ElemScalar::F16),
        "i" => Some(ElemScalar::I32),
        "u" => Some(ElemScalar::U32),
        "b" => Some(ElemScalar::Bool),
        _ => None,
    };
    if let Some(sc) = elem {
        return Ok(Some(ValType::Vector(size, sc)));
    }
    if let Some(inner) = suffix.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        let ValType::Scalar(sc) = resolve_type_str_depth(env, inner, depth + 1)? else {
            return Err(format!("vector element type `{inner}` must be scalar"));
        };
        return Ok(Some(ValType::Vector(size, sc)));
    }
    Ok(None)
}

fn parse_matrix_or_atomic_str(
    env: &ModuleEnv<'_>,
    s: &str,
    depth: usize,
) -> Result<Option<ValType>, String> {
    if let Some(rest) = s.strip_prefix("mat") {
        if let Some((cols_s, after_c)) = rest.split_at_checked(1) {
            if let Some(after_x) = after_c.strip_prefix('x') {
                if let Some((rows_s, suffix)) = after_x.split_at_checked(1) {
                    let cols = parse_vec_size(cols_s);
                    let rows = parse_vec_size(rows_s);
                    if let (Some(c), Some(r)) = (cols, rows) {
                        let sc = if suffix == "f" {
                            Some(ElemScalar::F32)
                        } else if suffix == "h" {
                            Some(ElemScalar::F16)
                        } else if let Some(inner) =
                            suffix.strip_prefix('<').and_then(|v| v.strip_suffix('>'))
                        {
                            match resolve_type_str_depth(env, inner, depth + 1)? {
                                ValType::Scalar(sc) if sc.is_float() => Some(sc),
                                _ => {
                                    return Err(format!(
                                        "matrix element type `{inner}` must be float"
                                    ));
                                }
                            }
                        } else {
                            None
                        };
                        if let Some(sc) = sc {
                            return Ok(Some(ValType::Matrix(c, r, sc)));
                        }
                    }
                }
            }
        }
    }
    if let Some(inner) = s.strip_prefix("atomic<").and_then(|r| r.strip_suffix('>')) {
        return match resolve_type_str_depth(env, inner, depth + 1)? {
            ValType::Scalar(ElemScalar::I32) => Ok(Some(ValType::Atomic(Scalar::I32))),
            ValType::Scalar(ElemScalar::U32) => Ok(Some(ValType::Atomic(Scalar::U32))),
            _ => Err(format!("atomic element `{inner}` must be i32 or u32")),
        };
    }
    Ok(None)
}

fn parse_vec_size(digit: &str) -> Option<VectorSize> {
    match digit {
        "2" => Some(VectorSize::Bi),
        "3" => Some(VectorSize::Tri),
        "4" => Some(VectorSize::Quad),
        _ => None,
    }
}

fn parse_array_str(env: &ModuleEnv<'_>, inner: &str, depth: usize) -> Result<ValType, String> {
    let parts = crate::front::wgsl::split_comma_top_level(inner);
    let Some(elem_src) = parts.first().map(|p| p.trim()).filter(|p| !p.is_empty()) else {
        return Err("array requires an element type".to_string());
    };
    if parts.len() > 2 {
        return Err("array expects at most two type parameters".to_string());
    }
    let elem = resolve_type_str_depth(env, elem_src, depth + 1)?;
    let size = if let Some(len_src) = parts.get(1) {
        let len_src = len_src.trim();
        let val = crate::front::wgsl::eval_u32_const_expr(len_src, &env.const_values)
            .and_then(std::num::NonZeroU32::new)
            .ok_or_else(|| format!("array length `{len_src}` is not a positive constant"))?;
        ArraySize::Constant(val)
    } else {
        ArraySize::Dynamic
    };
    Ok(ValType::Array(Box::new(elem), size))
}

fn parse_pointer_str(env: &ModuleEnv<'_>, inner: &str, depth: usize) -> Result<ValType, String> {
    let parts = crate::front::wgsl::split_comma_top_level(inner);
    if parts.len() < 2 || parts.len() > 3 {
        return Err("ptr expects 2 or 3 arguments".to_string());
    }
    let space_str = parts[0].trim();
    let access_str = parts.get(2).map(|s| s.trim()).unwrap_or(match space_str {
        "storage" | "uniform" => "read",
        _ => "read_write",
    });
    let access = match access_str {
        "read" => StorageAccess::Read,
        "write" => StorageAccess::Write,
        "read_write" => StorageAccess::ReadWrite,
        other => return Err(format!("invalid pointer access `{other}`")),
    };
    let space = match space_str {
        "function" => AddressSpace::Function,
        "private" => AddressSpace::Private,
        "workgroup" => AddressSpace::WorkGroup,
        "uniform" => AddressSpace::Uniform,
        "storage" => AddressSpace::Storage { access },
        other => return Err(format!("invalid pointer address space `{other}`")),
    };
    let base = resolve_type_str_depth(env, parts[1], depth + 1)?;
    Ok(ValType::Pointer {
        base: Box::new(base),
        space,
        access,
    })
}
