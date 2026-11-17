//! Parsing a template source into a [`Template`].

use super::*;

mod paint;
mod parts;
mod values;

/// Bitset of fields seen in the current template block. Every block accepts a small,
/// closed vocabulary of property and primitive names, so tracking duplicates in a `u64`
/// avoids allocating and dropping a `HashSet` for every shape, clip, brush, and label.
#[derive(Clone, Copy, Default)]
pub(in crate::template) struct SeenFields(u64);

impl SeenFields {
    #[inline]
    pub(in crate::template) fn bit(name: &str) -> Option<u64> {
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
            "font.family" | "fontFamily" => 13,
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
            "step" => 44,
            "min" => 45,
            "max" => 46,
            "font.weight" | "fontWeight" => 47,
            "link" => 48,
            _ => return None,
        };
        Some(1u64 << index)
    }

    #[inline]
    pub(in crate::template) fn insert(&mut self, name: &str) -> bool {
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
    pub(in crate::template) fn contains(&self, name: &str) -> bool {
        Self::bit(name).is_some_and(|bit| self.0 & bit != 0)
    }
}

pub(in crate::template) struct Parser<'a> {
    pub(super) lexer: Lexer<'a>,
    pub(super) current: Token<'a>,
    pub(super) props: ButtonSpec,
}

impl<'a> Parser<'a> {
    pub(in crate::template) fn new(source: &'a str, props: &'a ButtonSpec) -> Result<Self, String> {
        let mut lexer = Lexer { source, offset: 0 };
        let current = lexer.next()?;
        Ok(Self {
            lexer,
            current,
            props: props.clone(),
        })
    }

    pub(in crate::template) fn advance(&mut self) -> Result<Kind<'a>, String> {
        let next = self.lexer.next()?;
        Ok(std::mem::replace(&mut self.current, next).kind)
    }

    pub(in crate::template) fn error(&self, message: &str) -> String {
        format!("{message} (byte {})", self.current.offset)
    }

    pub(in crate::template) fn expect(
        &mut self,
        kind: Kind<'a>,
        description: &str,
    ) -> Result<(), String> {
        if self.current.kind != kind {
            return Err(self.error(&format!(
                "Expected {description}, found {:?}",
                self.current.kind
            )));
        }
        self.advance()?;
        Ok(())
    }

    pub(in crate::template) fn ident(&mut self) -> Result<Cow<'a, str>, String> {
        // The token owns the identifier: move it out instead of duplicating it.
        if let Kind::Ident(value) = &mut self.current.kind {
            let value = std::mem::take(value);
            self.advance()?;
            return Ok(value);
        }
        Err(self.error("Expected a primitive or property name"))
    }

    pub(in crate::template) fn colon(&mut self) -> Result<(), String> {
        self.expect(Kind::Colon, "':'")
    }

    pub(in crate::template) fn semi(&mut self) -> Result<(), String> {
        self.expect(Kind::Semi, "';'")
    }

    pub(in crate::template) fn open(&mut self) -> Result<(), String> {
        self.expect(Kind::Open, "'{'")
    }

    pub(in crate::template) fn close(&mut self) -> Result<(), String> {
        self.expect(Kind::Close, "'}'")
    }

    pub(in crate::template) fn unique(
        &self,
        seen: &mut SeenFields,
        name: &str,
    ) -> Result<(), String> {
        if !seen.insert(name) {
            return Err(self.error(&format!("Duplicate template field or primitive '{name}'")));
        }
        Ok(())
    }

    pub(in crate::template) fn rectangle(&mut self) -> Result<Template, String> {
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

    pub(in crate::template) fn content(&mut self, kind: &str) -> Result<Content, String> {
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

    pub(in crate::template) fn template(&mut self) -> Result<Template, String> {
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
