//! The WGSL front end: source text to a [`Module`].

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

mod attributes;
mod const_expr;
mod error;
mod items;
mod text;
mod types;

use attributes::*;
pub use const_expr::*;
pub use error::*;
use items::*;
pub use text::*;
pub use types::*;

pub fn parse_str(source: &str) -> Result<Module, ParseError> {
    let clean = strip_comments(source);
    validate_syntax(&clean)?;
    let mut parser = ModuleParser {
        clean: &clean,
        module: Module {
            raw_source: source.to_string(),
            ..Module::default()
        },
        type_map: BTreeMap::new(),
        const_values: BTreeMap::new(),
    };
    let items = split_top_level_items(&clean)?;

    // Aliases and constant values come first so struct array sizes and workgroup sizes can
    // resolve them. A constant's own type may be a struct, so it is resolved after the structs.
    let mut declared_constants: Vec<(Option<&str>, Constant)> = Vec::new();
    for item in &items {
        let Some((_, rest)) = parser.item(item)? else {
            continue;
        };
        if let Some(after_alias) = strip_keyword(rest, "alias") {
            parser.register_alias(after_alias);
        } else if let Some((after_const, is_override)) = strip_keyword(rest, "const")
            .map(|r| (r, false))
            .or_else(|| strip_keyword(rest, "override").map(|r| (r, true)))
        {
            declared_constants.extend(parser.declare_constant(after_const, is_override));
        }
    }

    // Structs, each after the structs its members name.
    for item in structs_in_dependency_order(&clean, &items)? {
        let Some((_, rest)) = parser.item(item)? else {
            continue;
        };
        if let Some(after_struct) = strip_keyword(rest, "struct") {
            parser.register_struct(after_struct)?;
        }
    }

    for (ty_src, mut constant) in declared_constants {
        if let Some(ty_src) = ty_src {
            constant.ty = Some(parser.resolve_type(ty_src)?);
        }
        parser.module.constants.append(constant);
    }

    let mut global_names: BTreeMap<String, Handle<GlobalVariable>> = BTreeMap::new();
    for item in &items {
        let Some((attrs, rest)) = parser.item(item)? else {
            continue;
        };
        if rest.starts_with("var<") || rest.starts_with("var ") || rest.starts_with("var\t") {
            let g = parse_global_var(
                &clean,
                &mut parser.module,
                &mut parser.type_map,
                &parser.const_values,
                &attrs,
                rest.trim_end_matches(';').trim(),
            )?;
            let name_opt = g.name.clone();
            let h = parser.module.global_variables.append(g);
            if let Some(n) = name_opt {
                global_names.insert(n, h);
            }
        }
    }

    for item in &items {
        let Some((attrs, rest)) = parser.item(item)? else {
            continue;
        };
        if let Some(after_fn) = strip_keyword(rest, "fn") {
            let func = parse_function(
                &clean,
                &mut parser.module,
                &mut parser.type_map,
                &parser.const_values,
                &global_names,
                after_fn.trim(),
            )?;
            match attrs.stage {
                Some(stage) => parser.register_entry_point(stage, &attrs.workgroup_size, func),
                None => {
                    parser.module.functions.append(func);
                }
            }
        }
    }

    Ok(parser.module)
}

/// The module being built from one source, with the lookups the item parsers share.
struct ModuleParser<'a> {
    /// The source without comments: error positions are computed against it.
    clean: &'a str,
    module: Module,
    type_map: BTreeMap<String, Handle<Type>>,
    /// Constants whose value is known while parsing, for array sizes and workgroup sizes.
    const_values: BTreeMap<String, u32>,
}

impl<'a> ModuleParser<'a> {
    /// Splits a top-level item into its attributes and the declaration after them. `None` for an
    /// empty item.
    fn item<'s>(&self, item: &'s str) -> Result<Option<(ParsedAttributes, &'s str)>, ParseError> {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let (attrs, rest) = parse_leading_attributes(self.clean, trimmed)?;
        Ok(Some((attrs, rest.trim())))
    }

    fn resolve_type(&mut self, ty_src: &str) -> Result<Handle<Type>, ParseError> {
        resolve_or_insert_type(
            &mut self.module,
            &mut self.type_map,
            &self.const_values,
            ty_src,
        )
        .map_err(|message| parse_error_at(self.clean, ty_src, message))
    }

    fn register_alias(&mut self, after_alias: &str) {
        let after_alias = after_alias.trim().trim_end_matches(';').trim();
        if let Some(eq_pos) = after_alias.find('=') {
            let name = after_alias[..eq_pos].trim().to_string();
            let target = after_alias[eq_pos + 1..].trim().to_string();
            self.module.aliases.insert(name, target);
        }
    }

    /// Records a `const` or `override`. Its type is resolved later, once structs are known.
    fn declare_constant<'s>(
        &mut self,
        after_const: &'s str,
        is_override: bool,
    ) -> Option<(Option<&'s str>, Constant)> {
        let after_const = after_const.trim().trim_end_matches(';').trim();
        if let Some(eq_pos) = after_const.find('=') {
            let lhs = after_const[..eq_pos].trim();
            let rhs = after_const[eq_pos + 1..].trim();
            let (name, ty_src) = match lhs.find(':') {
                Some(colon) => (lhs[..colon].trim(), Some(lhs[colon + 1..].trim())),
                None => (lhs, None),
            };
            let val = eval_u32_const_expr(rhs, &self.const_values);
            if let (Some(v), false) = (val, is_override) {
                self.const_values.insert(name.to_string(), v);
            }
            return Some((
                ty_src,
                Constant {
                    name: Some(name.to_string()),
                    ty: None,
                    expr_src: rhs.to_string(),
                    u32_value: if is_override { None } else { val },
                    is_override,
                },
            ));
        }
        None
    }

    fn register_struct(&mut self, after_struct: &str) -> Result<(), ParseError> {
        let clean = self.clean;
        let after_struct = after_struct.trim();
        let brace_pos = after_struct.find('{').ok_or_else(|| {
            parse_error_at(
                clean,
                after_struct,
                format!("expected '{{' in struct definition: {after_struct}"),
            )
        })?;
        let struct_name = after_struct[..brace_pos].trim().to_string();
        let close_brace = after_struct.rfind('}').ok_or_else(|| {
            parse_error_at(
                clean,
                after_struct,
                format!("expected '}}' in struct definition: {struct_name}"),
            )
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
            let (attrs, field_rest) = parse_leading_attributes(clean, field)?;
            let colon_pos = field_rest.find(':').ok_or_else(|| {
                parse_error_at(
                    clean,
                    field_rest,
                    format!("expected ':' in struct member `{field}`"),
                )
            })?;
            let field_name = field_rest[..colon_pos].trim().to_string();
            let field_ty_str = field_rest[colon_pos + 1..].trim();
            let field_ty = self.resolve_type(field_ty_str)?;
            let layout = compute_type_layout(&self.module.types, field_ty);
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
        let handle = self.module.types.append(Type {
            name: Some(struct_name.clone()),
            inner: TypeInner::Struct { members, span },
        });
        self.type_map.insert(struct_name, handle);
        self.module.struct_order.push(handle);
        Ok(())
    }

    fn register_entry_point(
        &mut self,
        stage: ShaderStage,
        workgroup_size: &[String],
        func: Function,
    ) {
        let mut wg_size = [1u32, 1u32, 1u32];
        let mut wg_exprs = ["1".to_string(), "1".to_string(), "1".to_string()];
        let mut wg_overrides: Option<[Option<String>; 3]> = None;
        for (i, expr) in workgroup_size.iter().enumerate().take(3) {
            wg_exprs[i] = expr.clone();
            if let Some(v) = eval_u32_const_expr(expr, &self.const_values) {
                wg_size[i] = v;
            } else {
                wg_size[i] = 1;
                let ov = wg_overrides.get_or_insert([None, None, None]);
                ov[i] = Some(expr.clone());
            }
        }
        self.module.entry_points.push(EntryPoint {
            name: func.name.clone().unwrap_or_else(|| "main".to_string()),
            stage,
            workgroup_size: wg_size,
            workgroup_size_overrides: wg_overrides,
            workgroup_size_exprs: wg_exprs,
            function: func,
        });
    }
}

#[cfg(test)]
mod tests;
