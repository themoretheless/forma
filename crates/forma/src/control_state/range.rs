//! Slider and stepper values on a step grid.

use super::*;

/// Slider/stepper value. Steps are anchored at min; both endpoints are reachable
/// even when max is not on the step grid. Mutations return whether value changed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeModel {
    min: f64,
    max: f64,
    step: f64,
    value: f64,
}
impl RangeModel {
    pub fn new(min: f64, max: f64, step: f64, value: f64) -> Result<Self, ModelError> {
        if !min.is_finite()
            || !max.is_finite()
            || min > max
            || !step.is_finite()
            || step <= 0.0
            || !(max - min).is_finite()
            || !((max - min) / step).is_finite()
        {
            return Err(ModelError::InvalidRange);
        }
        let mut model = Self {
            min,
            max,
            step,
            value: min,
        };
        model.set_value(value)?;
        Ok(model)
    }
    pub fn min(&self) -> f64 {
        self.min
    }
    pub fn max(&self) -> f64 {
        self.max
    }
    pub fn step(&self) -> f64 {
        self.step
    }
    pub fn value(&self) -> f64 {
        self.value
    }
    pub fn fraction(&self) -> f64 {
        if self.min == self.max {
            0.0
        } else {
            (self.value - self.min) / (self.max - self.min)
        }
    }
    pub fn set_value(&mut self, value: f64) -> Result<bool, ModelError> {
        if !value.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        let next = if value <= self.min {
            self.min
        } else if value >= self.max {
            self.max
        } else {
            (self.min + ((value - self.min) / self.step).round() * self.step)
                .clamp(self.min, self.max)
        };
        let changed = self.value != next;
        self.value = next;
        Ok(changed)
    }
    pub fn set_fraction(&mut self, fraction: f64) -> Result<bool, ModelError> {
        if !fraction.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        self.set_value(self.min + (self.max - self.min) * fraction.clamp(0.0, 1.0))
    }
    pub fn step_by(&mut self, steps: i64) -> bool {
        if steps == 0 || self.min == self.max {
            return false;
        }
        let position = (self.value - self.min) / self.step;
        // Avoid skipping a grid value because 0.3 / 0.1 is just below 3.
        let nearest = position.round();
        let position = if (position - nearest).abs() <= 8.0 * f64::EPSILON * position.abs().max(1.0)
        {
            nearest
        } else {
            position
        };
        let origin = if steps > 0 {
            position.floor()
        } else {
            position.ceil()
        };
        let index = origin + steps as f64;
        let target = if index <= 0.0 {
            self.min
        } else if index >= (self.max - self.min) / self.step {
            self.max
        } else {
            self.min + index * self.step
        };
        self.set_value(target)
            .expect("validated range produces finite values")
    }
}
