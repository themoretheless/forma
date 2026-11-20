//! Attributes in front of a declaration.

use super::*;

#[derive(Debug, Clone, Default)]
pub(super) struct ParsedAttributes {
    pub(super) group: Option<u32>,
    pub(super) binding: Option<u32>,
    pub(super) stage: Option<ShaderStage>,
    pub(super) workgroup_size: Vec<String>,
    pub(super) builtin: Option<BuiltIn>,
    pub(super) location: Option<u32>,
    pub(super) interpolation: Option<Interpolation>,
    pub(super) sampling: Option<Sampling>,
    pub(super) align: Option<u32>,
    pub(super) size: Option<u32>,
}

impl ParsedAttributes {
    pub(super) fn to_binding(&self) -> Option<Binding> {
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

pub(super) fn parse_leading_attributes<'a>(
    clean: &str,
    mut input: &'a str,
) -> Result<(ParsedAttributes, &'a str), ParseError> {
    let mut attrs = ParsedAttributes::default();
    loop {
        input = input.trim_start();
        if !input.starts_with('@') {
            break;
        }
        let (attr_token, attr_name, args_str, rest) = parse_one_attribute(clean, input)?;
        apply_attribute(clean, &mut attrs, attr_token, attr_name, args_str)?;
        input = rest;
    }
    Ok((attrs, input))
}

fn parse_one_attribute<'a>(
    clean: &str,
    input: &'a str,
) -> Result<(&'a str, &'a str, &'a str, &'a str), ParseError> {
    let attr_token = input;
    let after_at = input[1..].trim_start();
    let ident_len = after_at
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(after_at.len());
    if ident_len == 0 {
        return Err(parse_error_at(
            clean,
            attr_token,
            "expected attribute name after '@'",
        ));
    }
    let attr_name = &after_at[..ident_len];
    let after_ident = after_at[ident_len..].trim_start();
    if after_ident.starts_with('(') {
        let close = find_matching_delim(after_ident, '(', ')').ok_or_else(|| {
            parse_error_at(
                clean,
                after_ident,
                format!("unclosed '(' in attribute @{attr_name}"),
            )
        })?;
        Ok((
            attr_token,
            attr_name,
            after_ident[1..close].trim(),
            &after_ident[close + 1..],
        ))
    } else {
        Ok((attr_token, attr_name, "", after_ident))
    }
}

fn apply_attribute(
    clean: &str,
    attrs: &mut ParsedAttributes,
    attr_token: &str,
    attr_name: &str,
    args_str: &str,
) -> Result<(), ParseError> {
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
        "group" => attrs.group = Some(parse_attr_u32(clean, "group", args_str)?),
        "binding" => attrs.binding = Some(parse_attr_u32(clean, "binding", args_str)?),
        "location" => attrs.location = Some(parse_attr_u32(clean, "location", args_str)?),
        "builtin" => attrs.builtin = Some(parse_builtin_name(clean, args_str)?),
        "interpolate" => parse_interpolate_args(args_str, attrs),
        "align" => attrs.align = parse_u32_literal(args_str),
        "size" => attrs.size = parse_u32_literal(args_str),
        "must_use" | "invariant" | "diagnostic" => {}
        other => {
            return Err(parse_error_at(
                clean,
                attr_token,
                format!("unknown attribute `@{other}`"),
            ));
        }
    }
    Ok(())
}

fn parse_attr_u32(clean: &str, attr_name: &str, args_str: &str) -> Result<u32, ParseError> {
    parse_u32_literal(args_str)
        .ok_or_else(|| parse_error_at(clean, args_str, format!("invalid @{attr_name}({args_str})")))
}

fn parse_builtin_name(clean: &str, args_str: &str) -> Result<BuiltIn, ParseError> {
    match args_str {
        "position" => Ok(BuiltIn::Position),
        "vertex_index" => Ok(BuiltIn::VertexIndex),
        "instance_index" => Ok(BuiltIn::InstanceIndex),
        "front_facing" => Ok(BuiltIn::FrontFacing),
        "frag_depth" => Ok(BuiltIn::FragDepth),
        "global_invocation_id" => Ok(BuiltIn::GlobalInvocationId),
        "local_invocation_id" => Ok(BuiltIn::LocalInvocationId),
        "local_invocation_index" => Ok(BuiltIn::LocalInvocationIndex),
        "workgroup_id" => Ok(BuiltIn::WorkGroupId),
        "num_workgroups" => Ok(BuiltIn::NumWorkGroups),
        other => Err(parse_error_at(
            clean,
            args_str,
            format!("unknown builtin `{other}`"),
        )),
    }
}

fn parse_interpolate_args(args_str: &str, attrs: &mut ParsedAttributes) {
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
