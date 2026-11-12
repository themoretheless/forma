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
            let ch = src[i..].chars().next().unwrap();
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

pub fn find_top_level_char(s: &str, target: char) -> Option<usize> {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut angle = 0i32;
    let bytes = s.as_bytes();
    for (idx, ch) in s.char_indices() {
        if ch == target && paren == 0 && bracket == 0 && brace == 0 && angle == 0 {
            if target == '=' {
                let prev = if idx > 0 { Some(bytes[idx - 1]) } else { None };
                let next = bytes.get(idx + 1).copied();
                if matches!(prev, Some(b'=' | b'!' | b'<' | b'>')) || next == Some(b'=') {
                    continue;
                }
            }
            return Some(idx);
        }
        match ch {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => bracket += 1,
            ']' => bracket = (bracket - 1).max(0),
            '{' => brace += 1,
            '}' => brace = (brace - 1).max(0),
            '<' => {
                // Only treat `<` as template opener if preceded by identifier character
                if idx > 0 && (bytes[idx - 1].is_ascii_alphanumeric() || bytes[idx - 1] == b'_') {
                    angle += 1;
                }
            }
            '>' if angle > 0 => {
                angle -= 1;
            }
            _ => {}
        }
    }
    None
}

pub fn split_comma_top_level(s: &str) -> Vec<&str> {
    let mut res = Vec::new();
    let mut start = 0usize;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut angle = 0i32;
    let bytes = s.as_bytes();

    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => bracket += 1,
            ']' => bracket = (bracket - 1).max(0),
            '{' => brace += 1,
            '}' => brace = (brace - 1).max(0),
            '<' => {
                if idx > 0 && (bytes[idx - 1].is_ascii_alphanumeric() || bytes[idx - 1] == b'_') {
                    angle += 1;
                }
            }
            '>' if angle > 0 => {
                angle -= 1;
            }
            ',' if paren == 0 && bracket == 0 && brace == 0 && angle == 0 => {
                res.push(&s[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    res.push(&s[start..]);
    res
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
