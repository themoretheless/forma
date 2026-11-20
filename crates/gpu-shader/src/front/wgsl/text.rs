//! Source text helpers: comments, delimiters and top-level splitting.

use super::*;

pub fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut block_depth = 0usize;

    while i < bytes.len() {
        if block_depth > 0 {
            if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                block_depth += 1;
                out.push_str("  ");
                i += 2;
            } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                block_depth -= 1;
                out.push_str("  ");
                i += 2;
            } else {
                if bytes[i] == b'\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
                i += 1;
            }
            continue;
        }

        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            block_depth += 1;
            out.push_str("  ");
            i += 2;
        } else {
            let Some(ch) = src[i..].chars().next() else {
                break;
            };
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

pub(super) fn validate_syntax(clean: &str) -> Result<(), ParseError> {
    let mut stack: Vec<(char, usize, usize)> = Vec::new();
    let mut line = 1usize;
    let mut col = 1usize;

    for ch in clean.chars() {
        if ch == '\n' {
            line += 1;
            col = 1;
            continue;
        }
        match ch {
            '{' | '(' | '[' => stack.push((ch, line, col)),
            '}' => {
                if stack.pop().map(|(c, _, _)| c) != Some('{') {
                    return Err(ParseError {
                        message: "unmatched '}'".to_string(),
                        line,
                        column: col,
                    });
                }
            }
            ')' => {
                if stack.pop().map(|(c, _, _)| c) != Some('(') {
                    return Err(ParseError {
                        message: "unmatched ')'".to_string(),
                        line,
                        column: col,
                    });
                }
            }
            ']' if stack.pop().map(|(c, _, _)| c) != Some('[') => {
                return Err(ParseError {
                    message: "unmatched ']'".to_string(),
                    line,
                    column: col,
                });
            }
            _ => {}
        }
        col += 1;
    }

    if let Some((open, l, c)) = stack.pop() {
        return Err(ParseError {
            message: format!("unclosed delimiter '{open}'"),
            line: l,
            column: c,
        });
    }
    Ok(())
}

pub(super) fn split_top_level_items(clean: &str) -> Result<Vec<&str>, ParseError> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut brace_depth = 0i32;
    let mut paren_depth = 0i32;
    let bytes = clean.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'(' => paren_depth += 1,
            b')' => paren_depth -= 1,
            b'{' => brace_depth += 1,
            b'}' => {
                brace_depth -= 1;
                if brace_depth == 0 && paren_depth == 0 {
                    let mut end = i + 1;
                    while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                        end += 1;
                    }
                    if end < bytes.len() && bytes[end] == b';' {
                        end += 1;
                    }
                    items.push(&clean[start..end]);
                    start = end;
                    i = end;
                    continue;
                }
            }
            b';' if brace_depth == 0 && paren_depth == 0 => {
                let end = i + 1;
                items.push(&clean[start..end]);
                start = end;
            }
            _ => {}
        }
        i += 1;
    }

    let tail = clean[start..].trim();
    if !tail.is_empty() {
        return Err(parse_error_at(
            clean,
            tail,
            format!("unexpected trailing tokens: `{tail}`"),
        ));
    }

    Ok(items)
}

pub fn find_matching_delim(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    for (idx, ch) in s.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

/// Tracks delimiter nesting (`()`, `[]`, `{}`, and template `<>`) during a forward scan.
#[derive(Default)]
pub struct TopLevelScan {
    paren: i32,
    bracket: i32,
    brace: i32,
    angle: i32,
}

impl TopLevelScan {
    /// Returns `true` when outside all delimiters.
    #[inline]
    pub const fn at_top_level(&self) -> bool {
        self.paren == 0 && self.bracket == 0 && self.brace == 0 && self.angle == 0
    }

    /// Returns `true` when outside `()`, `[]`, and `{}` (ignoring angle brackets).
    #[inline]
    pub const fn outside_blocks(&self) -> bool {
        self.paren == 0 && self.bracket == 0 && self.brace == 0
    }

    /// Updates nesting state for `ch` at byte index `idx` in `bytes`.
    #[inline]
    pub fn step(&mut self, bytes: &[u8], idx: usize, ch: char) {
        match ch {
            '(' => self.paren += 1,
            ')' => self.paren = (self.paren - 1).max(0),
            '[' => self.bracket += 1,
            ']' => self.bracket = (self.bracket - 1).max(0),
            '{' => self.brace += 1,
            '}' => self.brace = (self.brace - 1).max(0),
            '<' => {
                // Only treat `<` as a template opener if preceded by an identifier character.
                if idx > 0 && (bytes[idx - 1].is_ascii_alphanumeric() || bytes[idx - 1] == b'_') {
                    self.angle += 1;
                }
            }
            '>' if self.angle > 0 => self.angle -= 1,
            _ => {}
        }
    }
}

pub fn find_top_level_char(s: &str, target: char) -> Option<usize> {
    let mut scan = TopLevelScan::default();
    let bytes = s.as_bytes();
    for (idx, ch) in s.char_indices() {
        if ch == target && scan.at_top_level() {
            if target == '=' {
                let prev = if idx > 0 { Some(bytes[idx - 1]) } else { None };
                let next = bytes.get(idx + 1).copied();
                if matches!(prev, Some(b'=' | b'!' | b'<' | b'>')) || next == Some(b'=') {
                    continue;
                }
            }
            return Some(idx);
        }
        scan.step(bytes, idx, ch);
    }
    None
}

pub fn split_comma_top_level(s: &str) -> Vec<&str> {
    let mut res = Vec::new();
    let mut start = 0usize;
    let mut scan = TopLevelScan::default();
    let bytes = s.as_bytes();

    for (idx, ch) in s.char_indices() {
        if ch == ',' && scan.at_top_level() {
            res.push(&s[start..idx]);
            start = idx + 1;
        } else {
            scan.step(bytes, idx, ch);
        }
    }
    res.push(&s[start..]);
    res
}

pub fn strip_kw_token<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    if rest.is_empty() {
        return Some(rest);
    }
    let next = rest.as_bytes()[0];
    if !next.is_ascii_alphanumeric() && next != b'_' {
        Some(rest)
    } else {
        None
    }
}

pub fn find_block_open(s: &str) -> Option<usize> {
    let mut scan = TopLevelScan::default();
    let bytes = s.as_bytes();
    for (idx, ch) in s.char_indices() {
        if ch == '{' && scan.outside_blocks() {
            return Some(idx);
        }
        scan.step(bytes, idx, ch);
    }
    None
}

pub fn find_stmt_semicolon(s: &str) -> Option<usize> {
    let mut scan = TopLevelScan::default();
    let bytes = s.as_bytes();
    for (idx, ch) in s.char_indices() {
        if ch == ';' && scan.outside_blocks() {
            return Some(idx);
        }
        scan.step(bytes, idx, ch);
    }
    None
}

pub(super) fn strip_keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    if rest.is_empty() {
        return Some(rest);
    }
    let first = rest.as_bytes()[0];
    if first.is_ascii_whitespace() || first == b'<' {
        Some(rest)
    } else {
        None
    }
}
