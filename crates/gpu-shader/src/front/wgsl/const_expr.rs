//! Constant `u32` expressions.

use super::*;

/// Evaluates a constant `u32` expression: literals, named constants, parentheses, `u32(..)` /
/// `i32(..)` conversions, unary `-` and `~`, and the binary operators `* / % + - << >> & ^ |`
/// with their usual precedence, each associating to the left.
///
/// Arithmetic wraps like `u32`; division or remainder by zero, a shift of 32 bits or more, an
/// unknown name and trailing input all give `None`.
pub fn eval_u32_const_expr(expr: &str, const_values: &BTreeMap<String, u32>) -> Option<u32> {
    let mut parser = ConstExpr {
        src: expr.as_bytes(),
        pos: 0,
        const_values,
    };
    let value = parser.binary(0)?;
    parser.skip_spaces();
    (parser.pos == parser.src.len()).then_some(value)
}

/// Binary operators by precedence level, loosest first.
const CONST_BINARY_LEVELS: [&[&str]; 6] = [
    &["|"],
    &["^"],
    &["&"],
    &["<<", ">>"],
    &["+", "-"],
    &["*", "/", "%"],
];

struct ConstExpr<'a> {
    src: &'a [u8],
    pos: usize,
    const_values: &'a BTreeMap<String, u32>,
}

impl ConstExpr<'_> {
    fn skip_spaces(&mut self) {
        while self.src.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip_spaces();
        let found = self.src[self.pos..].starts_with(token.as_bytes());
        if found {
            self.pos += token.len();
        }
        found
    }

    fn binary(&mut self, level: usize) -> Option<u32> {
        let Some(operators) = CONST_BINARY_LEVELS.get(level) else {
            return self.unary();
        };
        let mut left = self.binary(level + 1)?;
        while let Some(operator) = operators.iter().find(|operator| self.eat(operator)) {
            let right = self.binary(level + 1)?;
            left = match *operator {
                "|" => left | right,
                "^" => left ^ right,
                "&" => left & right,
                "<<" => left.checked_shl(right)?,
                ">>" => left.checked_shr(right)?,
                "+" => left.wrapping_add(right),
                "-" => left.wrapping_sub(right),
                "*" => left.wrapping_mul(right),
                "/" => left.checked_div(right)?,
                _ => left.checked_rem(right)?,
            };
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<u32> {
        if self.eat("-") {
            Some(self.unary()?.wrapping_neg())
        } else if self.eat("~") {
            Some(!self.unary()?)
        } else {
            self.primary()
        }
    }

    fn primary(&mut self) -> Option<u32> {
        if self.eat("(") {
            let value = self.binary(0)?;
            return self.eat(")").then_some(value);
        }
        self.skip_spaces();
        let start = self.pos;
        while self
            .src
            .get(self.pos)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.pos += 1;
        }
        let word = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
        if matches!(word, "u32" | "i32") && self.eat("(") {
            let value = self.binary(0)?;
            return self.eat(")").then_some(value);
        }
        match self.const_values.get(word) {
            Some(&value) => Some(value),
            None if word.starts_with(|first: char| first.is_ascii_digit()) => {
                parse_u32_literal(word)
            }
            None => None,
        }
    }
}

pub fn parse_u32_literal(s: &str) -> Option<u32> {
    let s = s.trim().trim_end_matches('u').trim_end_matches('i');
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}
