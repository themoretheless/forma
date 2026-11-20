//! WGSL expression parser and typechecker.

use super::builtins::{
    check_bitcast, check_builtin_call, check_constructor, check_inferred_array_ctor, err,
};
use super::types::{ElemScalar, ValType, resolve_type_str};
use super::{ModuleEnv, ScopeStack, ValidationError};
use crate::front::wgsl::find_matching_delim;
use crate::{AddressSpace, StorageAccess, TypeInner, VectorSize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LValueInfo {
    pub(super) space: AddressSpace,
    pub(super) access: StorageAccess,
    pub(super) mutable: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ExprResult {
    pub(super) ty: ValType,
    pub(super) lvalue: Option<LValueInfo>,
    pub(super) raw: String,
}

impl ExprResult {
    pub(super) fn rvalue(ty: ValType, raw: &str) -> Self {
        Self {
            ty,
            lvalue: None,
            raw: raw.to_string(),
        }
    }
}

pub(super) fn check_expr(
    env: &ModuleEnv<'_>,
    scopes: &ScopeStack,
    src: &str,
) -> Result<ExprResult, ValidationError> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Err(err("expected expression"));
    }
    let mut parser = ExprParser {
        env,
        scopes,
        src: trimmed,
        pos: 0,
    };
    let result = parser.parse_or()?;
    parser.skip_ws();
    if parser.pos != parser.src.len() {
        return Err(err(format!(
            "unexpected trailing token `{}` in expression `{trimmed}`",
            parser.rest()
        )));
    }
    Ok(result)
}

pub(super) fn check_value_expr(
    env: &ModuleEnv<'_>,
    scopes: &ScopeStack,
    src: &str,
) -> Result<ExprResult, ValidationError> {
    let res = check_expr(env, scopes, src)?;
    if res.ty == ValType::Void {
        return Err(err(format!(
            "void expression `{src}` used where a value is required"
        )));
    }
    Ok(res)
}

struct ExprParser<'a> {
    env: &'a ModuleEnv<'a>,
    scopes: &'a ScopeStack,
    src: &'a str,
    pos: usize,
}

impl<'a> ExprParser<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.src.as_bytes().get(self.pos) {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn eat_exact(&mut self, tok: &str) -> bool {
        self.skip_ws();
        if self.rest().starts_with(tok) {
            self.pos += tok.len();
            true
        } else {
            false
        }
    }

    fn eat_single_op(&mut self, ch: u8, disallow_next: &[u8]) -> bool {
        self.skip_ws();
        let bytes = self.rest().as_bytes();
        if bytes.first().copied() == Some(ch)
            && bytes.get(1).is_none_or(|b| !disallow_next.contains(b))
        {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_or(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_and()?;
        while self.eat_exact("||") {
            let rhs = self.parse_and()?;
            lhs = self.apply_logical("||", lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_bit_or()?;
        while self.eat_exact("&&") {
            let rhs = self.parse_bit_or()?;
            lhs = self.apply_logical("&&", lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_bit_or(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_bit_xor()?;
        while self.eat_single_op(b'|', b"|=") {
            let rhs = self.parse_bit_xor()?;
            lhs = self.apply_bitwise("|", lhs, rhs, true)?;
        }
        Ok(lhs)
    }

    fn parse_bit_xor(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_bit_and()?;
        while self.eat_single_op(b'^', b"=") {
            let rhs = self.parse_bit_and()?;
            lhs = self.apply_bitwise("^", lhs, rhs, false)?;
        }
        Ok(lhs)
    }

    fn parse_bit_and(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_eq()?;
        while self.eat_single_op(b'&', b"&=") {
            let rhs = self.parse_eq()?;
            lhs = self.apply_bitwise("&", lhs, rhs, true)?;
        }
        Ok(lhs)
    }

    fn parse_eq(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_rel()?;
        loop {
            let op = if self.eat_exact("==") {
                "=="
            } else if self.eat_exact("!=") {
                "!="
            } else {
                break;
            };
            let rhs = self.parse_rel()?;
            lhs = self.apply_eq(op, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_rel(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_shift()?;
        loop {
            let op = if self.eat_exact("<=") {
                "<="
            } else if self.eat_exact(">=") {
                ">="
            } else if self.eat_single_op(b'<', b"<=") {
                "<"
            } else if self.eat_single_op(b'>', b">=") {
                ">"
            } else {
                break;
            };
            let rhs = self.parse_shift()?;
            lhs = self.apply_rel(op, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_add()?;
        loop {
            self.skip_ws();
            let rest = self.rest();
            let op = if rest.starts_with("<<") && !rest.starts_with("<<=") {
                self.pos += 2;
                "<<"
            } else if rest.starts_with(">>") && !rest.starts_with(">>=") {
                self.pos += 2;
                ">>"
            } else {
                break;
            };
            let rhs = self.parse_add()?;
            lhs = self.apply_shift(op, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = if self.eat_single_op(b'+', b"+=") {
                "+"
            } else if self.eat_single_op(b'-', b"-=>") {
                "-"
            } else {
                break;
            };
            let rhs = self.parse_mul()?;
            lhs = self.apply_arith(op, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<ExprResult, ValidationError> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = if self.eat_single_op(b'*', b"=") {
                "*"
            } else if self.eat_single_op(b'/', b"=") {
                "/"
            } else if self.eat_single_op(b'%', b"=") {
                "%"
            } else {
                break;
            };
            let rhs = self.parse_unary()?;
            lhs = self.apply_arith(op, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<ExprResult, ValidationError> {
        self.skip_ws();
        let start = self.pos;
        if self.eat_exact("-") {
            let inner = self.parse_unary()?;
            let ok = match inner.ty {
                ValType::Scalar(s) | ValType::Vector(_, s) | ValType::Matrix(_, _, s) => {
                    s.is_signed_or_float()
                }
                _ => false,
            };
            if !ok {
                return Err(err(format!(
                    "unary `-` cannot be applied to `{}`",
                    inner.ty.describe(self.env.module)
                )));
            }
            return Ok(ExprResult::rvalue(inner.ty, &self.src[start..self.pos]));
        }
        if self.eat_exact("!") {
            let inner = self.parse_unary()?;
            let ok = matches!(
                inner.ty,
                ValType::Scalar(ElemScalar::Bool) | ValType::Vector(_, ElemScalar::Bool)
            );
            if !ok {
                return Err(err(format!(
                    "unary `!` cannot be applied to `{}`",
                    inner.ty.describe(self.env.module)
                )));
            }
            return Ok(ExprResult::rvalue(inner.ty, &self.src[start..self.pos]));
        }
        if self.eat_exact("~") {
            let inner = self.parse_unary()?;
            let ok = matches!(
                inner.ty,
                ValType::Scalar(s) | ValType::Vector(_, s) if s.is_int()
            );
            if !ok {
                return Err(err(format!(
                    "unary `~` requires integer operand, got `{}`",
                    inner.ty.describe(self.env.module)
                )));
            }
            return Ok(ExprResult::rvalue(inner.ty, &self.src[start..self.pos]));
        }
        if self.eat_single_op(b'&', b"&=") {
            let inner = self.parse_unary()?;
            let Some(lv) = inner.lvalue else {
                return Err(err("cannot take address `&` of non-addressable expression"));
            };
            let ptr_ty = ValType::Pointer {
                base: Box::new(inner.ty),
                space: lv.space,
                access: lv.access,
            };
            return Ok(ExprResult::rvalue(ptr_ty, &self.src[start..self.pos]));
        }
        if self.eat_exact("*") {
            let inner = self.parse_unary()?;
            let ValType::Pointer {
                base,
                space,
                access,
            } = inner.ty
            else {
                return Err(err(format!(
                    "cannot dereference non-pointer `{}`",
                    inner.ty.describe(self.env.module)
                )));
            };
            let mutable = space != AddressSpace::Uniform
                && space != AddressSpace::Handle
                && access != StorageAccess::Read;
            return Ok(ExprResult {
                ty: *base,
                lvalue: Some(LValueInfo {
                    space,
                    access,
                    mutable,
                }),
                raw: self.src[start..self.pos].to_string(),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<ExprResult, ValidationError> {
        let mut base = self.parse_primary()?;
        loop {
            self.skip_ws();
            if self.rest().starts_with('[') {
                self.pos += 1;
                let idx = self.parse_or()?;
                if !self.eat_exact("]") {
                    return Err(err("expected `]` after index expression"));
                }
                base = self.apply_index(base, idx)?;
            } else if self.rest().starts_with('.') {
                self.pos += 1;
                let field = self
                    .parse_ident_token()
                    .ok_or_else(|| err("expected member or swizzle identifier after `.`"))?;
                base = self.apply_member(base, field)?;
            } else {
                break;
            }
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<ExprResult, ValidationError> {
        self.skip_ws();
        let rest = self.rest();
        if rest.starts_with('(') {
            self.pos += 1;
            let inner = self.parse_or()?;
            if !self.eat_exact(")") {
                return Err(err("expected closing `)` in expression"));
            }
            return Ok(ExprResult {
                lvalue: None,
                ..inner
            });
        }
        let b = rest
            .as_bytes()
            .first()
            .copied()
            .ok_or_else(|| err(format!("unexpected end of expression `{}`", self.src)))?;
        if b.is_ascii_digit()
            || (b == b'.' && rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
        {
            return self.parse_number_literal();
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            return self.parse_ident_primary();
        }
        Err(err(format!("unexpected token at `{rest}`")))
    }

    fn parse_ident_token(&mut self) -> Option<&'a str> {
        self.skip_ws();
        let start = self.pos;
        let bytes = self.src.as_bytes();
        let first = bytes.get(start).copied()?;
        if !first.is_ascii_alphabetic() && first != b'_' {
            return None;
        }
        let mut end = start + 1;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        self.pos = end;
        Some(&self.src[start..end])
    }

    fn parse_ident_primary(&mut self) -> Result<ExprResult, ValidationError> {
        let start = self.pos;
        let ident = self
            .parse_ident_token()
            .ok_or_else(|| err("expected identifier"))?;
        if ident == "true" || ident == "false" {
            return Ok(ExprResult::rvalue(ValType::Scalar(ElemScalar::Bool), ident));
        }

        // Generic type constructors & bitcast: `vec3<f32>(...)`, `array<T, N>(...)`, `bitcast<T>(...)`
        if matches!(
            ident,
            "bitcast"
                | "array"
                | "vec2"
                | "vec3"
                | "vec4"
                | "mat2x2"
                | "mat2x3"
                | "mat2x4"
                | "mat3x2"
                | "mat3x3"
                | "mat3x4"
                | "mat4x2"
                | "mat4x3"
                | "mat4x4"
        ) {
            self.skip_ws();
            if self.rest().starts_with('<') {
                return self.parse_generic_call(ident, start);
            }
        }

        self.skip_ws();
        if self.rest().starts_with('(') {
            return self.parse_call_expr(ident, start);
        }

        self.resolve_identifier(ident)
    }

    fn parse_generic_call(
        &mut self,
        ident: &str,
        start: usize,
    ) -> Result<ExprResult, ValidationError> {
        let close_rel = find_matching_delim(self.rest(), '<', '>')
            .ok_or_else(|| err(format!("unclosed `<` in `{ident}<...>`")))?;
        let type_args = &self.rest()[1..close_rel];
        self.pos += close_rel + 1;
        let args = self.parse_call_arg_list()?;
        if ident == "bitcast" {
            if args.len() != 1 {
                return Err(err("`bitcast` requires exactly one argument"));
            }
            let ty = check_bitcast(self.env, type_args, &args[0])?;
            return Ok(ExprResult::rvalue(ty, &self.src[start..self.pos]));
        }
        let full_ty_str = format!("{ident}<{type_args}>");
        let target = resolve_type_str(self.env, &full_ty_str).map_err(err)?;
        let ty = check_constructor(self.env, &target, &args, &full_ty_str)?;
        Ok(ExprResult::rvalue(ty, &self.src[start..self.pos]))
    }

    fn parse_call_expr(
        &mut self,
        ident: &str,
        start: usize,
    ) -> Result<ExprResult, ValidationError> {
        let open_pos = self.pos;
        let close_rel = find_matching_delim(self.rest(), '(', ')')
            .ok_or_else(|| err(format!("unclosed `(` in call to `{ident}`")))?;
        let raw_args = &self.rest()[1..close_rel];
        let args = self.parse_call_arg_list()?;
        let _ = open_pos;

        if ident == "array" {
            let ty = check_inferred_array_ctor(self.env, &args)?;
            return Ok(ExprResult::rvalue(ty, &self.src[start..self.pos]));
        }
        if let Some(sig) = self.env.functions.get(ident) {
            if args.len() != sig.params.len() {
                return Err(err(format!(
                    "function `{ident}` expected {} argument(s), got {}",
                    sig.params.len(),
                    args.len()
                )));
            }
            for (i, (arg, param_ty)) in args.iter().zip(&sig.params).enumerate() {
                if !arg.ty.coerces_to(param_ty) {
                    return Err(err(format!(
                        "function `{ident}` argument {i}: expected `{}`, got `{}`",
                        param_ty.describe(self.env.module),
                        arg.ty.describe(self.env.module)
                    )));
                }
            }
            let ret = sig.ret.clone().unwrap_or(ValType::Void);
            return Ok(ExprResult::rvalue(ret, &self.src[start..self.pos]));
        }
        if let Some(ty) = check_builtin_call(self.env, ident, &args, raw_args)? {
            return Ok(ExprResult::rvalue(ty, &self.src[start..self.pos]));
        }
        Err(err(format!("unknown function `{ident}`")))
    }

    fn parse_call_arg_list(&mut self) -> Result<Vec<ExprResult>, ValidationError> {
        self.skip_ws();
        if !self.eat_exact("(") {
            return Err(err("expected `(` for call arguments"));
        }
        let mut args = Vec::new();
        self.skip_ws();
        if self.eat_exact(")") {
            return Ok(args);
        }
        loop {
            let arg = check_value_expr_from_parser(self)?;
            args.push(arg);
            self.skip_ws();
            if self.eat_exact(")") {
                break;
            }
            if !self.eat_exact(",") {
                return Err(err("expected `,` or `)` in call argument list"));
            }
            self.skip_ws();
            // Tolerate trailing comma in WGSL argument lists
            if self.eat_exact(")") {
                break;
            }
        }
        Ok(args)
    }

    fn parse_number_literal(&mut self) -> Result<ExprResult, ValidationError> {
        let start = self.pos;
        let bytes = self.src.as_bytes();
        let mut is_float = false;
        let mut is_hex = false;

        if self.rest().starts_with("0x") || self.rest().starts_with("0X") {
            is_hex = true;
            self.pos += 2;
            while let Some(&b) = bytes.get(self.pos) {
                if b.is_ascii_hexdigit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        } else {
            while let Some(&b) = bytes.get(self.pos) {
                if b.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if bytes.get(self.pos).copied() == Some(b'.') {
                is_float = true;
                self.pos += 1;
                while let Some(&b) = bytes.get(self.pos) {
                    if b.is_ascii_digit() {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
            if matches!(bytes.get(self.pos).copied(), Some(b'e' | b'E'))
                && bytes
                    .get(self.pos + 1)
                    .is_some_and(|c| c.is_ascii_digit() || *c == b'+' || *c == b'-')
            {
                is_float = true;
                self.pos += 1;
                if matches!(bytes.get(self.pos).copied(), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                if !bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
                    return Err(err("malformed exponent in float literal"));
                }
                while let Some(&b) = bytes.get(self.pos) {
                    if b.is_ascii_digit() {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
        }

        let suffix = bytes.get(self.pos).copied();
        let sc = if is_hex {
            match suffix {
                Some(b'u') => {
                    self.pos += 1;
                    ElemScalar::U32
                }
                Some(b'i') => {
                    self.pos += 1;
                    ElemScalar::I32
                }
                Some(s) if s.is_ascii_alphanumeric() || s == b'_' => {
                    return Err(err(format!(
                        "unexpected suffix `{}` on hex integer literal",
                        s as char
                    )));
                }
                _ => ElemScalar::AbstractInt,
            }
        } else {
            match (is_float, suffix) {
                (false, Some(b'u')) => {
                    self.pos += 1;
                    ElemScalar::U32
                }
                (false, Some(b'i')) => {
                    self.pos += 1;
                    ElemScalar::I32
                }
                (_, Some(b'f')) => {
                    self.pos += 1;
                    ElemScalar::F32
                }
                (_, Some(b'h')) => {
                    self.pos += 1;
                    ElemScalar::F16
                }
                (true, _) => ElemScalar::AbstractFloat,
                (false, _) => ElemScalar::AbstractInt,
            }
        };
        Ok(ExprResult::rvalue(
            ValType::Scalar(sc),
            &self.src[start..self.pos],
        ))
    }

    fn resolve_identifier(&self, ident: &str) -> Result<ExprResult, ValidationError> {
        if let Some(var) = self.scopes.lookup(ident) {
            return Ok(ExprResult {
                ty: var.ty.clone(),
                lvalue: Some(LValueInfo {
                    space: var.space,
                    access: StorageAccess::ReadWrite,
                    mutable: var.mutable,
                }),
                raw: ident.to_string(),
            });
        }
        if let Some(g) = self.env.globals.get(ident) {
            return Ok(ExprResult {
                ty: g.ty.clone(),
                lvalue: Some(LValueInfo {
                    space: g.space,
                    access: g.access,
                    mutable: g.mutable,
                }),
                raw: ident.to_string(),
            });
        }
        if let Some(ty) = self.env.constants.get(ident) {
            return Ok(ExprResult::rvalue(ty.clone(), ident));
        }
        Err(err(format!("unknown identifier `{ident}`")))
    }

    fn apply_index(
        &self,
        base: ExprResult,
        idx: ExprResult,
    ) -> Result<ExprResult, ValidationError> {
        if !matches!(idx.ty, ValType::Scalar(s) if s.is_int()) {
            return Err(err(format!(
                "index must be integer, got `{}`",
                idx.ty.describe(self.env.module)
            )));
        }
        let elem_ty = match base.ty {
            ValType::Array(elem, _) => *elem,
            ValType::Vector(_, sc) => ValType::Scalar(sc),
            ValType::Matrix(_, rows, sc) => ValType::Vector(rows, sc),
            other => {
                return Err(err(format!(
                    "cannot index into `{}`",
                    other.describe(self.env.module)
                )));
            }
        };
        Ok(ExprResult {
            ty: elem_ty,
            lvalue: base.lvalue,
            raw: format!("{}[{}]", base.raw, idx.raw),
        })
    }

    fn apply_member(&self, base: ExprResult, field: &str) -> Result<ExprResult, ValidationError> {
        match base.ty {
            ValType::Struct(handle) => {
                let TypeInner::Struct { members, .. } = &self.env.module.types[handle].inner else {
                    return Err(err("invalid struct handle"));
                };
                let member = members
                    .iter()
                    .find(|m| m.name.as_deref() == Some(field))
                    .ok_or_else(|| {
                        err(format!(
                            "struct `{}` has no member named `{field}`",
                            self.env.module.types[handle]
                                .name
                                .as_deref()
                                .unwrap_or("<anonymous>")
                        ))
                    })?;
                Ok(ExprResult {
                    ty: ValType::from_ir(self.env.module, member.ty),
                    lvalue: base.lvalue,
                    raw: format!("{}.{field}", base.raw),
                })
            }
            ValType::Vector(size, sc) => {
                let len = validate_swizzle(size, field)?;
                if len == 1 {
                    Ok(ExprResult {
                        ty: ValType::Scalar(sc),
                        lvalue: base.lvalue,
                        raw: format!("{}.{field}", base.raw),
                    })
                } else {
                    let out_sz = match len {
                        2 => VectorSize::Bi,
                        3 => VectorSize::Tri,
                        _ => VectorSize::Quad,
                    };
                    Ok(ExprResult::rvalue(
                        ValType::Vector(out_sz, sc),
                        &format!("{}.{field}", base.raw),
                    ))
                }
            }
            ValType::FrexpResult(sc, sz) => match field {
                "fract" => Ok(ExprResult::rvalue(
                    sz.map_or(ValType::Scalar(sc), |v| ValType::Vector(v, sc)),
                    &format!("{}.{field}", base.raw),
                )),
                "exp" => Ok(ExprResult::rvalue(
                    sz.map_or(ValType::Scalar(ElemScalar::I32), |v| {
                        ValType::Vector(v, ElemScalar::I32)
                    }),
                    &format!("{}.{field}", base.raw),
                )),
                _ => Err(err(format!("frexp result has no member `{field}`"))),
            },
            ValType::ModfResult(sc, sz) => match field {
                "fract" | "whole" => Ok(ExprResult::rvalue(
                    sz.map_or(ValType::Scalar(sc), |v| ValType::Vector(v, sc)),
                    &format!("{}.{field}", base.raw),
                )),
                _ => Err(err(format!("modf result has no member `{field}`"))),
            },
            ValType::AtomicCmpExchResult(s) => match field {
                "old_value" => Ok(ExprResult::rvalue(
                    ValType::Scalar(ElemScalar::from_ir(s)),
                    &format!("{}.{field}", base.raw),
                )),
                "exchanged" => Ok(ExprResult::rvalue(
                    ValType::Scalar(ElemScalar::Bool),
                    &format!("{}.{field}", base.raw),
                )),
                _ => Err(err(format!(
                    "atomicCompareExchangeWeak result has no member `{field}`"
                ))),
            },
            other => Err(err(format!(
                "cannot access member `.{field}` on `{}`",
                other.describe(self.env.module)
            ))),
        }
    }

    fn apply_logical(
        &self,
        op: &str,
        lhs: ExprResult,
        rhs: ExprResult,
    ) -> Result<ExprResult, ValidationError> {
        if lhs.ty != ValType::Scalar(ElemScalar::Bool)
            || rhs.ty != ValType::Scalar(ElemScalar::Bool)
        {
            return Err(err(format!(
                "operator `{op}` requires `bool` operands, got `{}` and `{}`",
                lhs.ty.describe(self.env.module),
                rhs.ty.describe(self.env.module)
            )));
        }
        Ok(ExprResult::rvalue(ValType::Scalar(ElemScalar::Bool), ""))
    }

    fn apply_bitwise(
        &self,
        op: &str,
        lhs: ExprResult,
        rhs: ExprResult,
        allow_bool: bool,
    ) -> Result<ExprResult, ValidationError> {
        let Some(common) = ValType::common(&lhs.ty, &rhs.ty) else {
            return Err(err(format!(
                "operator `{op}` type mismatch: `{}` and `{}`",
                lhs.ty.describe(self.env.module),
                rhs.ty.describe(self.env.module)
            )));
        };
        let valid = match common {
            ValType::Scalar(s) | ValType::Vector(_, s) => {
                s.is_int() || (allow_bool && s == ElemScalar::Bool)
            }
            _ => false,
        };
        if !valid {
            return Err(err(format!(
                "operator `{op}` invalid on `{}`",
                common.describe(self.env.module)
            )));
        }
        Ok(ExprResult::rvalue(common, ""))
    }

    fn apply_eq(
        &self,
        op: &str,
        lhs: ExprResult,
        rhs: ExprResult,
    ) -> Result<ExprResult, ValidationError> {
        let Some(common) = ValType::common(&lhs.ty, &rhs.ty) else {
            return Err(err(format!(
                "comparison `{op}` type mismatch: `{}` and `{}`",
                lhs.ty.describe(self.env.module),
                rhs.ty.describe(self.env.module)
            )));
        };
        let ty = match common {
            ValType::Scalar(_) => ValType::Scalar(ElemScalar::Bool),
            ValType::Vector(sz, _) => ValType::Vector(sz, ElemScalar::Bool),
            _ => {
                return Err(err(format!(
                    "comparison `{op}` is not supported for `{}`",
                    common.describe(self.env.module)
                )));
            }
        };
        Ok(ExprResult::rvalue(ty, ""))
    }

    fn apply_rel(
        &self,
        op: &str,
        lhs: ExprResult,
        rhs: ExprResult,
    ) -> Result<ExprResult, ValidationError> {
        let Some(common) = ValType::common(&lhs.ty, &rhs.ty) else {
            return Err(err(format!(
                "comparison `{op}` type mismatch: `{}` and `{}`",
                lhs.ty.describe(self.env.module),
                rhs.ty.describe(self.env.module)
            )));
        };
        let ty = match common {
            ValType::Scalar(s) if s.is_numeric() => ValType::Scalar(ElemScalar::Bool),
            ValType::Vector(sz, s) if s.is_numeric() => ValType::Vector(sz, ElemScalar::Bool),
            _ => {
                return Err(err(format!(
                    "comparison `{op}` requires numeric operands, got `{}`",
                    common.describe(self.env.module)
                )));
            }
        };
        Ok(ExprResult::rvalue(ty, ""))
    }

    fn apply_shift(
        &self,
        op: &str,
        lhs: ExprResult,
        rhs: ExprResult,
    ) -> Result<ExprResult, ValidationError> {
        match (&lhs.ty, &rhs.ty) {
            (ValType::Scalar(ls), ValType::Scalar(rs))
                if ls.is_int() && matches!(rs, ElemScalar::U32 | ElemScalar::AbstractInt) =>
            {
                Ok(ExprResult::rvalue(lhs.ty, ""))
            }
            (ValType::Vector(lsz, ls), ValType::Vector(rsz, rs))
                if ls.is_int()
                    && lsz == rsz
                    && matches!(rs, ElemScalar::U32 | ElemScalar::AbstractInt) =>
            {
                Ok(ExprResult::rvalue(lhs.ty, ""))
            }
            _ => Err(err(format!(
                "shift `{op}` invalid for `{}` and `{}`",
                lhs.ty.describe(self.env.module),
                rhs.ty.describe(self.env.module)
            ))),
        }
    }

    fn apply_arith(
        &self,
        op: &str,
        lhs: ExprResult,
        rhs: ExprResult,
    ) -> Result<ExprResult, ValidationError> {
        apply_binary_arith_types(self.env.module, op, &lhs.ty, &rhs.ty)
            .map(|ty| ExprResult::rvalue(ty, ""))
    }
}

fn check_value_expr_from_parser(
    parser: &mut ExprParser<'_>,
) -> Result<ExprResult, ValidationError> {
    let res = parser.parse_or()?;
    if res.ty == ValType::Void {
        return Err(err("void expression cannot be passed as an argument"));
    }
    Ok(res)
}

pub(super) fn apply_binary_arith_types(
    module: &crate::Module,
    op: &str,
    lhs: &ValType,
    rhs: &ValType,
) -> Result<ValType, ValidationError> {
    if let Some(common) = ValType::common(lhs, rhs) {
        let valid = match &common {
            ValType::Scalar(s) | ValType::Vector(_, s) => s.is_numeric(),
            ValType::Matrix(_, _, s) if matches!(op, "+" | "-" | "*") => s.is_float(),
            _ => false,
        };
        if valid {
            return Ok(common);
        }
    }
    match (op, lhs, rhs) {
        (_, ValType::Vector(sz, s1), ValType::Scalar(s2))
        | (_, ValType::Scalar(s2), ValType::Vector(sz, s1))
            if s1.is_numeric() && s2.is_numeric() =>
        {
            if let Some(sc) = ElemScalar::common(*s1, *s2) {
                return Ok(ValType::Vector(*sz, sc));
            }
        }
        ("*", ValType::Matrix(c, r, s1), ValType::Scalar(s2))
        | ("*", ValType::Scalar(s2), ValType::Matrix(c, r, s1))
            if s1.is_float() && s2.is_numeric() =>
        {
            if let Some(sc) = ElemScalar::common(*s1, *s2) {
                return Ok(ValType::Matrix(*c, *r, sc));
            }
        }
        ("*", ValType::Matrix(c, r, s1), ValType::Vector(v, s2)) if c == v => {
            if let Some(sc) = ElemScalar::common(*s1, *s2) {
                return Ok(ValType::Vector(*r, sc));
            }
        }
        ("*", ValType::Vector(v, s1), ValType::Matrix(c, r, s2)) if v == r => {
            if let Some(sc) = ElemScalar::common(*s1, *s2) {
                return Ok(ValType::Vector(*c, sc));
            }
        }
        ("*", ValType::Matrix(c1, r1, s1), ValType::Matrix(c2, r2, s2)) if c1 == r2 => {
            if let Some(sc) = ElemScalar::common(*s1, *s2) {
                return Ok(ValType::Matrix(*c2, *r1, sc));
            }
        }
        _ => {}
    }
    Err(err(format!(
        "operator `{op}` invalid for `{}` and `{}`",
        lhs.describe(module),
        rhs.describe(module)
    )))
}

fn validate_swizzle(size: VectorSize, field: &str) -> Result<usize, ValidationError> {
    if field.is_empty() || field.len() > 4 {
        return Err(err(format!("invalid vector member/swizzle `.{field}`")));
    }
    let bytes = field.as_bytes();
    let is_xyzw = bytes.iter().all(|b| matches!(b, b'x' | b'y' | b'z' | b'w'));
    let is_rgba = bytes.iter().all(|b| matches!(b, b'r' | b'g' | b'b' | b'a'));
    if !is_xyzw && !is_rgba {
        return Err(err(format!("invalid vector swizzle `.{field}`")));
    }
    let max_dim = size.as_u32();
    for b in bytes {
        let idx = match b {
            b'x' | b'r' => 0,
            b'y' | b'g' => 1,
            b'z' | b'b' => 2,
            b'w' | b'a' => 3,
            _ => unreachable!(),
        };
        if idx >= max_dim {
            return Err(err(format!(
                "swizzle component `{}` out of bounds for vec{max_dim}",
                *b as char
            )));
        }
    }
    Ok(field.len())
}
