//! Validation of WGSL type constructors, bitcasts, built-in functions, atomics, and textures.

use super::expr::ExprResult;
use super::types::{ElemScalar, ValType, resolve_type_str};
use super::{ModuleEnv, ValidationError};
use crate::{
    AddressSpace, ArraySize, ImageClass, ImageDimension, Scalar, ScalarKind, StorageAccess,
    TypeInner, VectorSize,
};

pub(super) fn err(message: impl Into<String>) -> ValidationError {
    ValidationError {
        message: message.into(),
    }
}

pub(super) fn check_constructor(
    env: &ModuleEnv<'_>,
    ty: &ValType,
    args: &[ExprResult],
    name_label: &str,
) -> Result<ValType, ValidationError> {
    match ty {
        ValType::Scalar(target_sc) => check_scalar_ctor(*target_sc, args, name_label),
        ValType::Vector(size, target_sc) => {
            check_typed_vec_ctor(*size, *target_sc, args, name_label, true)
        }
        ValType::Matrix(cols, rows, sc) => {
            check_typed_mat_ctor(env, *cols, *rows, *sc, args, name_label)
        }
        ValType::Array(elem, ArraySize::Constant(len)) => {
            if args.is_empty() {
                return Ok(ty.clone());
            }
            if args.len() != len.get() as usize {
                return Err(err(format!(
                    "`{name_label}` expected {} arguments, got {}",
                    len.get(),
                    args.len()
                )));
            }
            for (i, arg) in args.iter().enumerate() {
                if !arg.ty.coerces_to(elem) {
                    return Err(err(format!(
                        "`{name_label}` element {i}: expected `{}`, got `{}`",
                        elem.describe(env.module),
                        arg.ty.describe(env.module)
                    )));
                }
            }
            Ok(ty.clone())
        }
        ValType::Struct(handle) => {
            let TypeInner::Struct { members, .. } = &env.module.types[*handle].inner else {
                return Err(err(format!("invalid struct type `{name_label}`")));
            };
            if args.is_empty() {
                return Ok(ty.clone());
            }
            if args.len() != members.len() {
                return Err(err(format!(
                    "struct `{name_label}` constructor expected {} fields, got {}",
                    members.len(),
                    args.len()
                )));
            }
            for (i, (member, arg)) in members.iter().zip(args.iter()).enumerate() {
                let mty = ValType::from_ir(env.module, member.ty);
                if !arg.ty.coerces_to(&mty) {
                    return Err(err(format!(
                        "struct `{name_label}` field {i}: expected `{}`, got `{}`",
                        mty.describe(env.module),
                        arg.ty.describe(env.module)
                    )));
                }
            }
            Ok(ty.clone())
        }
        _ => Err(err(format!("`{name_label}` is not a constructible type"))),
    }
}

fn check_scalar_ctor(
    target: ElemScalar,
    args: &[ExprResult],
    name_label: &str,
) -> Result<ValType, ValidationError> {
    if args.is_empty() {
        return Ok(ValType::Scalar(target));
    }
    if args.len() != 1 {
        return Err(err(format!(
            "`{name_label}` scalar constructor expected 0 or 1 argument, got {}",
            args.len()
        )));
    }
    match args[0].ty {
        ValType::Scalar(_) => Ok(ValType::Scalar(target)),
        _ => Err(err(format!(
            "cannot construct `{name_label}` from non-scalar argument"
        ))),
    }
}

fn check_typed_vec_ctor(
    size: VectorSize,
    target_sc: ElemScalar,
    args: &[ExprResult],
    name_label: &str,
    allow_single_vec_convert: bool,
) -> Result<ValType, ValidationError> {
    if args.is_empty() {
        return Ok(ValType::Vector(size, target_sc));
    }
    if args.len() == 1 {
        match args[0].ty {
            ValType::Scalar(s) if s.coerces_to(target_sc) => {
                return Ok(ValType::Vector(size, target_sc));
            }
            ValType::Vector(in_sz, s)
                if in_sz == size && (s.coerces_to(target_sc) || allow_single_vec_convert) =>
            {
                return Ok(ValType::Vector(size, target_sc));
            }
            _ => {}
        }
    }
    let mut count = 0u32;
    for arg in args {
        match arg.ty {
            ValType::Scalar(sc) if sc.coerces_to(target_sc) => count += 1,
            ValType::Vector(sz, sc) if sc.coerces_to(target_sc) => count += sz.as_u32(),
            _ => {
                return Err(err(format!(
                    "`{name_label}` constructor argument is incompatible with `{}`",
                    target_sc.label()
                )));
            }
        }
    }
    if count != size.as_u32() {
        return Err(err(format!(
            "`{name_label}` expected {} scalar components, got {count}",
            size.as_u32()
        )));
    }
    Ok(ValType::Vector(size, target_sc))
}

fn check_inferred_vec_ctor(
    size: VectorSize,
    args: &[ExprResult],
    name_label: &str,
) -> Result<ValType, ValidationError> {
    if args.is_empty() {
        return Err(err(format!(
            "unparameterized `{name_label}()` requires type arguments or values"
        )));
    }
    let mut inferred: Option<ElemScalar> = None;
    let mut count = 0u32;
    for arg in args {
        let sc = match arg.ty {
            ValType::Scalar(s) => {
                count += 1;
                s
            }
            ValType::Vector(sz, s) => {
                count += sz.as_u32();
                s
            }
            _ => {
                return Err(err(format!(
                    "`{name_label}` expects scalar or vector arguments"
                )));
            }
        };
        inferred = Some(match inferred {
            None => sc,
            Some(prev) => ElemScalar::common(prev, sc).ok_or_else(|| {
                err(format!(
                    "`{name_label}` argument types `{}` and `{}` do not match",
                    prev.label(),
                    sc.label()
                ))
            })?,
        });
    }
    if count != size.as_u32() && !(args.len() == 1 && count == 1) {
        return Err(err(format!(
            "`{name_label}` expected {} scalar components, got {count}",
            size.as_u32()
        )));
    }
    Ok(ValType::Vector(size, inferred.unwrap_or(ElemScalar::F32)))
}

fn check_typed_mat_ctor(
    env: &ModuleEnv<'_>,
    cols: VectorSize,
    rows: VectorSize,
    sc: ElemScalar,
    args: &[ExprResult],
    name_label: &str,
) -> Result<ValType, ValidationError> {
    if args.is_empty() {
        return Ok(ValType::Matrix(cols, rows, sc));
    }
    if args.len() == 1 {
        if let ValType::Matrix(ac, ar, _) = args[0].ty {
            if ac == cols && ar == rows {
                return Ok(ValType::Matrix(cols, rows, sc));
            }
        }
    }
    if args.len() == cols.as_u32() as usize
        && args
            .iter()
            .all(|a| matches!(a.ty, ValType::Vector(r, s) if r == rows && s.coerces_to(sc)))
    {
        return Ok(ValType::Matrix(cols, rows, sc));
    }
    let total = (cols.as_u32() * rows.as_u32()) as usize;
    if args.len() == total
        && args
            .iter()
            .all(|a| matches!(a.ty, ValType::Scalar(s) if s.coerces_to(sc)))
    {
        return Ok(ValType::Matrix(cols, rows, sc));
    }
    Err(err(format!(
        "`{name_label}` constructor has incompatible arguments (got {} args in {})",
        args.len(),
        env.module.raw_source.len()
    )))
}

pub(super) fn check_bitcast(
    env: &ModuleEnv<'_>,
    ty_src: &str,
    arg: &ExprResult,
) -> Result<ValType, ValidationError> {
    let target = resolve_type_str(env, ty_src).map_err(err)?;
    let target_bits = target
        .bit_width()
        .ok_or_else(|| err(format!("`bitcast<{ty_src}>`: unsupported target type")))?;
    let src_bits = arg.ty.bit_width().ok_or_else(|| {
        err(format!(
            "`bitcast<{ty_src}>`: source type `{}` cannot be bitcast",
            arg.ty.describe(env.module)
        ))
    })?;
    if src_bits != target_bits {
        return Err(err(format!(
            "`bitcast<{ty_src}>`: source has {src_bits} bits, target has {target_bits} bits"
        )));
    }
    Ok(target)
}

pub(super) fn check_inferred_array_ctor(
    env: &ModuleEnv<'_>,
    args: &[ExprResult],
) -> Result<ValType, ValidationError> {
    let Some(first) = args.first() else {
        return Err(err("`array()` without type parameters requires elements"));
    };
    let mut elem = first.ty.clone();
    for arg in &args[1..] {
        elem = ValType::common(&elem, &arg.ty).ok_or_else(|| {
            err(format!(
                "`array(...)`: incompatible element types `{}` and `{}`",
                elem.describe(env.module),
                arg.ty.describe(env.module)
            ))
        })?;
    }
    let len = std::num::NonZeroU32::new(args.len() as u32)
        .ok_or_else(|| err("empty `array()` constructor"))?;
    Ok(ValType::Array(Box::new(elem), ArraySize::Constant(len)))
}

pub(super) fn check_builtin_call(
    env: &ModuleEnv<'_>,
    ident: &str,
    args: &[ExprResult],
    raw_args: &str,
) -> Result<Option<ValType>, ValidationError> {
    // 1. Check user-defined functions or type/alias constructors first only if exact builtin doesn't intercept;
    // Actually in WGSL aliases & struct names resolve cleanly first when resolved_ident is a type!
    let resolved_alias = env
        .module
        .aliases
        .get(ident)
        .map(String::as_str)
        .unwrap_or(ident);
    if resolved_alias != ident {
        if let Ok(ty) = resolve_type_str(env, resolved_alias) {
            return check_constructor(env, &ty, args, ident).map(Some);
        }
    }
    if let Ok(ty) = resolve_type_str(env, ident) {
        return check_constructor(env, &ty, args, ident).map(Some);
    }
    match ident {
        "vec2" => return check_inferred_vec_ctor(VectorSize::Bi, args, "vec2").map(Some),
        "vec3" => return check_inferred_vec_ctor(VectorSize::Tri, args, "vec3").map(Some),
        "vec4" => return check_inferred_vec_ctor(VectorSize::Quad, args, "vec4").map(Some),
        "mat2x2" => return check_inferred_mat_ctor(env, VectorSize::Bi, args, "mat2x2").map(Some),
        "mat3x3" => return check_inferred_mat_ctor(env, VectorSize::Tri, args, "mat3x3").map(Some),
        "mat4x4" => {
            return check_inferred_mat_ctor(env, VectorSize::Quad, args, "mat4x4").map(Some);
        }
        _ => {}
    }

    if let Some(res) = check_sync_or_atomic(env, ident, args)? {
        return Ok(Some(res));
    }
    if let Some(res) = check_texture_builtin(env, ident, args)? {
        return Ok(Some(res));
    }
    if ident == "arrayLength" {
        return check_array_length(env, args, raw_args).map(Some);
    }
    if let Some(res) = check_math_or_bit_builtin(env, ident, args)? {
        return Ok(Some(res));
    }
    if let Some(res) = check_subgroup_builtin(env, ident, args)? {
        return Ok(Some(res));
    }
    Ok(None)
}

fn check_inferred_mat_ctor(
    env: &ModuleEnv<'_>,
    n: VectorSize,
    args: &[ExprResult],
    name: &str,
) -> Result<ValType, ValidationError> {
    for sc in [ElemScalar::F32, ElemScalar::F16] {
        if check_typed_mat_ctor(env, n, n, sc, args, name).is_ok() {
            return Ok(ValType::Matrix(n, n, sc));
        }
    }
    Err(err(format!(
        "`{name}` constructor arguments must match {nx}x{nx} float matrix",
        nx = n.as_u32()
    )))
}

fn check_array_length(
    env: &ModuleEnv<'_>,
    args: &[ExprResult],
    raw_args: &str,
) -> Result<ValType, ValidationError> {
    if args.len() != 1 {
        return Err(err(format!(
            "arrayLength({raw_args}): expected 1 argument, got {}",
            args.len()
        )));
    }
    match &args[0].ty {
        ValType::Pointer {
            base,
            space: AddressSpace::Storage { .. },
            ..
        } if matches!(base.as_ref(), ValType::Array(_, ArraySize::Dynamic)) => {
            Ok(ValType::Scalar(ElemScalar::U32))
        }
        other => Err(err(format!(
            "arrayLength({raw_args}): expected pointer to storage runtime array, got `{}`",
            other.describe(env.module)
        ))),
    }
}

fn check_sync_or_atomic(
    env: &ModuleEnv<'_>,
    ident: &str,
    args: &[ExprResult],
) -> Result<Option<ValType>, ValidationError> {
    match ident {
        "workgroupBarrier" | "storageBarrier" | "textureBarrier" => {
            if !args.is_empty() {
                return Err(err(format!("`{ident}` takes no arguments")));
            }
            Ok(Some(ValType::Void))
        }
        "atomicLoad" => {
            expect_arity(ident, args, 1)?;
            let sc = atomic_pointer_scalar(env, ident, &args[0].ty, false)?;
            Ok(Some(ValType::Scalar(ElemScalar::from_ir(sc))))
        }
        "atomicStore" => {
            expect_arity(ident, args, 2)?;
            let sc = atomic_pointer_scalar(env, ident, &args[0].ty, true)?;
            let target = ValType::Scalar(ElemScalar::from_ir(sc));
            if !args[1].ty.coerces_to(&target) {
                return Err(err(format!(
                    "`{ident}` value `{}` does not match `{}`",
                    args[1].ty.describe(env.module),
                    target.describe(env.module)
                )));
            }
            Ok(Some(ValType::Void))
        }
        "atomicAdd" | "atomicSub" | "atomicMax" | "atomicMin" | "atomicAnd" | "atomicOr"
        | "atomicXor" | "atomicExchange" => {
            expect_arity(ident, args, 2)?;
            let sc = atomic_pointer_scalar(env, ident, &args[0].ty, true)?;
            let target = ValType::Scalar(ElemScalar::from_ir(sc));
            if !args[1].ty.coerces_to(&target) {
                return Err(err(format!(
                    "`{ident}` value `{}` does not match `{}`",
                    args[1].ty.describe(env.module),
                    target.describe(env.module)
                )));
            }
            Ok(Some(target))
        }
        "atomicCompareExchangeWeak" => {
            expect_arity(ident, args, 3)?;
            let sc = atomic_pointer_scalar(env, ident, &args[0].ty, true)?;
            let target = ValType::Scalar(ElemScalar::from_ir(sc));
            for arg in &args[1..] {
                if !arg.ty.coerces_to(&target) {
                    return Err(err(format!(
                        "`{ident}` operand `{}` does not match `{}`",
                        arg.ty.describe(env.module),
                        target.describe(env.module)
                    )));
                }
            }
            Ok(Some(ValType::AtomicCmpExchResult(sc)))
        }
        _ => Ok(None),
    }
}

fn atomic_pointer_scalar(
    env: &ModuleEnv<'_>,
    ident: &str,
    ty: &ValType,
    require_write: bool,
) -> Result<Scalar, ValidationError> {
    match ty {
        ValType::Pointer {
            base,
            space,
            access,
        } => {
            let valid_space = matches!(
                space,
                AddressSpace::WorkGroup | AddressSpace::Storage { .. }
            );
            if !valid_space {
                return Err(err(format!(
                    "`{ident}` requires workgroup or storage pointer"
                )));
            }
            if require_write && *access == StorageAccess::Read {
                return Err(err(format!("`{ident}` requires writable atomic pointer")));
            }
            match base.as_ref() {
                ValType::Atomic(sc) => Ok(*sc),
                other => Err(err(format!(
                    "`{ident}` expected atomic target, got `{}`",
                    other.describe(env.module)
                ))),
            }
        }
        other => Err(err(format!(
            "`{ident}` expected atomic pointer, got `{}`",
            other.describe(env.module)
        ))),
    }
}

fn check_texture_builtin(
    env: &ModuleEnv<'_>,
    ident: &str,
    args: &[ExprResult],
) -> Result<Option<ValType>, ValidationError> {
    match ident {
        "textureDimensions" => {
            if args.is_empty() || args.len() > 2 {
                return Err(err("`textureDimensions` expects 1 or 2 arguments"));
            }
            let ValType::Image { dim, .. } = args[0].ty else {
                return Err(err("`textureDimensions` first argument must be a texture"));
            };
            if let Some(level) = args.get(1) {
                expect_int_scalar(env, ident, &level.ty)?;
            }
            Ok(Some(match dim {
                ImageDimension::D1 => ValType::Scalar(ElemScalar::U32),
                ImageDimension::D2 | ImageDimension::Cube => {
                    ValType::Vector(VectorSize::Bi, ElemScalar::U32)
                }
                ImageDimension::D3 => ValType::Vector(VectorSize::Tri, ElemScalar::U32),
            }))
        }
        "textureNumLevels" | "textureNumLayers" | "textureNumSamples" => {
            expect_arity(ident, args, 1)?;
            if !matches!(args[0].ty, ValType::Image { .. }) {
                return Err(err(format!("`{ident}` argument must be a texture")));
            }
            Ok(Some(ValType::Scalar(ElemScalar::U32)))
        }
        "textureSample" => {
            if args.len() < 3 || args.len() > 4 {
                return Err(err("`textureSample` expects 3 or 4 arguments"));
            }
            let (dim, arrayed, class) = expect_image(&args[0].ty, ident)?;
            if !matches!(
                class,
                ImageClass::Sampled {
                    kind: ScalarKind::Float,
                    multi: false
                }
            ) {
                return Err(err("`textureSample` requires float sampled texture"));
            }
            if !matches!(args[1].ty, ValType::Sampler { comparison: false }) {
                return Err(err("`textureSample` requires regular sampler"));
            }
            expect_sample_coords(env, ident, dim, &args[2].ty)?;
            if arrayed {
                // Keep validation pragmatic for arrayed sample calls with extra index if 4 args
            }
            Ok(Some(ValType::Vector(VectorSize::Quad, ElemScalar::F32)))
        }
        "textureSampleLevel" => {
            if args.len() < 4 || args.len() > 5 {
                return Err(err("`textureSampleLevel` expects 4 or 5 arguments"));
            }
            let (dim, _, _) = expect_image(&args[0].ty, ident)?;
            if !matches!(args[1].ty, ValType::Sampler { comparison: false }) {
                return Err(err("`textureSampleLevel` requires regular sampler"));
            }
            expect_sample_coords(env, ident, dim, &args[2].ty)?;
            expect_numeric_scalar_or_vec(env, ident, &args[3].ty)?;
            Ok(Some(ValType::Vector(VectorSize::Quad, ElemScalar::F32)))
        }
        "textureSampleCompare" => {
            if args.len() < 4 || args.len() > 5 {
                return Err(err("`textureSampleCompare` expects 4 or 5 arguments"));
            }
            let (dim, _, class) = expect_image(&args[0].ty, ident)?;
            if !matches!(class, ImageClass::Depth { .. }) {
                return Err(err("`textureSampleCompare` requires depth texture"));
            }
            if !matches!(args[1].ty, ValType::Sampler { comparison: true }) {
                return Err(err("`textureSampleCompare` requires sampler_comparison"));
            }
            expect_sample_coords(env, ident, dim, &args[2].ty)?;
            if !args[3].ty.coerces_to(&ValType::Scalar(ElemScalar::F32)) {
                return Err(err("`textureSampleCompare` reference value must be f32"));
            }
            Ok(Some(ValType::Scalar(ElemScalar::F32)))
        }
        "textureLoad" => check_texture_load(env, args).map(Some),
        "textureStore" => check_texture_store(env, args).map(Some),
        _ => Ok(None),
    }
}

fn expect_image(
    ty: &ValType,
    ident: &str,
) -> Result<(ImageDimension, bool, ImageClass), ValidationError> {
    match *ty {
        ValType::Image {
            dim,
            arrayed,
            class,
        } => Ok((dim, arrayed, class)),
        _ => Err(err(format!("`{ident}` first argument must be a texture"))),
    }
}

fn expect_sample_coords(
    env: &ModuleEnv<'_>,
    ident: &str,
    dim: ImageDimension,
    ty: &ValType,
) -> Result<(), ValidationError> {
    let valid = match dim {
        ImageDimension::D1 => ty.coerces_to(&ValType::Scalar(ElemScalar::F32)),
        ImageDimension::D2 => ty.coerces_to(&ValType::Vector(VectorSize::Bi, ElemScalar::F32)),
        ImageDimension::D3 | ImageDimension::Cube => {
            ty.coerces_to(&ValType::Vector(VectorSize::Tri, ElemScalar::F32))
        }
    };
    if !valid {
        return Err(err(format!(
            "`{ident}` coordinate `{}` does not match texture dimension",
            ty.describe(env.module)
        )));
    }
    Ok(())
}

fn expect_int_coords(
    env: &ModuleEnv<'_>,
    ident: &str,
    dim: ImageDimension,
    ty: &ValType,
) -> Result<(), ValidationError> {
    let valid = match dim {
        ImageDimension::D1 => matches!(
            ty,
            ValType::Scalar(s) if s.is_int()
        ),
        ImageDimension::D2 | ImageDimension::Cube => matches!(
            ty,
            ValType::Vector(VectorSize::Bi, s) if s.is_int()
        ),
        ImageDimension::D3 => matches!(
            ty,
            ValType::Vector(VectorSize::Tri, s) if s.is_int()
        ),
    };
    if !valid {
        return Err(err(format!(
            "`{ident}` integer coordinates expected, got `{}`",
            ty.describe(env.module)
        )));
    }
    Ok(())
}

fn check_texture_load(
    env: &ModuleEnv<'_>,
    args: &[ExprResult],
) -> Result<ValType, ValidationError> {
    if args.len() < 2 || args.len() > 4 {
        return Err(err("`textureLoad` expects 2 to 4 arguments"));
    }
    let (dim, _, class) = expect_image(&args[0].ty, "textureLoad")?;
    expect_int_coords(env, "textureLoad", dim, &args[1].ty)?;
    if matches!(
        class,
        ImageClass::Storage {
            access: StorageAccess::Write,
            ..
        }
    ) {
        return Err(err(
            "`textureLoad` cannot read from a write-only storage texture",
        ));
    }
    match class {
        ImageClass::Depth { .. } => Ok(ValType::Scalar(ElemScalar::F32)),
        ImageClass::Sampled { kind, .. } => {
            Ok(ValType::Vector(VectorSize::Quad, scalar_kind_elem(kind)))
        }
        ImageClass::Storage { format, .. } => Ok(ValType::Vector(
            VectorSize::Quad,
            scalar_kind_elem(format.scalar_kind()),
        )),
    }
}

fn check_texture_store(
    env: &ModuleEnv<'_>,
    args: &[ExprResult],
) -> Result<ValType, ValidationError> {
    let (dim, arrayed, class) = expect_image(&args[0].ty, "textureStore")?;
    let expected_args = if arrayed { 4 } else { 3 };
    if args.len() != expected_args {
        return Err(err(format!(
            "`textureStore` expected {expected_args} arguments, got {}",
            args.len()
        )));
    }
    let ImageClass::Storage { format, access } = class else {
        return Err(err("`textureStore` requires a storage texture"));
    };
    if access == StorageAccess::Read {
        return Err(err(
            "`textureStore` cannot write to a read-only storage texture",
        ));
    }
    expect_int_coords(env, "textureStore", dim, &args[1].ty)?;
    if arrayed {
        expect_int_scalar(env, "textureStore", &args[2].ty)?;
    }
    let elem = scalar_kind_elem(format.scalar_kind());
    let expected_val = ValType::Vector(VectorSize::Quad, elem);
    let val_arg = &args[expected_args - 1];
    if !val_arg.ty.coerces_to(&expected_val) {
        return Err(err(format!(
            "`textureStore` value `{}` does not match texel type `{}`",
            val_arg.ty.describe(env.module),
            expected_val.describe(env.module)
        )));
    }
    Ok(ValType::Void)
}

fn scalar_kind_elem(kind: ScalarKind) -> ElemScalar {
    match kind {
        ScalarKind::Uint => ElemScalar::U32,
        ScalarKind::Sint => ElemScalar::I32,
        _ => ElemScalar::F32,
    }
}

fn check_math_or_bit_builtin(
    env: &ModuleEnv<'_>,
    ident: &str,
    args: &[ExprResult],
) -> Result<Option<ValType>, ValidationError> {
    match ident {
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "asinh"
        | "acosh" | "atanh" | "exp" | "exp2" | "log" | "log2" | "sqrt" | "inverseSqrt" | "ceil"
        | "floor" | "round" | "trunc" | "fract" | "saturate" | "degrees" | "radians" | "dpdx"
        | "dpdxCoarse" | "dpdxFine" | "dpdy" | "dpdyCoarse" | "dpdyFine" | "fwidth"
        | "fwidthCoarse" | "fwidthFine" | "quantizeToF16" => {
            expect_arity(ident, args, 1)?;
            expect_float_scalar_or_vec(env, ident, &args[0].ty)?;
            Ok(Some(args[0].ty.clone()))
        }
        "abs" | "sign" => {
            expect_arity(ident, args, 1)?;
            expect_numeric_scalar_or_vec(env, ident, &args[0].ty)?;
            Ok(Some(args[0].ty.clone()))
        }
        "atan2" | "pow" | "step" => {
            expect_arity(ident, args, 2)?;
            let common = ValType::common(&args[0].ty, &args[1].ty)
                .ok_or_else(|| err(format!("`{ident}` operand types do not match")))?;
            expect_float_scalar_or_vec(env, ident, &common)?;
            Ok(Some(common))
        }
        "min" | "max" => {
            expect_arity(ident, args, 2)?;
            let common = ValType::common(&args[0].ty, &args[1].ty)
                .ok_or_else(|| err(format!("`{ident}` operand types do not match")))?;
            expect_numeric_scalar_or_vec(env, ident, &common)?;
            Ok(Some(common))
        }
        "fma" | "smoothstep" | "clamp" => {
            expect_arity(ident, args, 3)?;
            let c01 = ValType::common(&args[0].ty, &args[1].ty)
                .ok_or_else(|| err(format!("`{ident}` operand types do not match")))?;
            let common = ValType::common(&c01, &args[2].ty)
                .ok_or_else(|| err(format!("`{ident}` operand types do not match")))?;
            if ident == "clamp" {
                expect_numeric_scalar_or_vec(env, ident, &common)?;
            } else {
                expect_float_scalar_or_vec(env, ident, &common)?;
            }
            Ok(Some(common))
        }
        "mix" => {
            expect_arity(ident, args, 3)?;
            let c01 = ValType::common(&args[0].ty, &args[1].ty)
                .ok_or_else(|| err("`mix` first two arguments must have compatible types"))?;
            expect_float_scalar_or_vec(env, ident, &c01)?;
            if ValType::common(&c01, &args[2].ty).is_some() {
                return Ok(Some(c01));
            }
            if let (ValType::Vector(_, sc), ValType::Scalar(s2)) = (&c01, &args[2].ty) {
                if ElemScalar::common(*sc, *s2).is_some() {
                    return Ok(Some(c01));
                }
            }
            Err(err(
                "`mix` blend factor is incompatible with blended values",
            ))
        }
        "select" => check_select(env, args).map(Some),
        "all" | "any" => {
            expect_arity(ident, args, 1)?;
            match args[0].ty {
                ValType::Scalar(ElemScalar::Bool) | ValType::Vector(_, ElemScalar::Bool) => {
                    Ok(Some(ValType::Scalar(ElemScalar::Bool)))
                }
                _ => Err(err(format!("`{ident}` expects bool or bool vector"))),
            }
        }
        "length" => {
            expect_arity(ident, args, 1)?;
            let sc = expect_float_scalar_or_vec(env, ident, &args[0].ty)?;
            Ok(Some(ValType::Scalar(sc)))
        }
        "distance" => {
            expect_arity(ident, args, 2)?;
            let common = ValType::common(&args[0].ty, &args[1].ty)
                .ok_or_else(|| err("`distance` arguments must have compatible types"))?;
            let sc = expect_float_scalar_or_vec(env, ident, &common)?;
            Ok(Some(ValType::Scalar(sc)))
        }
        "dot" => {
            expect_arity(ident, args, 2)?;
            let common = ValType::common(&args[0].ty, &args[1].ty)
                .ok_or_else(|| err("`dot` vectors must have compatible types"))?;
            let ValType::Vector(_, sc) = common else {
                return Err(err("`dot` requires vector arguments"));
            };
            if !sc.is_numeric() {
                return Err(err("`dot` requires numeric vectors"));
            }
            Ok(Some(ValType::Scalar(sc)))
        }
        "cross" => {
            expect_arity(ident, args, 2)?;
            let common = ValType::common(&args[0].ty, &args[1].ty)
                .ok_or_else(|| err("`cross` vectors must have compatible types"))?;
            let ValType::Vector(VectorSize::Tri, sc) = common else {
                return Err(err("`cross` requires vec3 float arguments"));
            };
            if !sc.is_float() {
                return Err(err("`cross` requires float vec3 arguments"));
            }
            Ok(Some(common))
        }
        "normalize" => {
            expect_arity(ident, args, 1)?;
            if !matches!(args[0].ty, ValType::Vector(_, sc) if sc.is_float()) {
                return Err(err("`normalize` requires float vector"));
            }
            Ok(Some(args[0].ty.clone()))
        }
        "reflect" => {
            expect_arity(ident, args, 2)?;
            let common = ValType::common(&args[0].ty, &args[1].ty)
                .ok_or_else(|| err("`reflect` vectors must match"))?;
            if !matches!(common, ValType::Vector(_, sc) if sc.is_float()) {
                return Err(err("`reflect` requires float vectors"));
            }
            Ok(Some(common))
        }
        "refract" => {
            expect_arity(ident, args, 3)?;
            let common = ValType::common(&args[0].ty, &args[1].ty)
                .ok_or_else(|| err("`refract` vectors must match"))?;
            let ValType::Vector(_, sc) = common else {
                return Err(err("`refract` requires float vectors"));
            };
            if !sc.is_float()
                || !matches!(args[2].ty, ValType::Scalar(s) if ElemScalar::common(sc, s).is_some())
            {
                return Err(err(
                    "`refract` requires float vectors and matching eta scalar",
                ));
            }
            Ok(Some(common))
        }
        "determinant" => {
            expect_arity(ident, args, 1)?;
            let ValType::Matrix(c, r, sc) = args[0].ty else {
                return Err(err("`determinant` requires square float matrix"));
            };
            if c != r {
                return Err(err("`determinant` requires square matrix"));
            }
            Ok(Some(ValType::Scalar(sc)))
        }
        "transpose" => {
            expect_arity(ident, args, 1)?;
            let ValType::Matrix(c, r, sc) = args[0].ty else {
                return Err(err("`transpose` requires matrix"));
            };
            Ok(Some(ValType::Matrix(r, c, sc)))
        }
        "countLeadingZeros" | "countTrailingZeros" | "countOneBits" | "reverseBits"
        | "firstLeadingBit" | "firstTrailingBit" => {
            expect_arity(ident, args, 1)?;
            match args[0].ty {
                ValType::Scalar(s) | ValType::Vector(_, s) if s.is_int() => {
                    Ok(Some(args[0].ty.clone()))
                }
                _ => Err(err(format!("`{ident}` requires integer scalar or vector"))),
            }
        }
        "frexp" | "modf" => {
            expect_arity(ident, args, 1)?;
            let (sc, sz) = match args[0].ty {
                ValType::Scalar(s) if s.is_float() => (s.concretize(), None),
                ValType::Vector(v, s) if s.is_float() => (s.concretize(), Some(v)),
                _ => return Err(err(format!("`{ident}` requires float scalar or vector"))),
            };
            Ok(Some(if ident == "frexp" {
                ValType::FrexpResult(sc, sz)
            } else {
                ValType::ModfResult(sc, sz)
            }))
        }
        "pack4x8snorm" | "pack4x8unorm" => {
            expect_arity(ident, args, 1)?;
            if !args[0]
                .ty
                .coerces_to(&ValType::Vector(VectorSize::Quad, ElemScalar::F32))
            {
                return Err(err(format!("`{ident}` expects vec4<f32>")));
            }
            Ok(Some(ValType::Scalar(ElemScalar::U32)))
        }
        "pack2x16snorm" | "pack2x16unorm" | "pack2x16float" => {
            expect_arity(ident, args, 1)?;
            if !args[0]
                .ty
                .coerces_to(&ValType::Vector(VectorSize::Bi, ElemScalar::F32))
            {
                return Err(err(format!("`{ident}` expects vec2<f32>")));
            }
            Ok(Some(ValType::Scalar(ElemScalar::U32)))
        }
        "unpack4x8snorm" | "unpack4x8unorm" => {
            expect_arity(ident, args, 1)?;
            if !args[0].ty.coerces_to(&ValType::Scalar(ElemScalar::U32)) {
                return Err(err(format!("`{ident}` expects u32")));
            }
            Ok(Some(ValType::Vector(VectorSize::Quad, ElemScalar::F32)))
        }
        "unpack2x16snorm" | "unpack2x16unorm" | "unpack2x16float" => {
            expect_arity(ident, args, 1)?;
            if !args[0].ty.coerces_to(&ValType::Scalar(ElemScalar::U32)) {
                return Err(err(format!("`{ident}` expects u32")));
            }
            Ok(Some(ValType::Vector(VectorSize::Bi, ElemScalar::F32)))
        }
        _ => Ok(None),
    }
}

fn check_select(env: &ModuleEnv<'_>, args: &[ExprResult]) -> Result<ValType, ValidationError> {
    expect_arity("select", args, 3)?;
    let common = ValType::common(&args[0].ty, &args[1].ty).ok_or_else(|| {
        err(format!(
            "`select` branches `{}` and `{}` do not match",
            args[0].ty.describe(env.module),
            args[1].ty.describe(env.module)
        ))
    })?;
    match (&args[2].ty, &common) {
        (ValType::Scalar(ElemScalar::Bool), _) => Ok(common),
        (ValType::Vector(c_sz, ElemScalar::Bool), ValType::Vector(v_sz, _)) if c_sz == v_sz => {
            Ok(common)
        }
        _ => Err(err(
            "`select` condition must be bool or matching vecN<bool>",
        )),
    }
}

fn check_subgroup_builtin(
    env: &ModuleEnv<'_>,
    ident: &str,
    args: &[ExprResult],
) -> Result<Option<ValType>, ValidationError> {
    match ident {
        "subgroupAdd"
        | "subgroupMin"
        | "subgroupMax"
        | "subgroupMul"
        | "subgroupAnd"
        | "subgroupOr"
        | "subgroupXor"
        | "subgroupBroadcastFirst" => {
            expect_arity(ident, args, 1)?;
            expect_numeric_scalar_or_vec(env, ident, &args[0].ty)?;
            Ok(Some(args[0].ty.clone()))
        }
        "subgroupBroadcast"
        | "subgroupShuffle"
        | "subgroupShuffleXor"
        | "subgroupShuffleUp"
        | "subgroupShuffleDown" => {
            expect_arity(ident, args, 2)?;
            expect_numeric_scalar_or_vec(env, ident, &args[0].ty)?;
            expect_int_scalar(env, ident, &args[1].ty)?;
            Ok(Some(args[0].ty.clone()))
        }
        "subgroupBallot" => {
            expect_arity(ident, args, 1)?;
            if args[0].ty != ValType::Scalar(ElemScalar::Bool) {
                return Err(err("`subgroupBallot` requires bool argument"));
            }
            Ok(Some(ValType::Vector(VectorSize::Quad, ElemScalar::U32)))
        }
        "subgroupElect" => {
            expect_arity(ident, args, 0)?;
            Ok(Some(ValType::Scalar(ElemScalar::Bool)))
        }
        "subgroupAll" | "subgroupAny" => {
            expect_arity(ident, args, 1)?;
            if args[0].ty != ValType::Scalar(ElemScalar::Bool) {
                return Err(err(format!("`{ident}` requires bool argument")));
            }
            Ok(Some(ValType::Scalar(ElemScalar::Bool)))
        }
        _ => Ok(None),
    }
}

pub(super) fn expect_arity(
    ident: &str,
    args: &[ExprResult],
    expected: usize,
) -> Result<(), ValidationError> {
    if args.len() != expected {
        return Err(err(format!(
            "`{ident}` expected {expected} argument(s), got {}",
            args.len()
        )));
    }
    Ok(())
}

fn expect_float_scalar_or_vec(
    env: &ModuleEnv<'_>,
    ident: &str,
    ty: &ValType,
) -> Result<ElemScalar, ValidationError> {
    match *ty {
        ValType::Scalar(s) | ValType::Vector(_, s)
            if s.is_float() || s == ElemScalar::AbstractInt =>
        {
            Ok(if s == ElemScalar::AbstractInt {
                ElemScalar::AbstractFloat
            } else {
                s
            })
        }
        _ => Err(err(format!(
            "`{ident}` expects float scalar or vector, got `{}`",
            ty.describe(env.module)
        ))),
    }
}

fn expect_numeric_scalar_or_vec(
    env: &ModuleEnv<'_>,
    ident: &str,
    ty: &ValType,
) -> Result<ElemScalar, ValidationError> {
    match *ty {
        ValType::Scalar(s) | ValType::Vector(_, s) if s.is_numeric() => Ok(s),
        _ => Err(err(format!(
            "`{ident}` expects numeric scalar or vector, got `{}`",
            ty.describe(env.module)
        ))),
    }
}

fn expect_int_scalar(
    env: &ModuleEnv<'_>,
    ident: &str,
    ty: &ValType,
) -> Result<(), ValidationError> {
    match *ty {
        ValType::Scalar(s) if s.is_int() => Ok(()),
        _ => Err(err(format!(
            "`{ident}` expects integer scalar, got `{}`",
            ty.describe(env.module)
        ))),
    }
}
