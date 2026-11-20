//! Global variables and functions.

use super::*;

pub(super) fn parse_global_var(
    clean: &str,
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    attrs: &ParsedAttributes,
    rest: &str,
) -> Result<GlobalVariable, ParseError> {
    let after_var = rest
        .strip_prefix("var")
        .ok_or_else(|| parse_error_at(clean, rest, format!("expected `var` in `{rest}`")))?
        .trim_start();
    let (space_opt, after_space) = parse_var_address_space(clean, after_var)?;
    let (name, ty_str, init) = split_var_decl_and_init(clean, rest, after_space)?;
    let ty = resolve_or_insert_type(module, type_map, const_values, ty_str)
        .map_err(|message| parse_error_at(clean, ty_str, message))?;

    let space = space_opt.unwrap_or_else(|| match &module.types[ty].inner {
        TypeInner::Image { .. } | TypeInner::Sampler { .. } => AddressSpace::Handle,
        _ => AddressSpace::Private,
    });
    let binding = match (attrs.group, attrs.binding) {
        (Some(group), Some(binding)) => Some(ResourceBinding { group, binding }),
        _ => None,
    };

    Ok(GlobalVariable {
        name: Some(name.to_string()),
        space,
        binding,
        ty,
        init,
    })
}

fn parse_var_address_space<'a>(
    clean: &str,
    after_var: &'a str,
) -> Result<(Option<AddressSpace>, &'a str), ParseError> {
    if !after_var.starts_with('<') {
        return Ok((None, after_var));
    }
    let close = find_matching_delim(after_var, '<', '>').ok_or_else(|| {
        parse_error_at(clean, after_var, format!("unclosed '<' in `{after_var}`"))
    })?;
    let parts: Vec<&str> = after_var[1..close]
        .trim()
        .split(',')
        .map(str::trim)
        .collect();
    let space = match parts.first().copied().unwrap_or("") {
        "uniform" => AddressSpace::Uniform,
        "storage" => AddressSpace::Storage {
            access: parse_storage_access(clean, parts.get(1).copied().unwrap_or("read"))?,
        },
        "workgroup" => AddressSpace::WorkGroup,
        "private" => AddressSpace::Private,
        "function" => AddressSpace::Function,
        "immediate" | "push_constant" => AddressSpace::Immediate,
        other => {
            return Err(parse_error_at(
                clean,
                other,
                format!("unknown address space `{other}`"),
            ));
        }
    };
    Ok((Some(space), after_var[close + 1..].trim_start()))
}

fn parse_storage_access(clean: &str, access: &str) -> Result<StorageAccess, ParseError> {
    match access {
        "read" => Ok(StorageAccess::Read),
        "write" => Ok(StorageAccess::Write),
        "read_write" => Ok(StorageAccess::ReadWrite),
        other => Err(parse_error_at(
            clean,
            other,
            format!("invalid storage access mode `{other}`"),
        )),
    }
}

fn split_var_decl_and_init<'a>(
    clean: &str,
    rest: &str,
    after_space: &'a str,
) -> Result<(&'a str, &'a str, Option<String>), ParseError> {
    let (decl_part, init_opt) = match find_top_level_char(after_space, '=') {
        Some(eq_pos) => (
            after_space[..eq_pos].trim(),
            Some(after_space[eq_pos + 1..].trim().to_string()),
        ),
        None => (after_space.trim(), None),
    };
    let colon_pos = decl_part.find(':').ok_or_else(|| {
        parse_error_at(
            clean,
            decl_part,
            format!("expected ':' in global variable declaration `{rest}`"),
        )
    })?;
    Ok((
        decl_part[..colon_pos].trim(),
        decl_part[colon_pos + 1..].trim(),
        init_opt,
    ))
}

pub(super) fn parse_function(
    clean: &str,
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    global_names: &BTreeMap<String, Handle<GlobalVariable>>,
    after_fn: &str,
) -> Result<Function, ParseError> {
    let (fn_name, args_src, ret_part, body_src) = parse_function_header(clean, after_fn)?;
    let arguments =
        parse_function_arguments(clean, module, type_map, const_values, &fn_name, args_src)?;
    let result = parse_function_return(clean, module, type_map, const_values, &fn_name, ret_part)?;

    let local_arg_names: BTreeSet<&str> =
        arguments.iter().filter_map(|a| a.name.as_deref()).collect();
    let (used_globals, called_functions) =
        analyze_identifiers(&body_src, &local_arg_names, global_names);

    Ok(Function {
        name: Some(fn_name),
        arguments,
        result,
        body_src,
        used_globals,
        called_functions,
    })
}

fn parse_function_header<'a>(
    clean: &str,
    after_fn: &'a str,
) -> Result<(String, &'a str, &'a str, String), ParseError> {
    let open_paren = after_fn.find('(').ok_or_else(|| {
        parse_error_at(
            clean,
            after_fn,
            format!("expected '(' in function declaration `{after_fn}`"),
        )
    })?;
    let fn_name = after_fn[..open_paren].trim().to_string();
    if fn_name.is_empty() {
        return Err(parse_error_at(clean, after_fn, "empty function name"));
    }
    let close_paren = find_matching_delim(&after_fn[open_paren..], '(', ')')
        .map(|i| open_paren + i)
        .ok_or_else(|| {
            parse_error_at(
                clean,
                &after_fn[open_paren..],
                format!("unclosed '(' in function `{fn_name}`"),
            )
        })?;
    let args_src = &after_fn[open_paren + 1..close_paren];
    let after_args = after_fn[close_paren + 1..].trim_start();

    let open_brace = find_top_level_char(after_args, '{').ok_or_else(|| {
        parse_error_at(
            clean,
            after_args,
            format!("expected '{{' in function `{fn_name}`"),
        )
    })?;
    let ret_part = after_args[..open_brace].trim();
    let body_block = &after_args[open_brace..];
    let close_brace = find_matching_delim(body_block, '{', '}').ok_or_else(|| {
        parse_error_at(
            clean,
            body_block,
            format!("unclosed '{{' in function `{fn_name}`"),
        )
    })?;
    Ok((
        fn_name,
        args_src,
        ret_part,
        body_block[1..close_brace].to_string(),
    ))
}

fn parse_function_arguments(
    clean: &str,
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    fn_name: &str,
    args_src: &str,
) -> Result<Vec<FunctionArgument>, ParseError> {
    let mut arguments = Vec::new();
    for arg in split_comma_top_level(args_src) {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        let (arg_attrs, arg_rest) = parse_leading_attributes(clean, arg)?;
        let colon = arg_rest.find(':').ok_or_else(|| {
            parse_error_at(
                clean,
                arg_rest,
                format!("expected ':' in argument `{arg}` of `{fn_name}`"),
            )
        })?;
        let arg_name = arg_rest[..colon].trim().to_string();
        let arg_ty_str = arg_rest[colon + 1..].trim();
        let arg_ty = resolve_or_insert_type(module, type_map, const_values, arg_ty_str)
            .map_err(|message| parse_error_at(clean, arg_ty_str, message))?;
        arguments.push(FunctionArgument {
            name: Some(arg_name),
            ty: arg_ty,
            binding: arg_attrs.to_binding(),
            raw_ty_src: arg_ty_str.to_string(),
        });
    }
    Ok(arguments)
}

fn parse_function_return(
    clean: &str,
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    fn_name: &str,
    ret_part: &str,
) -> Result<Option<FunctionResult>, ParseError> {
    if let Some(after_arrow) = ret_part.strip_prefix("->") {
        let (ret_attrs, ret_ty_str) = parse_leading_attributes(clean, after_arrow.trim())?;
        let ret_ty_str = ret_ty_str.trim();
        let ret_ty = resolve_or_insert_type(module, type_map, const_values, ret_ty_str)
            .map_err(|message| parse_error_at(clean, ret_ty_str, message))?;
        return Ok(Some(FunctionResult {
            ty: ret_ty,
            binding: ret_attrs.to_binding(),
            raw_ty_src: ret_ty_str.to_string(),
        }));
    }
    if !ret_part.is_empty() {
        return Err(parse_error_at(
            clean,
            ret_part,
            format!("unexpected return specifier `{ret_part}` in `{fn_name}`"),
        ));
    }
    Ok(None)
}

pub(super) fn analyze_identifiers(
    body: &str,
    local_args: &BTreeSet<&str>,
    global_names: &BTreeMap<String, Handle<GlobalVariable>>,
) -> (BTreeSet<Handle<GlobalVariable>>, BTreeSet<String>) {
    let mut scanner = BodyIdentScanner {
        body,
        global_names,
        local_vars: local_args.iter().copied().collect(),
        used_globals: BTreeSet::new(),
        called_functions: BTreeSet::new(),
        expect_decl_ident: false,
    };
    scanner.scan();
    (scanner.used_globals, scanner.called_functions)
}

struct BodyIdentScanner<'a> {
    body: &'a str,
    global_names: &'a BTreeMap<String, Handle<GlobalVariable>>,
    local_vars: BTreeSet<&'a str>,
    used_globals: BTreeSet<Handle<GlobalVariable>>,
    called_functions: BTreeSet<String>,
    expect_decl_ident: bool,
}

impl BodyIdentScanner<'_> {
    fn scan(&mut self) {
        let bytes = self.body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if self.expect_decl_ident && b == b'<' {
                // Skip `<...>` after `var` (e.g. `var<function> x`).
                if let Some(close) = find_matching_delim(&self.body[i..], '<', '>') {
                    i += close + 1;
                    continue;
                }
            }
            if b.is_ascii_alphabetic() || b == b'_' {
                i = self.visit_ident(i);
            } else {
                if !b.is_ascii_whitespace() {
                    self.expect_decl_ident = false;
                }
                i += 1;
            }
        }
    }

    fn visit_ident(&mut self, start: usize) -> usize {
        let bytes = self.body.as_bytes();
        let prev_non_space = if start > 0 {
            self.body[..start].trim_end().as_bytes().last().copied()
        } else {
            None
        };
        let mut end = start + 1;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        let ident = &self.body[start..end];
        if self.expect_decl_ident {
            self.expect_decl_ident = false;
            self.local_vars.insert(ident);
            return end;
        }
        if matches!(ident, "let" | "var" | "const") && prev_non_space != Some(b'.') {
            self.expect_decl_ident = true;
            return end;
        }
        // Ignore struct field accesses `.ident`.
        if prev_non_space == Some(b'.') {
            return end;
        }
        let next_non_space = self.body[end..].trim_start().as_bytes().first().copied();
        if next_non_space == Some(b'(') {
            self.called_functions.insert(ident.to_string());
        }
        if !self.local_vars.contains(ident) {
            if let Some(&gh) = self.global_names.get(ident) {
                self.used_globals.insert(gh);
            }
        }
        end
    }
}
