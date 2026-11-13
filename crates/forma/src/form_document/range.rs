//! The scale of a range control: a bound number lies between `min` and `max`, optionally on a
//! `step` grid, while the widget itself works with a fraction from 0 to 1.

use super::Value;
use crate::control_state::RangeModel;

/// `min`, `max` and `step` of a range control. Without them the scale is 0 to 1 with no grid,
/// which makes the bound number the fraction itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RangeBounds {
    min: f64,
    max: f64,
    step: f64,
}

impl RangeBounds {
    /// Reads the bounds from evaluated properties; anything missing, non-numeric or inconsistent
    /// falls back to the default scale.
    pub(super) fn of<'a>(property: impl Fn(&str) -> Option<&'a Value>) -> Self {
        let number = |name: &str, default: f64| match property(name) {
            Some(Value::Number(value)) if value.is_finite() => *value,
            _ => default,
        };
        let (min, max, step) = (number("min", 0.), number("max", 1.), number("step", 0.));
        if min < max && (max - min).is_finite() {
            Self {
                min,
                max,
                step: step.max(0.),
            }
        } else {
            Self {
                min: 0.,
                max: 1.,
                step: 0.,
            }
        }
    }

    /// Where `value` lies on the scale, from 0 to 1.
    pub(super) fn fraction(self, value: f64) -> f64 {
        ((value - self.min) / (self.max - self.min)).clamp(0., 1.)
    }

    /// The value at `fraction` of the scale, moved to the step grid when there is one.
    pub(super) fn value(self, fraction: f64) -> f64 {
        let value = self.min + fraction.clamp(0., 1.) * (self.max - self.min);
        if self.step > 0. {
            if let Ok(model) = RangeModel::new(self.min, self.max, self.step, value) {
                return model.value();
            }
        }
        value
    }
}
