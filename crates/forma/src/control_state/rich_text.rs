//! Rich text and Markdown documents with styled spans.

use super::*;

/// Inline text style flags for [`RichTextModel`] and [`MarkdownModel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InlineStyle {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Code,
}

/// Styled span over a UTF-8 byte range in [`RichTextModel`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichSpan {
    pub range: Range<usize>,
    pub style: InlineStyle,
}

/// Interactive state model for `RichTextDocument` and `MarkdownDocument`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichTextModel {
    editor: TextEdit,
    spans: Vec<RichSpan>,
    heading_level: u8,
}

impl RichTextModel {
    pub fn new(initial: String) -> Result<Self, ModelError> {
        Ok(Self {
            editor: TextEdit::with_multiline(initial, true)?,
            spans: Vec::new(),
            heading_level: 0,
        })
    }

    pub fn from_markdown(md: &str) -> Result<Self, ModelError> {
        let trimmed = md.trim_start();
        let (heading_level, body) = if let Some(rest) = trimmed.strip_prefix("### ") {
            (3, rest)
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            (2, rest)
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            (1, rest)
        } else {
            (0, trimmed)
        };
        let mut plain = String::new();
        let mut spans = Vec::new();
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if body[i..].starts_with("**") {
                if let Some(end) = body[i + 2..].find("**") {
                    let start_out = plain.len();
                    plain.push_str(&body[i + 2..i + 2 + end]);
                    spans.push(RichSpan {
                        range: start_out..plain.len(),
                        style: InlineStyle::Bold,
                    });
                    i += 4 + end;
                    continue;
                }
            }
            if body[i..].starts_with('`') {
                if let Some(end) = body[i + 1..].find('`') {
                    let start_out = plain.len();
                    plain.push_str(&body[i + 1..i + 1 + end]);
                    spans.push(RichSpan {
                        range: start_out..plain.len(),
                        style: InlineStyle::Code,
                    });
                    i += 2 + end;
                    continue;
                }
            }
            if body[i..].starts_with('*') {
                if let Some(end) = body[i + 1..].find('*') {
                    let start_out = plain.len();
                    plain.push_str(&body[i + 1..i + 1 + end]);
                    spans.push(RichSpan {
                        range: start_out..plain.len(),
                        style: InlineStyle::Italic,
                    });
                    i += 2 + end;
                    continue;
                }
            }
            let ch = body[i..].chars().next().unwrap();
            plain.push(ch);
            i += ch.len_utf8();
        }
        Ok(Self {
            editor: TextEdit::with_multiline(plain, true)?,
            spans,
            heading_level,
        })
    }

    pub fn to_markdown(&self) -> String {
        let prefix = match self.heading_level {
            1 => "# ",
            2 => "## ",
            3 => "### ",
            _ => "",
        };
        format!("{prefix}{}", self.editor.value())
    }

    pub fn text(&self) -> &str {
        self.editor.value()
    }

    pub fn editor_mut(&mut self) -> &mut TextEdit {
        &mut self.editor
    }

    pub fn spans(&self) -> &[RichSpan] {
        &self.spans
    }

    pub fn heading_level(&self) -> u8 {
        self.heading_level
    }

    pub fn set_heading_level(&mut self, level: u8) {
        self.heading_level = level.min(3);
    }

    pub fn toggle_style(&mut self, style: InlineStyle) -> bool {
        let sel = self.editor.selection();
        if sel.is_empty() {
            return false;
        }
        if let Some(pos) = self
            .spans
            .iter()
            .position(|s| s.range == sel && s.style == style)
        {
            self.spans.remove(pos);
            false
        } else {
            self.spans.push(RichSpan { range: sel, style });
            true
        }
    }

    pub fn clear_formatting(&mut self) {
        self.spans.clear();
        self.heading_level = 0;
    }
}
