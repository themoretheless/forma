use crate::proc::{align_up, compute_type_layout};
use crate::{
    AddressSpace, ArraySize, Binding, BuiltIn, Constant, EntryPoint, Function, FunctionArgument,
    FunctionResult, GlobalVariable, Handle, ImageClass, ImageDimension, Interpolation, Module,
    ResourceBinding, Sampling, Scalar, ScalarKind, ShaderStage, StorageAccess, StructMember, Type,
    TypeInner, VectorSize,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroU32;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl ParseError {
    pub fn emit_to_string(&self, _source: &str) -> String {
        format!(
            "WGSL parse error at {}:{}: {}",
            self.line, self.column, self.message
        )
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WGSL parse error at {}:{}: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for ParseError {}

pub fn parse_str(source: &str) -> Result<Module, ParseError> {
    let clean = strip_comments(source);
    validate_syntax(&clean)?;
    let mut module = Module {
        raw_source: source.to_string(),
        ..Module::default()
    };

    let mut type_map: BTreeMap<String, Handle<Type>> = BTreeMap::new();
    let mut const_values: BTreeMap<String, u32> = BTreeMap::new();

    // First pass: collect top-level items
    let items = split_top_level_items(&clean)?;

    // Register aliases and constants first so struct array sizes and workgroup sizes can resolve them
    for item in &items {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (_, rest) = parse_leading_attributes(trimmed)?;
        let rest = rest.trim();
        if let Some(after_alias) = strip_keyword(rest, "alias") {
            let after_alias = after_alias.trim().trim_end_matches(';').trim();
            if let Some(eq_pos) = after_alias.find('=') {
                let name = after_alias[..eq_pos].trim().to_string();
                let target = after_alias[eq_pos + 1..].trim().to_string();
                module.aliases.insert(name, target);
            }
        } else if let Some(after_const) = strip_keyword(rest, "const") {
            let after_const = after_const.trim().trim_end_matches(';').trim();
            if let Some(eq_pos) = after_const.find('=') {
                let lhs = after_const[..eq_pos].trim();
                let rhs = after_const[eq_pos + 1..].trim();
                let (name, ty_opt) = if let Some(colon) = lhs.find(':') {
                    let n = lhs[..colon].trim().to_string();
                    let t_str = lhs[colon + 1..].trim();
                    let th = resolve_or_insert_type(&mut module, &mut type_map, &const_values, t_str);
                    (n, Some(th))
                } else {
                    (lhs.to_string(), None)
                };
                let val = eval_u32_const_expr(rhs, &const_values);
                if let Some(v) = val {
                    const_values.insert(name.clone(), v);
                }
                module.constants.append(Constant {
                    name: Some(name),
                    ty: ty_opt,
                    expr_src: rhs.to_string(),
                    u32_value: val,
                });
            }
        }
    }

    // Second pass: register structs
    for item in &items {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (_, rest) = parse_leading_attributes(trimmed)?;
        let rest = rest.trim();
        if let Some(after_struct) = strip_keyword(rest, "struct") {
            let after_struct = after_struct.trim();
            let brace_pos = after_struct.find('{').ok_or_else(|| ParseError {
                message: format!("expected '{{' in struct definition: {after_struct}"),
                line: 1,
                column: 1,
            })?;
            let struct_name = after_struct[..brace_pos].trim().to_string();
            let close_brace = after_struct.rfind('}').ok_or_else(|| ParseError {
                message: format!("expected '}}' in struct definition: {struct_name}"),
                line: 1,
                column: 1,
            })?;
            let body = &after_struct[brace_pos + 1..close_brace];
            let fields = split_comma_top_level(body);
            let mut members = Vec::new();
            let mut offset = 0u32;
            let mut max_align = 1u32;

            for field in fields {
                let field = field.trim().trim_end_matches(';').trim();
                if field.is_empty() {
                    continue;
                }
                let (attrs, field_rest) = parse_leading_attributes(field)?;
                let colon_pos = field_rest.find(':').ok_or_else(|| ParseError {
                    message: format!("expected ':' in struct member `{field}`"),
                    line: 1,
                    column: 1,
                })?;
                let field_name = field_rest[..colon_pos].trim().to_string();
                let field_ty_str = field_rest[colon_pos + 1..].trim();
                let field_ty =
                    resolve_or_insert_type(&mut module, &mut type_map, &const_values, field_ty_str);
                let layout = compute_type_layout(&module.types, field_ty);
                let align = attrs
                    .align
                    .unwrap_or(layout.alignment)
                    .max(layout.alignment);
                let member_size = attrs.size.unwrap_or(layout.size);
                max_align = max_align.max(align);
                offset = align_up(offset, align);

                members.push(StructMember {
                    name: Some(field_name),
                    ty: field_ty,
                    binding: attrs.to_binding(),
                    offset,
                });
                offset += member_size;
            }
            let span = align_up(offset, max_align);
            let handle = module.types.append(Type {
                name: Some(struct_name.clone()),
                inner: TypeInner::Struct { members, span },
            });
            type_map.insert(struct_name, handle);
            module.struct_order.push(handle);
        }
    }

    // Third pass: register global variables
    let mut global_names: BTreeMap<String, Handle<GlobalVariable>> = BTreeMap::new();
    for item in &items {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (attrs, rest) = parse_leading_attributes(trimmed)?;
        let rest = rest.trim();
        if rest.starts_with("var<") || rest.starts_with("var ") || rest.starts_with("var\t") {
            let g = parse_global_var(
                &mut module,
                &mut type_map,
                &const_values,
                &attrs,
                rest.trim_end_matches(';').trim(),
            )?;
            let name_opt = g.name.clone();
            let h = module.global_variables.append(g);
            if let Some(n) = name_opt {
                global_names.insert(n, h);
            }
        }
    }

    // Fourth pass: register functions and entry points
    for item in &items {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (attrs, rest) = parse_leading_attributes(trimmed)?;
        let rest = rest.trim();
        if let Some(after_fn) = strip_keyword(rest, "fn") {
            let func = parse_function(
                &mut module,
                &mut type_map,
                &const_values,
                &global_names,
                after_fn.trim(),
            )?;
            if let Some(stage) = attrs.stage {
                let mut wg_size = [1u32, 1u32, 1u32];
                let mut wg_exprs = ["1".to_string(), "1".to_string(), "1".to_string()];
                let mut wg_overrides: Option<[Option<String>; 3]> = None;
                for (i, expr) in attrs.workgroup_size.iter().enumerate().take(3) {
                    wg_exprs[i] = expr.clone();
                    if let Some(v) = eval_u32_const_expr(expr, &const_values) {
                        wg_size[i] = v;
                    } else {
                        wg_size[i] = 1;
                        let ov = wg_overrides.get_or_insert([None, None, None]);
                        ov[i] = Some(expr.clone());
                    }
                }
                module.entry_points.push(EntryPoint {
                    name: func.name.clone().unwrap_or_else(|| "main".to_string()),
                    stage,
                    workgroup_size: wg_size,
                    workgroup_size_overrides: wg_overrides,
                    workgroup_size_exprs: wg_exprs,
                    function: func,
                });
            } else {
                module.functions.append(func);
            }
        }
    }

    Ok(module)
}

#[derive(Debug, Clone, Default)]
struct ParsedAttributes {
    group: Option<u32>,
    binding: Option<u32>,
    stage: Option<ShaderStage>,
    workgroup_size: Vec<String>,
    builtin: Option<BuiltIn>,
    location: Option<u32>,
    interpolation: Option<Interpolation>,
    sampling: Option<Sampling>,
    align: Option<u32>,
    size: Option<u32>,
}

impl ParsedAttributes {
    fn to_binding(&self) -> Option<Binding> {
        if let Some(b) = self.builtin {
            Some(Binding::BuiltIn(b))
        } else {
            self.location.map(|location| Binding::Location {
                location,
                interpolation: self.interpolation,
                sampling: self.sampling,
            })
        }
    }
}

fn parse_leading_attributes(mut input: &str) -> Result<(ParsedAttributes, &str), ParseError> {
    let mut attrs = ParsedAttributes::default();
    loop {
        input = input.trim_start();
        if !input.starts_with('@') {
            break;
        }
        input = &input[1..];
        input = input.trim_start();
        let ident_len = input
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(input.len());
        if ident_len == 0 {
            return Err(ParseError {
                message: "expected attribute name after '@'".to_string(),
                line: 1,
                column: 1,
            });
        }
        let attr_name = &input[..ident_len];
        input = input[ident_len..].trim_start();

        let mut args_str = "";
        if input.starts_with('(') {
            let close = find_matching_delim(input, '(', ')').ok_or_else(|| ParseError {
                message: format!("unclosed '(' in attribute @{attr_name}"),
                line: 1,
                column: 1,
            })?;
            args_str = input[1..close].trim();
            input = &input[close + 1..];
        }

        match attr_name {
            "compute" => attrs.stage = Some(ShaderStage::Compute),
            "vertex" => attrs.stage = Some(ShaderStage::Vertex),
            "fragment" => attrs.stage = Some(ShaderStage::Fragment),
            "workgroup_size" => {
                attrs.workgroup_size = split_comma_top_level(args_str)
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "group" => {
                attrs.group = Some(parse_u32_literal(args_str).ok_or_else(|| ParseError {
                    message: format!("invalid @group({args_str})"),
                    line: 1,
                    column: 1,
                })?);
            }
            "binding" => {
                attrs.binding = Some(parse_u32_literal(args_str).ok_or_else(|| ParseError {
                    message: format!("invalid @binding({args_str})"),
                    line: 1,
                    column: 1,
                })?);
            }
            "location" => {
                attrs.location = Some(parse_u32_literal(args_str).ok_or_else(|| ParseError {
                    message: format!("invalid @location({args_str})"),
                    line: 1,
                    column: 1,
                })?);
            }
            "builtin" => {
                attrs.builtin = Some(match args_str {
                    "position" => BuiltIn::Position,
                    "vertex_index" => BuiltIn::VertexIndex,
                    "instance_index" => BuiltIn::InstanceIndex,
                    "front_facing" => BuiltIn::FrontFacing,
                    "frag_depth" => BuiltIn::FragDepth,
                    "global_invocation_id" => BuiltIn::GlobalInvocationId,
                    "local_invocation_id" => BuiltIn::LocalInvocationId,
                    "local_invocation_index" => BuiltIn::LocalInvocationIndex,
                    "workgroup_id" => BuiltIn::WorkGroupId,
                    "num_workgroups" => BuiltIn::NumWorkGroups,
                    other => {
                        return Err(ParseError {
                            message: format!("unknown builtin `{other}`"),
                            line: 1,
                            column: 1,
                        })
                    }
                });
            }
            "interpolate" => {
                let parts = split_comma_top_level(args_str);
                if let Some(first) = parts.first() {
                    attrs.interpolation = match first.trim() {
                        "flat" => Some(Interpolation::Flat),
                        "linear" => Some(Interpolation::Linear),
                        "perspective" => Some(Interpolation::Perspective),
                        _ => None,
                    };
                }
                if let Some(second) = parts.get(1) {
                    attrs.sampling = match second.trim() {
                        "center" => Some(Sampling::Center),
                        "centroid" => Some(Sampling::Centroid),
                        "sample" => Some(Sampling::Sample),
                        _ => None,
                    };
                }
            }
            "align" => {
                attrs.align = parse_u32_literal(args_str);
            }
            "size" => {
                attrs.size = parse_u32_literal(args_str);
            }
            "must_use" | "invariant" | "diagnostic" => {}
            other => {
                return Err(ParseError {
                    message: format!("unknown attribute `@{other}`"),
                    line: 1,
                    column: 1,
                });
            }
        }
    }
    Ok((attrs, input))
}

fn parse_global_var(
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    attrs: &ParsedAttributes,
    rest: &str,
) -> Result<GlobalVariable, ParseError> {
    let after_var = rest
        .strip_prefix("var")
        .ok_or_else(|| ParseError {
            message: format!("expected `var` in `{rest}`"),
            line: 1,
            column: 1,
        })?
        .trim_start();

    let (space_opt, after_space) = if after_var.starts_with('<') {
        let close = find_matching_delim(after_var, '<', '>').ok_or_else(|| ParseError {
            message: format!("unclosed '<' in `{after_var}`"),
            line: 1,
            column: 1,
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
                        return Err(ParseError {
                            message: format!("invalid storage access mode `{other}`"),
                            line: 1,
                            column: 1,
                        })
                    }
                };
                AddressSpace::Storage { access }
            }
            "workgroup" => AddressSpace::WorkGroup,
            "private" => AddressSpace::Private,
            "function" => AddressSpace::Function,
            "immediate" | "push_constant" => AddressSpace::Immediate,
            other => {
                return Err(ParseError {
                    message: format!("unknown address space `{other}`"),
                    line: 1,
                    column: 1,
                })
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

    let colon_pos = decl_part.find(':').ok_or_else(|| ParseError {
        message: format!("expected ':' in global variable declaration `{rest}`"),
        line: 1,
        column: 1,
    })?;
    let name = decl_part[..colon_pos].trim().to_string();
    let ty_str = decl_part[colon_pos + 1..].trim();
    let ty = resolve_or_insert_type(module, type_map, const_values, ty_str);

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

fn parse_function(
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    global_names: &BTreeMap<String, Handle<GlobalVariable>>,
    after_fn: &str,
) -> Result<Function, ParseError> {
    let open_paren = after_fn.find('(').ok_or_else(|| ParseError {
        message: format!("expected '(' in function declaration `{after_fn}`"),
        line: 1,
        column: 1,
    })?;
    let fn_name = after_fn[..open_paren].trim().to_string();
    if fn_name.is_empty() {
        return Err(ParseError {
            message: "empty function name".to_string(),
            line: 1,
            column: 1,
        });
    }
    let close_paren = find_matching_delim(&after_fn[open_paren..], '(', ')')
        .map(|i| open_paren + i)
        .ok_or_else(|| ParseError {
            message: format!("unclosed '(' in function `{fn_name}`"),
            line: 1,
            column: 1,
        })?;
    let args_src = &after_fn[open_paren + 1..close_paren];
    let after_args = after_fn[close_paren + 1..].trim_start();

    let open_brace = find_top_level_char(after_args, '{').ok_or_else(|| ParseError {
        message: format!("expected '{{' in function `{fn_name}`"),
        line: 1,
        column: 1,
    })?;
    let ret_part = after_args[..open_brace].trim();
    let body_block = &after_args[open_brace..];
    let close_brace = find_matching_delim(body_block, '{', '}').ok_or_else(|| ParseError {
        message: format!("unclosed '{{' in function `{fn_name}`"),
        line: 1,
        column: 1,
    })?;
    let body_src = body_block[1..close_brace].to_string();

    let mut arguments = Vec::new();
    for arg in split_comma_top_level(args_src) {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        let (arg_attrs, arg_rest) = parse_leading_attributes(arg)?;
        let colon = arg_rest.find(':').ok_or_else(|| ParseError {
            message: format!("expected ':' in argument `{arg}` of `{fn_name}`"),
            line: 1,
            column: 1,
        })?;
        let arg_name = arg_rest[..colon].trim().to_string();
        let arg_ty_str = arg_rest[colon + 1..].trim();
        let arg_ty = resolve_or_insert_type(module, type_map, const_values, arg_ty_str);
        arguments.push(FunctionArgument {
            name: Some(arg_name),
            ty: arg_ty,
            binding: arg_attrs.to_binding(),
            raw_ty_src: arg_ty_str.to_string(),
        });
    }

    let result = if let Some(after_arrow) = ret_part.strip_prefix("->") {
        let (ret_attrs, ret_ty_str) = parse_leading_attributes(after_arrow.trim())?;
        let ret_ty_str = ret_ty_str.trim();
        let ret_ty = resolve_or_insert_type(module, type_map, const_values, ret_ty_str);
        Some(FunctionResult {
            ty: ret_ty,
            binding: ret_attrs.to_binding(),
            raw_ty_src: ret_ty_str.to_string(),
        })
    } else if !ret_part.is_empty() {
        return Err(ParseError {
            message: format!("unexpected return specifier `{ret_part}` in `{fn_name}`"),
            line: 1,
            column: 1,
        });
    } else {
        None
    };

    // Analyze body for referenced identifiers (globals and function calls)
    let local_arg_names: BTreeSet<&str> = arguments
        .iter()
        .filter_map(|a| a.name.as_deref())
        .collect();
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

fn analyze_identifiers(
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

pub fn resolve_or_insert_type(
    module: &mut Module,
    type_map: &mut BTreeMap<String, Handle<Type>>,
    const_values: &BTreeMap<String, u32>,
    raw_ty: &str,
) -> Handle<Type> {
    let ty_str = raw_ty.trim();
    if let Some(&h) = type_map.get(ty_str) {
        return h;
    }
    if let Some(aliased) = module.aliases.get(ty_str).cloned() {
        let h = resolve_or_insert_type(module, type_map, const_values, &aliased);
        type_map.insert(ty_str.to_string(), h);
        return h;
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
        _ if ty_str.starts_with("texture_2d<") => {
            let kind = if ty_str.contains("u32") {
                ScalarKind::Uint
            } else if ty_str.contains("i32") {
                ScalarKind::Sint
            } else {
                ScalarKind::Float
            };
            TypeInner::Image {
                dim: ImageDimension::D2,
                arrayed: false,
                class: ImageClass::Sampled { kind, multi: false },
            }
        }
        _ if ty_str.starts_with("texture_3d<") => {
            let kind = if ty_str.contains("u32") {
                ScalarKind::Uint
            } else if ty_str.contains("i32") {
                ScalarKind::Sint
            } else {
                ScalarKind::Float
            };
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
            let base = resolve_or_insert_type(module, type_map, const_values, elem_ty_str);
            let base_layout = compute_type_layout(&module.types, base);
            let stride = align_up(base_layout.size, base_layout.alignment);
            let size = if let Some(len_str) = parts.get(1) {
                let n = eval_u32_const_expr(len_str.trim(), const_values).unwrap_or(1);
                ArraySize::Constant(NonZeroU32::new(n.max(1)).unwrap())
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
            let base = resolve_or_insert_type(module, type_map, const_values, base_str);
            TypeInner::Pointer { base, space }
        }
        _ => {
            // Forward-referenced or unknown type fallback (4-byte scalar placeholder)
            TypeInner::Scalar(Scalar::U32)
        }
    };

    let h = module.types.append(Type {
        name: None,
        inner,
    });
    type_map.insert(ty_str.to_string(), h);
    h
}

pub fn eval_u32_const_expr(expr: &str, const_values: &BTreeMap<String, u32>) -> Option<u32> {
    let s = expr.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(inner) = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        if find_matching_delim(s, '(', ')') == Some(s.len() - 1) {
            return eval_u32_const_expr(inner, const_values);
        }
    }
    if let Some(pos) = find_top_level_char(s, '+') {
        let l = eval_u32_const_expr(&s[..pos], const_values)?;
        let r = eval_u32_const_expr(&s[pos + 1..], const_values)?;
        return Some(l.wrapping_add(r));
    }
    if let Some(pos) = find_top_level_char(s, '*') {
        let l = eval_u32_const_expr(&s[..pos], const_values)?;
        let r = eval_u32_const_expr(&s[pos + 1..], const_values)?;
        return Some(l.wrapping_mul(r));
    }
    if let Some(pos) = find_top_level_char(s, '/') {
        let l = eval_u32_const_expr(&s[..pos], const_values)?;
        let r = eval_u32_const_expr(&s[pos + 1..], const_values)?;
        return l.checked_div(r);
    }
    if let Some(&v) = const_values.get(s) {
        return Some(v);
    }
    parse_u32_literal(s)
}

pub fn parse_u32_literal(s: &str) -> Option<u32> {
    let s = s.trim().trim_end_matches('u').trim_end_matches('i');
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

pub fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut block_depth = 0usize;

    while i < bytes.len() {
        if block_depth > 0 {
            if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                block_depth += 1;
                out.push_str("  ");
                i += 2;
            } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                block_depth -= 1;
                out.push_str("  ");
                i += 2;
            } else {
                if bytes[i] == b'\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
                i += 1;
            }
            continue;
        }

        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            block_depth += 1;
            out.push_str("  ");
            i += 2;
        } else {
            let ch = src[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn validate_syntax(clean: &str) -> Result<(), ParseError> {
    let mut stack: Vec<(char, usize, usize)> = Vec::new();
    let mut line = 1usize;
    let mut col = 1usize;

    for ch in clean.chars() {
        if ch == '\n' {
            line += 1;
            col = 1;
            continue;
        }
        match ch {
            '{' | '(' | '[' => stack.push((ch, line, col)),
            '}' => {
                if stack.pop().map(|(c, _, _)| c) != Some('{') {
                    return Err(ParseError {
                        message: "unmatched '}'".to_string(),
                        line,
                        column: col,
                    });
                }
            }
            ')' => {
                if stack.pop().map(|(c, _, _)| c) != Some('(') {
                    return Err(ParseError {
                        message: "unmatched ')'".to_string(),
                        line,
                        column: col,
                    });
                }
            }
            ']' => {
                if stack.pop().map(|(c, _, _)| c) != Some('[') {
                    return Err(ParseError {
                        message: "unmatched ']'".to_string(),
                        line,
                        column: col,
                    });
                }
            }
            _ => {}
        }
        col += 1;
    }

    if let Some((open, l, c)) = stack.pop() {
        return Err(ParseError {
            message: format!("unclosed delimiter '{open}'"),
            line: l,
            column: c,
        });
    }
    Ok(())
}

fn split_top_level_items(clean: &str) -> Result<Vec<String>, ParseError> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut brace_depth = 0i32;
    let mut paren_depth = 0i32;
    let bytes = clean.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'(' => paren_depth += 1,
            b')' => paren_depth -= 1,
            b'{' => brace_depth += 1,
            b'}' => {
                brace_depth -= 1;
                if brace_depth == 0 && paren_depth == 0 {
                    let mut end = i + 1;
                    while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                        end += 1;
                    }
                    if end < bytes.len() && bytes[end] == b';' {
                        end += 1;
                    }
                    items.push(clean[start..end].to_string());
                    start = end;
                    i = end;
                    continue;
                }
            }
            b';' if brace_depth == 0 && paren_depth == 0 => {
                let end = i + 1;
                items.push(clean[start..end].to_string());
                start = end;
            }
            _ => {}
        }
        i += 1;
    }

    let tail = clean[start..].trim();
    if !tail.is_empty() {
        return Err(ParseError {
            message: format!("unexpected trailing tokens: `{tail}`"),
            line: 1,
            column: 1,
        });
    }

    Ok(items)
}

pub fn find_matching_delim(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    for (idx, ch) in s.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

pub fn find_top_level_char(s: &str, target: char) -> Option<usize> {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut angle = 0i32;
    let bytes = s.as_bytes();
    for (idx, ch) in s.char_indices() {
        if ch == target && paren == 0 && bracket == 0 && brace == 0 && angle == 0 {
            if target == '=' {
                let prev = if idx > 0 { Some(bytes[idx - 1]) } else { None };
                let next = bytes.get(idx + 1).copied();
                if matches!(prev, Some(b'=' | b'!' | b'<' | b'>')) || next == Some(b'=') {
                    continue;
                }
            }
            return Some(idx);
        }
        match ch {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => bracket += 1,
            ']' => bracket = (bracket - 1).max(0),
            '{' => brace += 1,
            '}' => brace = (brace - 1).max(0),
            '<' => {
                // Only treat `<` as template opener if preceded by identifier character
                if idx > 0 && (bytes[idx - 1].is_ascii_alphanumeric() || bytes[idx - 1] == b'_') {
                    angle += 1;
                }
            }
            '>' => {
                if angle > 0 {
                    angle -= 1;
                }
            }
            _ => {}
        }
    }
    None
}

pub fn split_comma_top_level(s: &str) -> Vec<&str> {
    let mut res = Vec::new();
    let mut start = 0usize;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut angle = 0i32;
    let bytes = s.as_bytes();

    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => bracket += 1,
            ']' => bracket = (bracket - 1).max(0),
            '{' => brace += 1,
            '}' => brace = (brace - 1).max(0),
            '<' => {
                if idx > 0 && (bytes[idx - 1].is_ascii_alphanumeric() || bytes[idx - 1] == b'_') {
                    angle += 1;
                }
            }
            '>' => {
                if angle > 0 {
                    angle -= 1;
                }
            }
            ',' if paren == 0 && bracket == 0 && brace == 0 && angle == 0 => {
                res.push(&s[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    res.push(&s[start..]);
    res
}

fn strip_keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    if rest.is_empty() {
        return Some(rest);
    }
    let first = rest.as_bytes()[0];
    if first.is_ascii_whitespace() || first == b'<' {
        Some(rest)
    } else {
        None
    }
}
