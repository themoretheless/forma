//! Small shared outline-text rasterizer and complex-script shaper, used unchanged by native and Web/WASM.
//!
//! `ttf-parser` reads the font and supplies vector contours. Curve flattening,
//! winding fill, antialiasing, complex-script shaping (`shape_text`: standard ligatures,
//! OpenType kerning, combining diacritics, BiDi/RTL run reordering, and fallback glyph
//! chains) and compositing below are our own.

use std::sync::OnceLock;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

mod bidi;
mod fonts;
mod metrics;
mod outline;
mod raster;
mod runs;
mod shaping;
mod unicode;
mod wrap;

pub(crate) use bidi::{caret_x, has_rtl, hit as hit_visual, selection_spans, visual_line};
pub use fonts::*;
pub use metrics::*;
pub(crate) use outline::*;
pub use raster::*;
pub use runs::{RunFragment, RunLayout, layout_runs};
pub use shaping::*;
pub use unicode::*;
pub use wrap::*;

#[cfg(test)]
mod tests;
