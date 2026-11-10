//! The expression builtins, grouped by what they operate on.
//!
//! `call` is the only entry point. Each family owns its names: a name reaches exactly one family,
//! and a family answers a call it does not accept (wrong arity included) with the same
//! "unknown function" error an unlisted name gets.

use super::{char_slice_range, expression_number, format_hex_color, parse_hex_color};
use crate::form_document::{Properties, Value, number, props};

mod color;
mod convert;
mod list;
mod locale;
mod math;
mod object;
mod sequence;
mod text;
mod validation;

fn unknown(name: &str) -> String {
    format!("Unknown function or invalid arguments: {name}")
}

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match name {
        "abs" | "floor" | "ceil" | "round" | "sqrt" | "sign" | "pow" | "lerp" | "remap" | "avg"
        | "sum" | "min" | "max" | "clamp" => math::call(name, arguments),
        "String" | "Number" | "Bool" | "formatNumber" => convert::call(name, arguments),
        "trim" | "upper" | "lower" | "capitalize" | "padStart" | "padEnd" | "truncate"
        | "repeat" | "startsWith" | "endsWith" | "replace" | "split" | "join" => {
            text::call(name, arguments)
        }
        "len" | "count" | "isEmpty" | "contains" | "includes" | "indexOf" | "slice" | "reverse" => {
            sequence::call(name, arguments)
        }
        "take" | "skip" | "range" | "unique" | "find" | "compact" | "flatten" | "chunk" | "zip"
        | "first" | "last" | "groupBy" | "pluck" | "sortBy" | "any" | "all" => {
            list::call(name, arguments)
        }
        "keys" | "values" | "entries" | "fromEntries" | "merge" | "pick" | "omit" => {
            object::call(name, arguments)
        }
        "plural" => locale::call(name, arguments),
        "isValid" => validation::call(name, arguments),
        "alpha" | "lighten" | "darken" | "saturate" | "desaturate" | "invert" | "contrast"
        | "mix" => color::call(name, arguments),
        _ => Err(unknown(name)),
    }
}
