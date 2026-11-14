//! What a parsed template consists of.

use super::*;

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
    pub inputs: Vec<Input>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub bounds: [f32; 4],
    pub value: String,
    pub placeholder: String,
    pub label: String,
    pub read_only: bool,
    pub color: [u8; 4],
    pub placeholder_color: [u8; 4],
    pub font_size: f32,
    pub multiline: bool,
    pub wrap: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            bounds: [0., 0., 100., 30.],
            value: String::new(),
            placeholder: String::new(),
            label: String::new(),
            read_only: false,
            color: [255; 4],
            placeholder_color: [150, 150, 150, 255],
            font_size: 15.,
            multiline: false,
            wrap: false,
        }
    }
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

#[derive(Debug, Clone, PartialEq)]
pub struct RangeInput {
    pub value: f32,
    /// The step grid as a fraction of the scale; 0 when the control moves freely.
    pub step: f32,
    /// The ends of the scale the application counts in; 0 and 1 when it gives none. The control
    /// itself works with `value` as a fraction, these are what a reader of the value is told.
    pub min: f32,
    pub max: f32,
    pub minimum: Vec<Content>,
    pub maximum: Vec<Content>,
}

impl RangeInput {
    /// Moves `fraction` to the step grid, which is anchored at the start of the scale.
    pub fn snapped(&self, fraction: f32) -> f32 {
        let fraction = fraction.clamp(0., 1.);
        if self.step > 0. && fraction > 0. && fraction < 1. {
            ((fraction / self.step).round() * self.step).clamp(0., 1.)
        } else {
            fraction
        }
    }

    /// Whether the application gave a scale of its own.
    pub fn has_scale(&self) -> bool {
        self.min != 0. || self.max != 1.
    }

    /// The current value on the application's scale.
    pub fn scaled_value(&self) -> f32 {
        self.min + self.value * (self.max - self.min)
    }

    /// The fraction at which `scaled` lies on the application's scale.
    pub fn fraction_of(&self, scaled: f32) -> f32 {
        ((scaled - self.min) / (self.max - self.min)).clamp(0., 1.)
    }

    /// The distance of one arrow key press and of one page key press.
    pub fn key_steps(&self) -> (f32, f32) {
        if self.step > 0. {
            (self.step, self.step.max(0.1))
        } else {
            (0.01, 0.1)
        }
    }

    pub(super) fn compatible(&self) -> bool {
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
