//! Parse errors and their source position.

use super::*;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl ParseError {
    pub fn emit_to_string(&self, _source: &str) -> String {
        format!(
            "WGSL parse error at {}:{}: {}",
            self.line, self.column, self.message
        )
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WGSL parse error at {}:{}: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for ParseError {}

pub(super) fn parse_error_at(
    clean: &str,
    fragment: &str,
    message: impl Into<String>,
) -> ParseError {
    let base = clean.as_ptr() as usize;
    let end = base + clean.len();
    let ptr = fragment.as_ptr() as usize;
    let offset = if ptr >= base && ptr <= end {
        ptr - base
    } else {
        0
    };
    let prefix = &clean[..offset];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
    let line_start = prefix.rfind('\n').map_or(0, |pos| pos + 1);
    let column = prefix[line_start..].chars().count() + 1;
    ParseError {
        message: message.into(),
        line,
        column,
    }
}
