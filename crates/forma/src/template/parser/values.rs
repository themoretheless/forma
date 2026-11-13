//! Scalar values: numbers, durations, colours, strings and font families.

use super::*;

impl<'a> Parser<'a> {
    pub(in crate::template) fn scalar(
        &mut self,
        name: &str,
        positive: bool,
    ) -> Result<f32, String> {
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

    pub(in crate::template) fn duration(&mut self) -> Result<f32, String> {
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
    pub(in crate::template) fn props_color(&self, reference: &str) -> Option<[u8; 4]> {
        match reference {
            "props.background" => Some(self.props.background),
            "props.color" => Some(self.props.color),
            _ => self.props.colors.get(reference.strip_prefix("props.")?),
        }
    }

    pub(in crate::template) fn color(&mut self, name: &str) -> Result<[u8; 4], String> {
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

    pub(in crate::template) fn text_value(&mut self) -> Result<Cow<'a, str>, String> {
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

    /// `sans` or `mono`, bare or quoted.
    pub(in crate::template) fn font_family(&mut self) -> Result<crate::text::FontFamily, String> {
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
    pub(in crate::template) fn scene_font_family(
        &mut self,
    ) -> Result<crate::text::FontFamily, String> {
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
}
