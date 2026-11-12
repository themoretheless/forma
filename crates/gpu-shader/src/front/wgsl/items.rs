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

    let (space_opt, after_space) = if after_var.starts_with('<') {
        let close = find_matching_delim(after_var, '<', '>').ok_or_else(|| {
            parse_error_at(clean, after_var, format!("unclosed '<' in `{after_var}`"))
        })?;
        let spec = after_var[1..close].trim();
        let parts: Vec<&str> = spec.split(',').map(|s| s.trim()).collect();
        let space = match parts.first().copied().unwrap_or("") {
            "uniform" => AddressSpace::Uniform,
            "storage" => {
                let access = match parts.get(1).copied().unwrap_or("read") {
                    "read" => StorageAccess::Read,
                    "write" => StorageAccess::Write,
                    "read_write" => StorageAccess::ReadWrite,
                    other => {
                        return Err(parse_error_at(
                            clean,
                            other,
                            format!("invalid storage access mode `{other}`"),
                        ));
                    }
                };
                AddressSpace::Storage { access }
            }
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
        (Some(space), after_var[close + 1..].trim_start())
    } else {
        (None, after_var)
    };

    let (decl_part, init_opt) = if let Some(eq_pos) = find_top_level_char(after_space, '=') {
        (
            after_space[..eq_pos].trim(),
            Some(after_space[eq_pos + 1..].trim().to_string()),
        )
    } else {
        (after_space.trim(), None)
    };

    let colon_pos = decl_part.find(':').ok_or_else(|| {
        parse_error_at(
            clean,
            decl_part,
            format!("expected ':' in global variable declaration `{rest}`"),
        )
    })?;
    let name = decl_part[..colon_pos].trim().to_string();
    let ty_str = decl_part[colon_pos + 1..].trim();
    let ty = resolve_or_insert_type(module, type_map, const_values, ty_str)
        .map_err(|message| parse_error_at(clean, ty_str, message))?;

    let inferred_space = space_opt.unwrap_or_else(|| match &module.types[ty].inner {
        TypeInner::Image { .. } | TypeInner::Sampler { .. } => AddressSpace::Handle,
        _ => AddressSpace::Private,
    });

    let binding = match (attrs.group, attrs.binding) {
        (Some(group), Some(binding)) => Some(ResourceBinding { group, binding }),
        _ => None,
    };

    Ok(GlobalVariable {
        name: Some(name),
        space: inferred_space,
        binding,
        ty,
        init: init_opt,
    })
}

pub(super) fn parse_function(
    clean: &str,
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    global_names: &BTreeMap<String, Handle<GlobalVariable>>,
    after_fn: &str,
) -> Result<Function, ParseError> {
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
    let body_src = body_block[1..close_brace].to_string();

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

    let result = if let Some(after_arrow) = ret_part.strip_prefix("->") {
        let (ret_attrs, ret_ty_str) = parse_leading_attributes(clean, after_arrow.trim())?;
        let ret_ty_str = ret_ty_str.trim();
        let ret_ty = resolve_or_insert_type(module, type_map, const_values, ret_ty_str)
            .map_err(|message| parse_error_at(clean, ret_ty_str, message))?;
        Some(FunctionResult {
            ty: ret_ty,
            binding: ret_attrs.to_binding(),
            raw_ty_src: ret_ty_str.to_string(),
        })
    } else if !ret_part.is_empty() {
        return Err(parse_error_at(
            clean,
            ret_part,
            format!("unexpected return specifier `{ret_part}` in `{fn_name}`"),
        ));
    } else {
        None
    };

    // Analyze body for referenced identifiers (globals and function calls)
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

pub(super) fn analyze_identifiers(
    body: &str,
    local_args: &BTreeSet<&str>,
    global_names: &BTreeMap<String, Handle<GlobalVariable>>,
) -> (BTreeSet<Handle<GlobalVariable>>, BTreeSet<String>) {
    let mut used_globals = BTreeSet::new();
    let mut called_functions = BTreeSet::new();
    let mut local_vars: BTreeSet<&str> = local_args.iter().copied().collect();
    let mut expect_decl_ident = false;
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if expect_decl_ident && b == b'<' {
            // Skip `<...>` after `var` (e.g. `var<function> x`)
            if let Some(close) = find_matching_delim(&body[i..], '<', '>') {
                i += close + 1;
                continue;
            }
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let prev_non_space = if start > 0 {
                body[..start].trim_end().as_bytes().last().copied()
            } else {
                None
            };
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &body[start..i];
            if expect_decl_ident {
                expect_decl_ident = false;
                local_vars.insert(ident);
                continue;
            }
            if matches!(ident, "let" | "var" | "const") && prev_non_space != Some(b'.') {
                expect_decl_ident = true;
                continue;
            }
            // Ignore struct field accesses `.ident`
            if prev_non_space == Some(b'.') {
                continue;
            }
            let next_non_space = body[i..].trim_start().as_bytes().first().copied();
            if next_non_space == Some(b'(') {
                called_functions.insert(ident.to_string());
                if !local_vars.contains(ident) {
                    if let Some(&gh) = global_names.get(ident) {
                        used_globals.insert(gh);
                    }
                }
            } else if !local_vars.contains(ident) {
                if let Some(&gh) = global_names.get(ident) {
                    used_globals.insert(gh);
                }
            }
        } else {
            if !b.is_ascii_whitespace() {
                expect_decl_ident = false;
            }
            i += 1;
        }
    }
    (used_globals, called_functions)
}
