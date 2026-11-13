//! Brushes, borders and reveal effects.

use super::*;

impl<'a> Parser<'a> {
    pub(in crate::template) fn brush(&mut self) -> Result<Brush, String> {
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

    pub(in crate::template) fn brush_value(&mut self) -> Result<Brush, String> {
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

    pub(in crate::template) fn border(&mut self) -> Result<Border, String> {
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

    pub(in crate::template) fn reveal(&mut self) -> Result<crate::reveal::Reveal, String> {
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
}
