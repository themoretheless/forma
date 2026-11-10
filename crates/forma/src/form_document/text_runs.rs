//! `Text { runs: [...] }`: styled runs inside one passive Text. Each run is an object with a
//! required `text` and optional `color`, `fontWeight`, `fontFamily`, `background` and `link`.
//! Missing fields inherit the Text's own values. The paragraph is laid out by
//! [`crate::text::layout_runs`] and flattened to one `ContentText` per placed fragment, a
//! `ContentShape` rectangle under fragments with a background and a `ContentLink` region over
//! fragments with a link.

use super::{Properties, Value, family_field, font_family, text_ref, write_str_literal};
use crate::text::{FontFamily, FontWeight, RunLayout, layout_runs};

const RUN_FIELDS: &[&str] = &[
    "text",
    "color",
    "fontWeight",
    "fontFamily",
    "background",
    "link",
];
/// Bound on runs per Text, so state cannot blow up the template.
pub(super) const MAX_RUNS: usize = 1024;

pub(super) struct StyledRun {
    text: String,
    family: FontFamily,
    color: String,
    background: Option<String>,
    link: Option<String>,
}

fn optional(p: &Properties, key: &str) -> Result<Option<String>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => text_ref(p, key, "").map(|v| Some(v.into_owned())),
    }
}

/// The Text's runs, or `None` when it has no `runs` property.
pub(super) fn runs(p: &Properties) -> Result<Option<Vec<StyledRun>>, String> {
    let list = match p.get("runs") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::List(list)) => list,
        Some(_) => return Err("Text.runs expects a list of objects".into()),
    };
    if list.len() > MAX_RUNS {
        return Err(format!("Text.runs is limited to {MAX_RUNS} runs"));
    }
    // The built-in Text forwards `text: ''` alongside its runs; only real text conflicts.
    if !text_ref(p, "text", "")?.is_empty() {
        return Err("Text accepts either text or runs, not both".into());
    }
    let base_family = font_family(p)?;
    let base_color = text_ref(p, "color", "#ffffff")?;
    list.iter()
        .map(|item| {
            let Value::Object(run) = item else {
                return Err("Text.runs items must be objects".into());
            };
            if let Some(key) = run.keys().find(|k| !RUN_FIELDS.contains(&k.as_str())) {
                return Err(format!("Unknown Text.runs field '{key}'"));
            }
            let text = match run.get("text") {
                Some(Value::Text(t)) => t.clone(),
                Some(Value::Number(_)) => text_ref(run, "text", "")?.into_owned(),
                _ => return Err("Text.runs item requires text".into()),
            };
            let mut family = base_family;
            if let Some(name) = optional(run, "fontFamily")? {
                family = FontFamily::from_name(&name).ok_or_else(|| {
                    format!("Text.runs fontFamily expects 'sans' or 'mono', got '{name}'")
                })?;
                // A run that names its family keeps the Text's weight.
                if base_family == FontFamily::SansBold {
                    family = family.with_weight(FontWeight::Bold);
                }
            }
            if let Some(name) = optional(run, "fontWeight")? {
                let weight = FontWeight::from_name(&name).ok_or_else(|| {
                    format!("Text.runs fontWeight expects 'regular' or 'bold', got '{name}'")
                })?;
                family = family.with_weight(weight);
            }
            let link = optional(run, "link")?;
            if link.as_deref() == Some("") {
                return Err("Text.runs link must not be empty".into());
            }
            Ok(StyledRun {
                text,
                family,
                color: optional(run, "color")?.unwrap_or_else(|| base_color.to_string()),
                background: optional(run, "background")?,
                link,
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Some)
}

/// Concatenated text of a `runs` list when the control's own `text` is empty.
pub(super) fn plain_label(p: &Properties) -> Option<String> {
    let Some(Value::List(list)) = p.get("runs") else {
        return None;
    };
    if text_ref(p, "text", "").ok()?.is_empty() {
        Some(
            list.iter()
                .filter_map(|item| match item {
                    Value::Object(run) => text_ref(run, "text", "").ok().map(|t| t.into_owned()),
                    _ => None,
                })
                .collect(),
        )
    } else {
        None
    }
}

fn layout(runs: &[StyledRun], font_size: f64, max_width: Option<f64>) -> RunLayout {
    let parts: Vec<(FontFamily, &str)> = runs.iter().map(|r| (r.family, r.text.as_str())).collect();
    layout_runs(&parts, font_size as f32, max_width.map(|w| w as f32))
}

/// Line height of a run paragraph. A single line uses the tallest face's natural height, so a
/// one-line run Text sizes like a plain Text; several lines use the plain Text's 1.5 em.
fn single_line_height(runs: &[StyledRun], font_size: f64) -> f64 {
    runs.iter()
        .map(|r| crate::text::measure_line_in(r.family, "", font_size as f32)[1] as f64)
        .fold(
            crate::text::measure_line_in(FontFamily::Sans, "", font_size as f32)[1] as f64,
            f64::max,
        )
}

/// Natural size of a run Text on `axis` (0 = width, 1 = height).
pub(super) fn natural(
    runs: &[StyledRun],
    font_size: f64,
    wrap_width: Option<f64>,
    axis: usize,
) -> f64 {
    let placed = layout(runs, font_size, wrap_width);
    if axis == 0 {
        placed.width as f64
    } else if placed.lines == 1 {
        single_line_height(runs, font_size)
    } else {
        placed.lines as f64 * font_size * 1.5
    }
}

/// Flatten a run Text placed at `rect` into template primitives.
pub(super) fn flatten(
    runs: &[StyledRun],
    font_size: f64,
    wrap: bool,
    rect: [f64; 4],
    out: &mut String,
) {
    let [x, y, w, h] = rect;
    let placed = layout(runs, font_size, (wrap && w > 0.).then_some(w));
    let multiline = placed.lines > 1;
    let line_height = font_size * 1.5;
    if multiline {
        let _ = std::fmt::Write::write_fmt(
            out,
            format_args!("ContentClip {{ x:{x}; y:{y}; width:{w}; height:{h}; radius:0; }} "),
        );
    }
    // One line keeps the plain Text's vertical centring in the whole rectangle.
    let line_box = |line: usize| {
        if multiline {
            (y + line as f64 * line_height, line_height)
        } else {
            (y, h)
        }
    };
    for f in placed.fragments.iter().filter(|f| f.width > 0.) {
        let run = &runs[f.run];
        let Some(background) = &run.background else {
            continue;
        };
        let (top, height) = line_box(f.line);
        let (left, right, bottom) = (x + f.x as f64, x + (f.x + f.width) as f64, top + height);
        let _ = std::fmt::Write::write_fmt(
            out,
            format_args!(
                "ContentShape {{ points:'{left} {top} {right} {top} {right} {bottom} {left} {bottom}'; color:{background}; }} "
            ),
        );
    }
    for f in &placed.fragments {
        let run = &runs[f.run];
        let piece = &run.text[f.range.clone()];
        if piece.trim().is_empty() {
            continue;
        }
        let (top, height) = line_box(f.line);
        let _ = std::fmt::Write::write_fmt(
            out,
            format_args!(
                "ContentText {{ x:{}; y:{top}; width:{}; height:{height}; text:",
                x + f.x as f64,
                f.width
            ),
        );
        write_str_literal(piece, out);
        let family = family_field(run.family);
        let _ = std::fmt::Write::write_fmt(
            out,
            format_args!("; color:{}; fontSize:{font_size};{family} }} ", run.color),
        );
    }
    for f in placed.fragments.iter().filter(|f| f.width > 0.) {
        let Some(link) = &runs[f.run].link else {
            continue;
        };
        let (top, height) = line_box(f.line);
        let _ = std::fmt::Write::write_fmt(
            out,
            format_args!(
                "ContentLink {{ x:{}; y:{top}; width:{}; height:{height}; link:",
                x + f.x as f64,
                f.width
            ),
        );
        write_str_literal(link, out);
        out.push_str("; } ");
    }
    if multiline {
        out.push_str("ContentClipEnd {} ");
    }
}
