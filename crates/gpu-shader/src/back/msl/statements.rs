//! Function bodies: statements, loops and switches.

use super::*;

pub(super) fn translate_function_body(
    module: &Module,
    body: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    rewrite_statements(
        module,
        body,
        ptr_params,
        func_globals,
        binding_map,
        direct_out_struct,
    )
}

pub(super) fn rewrite_statements(
    module: &Module,
    src: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    let mut out = String::with_capacity(src.len() + 128);
    let bytes = src.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        // Preserve whitespace
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            out.push(bytes[i] as char);
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let rest = &src[i..];

        // Handle `{` or `}`
        if bytes[i] == b'{' || bytes[i] == b'}' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }

        // Handle `if`, `else if`, `else`
        if let Some(after_if) = strip_kw_token(rest, "if") {
            let cond_len = find_block_open(after_if).unwrap_or(0);
            let cond = after_if[..cond_len].trim();
            let cond_msl = translate_expr_with_ctx(module, cond, func_globals, binding_map);
            let cond_wrapped = if cond_msl.starts_with('(')
                && find_matching_delim(&cond_msl, '(', ')') == Some(cond_msl.len() - 1)
            {
                cond_msl
            } else {
                format!("({cond_msl})")
            };
            out.push_str(&format!("if {cond_wrapped} {{"));
            let consumed = (src.len() - after_if.len()) - i + cond_len + 1;
            i += consumed;
            continue;
        }
        if let Some(after_else) = strip_kw_token(rest, "else") {
            let trimmed_else = after_else.trim_start();
            if trimmed_else.starts_with("if")
                && trimmed_else[2..]
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_whitespace() || c == '(')
                    .unwrap_or(false)
            {
                out.push_str("else ");
                i = src.len() - trimmed_else.len();
                continue;
            } else if let Some(brace_off) = find_block_open(after_else) {
                if after_else[..brace_off].trim().is_empty() {
                    out.push_str("else {");
                    i = (src.len() - after_else.len()) + brace_off + 1;
                    continue;
                }
            }
        }

        // Handle `while`
        if let Some(after_while) = strip_kw_token(rest, "while") {
            let cond_len = find_block_open(after_while).unwrap_or(0);
            let cond = after_while[..cond_len].trim();
            let cond_msl = translate_expr_with_ctx(module, cond, func_globals, binding_map);
            let cond_wrapped = if cond_msl.starts_with('(')
                && find_matching_delim(&cond_msl, '(', ')') == Some(cond_msl.len() - 1)
            {
                cond_msl
            } else {
                format!("({cond_msl})")
            };
            out.push_str(&format!("while {cond_wrapped} {{"));
            let consumed = (src.len() - after_while.len()) - i + cond_len + 1;
            i += consumed;
            continue;
        }

        // Handle `loop { ... continuing { ... } }`
        if let Some(after_loop) = strip_kw_token(rest, "loop") {
            if let Some(open_off) = find_block_open(after_loop) {
                let block_str = &after_loop[open_off..];
                if let Some(close_off) = find_matching_delim(block_str, '{', '}') {
                    let loop_inner = &block_str[1..close_off];
                    out.push_str("while (true) {\n");
                    let inner_msl = rewrite_loop_body(
                        module,
                        loop_inner,
                        ptr_params,
                        func_globals,
                        binding_map,
                        direct_out_struct,
                    );
                    out.push_str(&inner_msl);
                    out.push('}');
                    i = (src.len() - after_loop.len()) + open_off + close_off + 1;
                    continue;
                }
            }
        }

        // Handle `for (...) {`
        if let Some(after_for) = strip_kw_token(rest, "for") {
            let after_for_trim = after_for.trim_start();
            if after_for_trim.starts_with('(') {
                if let Some(close_paren) = find_matching_delim(after_for_trim, '(', ')') {
                    let header = &after_for_trim[1..close_paren];
                    let parts: Vec<&str> = header.splitn(3, ';').collect();
                    let init_str = parts.first().copied().unwrap_or("").trim();
                    let cond_str = parts.get(1).copied().unwrap_or("").trim();
                    let update_str = parts.get(2).copied().unwrap_or("").trim();

                    let init_msl = if init_str.is_empty() {
                        String::new()
                    } else {
                        translate_simple_stmt(
                            module,
                            init_str,
                            ptr_params,
                            func_globals,
                            binding_map,
                            None,
                        )
                    };
                    let cond_msl = if cond_str.is_empty() {
                        String::new()
                    } else {
                        translate_expr_with_ctx(module, cond_str, func_globals, binding_map)
                    };
                    let update_msl = if update_str.is_empty() {
                        String::new()
                    } else {
                        translate_simple_stmt(
                            module,
                            update_str,
                            ptr_params,
                            func_globals,
                            binding_map,
                            None,
                        )
                    };
                    out.push_str(&format!("for ({init_msl}; {cond_msl}; {update_msl})"));
                    i = (src.len() - after_for_trim.len()) + close_paren + 1;
                    continue;
                }
            }
        }

        // Handle `switch expr { case ...: { ... } default: { ... } }`
        if let Some(after_switch) = strip_kw_token(rest, "switch") {
            if let Some(open_off) = find_block_open(after_switch) {
                let expr_str = after_switch[..open_off].trim();
                let block_str = &after_switch[open_off..];
                if let Some(close_off) = find_matching_delim(block_str, '{', '}') {
                    let expr_msl =
                        translate_expr_with_ctx(module, expr_str, func_globals, binding_map);
                    let switch_body = &block_str[1..close_off];
                    let msl_cases = rewrite_switch_body(
                        module,
                        switch_body,
                        ptr_params,
                        func_globals,
                        binding_map,
                        direct_out_struct,
                    );
                    out.push_str(&format!("switch ({expr_msl}) {{\n{msl_cases}}}"));
                    i = (src.len() - after_switch.len()) + open_off + close_off + 1;
                    continue;
                }
            }
        }

        // Otherwise read until top-level `;`
        if let Some(semi_off) = find_stmt_semicolon(rest) {
            let stmt = rest[..semi_off].trim();
            if !stmt.is_empty() {
                let stmt_msl = translate_simple_stmt(
                    module,
                    stmt,
                    ptr_params,
                    func_globals,
                    binding_map,
                    direct_out_struct,
                );
                out.push_str(&stmt_msl);
            }
            out.push(';');
            i += semi_off + 1;
        } else {
            // Remaining fragment without semicolon
            let stmt = rest.trim();
            if !stmt.is_empty() {
                let stmt_msl = translate_simple_stmt(
                    module,
                    stmt,
                    ptr_params,
                    func_globals,
                    binding_map,
                    direct_out_struct,
                );
                out.push_str(&stmt_msl);
            }
            break;
        }
    }

    out
}

pub(super) fn rewrite_loop_body(
    module: &Module,
    loop_inner: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    // Check if there is a top-level `continuing { ... }` block
    if let Some((before_cont, cont_body)) = split_continuing_block(loop_inner) {
        let cont_msl = rewrite_statements(
            module,
            cont_body,
            ptr_params,
            func_globals,
            binding_map,
            direct_out_struct,
        );
        let main_msl = rewrite_statements(
            module,
            before_cont,
            ptr_params,
            func_globals,
            binding_map,
            direct_out_struct,
        );
        format!("{main_msl}\n{cont_msl}\n")
    } else {
        rewrite_statements(
            module,
            loop_inner,
            ptr_params,
            func_globals,
            binding_map,
            direct_out_struct,
        )
    }
}

pub(super) fn split_continuing_block(src: &str) -> Option<(&str, &str)> {
    let mut brace = 0i32;
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => brace += 1,
            b'}' => brace = (brace - 1).max(0),
            _ if brace == 0 => {
                if let Some(after) = strip_kw_token(&src[i..], "continuing") {
                    if let Some(open) = find_block_open(after) {
                        let blk = &after[open..];
                        if let Some(close) = find_matching_delim(blk, '{', '}') {
                            return Some((&src[..i], &blk[1..close]));
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

pub(super) fn rewrite_switch_body(
    module: &Module,
    switch_body: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    let mut out = String::new();
    let mut rest = switch_body.trim();
    while !rest.is_empty() {
        if let Some(after_case) = strip_kw_token(rest, "case") {
            let open_brace = find_block_open(after_case).unwrap_or(0);
            let selectors_str = after_case[..open_brace].trim().trim_end_matches(':').trim();
            for sel in split_comma_top_level(selectors_str) {
                let sel = sel.trim();
                if sel == "default" {
                    out.push_str("default:\n");
                } else if !sel.is_empty() {
                    let sel_msl = translate_expr_with_ctx(module, sel, func_globals, binding_map);
                    out.push_str(&format!("case {sel_msl}:\n"));
                }
            }
            let blk = &after_case[open_brace..];
            if let Some(close_brace) = find_matching_delim(blk, '{', '}') {
                let inner = &blk[1..close_brace];
                let inner_msl = rewrite_statements(
                    module,
                    inner,
                    ptr_params,
                    func_globals,
                    binding_map,
                    direct_out_struct,
                );
                out.push_str(&format!("{{\n{inner_msl}\nbreak;\n}}\n"));
                rest = blk[close_brace + 1..].trim_start();
            } else {
                break;
            }
        } else if let Some(after_def) = strip_kw_token(rest, "default") {
            let open_brace = find_block_open(after_def).unwrap_or(0);
            let blk = &after_def[open_brace..];
            if let Some(close_brace) = find_matching_delim(blk, '{', '}') {
                let inner = &blk[1..close_brace];
                let inner_msl = rewrite_statements(
                    module,
                    inner,
                    ptr_params,
                    func_globals,
                    binding_map,
                    direct_out_struct,
                );
                out.push_str(&format!("default: {{\n{inner_msl}\nbreak;\n}}\n"));
                rest = blk[close_brace + 1..].trim_start();
            } else {
                break;
            }
        } else {
            break;
        }
    }
    out
}

pub(super) fn translate_simple_stmt(
    module: &Module,
    stmt: &str,
    _ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    let s = stmt.trim();
    if s == "discard" {
        return "discard_fragment()".to_string();
    }
    if s == "break" || s == "continue" || s == "return" {
        return s.to_string();
    }
    if let Some(after_ret) = strip_kw_token(s, "return") {
        let expr_msl = translate_expr_with_ctx(module, after_ret.trim(), func_globals, binding_map);
        if let Some(out_struct) = direct_out_struct {
            return format!("return {out_struct}{{{expr_msl}}}");
        } else {
            return format!("return {expr_msl}");
        }
    }

    // `let name [: Type] = expr` or `const name [: Type] = expr`
    if let Some(after_let) = strip_kw_token(s, "let").or_else(|| strip_kw_token(s, "const")) {
        if let Some(eq_pos) = find_top_level_char(after_let, '=') {
            let lhs = after_let[..eq_pos].trim();
            let rhs = after_let[eq_pos + 1..].trim();
            let rhs_msl = translate_expr_with_ctx(module, rhs, func_globals, binding_map);
            if let Some(colon) = lhs.find(':') {
                let name = msl_escape_ident(lhs[..colon].trim());
                let ty_msl = wgsl_type_str_to_msl(module, lhs[colon + 1..].trim());
                return format!("const {ty_msl} {name} = {rhs_msl}");
            } else {
                let name = msl_escape_ident(lhs);
                return format!("const auto {name} = {rhs_msl}");
            }
        }
    }

    // `var name [: Type] [= expr]`
    if let Some(after_var) = strip_kw_token(s, "var") {
        let after_var = if after_var.trim_start().starts_with('<') {
            let trimmed = after_var.trim_start();
            if let Some(close) = find_matching_delim(trimmed, '<', '>') {
                trimmed[close + 1..].trim_start()
            } else {
                after_var
            }
        } else {
            after_var.trim_start()
        };

        if let Some(eq_pos) = find_top_level_char(after_var, '=') {
            let lhs = after_var[..eq_pos].trim();
            let rhs = after_var[eq_pos + 1..].trim();
            let rhs_msl = translate_expr_with_ctx(module, rhs, func_globals, binding_map);
            if let Some(colon) = lhs.find(':') {
                let name = msl_escape_ident(lhs[..colon].trim());
                let ty_msl = wgsl_type_str_to_msl(module, lhs[colon + 1..].trim());
                return format!("{ty_msl} {name} = {rhs_msl}");
            } else {
                let name = msl_escape_ident(lhs);
                return format!("auto {name} = {rhs_msl}");
            }
        } else if let Some(colon) = after_var.find(':') {
            let name = msl_escape_ident(after_var[..colon].trim());
            let ty_msl = wgsl_type_str_to_msl(module, after_var[colon + 1..].trim());
            return format!("{ty_msl} {name} = {{}}");
        }
    }

    translate_expr_with_ctx(module, s, func_globals, binding_map)
}

pub(super) fn strip_kw_token<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    if rest.is_empty() {
        return Some(rest);
    }
    let next = rest.as_bytes()[0];
    if !next.is_ascii_alphanumeric() && next != b'_' {
        Some(rest)
    } else {
        None
    }
}

pub(super) fn find_block_open(s: &str) -> Option<usize> {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => bracket += 1,
            ']' => bracket = (bracket - 1).max(0),
            '{' if paren == 0 && bracket == 0 => return Some(idx),
            _ => {}
        }
    }
    None
}

pub(super) fn find_stmt_semicolon(s: &str) -> Option<usize> {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => bracket += 1,
            ']' => bracket = (bracket - 1).max(0),
            '{' => brace += 1,
            '}' => brace = (brace - 1).max(0),
            ';' if paren == 0 && bracket == 0 && brace == 0 => return Some(idx),
            _ => {}
        }
    }
    None
}
