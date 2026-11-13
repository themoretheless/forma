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
    let rewriter = StatementRewriter {
        module,
        src,
        ptr_params,
        func_globals,
        binding_map,
        direct_out_struct,
    };
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

        // Handle `{` or `}`
        if bytes[i] == b'{' || bytes[i] == b'}' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }

        let rest = &src[i..];
        if let Some(next) = rewriter.control_flow(rest, &mut out) {
            i = next;
            continue;
        }

        // Otherwise read until top-level `;`
        if let Some(semi_off) = find_stmt_semicolon(rest) {
            rewriter.simple(rest[..semi_off].trim(), &mut out);
            out.push(';');
            i += semi_off + 1;
        } else {
            // Remaining fragment without semicolon
            rewriter.simple(rest.trim(), &mut out);
            break;
        }
    }

    out
}

/// What every statement of one function body is translated with.
struct StatementRewriter<'a> {
    module: &'a Module,
    /// The whole body: handlers return positions in it.
    src: &'a str,
    ptr_params: &'a BTreeSet<String>,
    func_globals: &'a BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &'a BindingMap,
    direct_out_struct: Option<&'a str>,
}

impl StatementRewriter<'_> {
    /// The position in the body where `tail`, a suffix of it, starts.
    fn position_of(&self, tail: &str) -> usize {
        self.src.len() - tail.len()
    }

    fn expr(&self, expr: &str) -> String {
        translate_expr_with_ctx(self.module, expr, self.func_globals, self.binding_map)
    }

    fn simple_stmt(&self, stmt: &str, direct_out_struct: Option<&str>) -> String {
        translate_simple_stmt(
            self.module,
            stmt,
            self.ptr_params,
            self.func_globals,
            self.binding_map,
            direct_out_struct,
        )
    }

    /// Translates a statement that ends at a `;`. An empty one emits nothing.
    fn simple(&self, stmt: &str, out: &mut String) {
        if !stmt.is_empty() {
            out.push_str(&self.simple_stmt(stmt, self.direct_out_struct));
        }
    }

    /// A condition in parentheses, without doubling the pair it may already have.
    fn condition(&self, cond: &str) -> String {
        let cond_msl = self.expr(cond);
        if cond_msl.starts_with('(')
            && find_matching_delim(&cond_msl, '(', ')') == Some(cond_msl.len() - 1)
        {
            cond_msl
        } else {
            format!("({cond_msl})")
        }
    }

    /// Translates the control-flow statement `rest` starts with, if any, and returns the position
    /// in the body to continue from.
    fn control_flow(&self, rest: &str, out: &mut String) -> Option<usize> {
        if let Some(after_if) = strip_kw_token(rest, "if") {
            return Some(self.conditional("if", after_if, out));
        }
        if let Some(after_else) = strip_kw_token(rest, "else") {
            if let Some(next) = self.else_branch(after_else, out) {
                return Some(next);
            }
        }
        if let Some(after_while) = strip_kw_token(rest, "while") {
            return Some(self.conditional("while", after_while, out));
        }
        if let Some(after_loop) = strip_kw_token(rest, "loop") {
            if let Some(next) = self.loop_block(after_loop, out) {
                return Some(next);
            }
        }
        if let Some(after_for) = strip_kw_token(rest, "for") {
            if let Some(next) = self.for_header(after_for, out) {
                return Some(next);
            }
        }
        if let Some(after_switch) = strip_kw_token(rest, "switch") {
            if let Some(next) = self.switch_block(after_switch, out) {
                return Some(next);
            }
        }
        None
    }

    /// `if cond {` or `while cond {`.
    fn conditional(&self, keyword: &str, after_keyword: &str, out: &mut String) -> usize {
        let cond_len = find_block_open(after_keyword).unwrap_or(0);
        let cond_wrapped = self.condition(after_keyword[..cond_len].trim());
        out.push_str(&format!("{keyword} {cond_wrapped} {{"));
        self.position_of(after_keyword) + cond_len + 1
    }

    /// `else if`, which leaves the `if` to the next round, or `else {`.
    fn else_branch(&self, after_else: &str, out: &mut String) -> Option<usize> {
        let trimmed_else = after_else.trim_start();
        if trimmed_else.starts_with("if")
            && trimmed_else[2..]
                .chars()
                .next()
                .map(|c| c.is_ascii_whitespace() || c == '(')
                .unwrap_or(false)
        {
            out.push_str("else ");
            return Some(self.position_of(trimmed_else));
        }
        let brace_off = find_block_open(after_else)?;
        if !after_else[..brace_off].trim().is_empty() {
            return None;
        }
        out.push_str("else {");
        Some(self.position_of(after_else) + brace_off + 1)
    }

    /// `loop { ... continuing { ... } }`.
    fn loop_block(&self, after_loop: &str, out: &mut String) -> Option<usize> {
        let open_off = find_block_open(after_loop)?;
        let block_str = &after_loop[open_off..];
        let close_off = find_matching_delim(block_str, '{', '}')?;
        out.push_str("while (true) {\n");
        let inner_msl = rewrite_loop_body(
            self.module,
            &block_str[1..close_off],
            self.ptr_params,
            self.func_globals,
            self.binding_map,
            self.direct_out_struct,
        );
        out.push_str(&inner_msl);
        out.push('}');
        Some(self.position_of(after_loop) + open_off + close_off + 1)
    }

    /// `for (init; cond; update)`; the block that follows is handled as any other.
    fn for_header(&self, after_for: &str, out: &mut String) -> Option<usize> {
        let after_for_trim = after_for.trim_start();
        if !after_for_trim.starts_with('(') {
            return None;
        }
        let close_paren = find_matching_delim(after_for_trim, '(', ')')?;
        let header = &after_for_trim[1..close_paren];
        let mut parts = header.splitn(3, ';').map(str::trim);
        let mut clause = |translate: &dyn Fn(&str) -> String| match parts.next() {
            Some(part) if !part.is_empty() => translate(part),
            _ => String::new(),
        };
        let init_msl = clause(&|part| self.simple_stmt(part, None));
        let cond_msl = clause(&|part| self.expr(part));
        let update_msl = clause(&|part| self.simple_stmt(part, None));
        out.push_str(&format!("for ({init_msl}; {cond_msl}; {update_msl})"));
        Some(self.position_of(after_for_trim) + close_paren + 1)
    }

    /// `switch expr { case ...: { ... } default: { ... } }`.
    fn switch_block(&self, after_switch: &str, out: &mut String) -> Option<usize> {
        let open_off = find_block_open(after_switch)?;
        let block_str = &after_switch[open_off..];
        let close_off = find_matching_delim(block_str, '{', '}')?;
        let expr_msl = self.expr(after_switch[..open_off].trim());
        let msl_cases = rewrite_switch_body(
            self.module,
            &block_str[1..close_off],
            self.ptr_params,
            self.func_globals,
            self.binding_map,
            self.direct_out_struct,
        );
        out.push_str(&format!("switch ({expr_msl}) {{\n{msl_cases}}}"));
        Some(self.position_of(after_switch) + open_off + close_off + 1)
    }
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
