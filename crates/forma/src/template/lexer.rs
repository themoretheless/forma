//! Tokens of the template language.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Unit {
    Logical,
    Px,
    Ms,
}

/// `str::as_str` is unstable, so borrowed token text gets a local accessor.
pub(super) trait TokenText {
    fn text(&self) -> &str;
}

impl TokenText for Cow<'_, str> {
    fn text(&self) -> &str {
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Kind<'a> {
    Ident(Cow<'a, str>),
    String(Cow<'a, str>),
    Number(f32, Unit),
    Hex(Cow<'a, str>),
    Open,
    Close,
    Colon,
    Semi,
    LeftParen,
    RightParen,
    Arrow,
    Eof,
}

#[derive(Debug, Clone)]
pub(super) struct Token<'a> {
    pub(super) kind: Kind<'a>,
    pub(super) offset: usize,
}

pub(super) struct Lexer<'a> {
    pub(super) source: &'a str,
    pub(super) offset: usize,
}

impl<'a> Lexer<'a> {
    pub(super) fn rest(&self) -> &str {
        &self.source[self.offset..]
    }
    pub(super) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    pub(super) fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }
    pub(super) fn peek_byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.offset).copied()
    }
    pub(super) fn starts_with(&self, token: &str) -> bool {
        self.source.as_bytes()[self.offset..].starts_with(token.as_bytes())
    }
    pub(super) fn skip_bytes(&mut self, mut accepted: impl FnMut(u8) -> bool) {
        while self.peek_byte().is_some_and(&mut accepted) {
            self.offset += 1;
        }
    }
    pub(super) fn skip_whitespace(&mut self) {
        while self
            .peek_byte()
            .is_some_and(|b| b.is_ascii_whitespace() || b == b'\x0b')
        {
            self.offset += 1;
        }
        while self.peek_byte().is_some_and(|b| b >= 0x80)
            && self.peek().is_some_and(char::is_whitespace)
        {
            self.bump();
        }
    }
    pub(super) fn next(&mut self) -> Result<Token<'a>, String> {
        loop {
            self.skip_whitespace();
            if self.starts_with("//") {
                while self.peek_byte().is_some_and(|b| b != b'\n') {
                    self.offset += 1;
                }
            } else if self.starts_with("/*") {
                let start = self.offset;
                self.offset += 2;
                let mut depth = 1;
                while depth > 0 {
                    if self.starts_with("/*") {
                        depth += 1;
                        self.offset += 2;
                    } else if self.starts_with("*/") {
                        depth -= 1;
                        self.offset += 2;
                    } else if self.peek_byte().is_none() {
                        return Err(format!("Unclosed block comment (byte {start})"));
                    } else {
                        self.offset += 1;
                    }
                }
            } else {
                break;
            }
        }
        let offset = self.offset;
        let Some(ch) = self.bump() else {
            return Ok(Token {
                kind: Kind::Eof,
                offset,
            });
        };
        let kind = match ch {
            '{' => Kind::Open,
            '}' => Kind::Close,
            ':' => Kind::Colon,
            ';' => Kind::Semi,
            '(' => Kind::LeftParen,
            ')' => Kind::RightParen,
            '-' if self.starts_with(">") => {
                self.offset += 1;
                Kind::Arrow
            }
            '\'' | '"' => {
                // Escape-free literals (icon point lists dominate templates) are
                // one byte scan plus one copy instead of a char-by-char build.
                let quote = ch as u8;
                let bytes = self.source.as_bytes();
                let start = self.offset;
                let mut end = start;
                while end < bytes.len() && bytes[end] != quote && bytes[end] != b'\\' {
                    end += 1;
                }
                if end == bytes.len() {
                    return Err(format!("Unclosed string (byte {offset})"));
                }
                if bytes[end] != b'\\' {
                    self.offset = end + 1;
                    Kind::String(Cow::Borrowed(&self.source[start..end]))
                } else {
                    let mut value = String::new();
                    loop {
                        match self.bump() {
                            Some(c) if c == ch => break,
                            Some('\\') => {
                                let escaped = match self.bump() {
                                    Some('n') => '\n',
                                    Some('r') => '\r',
                                    Some('t') => '\t',
                                    Some('\\') => '\\',
                                    Some('\'') => '\'',
                                    Some('"') => '"',
                                    Some('$') => '$',
                                    Some(c) => {
                                        return Err(format!(
                                            "Unsupported escape \\{c} (byte {})",
                                            self.offset
                                        ));
                                    }
                                    None => return Err(format!("Unclosed string (byte {offset})")),
                                };
                                value.push(escaped);
                            }
                            Some(c) => value.push(c),
                            None => return Err(format!("Unclosed string (byte {offset})")),
                        }
                    }
                    Kind::String(Cow::Owned(value))
                }
            }
            '#' => {
                let start = self.offset;
                self.skip_bytes(|b| b.is_ascii_alphanumeric());
                Kind::Hex(Cow::Borrowed(&self.source[start..self.offset]))
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                self.skip_bytes(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.');
                Kind::Ident(Cow::Borrowed(&self.source[offset..self.offset]))
            }
            c if c.is_ascii_digit() || matches!(c, '.' | '-' | '+') => {
                self.skip_bytes(|b| b.is_ascii_digit() || b == b'.');
                if self.peek_byte().is_some_and(|b| b == b'e' || b == b'E') {
                    self.offset += 1;
                    if self.peek_byte().is_some_and(|b| b == b'+' || b == b'-') {
                        self.offset += 1;
                    }
                    self.skip_bytes(|b| b.is_ascii_digit());
                }
                let raw = &self.source[offset..self.offset];
                let value: f32 = raw
                    .parse()
                    .map_err(|_| format!("Invalid number '{raw}' (byte {offset})"))?;
                if !value.is_finite() {
                    return Err(format!("Number must be finite (byte {offset})"));
                }
                let unit = if self.starts_with("px") {
                    self.offset += 2;
                    Unit::Px
                } else if self.starts_with("ms") {
                    self.offset += 2;
                    Unit::Ms
                } else {
                    Unit::Logical
                };
                Kind::Number(value, unit)
            }
            c => {
                return Err(format!(
                    "Unsupported template character '{c}' (byte {offset})"
                ));
            }
        };
        Ok(Token { kind, offset })
    }
}
