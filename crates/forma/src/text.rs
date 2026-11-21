//! Small shared outline-text rasterizer and complex-script shaper, used unchanged by native and Web/WASM.
//!
//! `skrifa` reads the font tables and supplies vector contours. Curve flattening,
//! winding fill, antialiasing, complex-script shaping (`shape_text`: standard ligatures,
//! OpenType kerning, combining diacritics, BiDi/RTL run reordering, and fallback glyph
//! chains) and compositing below are our own.

use std::sync::OnceLock;

mod arabic;
mod bidi;
mod color_emoji;
mod face;
mod fonts;
mod linebreak;
mod metrics;
mod opentype;
mod outline;
mod raster;
mod runs;
mod shaping;
mod unicode;
mod wrap;

pub use bidi::{ResolvedDirection, TextDirection, resolve_direction};
pub(crate) use bidi::{
    caret_x, has_rtl, hit as hit_visual, selection_spans, step as step_visual, visual_line_with_dir,
};
#[doc(hidden)]
pub use color_emoji::{colr_cpal_layers, is_color_emoji_scalar};
pub use face::{Face, FontFace, GlyphId, OutlineBuilder};
pub use fonts::*;
pub use metrics::*;
#[doc(hidden)]
pub use opentype::{MarkOffset, OpenTypeTables, PairAdjustment};
pub(crate) use outline::*;
#[doc(hidden)]
pub use outline::{GlyphEdges, TextDraw, VectorScratch, vector_colored_glyphs};
pub use raster::*;
pub use runs::{RunFragment, RunLayout, layout_runs};
pub use shaping::*;
pub use unicode::*;
pub use wrap::*;

#[cfg(test)]
mod tests;
