//! Multiline code editing with indentation, bracket pairing and `.ui` tokens.

use super::*;

/// Syntax token span emitted by [`CodeEditorModel`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeTokenSpan {
    pub kind: String,
    pub value: String,
    pub line: usize,
    pub column: usize,
}

/// Interactive state model for `CodeEditor`: multiline editing, line/column metrics,
/// indentation/dedentation, auto-closing brackets, and `.ui` syntax tokenization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeEditorModel {
    editor: TextEdit,
    tab_size: usize,
}

impl CodeEditorModel {
    pub fn new(initial: String, tab_size: usize) -> Result<Self, ModelError> {
        Ok(Self {
            editor: TextEdit::with_multiline(initial, true)?,
            tab_size: tab_size.clamp(1, 8),
        })
    }

    pub fn value(&self) -> &str {
        self.editor.value()
    }

    pub fn editor_mut(&mut self) -> &mut TextEdit {
        &mut self.editor
    }

    pub fn line_count(&self) -> usize {
        self.editor.value().split('\n').count().max(1)
    }

    /// Returns 1-based `(line, column)` of the current caret.
    pub fn cursor_line_column(&self) -> (usize, usize) {
        let prefix = &self.editor.value()[..self.editor.caret()];
        let mut line = 1usize;
        let mut last_nl = 0usize;
        for (i, ch) in prefix.char_indices() {
            if ch == '\n' {
                line += 1;
                last_nl = i + 1;
            }
        }
        let col = prefix[last_nl..].chars().count() + 1;
        (line, col)
    }

    pub fn indent(&mut self) -> bool {
        let spaces = " ".repeat(self.tab_size);
        self.editor.insert(&spaces).unwrap_or(false)
    }

    pub fn insert_pair(&mut self, open: char) -> Result<bool, ModelError> {
        let close = match open {
            '{' => Some('}'),
            '(' => Some(')'),
            '[' => Some(']'),
            '"' => Some('"'),
            '\'' => Some('\''),
            _ => None,
        };
        if let Some(closing) = close {
            let pair = format!("{open}{closing}");
            let changed = self.editor.insert(&pair)?;
            let _ = self.editor.left(false);
            Ok(changed)
        } else {
            self.editor.insert(&open.to_string())
        }
    }

    /// Tokenizes the current buffer using `themoretheless_tokenizer_forma::tokenize`.
    pub fn syntax_tokens(&self) -> Result<Vec<CodeTokenSpan>, String> {
        let src = self.editor.value();
        let tokens = themoretheless_tokenizer_forma::tokenize(src).map_err(|e| e.to_string())?;
        Ok(tokens
            .into_iter()
            .filter(|t| !t.text.is_empty())
            .map(|t| {
                let prefix = &src[..t.start.min(src.len())];
                let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
                let col_start = prefix.rfind('\n').map_or(0, |idx| idx + 1);
                let column = prefix[col_start..].chars().count() + 1;
                let kind = match t.text.as_str() {
                    "component" | "base" | "use" | "prop" | "state" | "event" | "enum"
                    | "struct" | "where" | "tokens" | "i18n" | "style" | "let" | "slot"
                    | "override" | "match" | "shortcut" | "true" | "false" => "keyword",
                    "{" | "}" | "(" | ")" | "[" | "]" | ":" | ";" | "," | "->" | "=>" => "punct",
                    s if s.starts_with('\'') || s.starts_with('"') => "string",
                    s if s.starts_with('#') => "color",
                    s if s.chars().next().is_some_and(|c| c.is_ascii_digit()) => "number",
                    _ => "ident",
                };
                CodeTokenSpan {
                    kind: kind.to_string(),
                    value: t.text,
                    line,
                    column,
                }
            })
            .collect())
    }
}
