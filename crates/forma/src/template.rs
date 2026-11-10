//! The first explicit primitive-composition subset of a Forma control template.
//! The host supplies the bounds; every painted primitive comes from this file.

use crate::markup::ButtonSpec;
use memory_budget::{CacheLimits, WeightedLru};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub props: ButtonSpec,
    pub radius: f32,
    pub fill: Option<Brush>,
    pub border: Option<Border>,
    pub reveal: Option<crate::reveal::Reveal>,
    pub text: Option<Text>,
    pub clickable: bool,
    pub content: Vec<Content>,
    pub inputs: Vec<crate::text_input::Input>,
    pub range: Option<RangeInput>,
    /// Rectangle reserved for application geometry (`Canvas {}`), relative to
    /// the control. Geometry comes from the application at runtime.
    pub canvas: Option<CanvasSlot>,
    /// Link regions of styled text runs (`ContentLink`), relative to the control. They draw
    /// nothing; the runtime hit-tests them and queues a link event.
    pub links: Vec<TextLink>,
}

/// `ContentLink { x; y; width; height; link: 'id'; }`: one placed fragment of a link run.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLink {
    pub bounds: [f32; 4],
    pub id: String,
}

/// `Canvas { x; y; width; height; }` inside a control template. Omitted width
/// or height extends the canvas to the control's right or bottom edge.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CanvasSlot {
    pub x: f32,
    pub y: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl CanvasSlot {
    /// Rectangle relative to a control of `size`.
    pub fn resolve(&self, size: [f32; 2]) -> [f32; 4] {
        [
            self.x,
            self.y,
            self.width.unwrap_or((size[0] - self.x).max(0.)),
            self.height.unwrap_or((size[1] - self.y).max(0.)),
        ]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Clip {
        bounds: [f32; 4],
        radius: f32,
    },
    ClipEnd,
    Text {
        bounds: [f32; 4],
        text: Text,
    },
    Shape {
        points: Vec<[f32; 2]>,
        color: [u8; 4],
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Brush {
    pub color: [u8; 4],
    pub hover: Option<[u8; 4]>,
    pub pressed: Option<[u8; 4]>,
    pub disabled: Option<[u8; 4]>,
    pub focus: Option<[u8; 4]>,
    pub duration_ms: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Border {
    pub width: f32,
    pub brush: Brush,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub text: String,
    pub color: [u8; 4],
    pub font_size: f32,
    pub family: crate::text::FontFamily,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Unit {
    Logical,
    Px,
    Ms,
}

/// `str::as_str` is unstable, so borrowed token text gets a local accessor.
trait TokenText {
    fn text(&self) -> &str;
}

impl TokenText for Cow<'_, str> {
    fn text(&self) -> &str {
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Kind<'a> {
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
struct Token<'a> {
    kind: Kind<'a>,
    offset: usize,
}

struct Lexer<'a> {
    source: &'a str,
    offset: usize,
}

impl<'a> Lexer<'a> {
    fn rest(&self) -> &str {
        &self.source[self.offset..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }
    fn peek_byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.offset).copied()
    }
    fn starts_with(&self, token: &str) -> bool {
        self.source.as_bytes()[self.offset..].starts_with(token.as_bytes())
    }
    fn skip_bytes(&mut self, mut accepted: impl FnMut(u8) -> bool) {
        while self.peek_byte().is_some_and(&mut accepted) {
            self.offset += 1;
        }
    }
    fn skip_whitespace(&mut self) {
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
    fn next(&mut self) -> Result<Token<'a>, String> {
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

/// Bitset of fields seen in the current template block. Every block accepts a small,
/// closed vocabulary of property and primitive names, so tracking duplicates in a `u64`
/// avoids allocating and dropping a `HashSet` for every shape, clip, brush, and label.
#[derive(Clone, Copy, Default)]
struct SeenFields(u64);

impl SeenFields {
    #[inline]
    fn bit(name: &str) -> Option<u64> {
        let index = match name {
            "color" => 0,
            "hover" => 1,
            "pressed" => 2,
            "disabled" => 3,
            "focus" => 4,
            "transition" => 5,
            "width" => 6,
            "height" => 7,
            "x" => 8,
            "y" => 9,
            "radius" => 10,
            "background" => 11,
            "text" => 12,
            "font.size" | "fontSize" => 14,
            "points" => 15,
            "Border" => 16,
            "Reveal" => 17,
            "Text" => 18,
            "PointerArea" => 19,
            "targetX" => 20,
            "targetY" => 21,
            "targetWidth" => 22,
            "targetHeight" => 23,
            "targetRadius" => 24,
            "activeRadius" => 25,
            "stop" => 26,
            "activeStop" => 27,
            "value" => 28,
            "Minimum" => 29,
            "Maximum" => 30,
            "placeholder" => 31,
            "placeholderColor" => 32,
            "multiline" => 33,
            "borderWidth" => 34,
            "hoverBackground" => 35,
            "pressedBackground" => 36,
            "disabledBackground" => 37,
            "borderColor" => 38,
            "focusBorderColor" => 39,
            "transitionDuration" => 40,
            "label" => 41,
            "readOnly" => 42,
            "wrap" => 43,
            _ => return None,
        };
        Some(1u64 << index)
    }

    #[inline]
    fn insert(&mut self, name: &str) -> bool {
        match Self::bit(name) {
            Some(bit) if self.0 & bit != 0 => false,
            Some(bit) => {
                self.0 |= bit;
                true
            }
            None => true,
        }
    }

    #[inline]
    fn contains(&self, name: &str) -> bool {
        Self::bit(name).is_some_and(|bit| self.0 & bit != 0)
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token<'a>,
    props: ButtonSpec,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, props: &'a ButtonSpec) -> Result<Self, String> {
        let mut lexer = Lexer { source, offset: 0 };
        let current = lexer.next()?;
        Ok(Self {
            lexer,
            current,
            props: props.clone(),
        })
    }
    fn advance(&mut self) -> Result<Kind<'a>, String> {
        let next = self.lexer.next()?;
        Ok(std::mem::replace(&mut self.current, next).kind)
    }
    fn error(&self, message: &str) -> String {
        format!("{message} (byte {})", self.current.offset)
    }
    fn expect(&mut self, kind: Kind<'a>, description: &str) -> Result<(), String> {
        if self.current.kind != kind {
            return Err(self.error(&format!(
                "Expected {description}, found {:?}",
                self.current.kind
            )));
        }
        self.advance()?;
        Ok(())
    }
    fn ident(&mut self) -> Result<Cow<'a, str>, String> {
        // The token owns the identifier: move it out instead of duplicating it.
        if let Kind::Ident(value) = &mut self.current.kind {
            let value = std::mem::take(value);
            self.advance()?;
            return Ok(value);
        }
        Err(self.error("Expected a primitive or property name"))
    }
    fn colon(&mut self) -> Result<(), String> {
        self.expect(Kind::Colon, "':'")
    }
    fn semi(&mut self) -> Result<(), String> {
        self.expect(Kind::Semi, "';'")
    }
    fn open(&mut self) -> Result<(), String> {
        self.expect(Kind::Open, "'{'")
    }
    fn close(&mut self) -> Result<(), String> {
        self.expect(Kind::Close, "'}'")
    }
    fn unique(&self, seen: &mut SeenFields, name: &str) -> Result<(), String> {
        if !seen.insert(name) {
            return Err(self.error(&format!("Duplicate template field or primitive '{name}'")));
        }
        Ok(())
    }
    fn scalar(&mut self, name: &str, positive: bool) -> Result<f32, String> {
        let value = match &self.current.kind {
            Kind::Number(value, Unit::Logical | Unit::Px) => *value,
            Kind::Ident(reference) => match reference.text() {
                "props.radius" => self.props.radius,
                "props.width" => self.props.width,
                "props.height" => self.props.height,
                "props.font.size" | "props.fontSize" => self.props.font_size,
                "props.borderWidth" => self.props.numbers.border_width,
                _ => {
                    return Err(self.error(&format!(
                        "Unknown or non-numeric props reference '{reference}' for {name}"
                    )));
                }
            },
            _ => {
                return Err(self.error(&format!(
                    "'{name}' requires a pixel number or numeric props reference"
                )));
            }
        };
        if !value.is_finite() || !(0. ..=4096.).contains(&value) || (positive && value == 0.) {
            return Err(self.error(&format!(
                "'{name}' must be finite, {} and at most 4096",
                if positive { "positive" } else { "non-negative" }
            )));
        }
        self.advance()?;
        Ok(value)
    }
    fn duration(&mut self) -> Result<f32, String> {
        let value = match &self.current.kind {
            Kind::Number(v, Unit::Ms) => *v,
            Kind::Ident(r) if r == "props.transitionDuration" => {
                self.props.numbers.transition_duration
            }
            _ => return Err(self.error("'transition' requires ms or props.transitionDuration")),
        };
        if !(0. ..=2000.).contains(&value) {
            return Err(self.error("'transition' must be between 0ms and 2000ms"));
        }
        self.advance()?;
        Ok(value)
    }
    /// Resolves a `props.<name>` reference to the colour the control declared. Only the
    /// override properties are stored by name, so the two always-present ones are matched here.
    fn props_color(&self, reference: &str) -> Option<[u8; 4]> {
        match reference {
            "props.background" => Some(self.props.background),
            "props.color" => Some(self.props.color),
            _ => self.props.colors.get(reference.strip_prefix("props.")?),
        }
    }
    fn color(&mut self, name: &str) -> Result<[u8; 4], String> {
        let color = match &self.current.kind {
            Kind::Ident(reference) => match self.props_color(reference.text()) {
                Some(color) => color,
                None => {
                    return Err(self.error(&format!(
                        "Unknown or non-color props reference '{reference}' for {name}"
                    )));
                }
            },
            Kind::Hex(raw) => {
                const HEX_LUT: [u8; 256] = {
                    let mut lut = [0xFF; 256];
                    let mut i = 0;
                    while i < 10 {
                        lut[(b'0' + i) as usize] = i;
                        i += 1;
                    }
                    let mut i = 0;
                    while i < 6 {
                        lut[(b'a' + i) as usize] = 10 + i;
                        lut[(b'A' + i) as usize] = 10 + i;
                        i += 1;
                    }
                    lut
                };
                let bytes = raw.as_bytes();
                match bytes.len() {
                    3 => {
                        let r = HEX_LUT[bytes[0] as usize];
                        let g = HEX_LUT[bytes[1] as usize];
                        let b = HEX_LUT[bytes[2] as usize];
                        if (r | g | b) == 0xFF {
                            return Err(self.error("Expected #RGB, #RRGGBB or #RRGGBBAA color"));
                        }
                        [r * 17, g * 17, b * 17, 255]
                    }
                    6 => {
                        let r0 = HEX_LUT[bytes[0] as usize];
                        let r1 = HEX_LUT[bytes[1] as usize];
                        let g0 = HEX_LUT[bytes[2] as usize];
                        let g1 = HEX_LUT[bytes[3] as usize];
                        let b0 = HEX_LUT[bytes[4] as usize];
                        let b1 = HEX_LUT[bytes[5] as usize];
                        if (r0 | r1 | g0 | g1 | b0 | b1) == 0xFF {
                            return Err(self.error("Expected #RGB, #RRGGBB or #RRGGBBAA color"));
                        }
                        [(r0 << 4) | r1, (g0 << 4) | g1, (b0 << 4) | b1, 255]
                    }
                    8 => {
                        let r0 = HEX_LUT[bytes[0] as usize];
                        let r1 = HEX_LUT[bytes[1] as usize];
                        let g0 = HEX_LUT[bytes[2] as usize];
                        let g1 = HEX_LUT[bytes[3] as usize];
                        let b0 = HEX_LUT[bytes[4] as usize];
                        let b1 = HEX_LUT[bytes[5] as usize];
                        let a0 = HEX_LUT[bytes[6] as usize];
                        let a1 = HEX_LUT[bytes[7] as usize];
                        if (r0 | r1 | g0 | g1 | b0 | b1 | a0 | a1) == 0xFF {
                            return Err(self.error("Expected #RGB, #RRGGBB or #RRGGBBAA color"));
                        }
                        [
                            (r0 << 4) | r1,
                            (g0 << 4) | g1,
                            (b0 << 4) | b1,
                            (a0 << 4) | a1,
                        ]
                    }
                    _ => return Err(self.error("Expected #RGB, #RRGGBB or #RRGGBBAA color")),
                }
            }
            _ => {
                return Err(self.error(&format!(
                    "'{name}' requires an unquoted hex color or color props reference"
                )));
            }
        };
        self.advance()?;
        Ok(color)
    }
    fn text_value(&mut self) -> Result<Cow<'a, str>, String> {
        // Quoted strings already live in the token, so the fast path moves them.
        if let Kind::String(value) = &mut self.current.kind {
            let value = std::mem::take(value);
            self.advance()?;
            return Ok(value);
        }
        let value = match &self.current.kind {
            Kind::String(value) => value.clone(),
            Kind::Ident(reference) => match reference.text() {
                "props.text" => Cow::Owned(self.props.text.clone()),
                "props.key" => Cow::Owned(self.props.key.clone()),
                _ => {
                    return Err(self.error(&format!(
                        "Unknown or non-string props reference '{reference}' for text"
                    )));
                }
            },
            _ => {
                return Err(self.error("'text' requires a quoted string or string props reference"));
            }
        };
        self.advance()?;
        Ok(value)
    }
    fn brush(&mut self) -> Result<Brush, String> {
        self.open()?;
        let mut color = None;
        let mut brush = Brush {
            color: [0; 4],
            hover: None,
            pressed: None,
            disabled: None,
            focus: None,
            duration_ms: 0.,
        };
        let mut seen = SeenFields::default();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            if !matches!(
                name.text(),
                "color" | "hover" | "pressed" | "disabled" | "focus" | "transition"
            ) {
                return Err(self.error(&format!("Unsupported Brush field or primitive '{name}'")));
            }
            self.colon()?;
            match name.text() {
                "color" => color = Some(self.color("color")?),
                "hover" => brush.hover = Some(self.color("hover")?),
                "pressed" => brush.pressed = Some(self.color("pressed")?),
                "disabled" => brush.disabled = Some(self.color("disabled")?),
                "focus" => brush.focus = Some(self.color("focus")?),
                "transition" => brush.duration_ms = self.duration()?,
                _ => unreachable!(),
            }
            self.semi()?;
        }
        self.close()?;
        brush.color = color.ok_or_else(|| self.error("Brush requires a base 'color'"))?;
        Ok(brush)
    }
    fn brush_value(&mut self) -> Result<Brush, String> {
        if matches!(&self.current.kind, Kind::Ident(id) if id.text() == "Brush") {
            self.advance()?;
            self.brush()
        } else {
            Ok(Brush {
                color: self.color("brush")?,
                hover: None,
                pressed: None,
                disabled: None,
                focus: None,
                duration_ms: 0.,
            })
        }
    }
    fn border(&mut self) -> Result<Border, String> {
        self.open()?;
        let mut width = 1.;
        let mut brush = None;
        let mut seen = SeenFields::default();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            match name.text() {
                "width" => {
                    self.colon()?;
                    width = self.scalar("width", false)?;
                    self.semi()?;
                }
                "background" => {
                    self.colon()?;
                    brush = Some(self.brush_value()?);
                    self.semi()?;
                }
                _ => {
                    return Err(
                        self.error(&format!("Unsupported Border field or primitive '{name}'"))
                    );
                }
            }
        }
        self.close()?;
        Ok(Border {
            width,
            brush: brush.ok_or_else(|| self.error("Border requires background: Brush { ... };"))?,
        })
    }
    fn reveal(&mut self) -> Result<crate::reveal::Reveal, String> {
        self.open()?;
        let mut reveal = crate::reveal::Reveal::default();
        let mut seen = SeenFields::default();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            self.colon()?;
            match name.text() {
                "targetX" => reveal.target_x = Some(self.scalar("targetX", false)?),
                "targetY" => reveal.target_y = Some(self.scalar("targetY", false)?),
                "targetWidth" => reveal.target_width = Some(self.scalar("targetWidth", true)?),
                "targetHeight" => reveal.target_height = Some(self.scalar("targetHeight", true)?),
                "targetRadius" => reveal.target_radius = Some(self.scalar("targetRadius", false)?),
                "width" => reveal.width = self.scalar("width", false)?,
                "color" => reveal.color = self.color("color")?,
                "radius" => reveal.radius = self.scalar("radius", true)?,
                "activeRadius" => reveal.active_radius = self.scalar("activeRadius", true)?,
                "transition" => reveal.duration_ms = self.duration()?,
                "stop" | "activeStop" => {
                    let value = match self.current.kind {
                        Kind::Number(value, Unit::Logical) if value > 0. && value <= 1. => value,
                        _ => {
                            return Err(self.error(
                                "Reveal stop requires a unitless fraction above 0 and at most 1",
                            ));
                        }
                    };
                    self.advance()?;
                    if name == "stop" {
                        reveal.stop = value;
                    } else {
                        reveal.active_stop = value;
                    }
                }
                _ => return Err(self.error(&format!("Unsupported Reveal field '{name}'"))),
            }
            self.semi()?;
        }
        self.close()?;
        Ok(reveal)
    }
    fn range_input(&mut self) -> Result<RangeInput, String> {
        self.open()?;
        let mut value = 0.;
        let mut minimum = None;
        let mut maximum = None;
        let mut seen = SeenFields::default();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            if name == "value" {
                self.colon()?;
                value = self.scalar("value", false)?;
                self.semi()?;
            } else if name == "Minimum" || name == "Maximum" {
                self.open()?;
                let mut content = Vec::new();
                let mut depth = 0;
                while self.current.kind != Kind::Close {
                    let kind = self.ident()?;
                    if !matches!(
                        kind.text(),
                        "ContentShape" | "ContentText" | "ContentClip" | "ContentClipEnd"
                    ) {
                        return Err(self.error("Expected range visual content"));
                    }
                    if kind == "ContentClip" {
                        depth += 1;
                    }
                    if kind == "ContentClipEnd" {
                        depth -= 1;
                        if depth < 0 {
                            return Err(self.error("Unmatched range clip"));
                        }
                    }
                    if content.len() >= 4096 || depth > 16 {
                        return Err(self.error("Range content limit"));
                    }
                    content.push(self.content(&kind)?);
                }
                self.close()?;
                if depth != 0 {
                    return Err(self.error("Unclosed range clip"));
                }
                if name == "Minimum" {
                    minimum = Some(content);
                } else {
                    maximum = Some(content);
                }
            } else {
                return Err(self.error("Unknown RangeInput property"));
            }
        }
        self.close()?;
        let range = RangeInput {
            value: value.clamp(0., 1.),
            minimum: minimum.ok_or("Missing Minimum")?,
            maximum: maximum.ok_or("Missing Maximum")?,
        };
        if !range.compatible() {
            return Err(self.error("Range endpoints must have matching geometry"));
        }
        Ok(range)
    }
    fn input(&mut self) -> Result<crate::text_input::Input, String> {
        self.open()?;
        let mut input = crate::text_input::Input::default();
        let mut seen = SeenFields::default();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            self.colon()?;
            match name.text() {
                "x" => input.bounds[0] = self.scalar("x", false)?,
                "y" => input.bounds[1] = self.scalar("y", false)?,
                "width" => input.bounds[2] = self.scalar("width", false)?,
                "height" => input.bounds[3] = self.scalar("height", false)?,
                "value" => input.value = self.text_value()?.into_owned(),
                "label" => input.label = self.text_value()?.into_owned(),
                "placeholder" => input.placeholder = self.text_value()?.into_owned(),
                "color" => input.color = self.color("color")?,
                "placeholderColor" => input.placeholder_color = self.color("placeholderColor")?,
                "fontSize" => input.font_size = self.scalar("fontSize", true)?,
                "readOnly" => {
                    let value = self.ident()?;
                    input.read_only = match value.text() {
                        "true" => true,
                        "false" => false,
                        _ => return Err(self.error("Expected boolean")),
                    };
                }
                "multiline" => {
                    let value = self.ident()?;
                    input.multiline = match value.text() {
                        "true" => true,
                        "false" => false,
                        _ => return Err(self.error("Expected boolean")),
                    };
                }
                "wrap" => {
                    let value = self.ident()?;
                    input.wrap = match value.text() {
                        "true" => true,
                        "false" => false,
                        _ => return Err(self.error("Expected boolean")),
                    };
                }
                _ => return Err(self.error("Unknown ContentInput property")),
            }
            self.semi()?;
        }
        self.close()?;
        Ok(input)
    }
    /// `sans` or `mono`, bare or quoted.
    fn font_family(&mut self) -> Result<crate::text::FontFamily, String> {
        let family = match &self.current.kind {
            Kind::Ident(name) => crate::text::FontFamily::from_name(name.text()),
            Kind::String(name) => crate::text::FontFamily::from_name(name),
            _ => None,
        };
        let family = family.ok_or_else(|| self.error("'fontFamily' requires sans or mono"))?;
        self.advance()?;
        Ok(family)
    }
    /// Compiled scene face: [`Self::font_family`] names plus `sans-bold`.
    fn scene_font_family(&mut self) -> Result<crate::text::FontFamily, String> {
        let family = match &self.current.kind {
            Kind::Ident(name) => crate::text::FontFamily::from_scene_name(name.text()),
            Kind::String(name) => crate::text::FontFamily::from_scene_name(name),
            _ => None,
        };
        let family =
            family.ok_or_else(|| self.error("'fontFamily' requires sans, mono or sans-bold"))?;
        self.advance()?;
        Ok(family)
    }
    fn text(&mut self) -> Result<Text, String> {
        self.open()?;
        let mut text = Text {
            text: String::new(),
            color: [0, 0, 0, 255],
            font_size: 16.,
            family: Default::default(),
        };
        let mut seen = SeenFields::default();
        let mut weight = crate::text::FontWeight::Regular;
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            if !matches!(
                name.text(),
                "text"
                    | "color"
                    | "font.size"
                    | "fontSize"
                    | "font.family"
                    | "fontFamily"
                    | "font.weight"
                    | "fontWeight"
                    | "wrap"
            ) {
                return Err(self.error(&format!("Unsupported Text field or primitive '{name}'")));
            }
            self.colon()?;
            match name.text() {
                "text" => text.text = self.text_value()?.into_owned(),
                "color" => text.color = self.color("color")?,
                "font.size" | "fontSize" => text.font_size = self.scalar("fontSize", true)?,
                "font.family" | "fontFamily" => text.family = self.font_family()?,
                "font.weight" | "fontWeight" => {
                    let w = match &self.current.kind {
                        Kind::Ident(name) => crate::text::FontWeight::from_name(name.text()),
                        Kind::String(name) => crate::text::FontWeight::from_name(name),
                        _ => None,
                    };
                    weight = w.ok_or_else(|| {
                        self.error("Unsupported fontWeight: requires regular or bold")
                    })?;
                    self.advance()?;
                }
                "wrap" => {
                    let v = self.ident()?;
                    if !matches!(v.text(), "true" | "false" | "props.wrap") {
                        return Err(self.error("Expected boolean for wrap"));
                    }
                }
                _ => unreachable!(),
            }
            self.semi()?;
        }
        self.close()?;
        text.family = text.family.with_weight(weight);
        Ok(text)
    }
    fn pointer_area(&mut self) -> Result<bool, String> {
        self.open()?;
        let mut clicked = false;
        while self.current.kind != Kind::Close {
            let event = self.ident()?;
            if event != "clicked" {
                return Err(
                    self.error(&format!("Unsupported PointerArea event or field '{event}'"))
                );
            }
            if clicked {
                return Err(self.error("Duplicate PointerArea clicked event"));
            }
            self.expect(Kind::Arrow, "'->' after clicked")?;
            self.expect(
                Kind::Ident(Cow::Borrowed("events.clicked")),
                "events.clicked() forwarding; arbitrary code is not supported",
            )?;
            self.expect(Kind::LeftParen, "'(' after events.clicked")?;
            self.expect(Kind::RightParen, "')'; event arguments are not supported")?;
            self.semi()?;
            clicked = true;
        }
        self.close()?;
        Ok(clicked)
    }
    fn canvas_slot(&mut self) -> Result<CanvasSlot, String> {
        self.open()?;
        let mut slot = CanvasSlot::default();
        let mut seen = SeenFields::default();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            self.colon()?;
            match name.text() {
                "x" => slot.x = self.scalar("x", false)?,
                "y" => slot.y = self.scalar("y", false)?,
                "width" => slot.width = Some(self.scalar("width", false)?),
                "height" => slot.height = Some(self.scalar("height", false)?),
                _ => return Err(self.error(&format!("Unsupported Canvas property {name}"))),
            }
            self.semi()?;
        }
        self.close()?;
        Ok(slot)
    }
    fn link(&mut self) -> Result<TextLink, String> {
        self.open()?;
        let mut link = TextLink {
            bounds: [0.; 4],
            id: String::new(),
        };
        let mut seen = SeenFields::default();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            self.colon()?;
            match name.text() {
                "x" | "y" => {
                    let v = match self.current.kind {
                        Kind::Number(v, Unit::Logical | Unit::Px)
                            if v.is_finite() && v.abs() <= 100_000. =>
                        {
                            v
                        }
                        _ => return Err(self.error("Invalid content coordinate")),
                    };
                    link.bounds[if name == "x" { 0 } else { 1 }] = v;
                    self.advance()?;
                }
                "width" => link.bounds[2] = self.scalar("width", false)?,
                "height" => link.bounds[3] = self.scalar("height", false)?,
                "link" => link.id = self.text_value()?.into_owned(),
                _ => return Err(self.error(&format!("Unsupported ContentLink property {name}"))),
            }
            self.semi()?;
        }
        self.close()?;
        if link.id.is_empty() {
            return Err(self.error("ContentLink requires a link id"));
        }
        Ok(link)
    }
    fn rectangle(&mut self) -> Result<Template, String> {
        self.open()?;
        let mut template = Template {
            props: self.props.clone(),
            radius: 0.,
            fill: None,
            border: None,
            reveal: None,
            text: None,
            clickable: false,
            content: Vec::new(),
            inputs: Vec::new(),
            range: None,
            canvas: None,
            links: Vec::new(),
        };
        let mut seen = SeenFields::default();
        let mut clip_depth = 0;
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            if name == "RangeInput" {
                if template.range.is_some() {
                    return Err(self.error("Duplicate RangeInput"));
                }
                template.range = Some(self.range_input()?);
                continue;
            }
            if name == "Canvas" {
                if template.canvas.is_some() {
                    return Err(self.error("One Canvas per control is supported"));
                }
                template.canvas = Some(self.canvas_slot()?);
                continue;
            }
            if name == "ContentLink" {
                if template.links.len() >= 4096 {
                    return Err(self.error("Too many link regions"));
                }
                template.links.push(self.link()?);
                continue;
            }
            if name == "ContentInput" {
                template.inputs.push(self.input()?);
                if template.inputs.len() > 1 {
                    return Err(self.error("One TextInput per control is supported"));
                }
                continue;
            }
            if name == "ContentText"
                || name == "ContentShape"
                || name == "ContentClip"
                || name == "ContentClipEnd"
            {
                if name == "ContentClip" {
                    clip_depth += 1;
                    if clip_depth > 16 {
                        return Err(self.error("Clip nesting limit is 16"));
                    }
                }
                if name == "ContentClipEnd" {
                    if clip_depth == 0 {
                        return Err(self.error("Unmatched clip end"));
                    }
                    clip_depth -= 1;
                }
                if template.content.len() >= 4096 {
                    return Err(self.error("Too many content primitives"));
                }
                template.content.push(self.content(&name)?);
                continue;
            }
            self.unique(&mut seen, &name)?;
            match name.text() {
                "radius" => {
                    self.colon()?;
                    template.radius = self.scalar("radius", false)?;
                    self.semi()?;
                }
                "background" => {
                    self.colon()?;
                    template.fill = Some(self.brush_value()?);
                    self.semi()?;
                }
                "Border" => template.border = Some(self.border()?),
                "Reveal" => template.reveal = Some(self.reveal()?),
                "Text" => template.text = Some(self.text()?),
                "PointerArea" => template.clickable = self.pointer_area()?,
                _ => {
                    return Err(self.error(&format!(
                        "Unsupported Rectangle field or primitive '{name}'"
                    )));
                }
            }
        }
        self.close()?;
        if clip_depth != 0 {
            return Err(self.error("Unclosed content clip"));
        }
        Ok(template)
    }
    fn content(&mut self, kind: &str) -> Result<Content, String> {
        self.open()?;
        if kind == "ContentClipEnd" {
            self.close()?;
            return Ok(Content::ClipEnd);
        }
        let mut bounds = [0.; 4];
        let mut text = Text {
            text: String::new(),
            color: [255; 4],
            font_size: 16.,
            family: Default::default(),
        };
        let mut points = None;
        let mut radius = 0.;
        let mut seen = SeenFields::default();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            self.colon()?;
            match (kind, name.text()) {
                ("ContentText" | "ContentClip", "x" | "y") => {
                    let v = match self.current.kind {
                        Kind::Number(v, Unit::Logical | Unit::Px)
                            if v.is_finite() && v.abs() <= 100_000. =>
                        {
                            v
                        }
                        _ => return Err(self.error("Invalid content coordinate")),
                    };
                    bounds[if name == "x" { 0 } else { 1 }] = v;
                    self.advance()?;
                }
                ("ContentText" | "ContentClip", "width" | "height") => {
                    bounds[if name == "width" { 2 } else { 3 }] = self.scalar(&name, false)?
                }
                ("ContentClip", "radius") => radius = self.scalar(&name, false)?,
                ("ContentText", "text") => text.text = self.text_value()?.into_owned(),
                ("ContentText", "fontSize") => text.font_size = self.scalar(&name, true)?,
                ("ContentText", "fontFamily") => text.family = self.scene_font_family()?,
                (_, "color") => text.color = self.color("color")?,
                ("ContentShape", "points") => {
                    // One pass straight into pairs: no intermediate scalar vector.
                    let raw = self.text_value()?;
                    // Separation bytes only size the allocation; the cap keeps a
                    // hostile literal from reserving more than the limit needs.
                    let separations = raw
                        .as_bytes()
                        .iter()
                        .filter(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
                        .count();
                    let mut pairs: Vec<[f32; 2]> =
                        Vec::with_capacity((separations / 2 + 2).min(4096));
                    let mut words = raw.split_whitespace();
                    while let Some(word) = words.next() {
                        let next = words
                            .next()
                            .ok_or_else(|| self.error("Invalid polygon points"))?;
                        let a = word
                            .parse::<f32>()
                            .map_err(|_| self.error("Invalid polygon points"))?;
                        let b = next
                            .parse::<f32>()
                            .map_err(|_| self.error("Invalid polygon points"))?;
                        if !a.is_finite()
                            || !b.is_finite()
                            || a.abs() > 100_000.
                            || b.abs() > 100_000.
                        {
                            return Err(self.error("Invalid polygon points"));
                        }
                        pairs.push([a, b]);
                        if pairs.len() > 4096 {
                            return Err(self.error("Invalid polygon points"));
                        }
                    }
                    if pairs.len() < 3 {
                        return Err(self.error("Invalid polygon points"));
                    }
                    points = Some(pairs);
                }
                _ => return Err(self.error(&format!("Unsupported {kind} property {name}"))),
            }
            self.semi()?;
        }
        self.close()?;
        if kind == "ContentShape" {
            Ok(Content::Shape {
                points: points.ok_or_else(|| self.error("Shape requires points"))?,
                color: text.color,
            })
        } else {
            if !seen.contains("width") || !seen.contains("height") {
                return Err(self.error("Content requires width and height"));
            }
            if kind == "ContentClip" {
                Ok(Content::Clip { bounds, radius })
            } else {
                Ok(Content::Text { bounds, text })
            }
        }
    }
    fn template(&mut self) -> Result<Template, String> {
        self.expect(Kind::Ident(Cow::Borrowed("component")), "'component'")?;
        self.expect(
            Kind::Ident(Cow::Borrowed("Button")),
            "Button component name",
        )?;
        self.open()?;
        let mut seen = SeenFields::default();
        while !matches!(&self.current.kind, Kind::Ident(id) if id.text() == "Rectangle") {
            let name = self.ident()?;
            let name = if name.text() == "font.size" {
                Cow::Borrowed("fontSize")
            } else {
                name
            };
            self.unique(&mut seen, &name)?;
            self.colon()?;
            // Defaults are literals: no ordering-dependent references or cycles.
            if matches!(&self.current.kind,Kind::Ident(r) if r.starts_with("props.")) {
                return Err(self.error("Component defaults must be literals"));
            }
            let apply = !self.props.specified.contains(name.text());
            match name.text() {
                "width" | "height" | "radius" | "fontSize" | "borderWidth" => {
                    let v = self.scalar(
                        &name,
                        matches!(name.text(), "width" | "height" | "fontSize"),
                    )?;
                    if apply {
                        match name.text() {
                            "width" => self.props.width = v,
                            "height" => self.props.height = v,
                            "radius" => self.props.radius = v,
                            "fontSize" => self.props.font_size = v,
                            _ => {
                                self.props.numbers.set(name.text(), v);
                            }
                        }
                    }
                }
                "background" | "color" | "hoverBackground" | "pressedBackground"
                | "disabledBackground" | "borderColor" | "focusBorderColor" => {
                    let v = self.color(&name)?;
                    if apply {
                        match name.text() {
                            "background" => self.props.background = v,
                            "color" => self.props.color = v,
                            _ => {
                                self.props.colors.set(name.text(), v);
                            }
                        }
                    }
                }
                "text" => {
                    let v = self.text_value()?;
                    if apply {
                        self.props.text = v.into_owned();
                    }
                }
                "disabled" => {
                    let v = match &self.current.kind {
                        Kind::Ident(v) if v == "true" => true,
                        Kind::Ident(v) if v == "false" => false,
                        _ => return Err(self.error("disabled requires true or false")),
                    };
                    self.advance()?;
                    if apply {
                        self.props.disabled = v;
                    }
                }
                "transitionDuration" => {
                    let v = self.duration()?;
                    if apply {
                        self.props.numbers.transition_duration = v;
                    }
                }
                _ => return Err(self.error(&format!("Unsupported component property '{name}'"))),
            }
            self.semi()?;
        }
        self.expect(
            Kind::Ident(Cow::Borrowed("Rectangle")),
            "exactly one Rectangle root",
        )?;
        let template = self.rectangle()?;
        self.close()?;
        if self.current.kind != Kind::Eof {
            return Err(self.error("Unexpected content after Button template"));
        }
        Ok(template)
    }
}

pub fn parse(source: &str, props: &ButtonSpec) -> Result<Template, String> {
    Parser::new(source, props)?.template()
}

// A keystroke rewrites the layout text of one control, so every other control of the
// document is re-derived from identical inputs. Parsing costs about 19x what cloning a
// finished Template costs (see `examples/template_load_perf.rs`), which is what this
// cache trades memory for.
pub const MAX_PARSED_TEMPLATES: usize = 1_024;
pub const MAX_PARSED_BYTES: usize = 16 * 1024 * 1024;

struct ParsedTemplate {
    id: u64,
    source: String,
    props: ButtonSpec,
    template: Arc<Template>,
}

/// Thread-local and process-wide rather than owned by `Runtime`, because the preview
/// builds a new `Runtime` for every keystroke. The hash only selects candidates: reuse
/// still requires the whole template text and the whole spec to compare equal, so a
/// short hash is never the sole proof of equality (`CACHE_POLICY.md`). Controls of one
/// document share their template text, so one hash can hold many specs.
struct TemplateCache {
    by_hash: HashMap<u64, Vec<ParsedTemplate>>,
    /// Recency and limits per template id; the value is the hash that holds the template.
    lru: WeightedLru<u64, u64>,
    next_id: u64,
    hits: u64,
    misses: u64,
}

impl TemplateCache {
    fn new() -> Self {
        Self {
            by_hash: HashMap::new(),
            lru: WeightedLru::with_limits(CacheLimits {
                max_bytes: MAX_PARSED_BYTES as u64,
                max_entries: Some(MAX_PARSED_TEMPLATES),
                ttl: None,
            }),
            next_id: 0,
            hits: 0,
            misses: 0,
        }
    }

    fn get(&mut self, hash: u64, source: &str, props: &ButtonSpec) -> Option<Arc<Template>> {
        let entry = self
            .by_hash
            .get(&hash)?
            .iter()
            .find(|entry| entry.source == source && &entry.props == props)?;
        self.lru.get(&entry.id);
        Some(Arc::clone(&entry.template))
    }

    /// Caches a parsed template, evicting least recently used templates first. One that
    /// does not fit the budget even when the cache is empty is used without caching.
    fn insert(&mut self, hash: u64, source: &str, props: &ButtonSpec, template: &Arc<Template>) {
        let id = self.next_id;
        self.next_id += 1;
        let weight = entry_weight(source, template) as u64;
        let (admitted, evicted) = self.lru.insert_prefetch_with_evictions(id, hash, weight);
        for (victim, victim_hash) in evicted {
            if let Some(entries) = self.by_hash.get_mut(&victim_hash) {
                entries.retain(|entry| entry.id != victim);
                if entries.is_empty() {
                    self.by_hash.remove(&victim_hash);
                }
            }
        }
        if admitted {
            self.by_hash.entry(hash).or_default().push(ParsedTemplate {
                id,
                source: source.to_owned(),
                props: props.clone(),
                template: Arc::clone(template),
            });
        }
    }

    fn clear(&mut self) {
        self.lru.clear();
        self.by_hash.clear();
    }
}

thread_local! {
    static TEMPLATE_CACHE: RefCell<TemplateCache> = RefCell::new(TemplateCache::new());
}

fn key_hash(source: &str) -> u64 {
    // Full-text coverage folded with adds and rotates: cheap enough to pay on every
    // load, and a collision costs one rejected comparison rather than a wrong template.
    const MIX: u64 = 0x9E37_79B9_7F4A_7C15;
    let bytes = source.as_bytes();
    let mut words = bytes.chunks_exact(16);
    let (mut a, mut b) = (bytes.len() as u64, MIX);
    for word in &mut words {
        a = a.wrapping_add(u64::from_le_bytes(word[..8].try_into().unwrap())) ^ b.rotate_left(11);
        b = b.wrapping_add(u64::from_le_bytes(word[8..].try_into().unwrap())) ^ a.rotate_left(31);
    }
    for byte in words.remainder() {
        a = a.wrapping_add(*byte as u64);
    }
    let mut h = (a ^ b.rotate_left(23)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 31;
    h.wrapping_mul(0x94D0_49BB_1331_11EB)
}

fn content_weight(items: &Vec<Content>) -> usize {
    items.capacity() * std::mem::size_of::<Content>()
        + items
            .iter()
            .map(|item| match item {
                Content::Shape { points, .. } => points.capacity() * 8,
                Content::Text { text, .. } => text.text.len(),
                _ => 0,
            })
            .sum::<usize>()
}

/// Declared weight, matching the JS cache convention: predictable and monotonic in the
/// template size, not an exact heap measurement.
fn entry_weight(source: &str, template: &Template) -> usize {
    let props = &template.props;
    source.len()
        + std::mem::size_of::<Template>()
        + props.text.len()
        + props.key.len()
        + content_weight(&template.content)
        + template.range.as_ref().map_or(0, |range| {
            content_weight(&range.minimum) + content_weight(&range.maximum)
        })
        + template.inputs.len() * 64
        + template.text.as_ref().map_or(0, |text| text.text.len())
}

/// `parse` with the process-wide cache in front of it. A hit shares one reference-counted
/// template with every other caller, which is the whole point: handing out a deep clone
/// cost almost as much as the parse it replaced. A caller that wants to edit takes its own
/// copy first with `Arc::make_mut`, so the shared entry can never be poisoned.
pub fn parse_cached(source: &str, props: &ButtonSpec) -> Result<Arc<Template>, String> {
    let hash = key_hash(source);
    TEMPLATE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(template) = cache.get(hash, source, props) {
            cache.hits += 1;
            return Ok(template);
        }
        let template = Arc::new(parse(source, props)?);
        cache.misses += 1;
        cache.insert(hash, source, props, &template);
        Ok(template)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseCacheStats {
    pub entries: usize,
    pub bytes: usize,
    pub hits: u64,
    pub misses: u64,
}

pub fn parse_cache_stats() -> ParseCacheStats {
    TEMPLATE_CACHE.with(|cache| {
        let cache = cache.borrow();
        ParseCacheStats {
            entries: cache.lru.len(),
            bytes: cache.lru.resident_weight() as usize,
            hits: cache.hits,
            misses: cache.misses,
        }
    })
}

pub fn flush_parse_cache() {
    TEMPLATE_CACHE.with(|cache| {
        cache.borrow_mut().clear();
    });
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq)]
pub struct RangeInput {
    pub value: f32,
    pub minimum: Vec<Content>,
    pub maximum: Vec<Content>,
}
impl RangeInput {
    fn compatible(&self) -> bool {
        self.minimum.len() == self.maximum.len()
            && self
                .minimum
                .iter()
                .zip(&self.maximum)
                .all(|(a, b)| match (a, b) {
                    (Content::Shape { points: a, .. }, Content::Shape { points: b, .. }) => {
                        a.len() == b.len()
                    }
                    (Content::Clip { .. }, Content::Clip { .. })
                    | (Content::ClipEnd, Content::ClipEnd)
                    | (Content::Text { .. }, Content::Text { .. }) => true,
                    _ => false,
                })
    }
    pub fn content(&self) -> Vec<Content> {
        let mix = |a: f32, b: f32| a + (b - a) * self.value;
        self.minimum
            .iter()
            .zip(&self.maximum)
            .map(|(a, b)| match (a, b) {
                (Content::Shape { points: a, color }, Content::Shape { points: b, .. }) => {
                    Content::Shape {
                        points: a
                            .iter()
                            .zip(b)
                            .map(|(a, b)| [mix(a[0], b[0]), mix(a[1], b[1])])
                            .collect(),
                        color: *color,
                    }
                }
                (Content::Clip { bounds: a, radius }, Content::Clip { bounds: b, .. }) => {
                    Content::Clip {
                        bounds: std::array::from_fn(|i| mix(a[i], b[i])),
                        radius: *radius,
                    }
                }
                (Content::Text { bounds: a, text }, Content::Text { bounds: b, .. }) => {
                    Content::Text {
                        bounds: std::array::from_fn(|i| mix(a[i], b[i])),
                        text: text.clone(),
                    }
                }
                _ => a.clone(),
            })
            .collect()
    }
}
