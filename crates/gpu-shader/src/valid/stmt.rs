//! WGSL statement and control-flow validation.

use super::builtins::err;
use super::expr::{apply_binary_arith_types, check_expr, check_value_expr};
use super::types::{ElemScalar, ValType, resolve_type_str};
use super::{LocalVar, ModuleEnv, ScopeStack, ValidationError};
use crate::AddressSpace;
use crate::front::wgsl::{
    TopLevelScan, find_block_open, find_matching_delim, find_stmt_semicolon, find_top_level_char,
    split_comma_top_level, strip_kw_token,
};

pub(super) fn check_block(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    body: &str,
    expected_ret: Option<&ValType>,
) -> Result<bool, ValidationError> {
    scopes.push_scope();
    let res = check_stmt_list(env, scopes, body, expected_ret);
    scopes.pop_scope();
    res
}

fn check_stmt_list(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    body: &str,
    expected_ret: Option<&ValType>,
) -> Result<bool, ValidationError> {
    let bytes = body.as_bytes();
    let mut i = 0usize;
    let mut returns = false;

    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let rest = &body[i..];
        if rest.starts_with(';') {
            i += 1;
            continue;
        }
        if rest.starts_with('{') {
            let close =
                find_matching_delim(rest, '{', '}').ok_or_else(|| err("unclosed `{` in block"))?;
            returns |= check_block(env, scopes, &rest[1..close], expected_ret)?;
            i += close + 1;
            continue;
        }
        if let Some((consumed, br)) = try_control_flow(env, scopes, rest, expected_ret)? {
            returns |= br;
            i += consumed;
            continue;
        }
        if let Some(semi_off) = find_stmt_semicolon(rest) {
            returns |= check_simple_stmt(env, scopes, &rest[..semi_off], expected_ret)?;
            i += semi_off + 1;
        } else {
            returns |= check_simple_stmt(env, scopes, rest, expected_ret)?;
            break;
        }
    }
    Ok(returns)
}

fn try_control_flow(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    rest: &str,
    expected_ret: Option<&ValType>,
) -> Result<Option<(usize, bool)>, ValidationError> {
    if let Some(after_if) = strip_kw_token(rest, "if") {
        return check_if_stmt(env, scopes, rest, after_if, expected_ret).map(Some);
    }
    if let Some(after_while) = strip_kw_token(rest, "while") {
        return check_while_stmt(env, scopes, rest, after_while, expected_ret).map(Some);
    }
    if let Some(after_loop) = strip_kw_token(rest, "loop") {
        return check_loop_stmt(env, scopes, rest, after_loop, expected_ret).map(Some);
    }
    if let Some(after_for) = strip_kw_token(rest, "for") {
        return check_for_stmt(env, scopes, rest, after_for, expected_ret).map(Some);
    }
    if let Some(after_switch) = strip_kw_token(rest, "switch") {
        return check_switch_stmt(env, scopes, rest, after_switch, expected_ret).map(Some);
    }
    Ok(None)
}

fn parse_block_at<'a>(rest: &'a str, kw: &str) -> Result<(&'a str, usize), ValidationError> {
    let blk = rest.trim_start();
    if !blk.starts_with('{') {
        return Err(err(format!("expected `{{` in `{kw}` statement")));
    }
    let close = find_matching_delim(blk, '{', '}')
        .ok_or_else(|| err(format!("unclosed `{{` in `{kw}` statement")))?;
    let total = (rest.len() - blk.len()) + close + 1;
    Ok((&blk[1..close], total))
}

fn check_bool_condition(
    env: &ModuleEnv<'_>,
    scopes: &ScopeStack,
    cond_src: &str,
    kw: &str,
) -> Result<(), ValidationError> {
    let cond = check_value_expr(env, scopes, cond_src)?;
    if cond.ty != ValType::Scalar(ElemScalar::Bool) {
        return Err(err(format!(
            "`{kw}` condition must have type `bool`, got `{}`",
            cond.ty.describe(env.module)
        )));
    }
    Ok(())
}

fn check_if_stmt(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    full: &str,
    after_if: &str,
    expected_ret: Option<&ValType>,
) -> Result<(usize, bool), ValidationError> {
    let open = find_block_open(after_if).ok_or_else(|| err("expected `{` after `if` condition"))?;
    check_bool_condition(env, scopes, after_if[..open].trim(), "if")?;
    let (then_body, then_len) = parse_block_at(&after_if[open..], "if")?;
    let then_returns = check_block(env, scopes, then_body, expected_ret)?;

    let mut consumed = (full.len() - after_if.len()) + open + then_len;
    let tail = full[consumed..].trim_start();
    let ws_before_else = full[consumed..].len() - tail.len();
    let mut else_returns = false;
    let mut has_else = false;

    if let Some(after_else) = strip_kw_token(tail, "else") {
        let after_else_trim = after_else.trim_start();
        if strip_kw_token(after_else_trim, "if").is_some() {
            let (elif_consumed, elif_returns) = check_if_stmt(
                env,
                scopes,
                after_else_trim,
                strip_kw_token(after_else_trim, "if").unwrap_or_default(),
                expected_ret,
            )?;
            else_returns = elif_returns;
            has_else = true;
            consumed += ws_before_else + (tail.len() - after_else_trim.len()) + elif_consumed;
        } else {
            let (else_body, else_len) = parse_block_at(after_else, "else")?;
            else_returns = check_block(env, scopes, else_body, expected_ret)?;
            has_else = true;
            consumed += ws_before_else + (tail.len() - after_else.len()) + else_len;
        }
    }

    Ok((consumed, then_returns && has_else && else_returns))
}

fn check_while_stmt(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    full: &str,
    after_while: &str,
    expected_ret: Option<&ValType>,
) -> Result<(usize, bool), ValidationError> {
    let open =
        find_block_open(after_while).ok_or_else(|| err("expected `{` after `while` condition"))?;
    check_bool_condition(env, scopes, after_while[..open].trim(), "while")?;
    let (body, body_len) = parse_block_at(&after_while[open..], "while")?;
    check_block(env, scopes, body, expected_ret)?;
    Ok(((full.len() - after_while.len()) + open + body_len, false))
}

fn check_loop_stmt(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    full: &str,
    after_loop: &str,
    expected_ret: Option<&ValType>,
) -> Result<(usize, bool), ValidationError> {
    let (inner, block_len) = parse_block_at(after_loop, "loop")?;
    if let Some((before_cont, cont_body)) = split_continuing_block(inner) {
        scopes.push_scope();
        let body_res = check_stmt_list(env, scopes, before_cont, expected_ret);
        let cont_res = if body_res.is_ok() {
            check_continuing_block(env, scopes, cont_body, expected_ret)
        } else {
            Ok(())
        };
        scopes.pop_scope();
        body_res?;
        cont_res?;
    } else {
        check_block(env, scopes, inner, expected_ret)?;
    }
    Ok(((full.len() - after_loop.len()) + block_len, false))
}

fn check_continuing_block(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    cont_body: &str,
    expected_ret: Option<&ValType>,
) -> Result<(), ValidationError> {
    let trimmed = cont_body.trim();
    if let Some(break_pos) = find_break_if(trimmed) {
        check_stmt_list(env, scopes, &trimmed[..break_pos], expected_ret)?;
        let after_break = strip_kw_token(&trimmed[break_pos..], "break")
            .and_then(|s| strip_kw_token(s.trim_start(), "if"))
            .unwrap_or_default();
        let cond_src = after_break.trim().trim_end_matches(';').trim();
        check_bool_condition(env, scopes, cond_src, "break if")?;
    } else {
        check_stmt_list(env, scopes, trimmed, expected_ret)?;
    }
    Ok(())
}

fn find_break_if(src: &str) -> Option<usize> {
    let mut scan = TopLevelScan::default();
    let bytes = src.as_bytes();
    for (i, ch) in src.char_indices() {
        if scan.outside_blocks() {
            if let Some(after) = strip_kw_token(&src[i..], "break") {
                if strip_kw_token(after.trim_start(), "if").is_some() {
                    return Some(i);
                }
            }
        }
        scan.step(bytes, i, ch);
    }
    None
}

fn split_continuing_block(src: &str) -> Option<(&str, &str)> {
    let mut scan = TopLevelScan::default();
    let bytes = src.as_bytes();
    for (i, ch) in src.char_indices() {
        if scan.outside_blocks() {
            if let Some(after) = strip_kw_token(&src[i..], "continuing") {
                if let Some(open) = find_block_open(after) {
                    let blk = &after[open..];
                    if let Some(close) = find_matching_delim(blk, '{', '}') {
                        return Some((&src[..i], &blk[1..close]));
                    }
                }
            }
        }
        scan.step(bytes, i, ch);
    }
    None
}

fn check_for_stmt(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    full: &str,
    after_for: &str,
    expected_ret: Option<&ValType>,
) -> Result<(usize, bool), ValidationError> {
    let trimmed = after_for.trim_start();
    if !trimmed.starts_with('(') {
        return Err(err("expected `(` in `for` loop"));
    }
    let close_paren =
        find_matching_delim(trimmed, '(', ')').ok_or_else(|| err("unclosed `(` in `for` loop"))?;
    let header = &trimmed[1..close_paren];
    let mut parts = header.splitn(3, ';').map(str::trim);
    let init = parts.next().unwrap_or("");
    let cond = parts.next().unwrap_or("");
    let update = parts.next().unwrap_or("");

    let after_header = &trimmed[close_paren + 1..];
    let (body, body_len) = parse_block_at(after_header, "for")?;

    scopes.push_scope();
    let res = (|| {
        if !init.is_empty() {
            check_simple_stmt(env, scopes, init, expected_ret)?;
        }
        if !cond.is_empty() {
            check_bool_condition(env, scopes, cond, "for")?;
        }
        if !update.is_empty() {
            check_simple_stmt(env, scopes, update, expected_ret)?;
        }
        check_stmt_list(env, scopes, body, expected_ret)
    })();
    scopes.pop_scope();
    res?;

    let consumed = (full.len() - trimmed.len()) + close_paren + 1 + body_len;
    Ok((consumed, false))
}

fn check_switch_stmt(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    full: &str,
    after_switch: &str,
    expected_ret: Option<&ValType>,
) -> Result<(usize, bool), ValidationError> {
    let open =
        find_block_open(after_switch).ok_or_else(|| err("expected `{` after `switch` selector"))?;
    let sel_res = check_value_expr(env, scopes, after_switch[..open].trim())?;
    let sel_scalar = match sel_res.ty {
        ValType::Scalar(s) if s.is_int() => s.concretize(),
        other => {
            return Err(err(format!(
                "`switch` selector must have integer type, got `{}`",
                other.describe(env.module)
            )));
        }
    };
    let (switch_body, block_len) = parse_block_at(&after_switch[open..], "switch")?;
    let mut rest = switch_body.trim();
    let mut has_default = false;
    let mut all_return = true;

    while !rest.is_empty() {
        if let Some(after_case) = strip_kw_token(rest, "case") {
            let case_open =
                find_block_open(after_case).ok_or_else(|| err("expected `{` in switch `case`"))?;
            let selectors_str = after_case[..case_open].trim().trim_end_matches(':').trim();
            if selectors_str.is_empty() {
                return Err(err("switch `case` requires at least one selector"));
            }
            for sel_item in split_comma_top_level(selectors_str) {
                let sel_item = sel_item.trim();
                if sel_item == "default" {
                    has_default = true;
                } else if !sel_item.is_empty() {
                    let cs = check_value_expr(env, scopes, sel_item)?;
                    if !cs.ty.coerces_to(&ValType::Scalar(sel_scalar)) {
                        return Err(err(format!(
                            "switch case selector `{sel_item}` does not match selector type `{}`",
                            sel_scalar.label()
                        )));
                    }
                }
            }
            let (case_body, case_len) = parse_block_at(&after_case[case_open..], "case")?;
            all_return &= check_block(env, scopes, case_body, expected_ret)?;
            rest = after_case[case_open + case_len..].trim_start();
        } else if let Some(after_def) = strip_kw_token(rest, "default") {
            let def_after_colon = after_def.trim_start();
            let def_after_colon = def_after_colon.strip_prefix(':').unwrap_or(def_after_colon);
            let (def_body, def_len) = parse_block_at(def_after_colon, "default")?;
            has_default = true;
            all_return &= check_block(env, scopes, def_body, expected_ret)?;
            rest = def_after_colon[def_len..].trim_start();
        } else {
            return Err(err(format!("unexpected clause in `switch` body: `{rest}`")));
        }
    }

    if !has_default {
        return Err(err("`switch` statement must contain a `default` clause"));
    }
    let consumed = (full.len() - after_switch.len()) + open + block_len;
    Ok((consumed, all_return && has_default))
}

fn check_simple_stmt(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    raw: &str,
    expected_ret: Option<&ValType>,
) -> Result<bool, ValidationError> {
    let s = raw.trim();
    if s.is_empty() || matches!(s, "break" | "continue" | "discard") {
        return Ok(false);
    }
    if s == "return" {
        if let Some(ret) = expected_ret {
            return Err(err(format!(
                "missing return value (expected `{}`)",
                ret.describe(env.module)
            )));
        }
        return Ok(true);
    }
    if let Some(after_ret) = strip_kw_token(s, "return") {
        let Some(ret_ty) = expected_ret else {
            return Err(err("cannot return a value from a void function"));
        };
        let expr_res = check_value_expr(env, scopes, after_ret)?;
        if !expr_res.ty.coerces_to(ret_ty) {
            return Err(err(format!(
                "return type mismatch: expected `{}`, got `{}`",
                ret_ty.describe(env.module),
                expr_res.ty.describe(env.module)
            )));
        }
        return Ok(true);
    }
    if let Some((is_const, after_kw)) = strip_kw_token(s, "let")
        .map(|r| (false, r))
        .or_else(|| strip_kw_token(s, "const").map(|r| (true, r)))
    {
        check_let_or_const_decl(env, scopes, after_kw, is_const)?;
        return Ok(false);
    }
    if let Some(after_var) = strip_kw_token(s, "var") {
        check_var_decl(env, scopes, after_var)?;
        return Ok(false);
    }
    check_assign_or_call_stmt(env, scopes, s)?;
    Ok(false)
}

fn check_let_or_const_decl(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    after_kw: &str,
    is_const: bool,
) -> Result<(), ValidationError> {
    let kw = if is_const { "const" } else { "let" };
    let eq_pos = find_top_level_char(after_kw, '=')
        .ok_or_else(|| err(format!("`{kw}` declaration requires `=` initializer")))?;
    let lhs = after_kw[..eq_pos].trim();
    let rhs_res = check_value_expr(env, scopes, &after_kw[eq_pos + 1..])?;
    let (name, bound_ty) = if let Some(colon) = lhs.find(':') {
        let var_name = lhs[..colon].trim();
        let ty_src = lhs[colon + 1..].trim();
        let declared = resolve_type_str(env, ty_src).map_err(err)?;
        if !rhs_res.ty.coerces_to(&declared) {
            return Err(err(format!(
                "`{kw} {var_name}: {ty_src}` cannot be initialized with `{}`",
                rhs_res.ty.describe(env.module)
            )));
        }
        (var_name, declared)
    } else {
        (
            lhs,
            if is_const {
                rhs_res.ty
            } else {
                rhs_res.ty.concretize()
            },
        )
    };
    scopes.insert(
        name.to_string(),
        LocalVar {
            ty: bound_ty,
            mutable: false,
            space: AddressSpace::Function,
        },
    );
    Ok(())
}

fn check_var_decl(
    env: &ModuleEnv<'_>,
    scopes: &mut ScopeStack,
    after_var: &str,
) -> Result<(), ValidationError> {
    let trimmed = after_var.trim_start();
    let (space, decl_rest) = if trimmed.starts_with('<') {
        let close = find_matching_delim(trimmed, '<', '>')
            .ok_or_else(|| err("unclosed `<` in `var` address space"))?;
        let sp = match trimmed[1..close].trim() {
            "function" => AddressSpace::Function,
            other => {
                return Err(err(format!(
                    "function-local `var` cannot have address space `{other}`"
                )));
            }
        };
        (sp, trimmed[close + 1..].trim_start())
    } else {
        (AddressSpace::Function, trimmed)
    };

    let (lhs, init_src) = match find_top_level_char(decl_rest, '=') {
        Some(eq_pos) => (decl_rest[..eq_pos].trim(), Some(&decl_rest[eq_pos + 1..])),
        None => (decl_rest.trim(), None),
    };
    let (name, ann_ty) = if let Some(colon) = lhs.find(':') {
        (
            lhs[..colon].trim(),
            Some(resolve_type_str(env, &lhs[colon + 1..]).map_err(err)?),
        )
    } else {
        (lhs, None)
    };
    let var_ty = match (ann_ty, init_src) {
        (Some(declared), Some(rhs)) => {
            let rhs_res = check_value_expr(env, scopes, rhs)?;
            if !rhs_res.ty.coerces_to(&declared) {
                return Err(err(format!(
                    "`var {name}` initializer `{}` does not match `{}`",
                    rhs_res.ty.describe(env.module),
                    declared.describe(env.module)
                )));
            }
            declared
        }
        (Some(declared), None) => declared,
        (None, Some(rhs)) => check_value_expr(env, scopes, rhs)?.ty.concretize(),
        (None, None) => {
            return Err(err(format!(
                "`var {name}` requires a type annotation or initializer"
            )));
        }
    };
    scopes.insert(
        name.to_string(),
        LocalVar {
            ty: var_ty,
            mutable: true,
            space,
        },
    );
    Ok(())
}

fn check_assign_or_call_stmt(
    env: &ModuleEnv<'_>,
    scopes: &ScopeStack,
    s: &str,
) -> Result<(), ValidationError> {
    if let Some(lhs_src) = s.strip_suffix("++").or_else(|| s.strip_suffix("--")) {
        let lhs = check_expr(env, scopes, lhs_src)?;
        require_mutable_lvalue(env, &lhs)?;
        if !matches!(lhs.ty, ValType::Scalar(ElemScalar::I32 | ElemScalar::U32)) {
            return Err(err(format!(
                "`++`/`--` requires `i32` or `u32`, got `{}`",
                lhs.ty.describe(env.module)
            )));
        }
        return Ok(());
    }

    if let Some((lhs_src, op, rhs_src)) = split_assignment(s) {
        let lhs_src = lhs_src.trim();
        let rhs = check_value_expr(env, scopes, rhs_src)?;
        if lhs_src == "_" && op == "=" {
            return Ok(());
        }
        let lhs = check_expr(env, scopes, lhs_src)?;
        require_mutable_lvalue(env, &lhs)?;
        if op == "=" {
            if !rhs.ty.coerces_to(&lhs.ty) {
                return Err(err(format!(
                    "cannot assign `{}` to `{}`",
                    rhs.ty.describe(env.module),
                    lhs.ty.describe(env.module)
                )));
            }
        } else {
            let base_op = op.trim_end_matches('=');
            let combined = match base_op {
                "<<" | ">>" => {
                    check_expr(env, scopes, &format!("({lhs_src}) {base_op} ({rhs_src})"))?.ty
                }
                "&" | "|" | "^" => {
                    if !matches!(
                        lhs.ty,
                        ValType::Scalar(sc) | ValType::Vector(_, sc) if sc.is_int()
                    ) || !rhs.ty.coerces_to(&lhs.ty)
                    {
                        return Err(err(format!(
                            "compound `{op}` invalid for `{}` and `{}`",
                            lhs.ty.describe(env.module),
                            rhs.ty.describe(env.module)
                        )));
                    }
                    lhs.ty.clone()
                }
                _ => apply_binary_arith_types(env.module, base_op, &lhs.ty, &rhs.ty)?,
            };
            if !combined.coerces_to(&lhs.ty) {
                return Err(err(format!(
                    "compound `{op}` result `{}` cannot assign to `{}`",
                    combined.describe(env.module),
                    lhs.ty.describe(env.module)
                )));
            }
        }
        return Ok(());
    }

    let _ = check_expr(env, scopes, s)?;
    Ok(())
}

fn require_mutable_lvalue(
    env: &ModuleEnv<'_>,
    lhs: &super::expr::ExprResult,
) -> Result<(), ValidationError> {
    match lhs.lvalue {
        Some(lv) if lv.mutable => Ok(()),
        Some(_) => Err(err(format!(
            "cannot assign to immutable `{}` (`{}`)",
            lhs.raw,
            lhs.ty.describe(env.module)
        ))),
        None => Err(err(format!(
            "left-hand side `{}` is not assignable",
            lhs.raw
        ))),
    }
}

fn split_assignment(s: &str) -> Option<(&str, &str, &str)> {
    let mut scan = TopLevelScan::default();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = s[i..].chars().next()?;
        if scan.at_top_level() {
            for op in ["<<=", ">>=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^="] {
                if s[i..].starts_with(op) {
                    return Some((&s[..i], op, &s[i + op.len()..]));
                }
            }
            if ch == '=' {
                let prev = i.checked_sub(1).map(|p| bytes[p]);
                let next = bytes.get(i + 1).copied();
                if !matches!(prev, Some(b'=' | b'!' | b'<' | b'>')) && next != Some(b'=') {
                    return Some((&s[..i], "=", &s[i + 1..]));
                }
            }
        }
        scan.step(bytes, i, ch);
        i += ch.len_utf8();
    }
    None
}
