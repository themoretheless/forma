//! The parts a template is built from: inputs, text, pointer areas, canvas slots and links.

use super::*;

impl<'a> Parser<'a> {
    pub(in crate::template) fn range_input(&mut self) -> Result<RangeInput, String> {
        self.open()?;
        let mut value = 0.;
        let mut step = 0.;
        let (mut min, mut max) = (0., 1.);
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
            } else if name == "step" {
                self.colon()?;
                step = self.scalar("step", false)?;
                self.semi()?;
            } else if name == "min" {
                self.colon()?;
                min = self.scalar("min", false)?;
                self.semi()?;
            } else if name == "max" {
                self.colon()?;
                max = self.scalar("max", false)?;
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
        let scaled = min.is_finite() && max.is_finite() && min < max && (max - min).is_finite();
        let range = RangeInput {
            value: value.clamp(0., 1.),
            step: if step.is_finite() {
                step.clamp(0., 1.)
            } else {
                0.
            },
            min: if scaled { min } else { 0. },
            max: if scaled { max } else { 1. },
            minimum: minimum.ok_or("Missing Minimum")?,
            maximum: maximum.ok_or("Missing Maximum")?,
        };
        if !range.compatible() {
            return Err(self.error("Range endpoints must have matching geometry"));
        }
        Ok(range)
    }

    pub(in crate::template) fn input(&mut self) -> Result<Input, String> {
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
                "direction" | "dir" => input.direction = self.text_direction()?,
                _ => return Err(self.error("Unknown ContentInput property")),
            }
            self.semi()?;
        }
        self.close()?;
        Ok(input)
    }

    pub(in crate::template) fn text(&mut self) -> Result<Text, String> {
        self.open()?;
        let mut text = Text {
            text: String::new(),
            color: [0, 0, 0, 255],
            font_size: 16.,
            family: Default::default(),
            direction: Default::default(),
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
                "direction" | "dir" => text.direction = self.text_direction()?,
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

    pub(in crate::template) fn pointer_area(&mut self) -> Result<bool, String> {
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

    pub(in crate::template) fn canvas_slot(&mut self) -> Result<CanvasSlot, String> {
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

    pub(in crate::template) fn link(&mut self) -> Result<TextLink, String> {
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
}
