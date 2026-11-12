//! Parsing a template source into a [`Template`].

use super::*;

/// Bitset of fields seen in the current template block. Every block accepts a small,
/// closed vocabulary of property and primitive names, so tracking duplicates in a `u64`
/// avoids allocating and dropping a `HashSet` for every shape, clip, brush, and label.
#[derive(Clone, Copy, Default)]
pub(super) struct SeenFields(u64);

impl SeenFields {
    #[inline]
    pub(super) fn bit(name: &str) -> Option<u64> {
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
    pub(super) fn insert(&mut self, name: &str) -> bool {
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
    pub(super) fn contains(&self, name: &str) -> bool {
        Self::bit(name).is_some_and(|bit| self.0 & bit != 0)
    }
}

pub(super) struct Parser<'a> {
    pub(super) lexer: Lexer<'a>,
    pub(super) current: Token<'a>,
    pub(super) props: ButtonSpec,
}

impl<'a> Parser<'a> {
    pub(super) fn new(source: &'a str, props: &'a ButtonSpec) -> Result<Self, String> {
        let mut lexer = Lexer { source, offset: 0 };
        let current = lexer.next()?;
        Ok(Self {
            lexer,
            current,
            props: props.clone(),
        })
    }
    pub(super) fn advance(&mut self) -> Result<Kind<'a>, String> {
        let next = self.lexer.next()?;
        Ok(std::mem::replace(&mut self.current, next).kind)
    }
    pub(super) fn error(&self, message: &str) -> String {
        format!("{message} (byte {})", self.current.offset)
    }
    pub(super) fn expect(&mut self, kind: Kind<'a>, description: &str) -> Result<(), String> {
        if self.current.kind != kind {
            return Err(self.error(&format!(
                "Expected {description}, found {:?}",
                self.current.kind
            )));
        }
        self.advance()?;
        Ok(())
    }
    pub(super) fn ident(&mut self) -> Result<Cow<'a, str>, String> {
        // The token owns the identifier: move it out instead of duplicating it.
        if let Kind::Ident(value) = &mut self.current.kind {
            let value = std::mem::take(value);
            self.advance()?;
            return Ok(value);
        }
        Err(self.error("Expected a primitive or property name"))
    }
    pub(super) fn colon(&mut self) -> Result<(), String> {
        self.expect(Kind::Colon, "':'")
    }
    pub(super) fn semi(&mut self) -> Result<(), String> {
        self.expect(Kind::Semi, "';'")
    }
    pub(super) fn open(&mut self) -> Result<(), String> {
        self.expect(Kind::Open, "'{'")
    }
    pub(super) fn close(&mut self) -> Result<(), String> {
        self.expect(Kind::Close, "'}'")
    }
    pub(super) fn unique(&self, seen: &mut SeenFields, name: &str) -> Result<(), String> {
        if !seen.insert(name) {
            return Err(self.error(&format!("Duplicate template field or primitive '{name}'")));
        }
        Ok(())
    }
    pub(super) fn scalar(&mut self, name: &str, positive: bool) -> Result<f32, String> {
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
    pub(super) fn duration(&mut self) -> Result<f32, String> {
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
    pub(super) fn props_color(&self, reference: &str) -> Option<[u8; 4]> {
        match reference {
            "props.background" => Some(self.props.background),
            "props.color" => Some(self.props.color),
            _ => self.props.colors.get(reference.strip_prefix("props.")?),
        }
    }
    pub(super) fn color(&mut self, name: &str) -> Result<[u8; 4], String> {
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
    pub(super) fn text_value(&mut self) -> Result<Cow<'a, str>, String> {
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
    pub(super) fn brush(&mut self) -> Result<Brush, String> {
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
    pub(super) fn brush_value(&mut self) -> Result<Brush, String> {
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
    pub(super) fn border(&mut self) -> Result<Border, String> {
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
    pub(super) fn reveal(&mut self) -> Result<crate::reveal::Reveal, String> {
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
    pub(super) fn range_input(&mut self) -> Result<RangeInput, String> {
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
    pub(super) fn input(&mut self) -> Result<Input, String> {
        self.open()?;
        let mut input = Input::default();
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
    pub(super) fn font_family(&mut self) -> Result<crate::text::FontFamily, String> {
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
    pub(super) fn scene_font_family(&mut self) -> Result<crate::text::FontFamily, String> {
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
    pub(super) fn text(&mut self) -> Result<Text, String> {
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
    pub(super) fn pointer_area(&mut self) -> Result<bool, String> {
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
    pub(super) fn canvas_slot(&mut self) -> Result<CanvasSlot, String> {
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
    pub(super) fn link(&mut self) -> Result<TextLink, String> {
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
    pub(super) fn rectangle(&mut self) -> Result<Template, String> {
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
    pub(super) fn content(&mut self, kind: &str) -> Result<Content, String> {
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
    pub(super) fn template(&mut self) -> Result<Template, String> {
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
