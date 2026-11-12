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
        let attr_token = input;
        input = &input[1..];
        input = input.trim_start();
        let ident_len = input
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(input.len());
        if ident_len == 0 {
            return Err(parse_error_at(
                clean,
                attr_token,
                "expected attribute name after '@'",
            ));
        }
        let attr_name = &input[..ident_len];
        input = input[ident_len..].trim_start();

        let mut args_str = "";
        if input.starts_with('(') {
            let close = find_matching_delim(input, '(', ')').ok_or_else(|| {
                parse_error_at(
                    clean,
                    input,
                    format!("unclosed '(' in attribute @{attr_name}"),
                )
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
                attrs.group = Some(parse_u32_literal(args_str).ok_or_else(|| {
                    parse_error_at(clean, args_str, format!("invalid @group({args_str})"))
                })?);
            }
            "binding" => {
                attrs.binding = Some(parse_u32_literal(args_str).ok_or_else(|| {
                    parse_error_at(clean, args_str, format!("invalid @binding({args_str})"))
                })?);
            }
            "location" => {
                attrs.location = Some(parse_u32_literal(args_str).ok_or_else(|| {
                    parse_error_at(clean, args_str, format!("invalid @location({args_str})"))
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
                        return Err(parse_error_at(
                            clean,
                            args_str,
                            format!("unknown builtin `{other}`"),
                        ));
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
                return Err(parse_error_at(
                    clean,
                    attr_token,
                    format!("unknown attribute `@{other}`"),
                ));
            }
        }
    }
    Ok((attrs, input))
}
