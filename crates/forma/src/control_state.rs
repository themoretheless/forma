//! Renderer-independent control state shared by native and WASM hosts.
//!
//! Hosts gate disabled/read-only input, map platform events, and invalidate paint.
//! These models never execute application actions or own bindings. Scalar editing APIs preserve UTF-8 boundaries. Grapheme editing
//! is explicit and used by the UI; neither API performs shaping or owns an IME session.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

mod check;
mod code_editor;
mod command_palette;
mod data_table;
mod date_time;
mod error;
mod range;
mod rich_text;
mod selection;
mod text_edit;
mod toggle;
mod tree;
mod virtual_list;

pub use check::*;
pub use code_editor::*;
pub use command_palette::*;
pub use data_table::*;
pub use date_time::*;
pub use error::*;
pub use range::*;
pub use rich_text::*;
pub use selection::*;
pub use text_edit::*;
pub use toggle::*;
pub use tree::*;
pub use virtual_list::*;

#[cfg(test)]
mod tests;
