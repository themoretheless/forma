//! Validation for directly constructed [`crate::markup::Scene`] and [`Template`] values.

use super::{Border, Brush, CanvasSlot, Content, Input, RangeInput, Template, TextLink};
use crate::markup::{ButtonSpec, ModalSpec, Scene};
use std::collections::HashSet;

fn check_finite_non_neg(value: f32, name: &str, positive: bool) -> Result<(), String> {
    if !value.is_finite() || value < 0. || (positive && value == 0.) {
        return Err(format!(
            "'{name}' must be {}",
            if positive { "positive" } else { "non-negative" }
        ));
    }
    Ok(())
}

fn check_scalar_4096(value: f32, name: &str, positive: bool) -> Result<(), String> {
    if !value.is_finite() || !(0. ..=4096.).contains(&value) || (positive && value == 0.) {
        return Err(format!(
            "'{name}' must be finite, {} and at most 4096",
            if positive { "positive" } else { "non-negative" }
        ));
    }
    Ok(())
}

fn check_duration(value: f32, name: &str) -> Result<(), String> {
    if !value.is_finite() || !(0. ..=2000.).contains(&value) {
        return Err(format!("'{name}' must be between 0ms and 2000ms"));
    }
    Ok(())
}

fn check_content_list(content: &[Content], range_mode: bool) -> Result<(), String> {
    if content.len() > 4096 {
        return Err("Too many content primitives".into());
    }
    let mut clip_depth = 0usize;
    for item in content {
        match item {
            Content::Clip { bounds, radius } => {
                clip_depth += 1;
                if clip_depth > 16 {
                    return Err("Clip nesting limit is 16".into());
                }
                if !bounds[0].is_finite()
                    || !bounds[1].is_finite()
                    || bounds[0].abs() > 100_000.
                    || bounds[1].abs() > 100_000.
                {
                    return Err("Invalid content coordinate".into());
                }
                check_scalar_4096(bounds[2], "width", false)?;
                check_scalar_4096(bounds[3], "height", false)?;
                check_scalar_4096(*radius, "radius", false)?;
            }
            Content::ClipEnd => {
                if clip_depth == 0 {
                    return Err(if range_mode {
                        "Unmatched range clip"
                    } else {
                        "Unmatched clip end"
                    }
                    .into());
                }
                clip_depth -= 1;
            }
            Content::Text { bounds, text } => {
                if !bounds[0].is_finite()
                    || !bounds[1].is_finite()
                    || bounds[0].abs() > 100_000.
                    || bounds[1].abs() > 100_000.
                {
                    return Err("Invalid content coordinate".into());
                }
                check_scalar_4096(bounds[2], "width", false)?;
                check_scalar_4096(bounds[3], "height", false)?;
                check_scalar_4096(text.font_size, "fontSize", true)?;
            }
            Content::Shape { points, .. } => {
                if points.len() < 3 || points.len() > 4096 {
                    return Err("Invalid polygon points".into());
                }
                for [x, y] in points {
                    if !x.is_finite() || !y.is_finite() || x.abs() > 100_000. || y.abs() > 100_000.
                    {
                        return Err("Invalid polygon points".into());
                    }
                }
            }
        }
    }
    if clip_depth != 0 {
        return Err(if range_mode {
            "Unclosed range clip"
        } else {
            "Unclosed content clip"
        }
        .into());
    }
    Ok(())
}

fn validate_button_spec(button: &mut ButtonSpec) -> Result<(), String> {
    if button.specified.contains("key") && button.key.is_empty() {
        return Err("Explicit 'key' must not be empty".into());
    }
    check_finite_non_neg(button.x, "x", false)?;
    check_finite_non_neg(button.y, "y", false)?;
    check_finite_non_neg(button.width, "width", true)?;
    check_finite_non_neg(button.height, "height", true)?;
    check_finite_non_neg(button.radius, "radius", false)?;
    check_finite_non_neg(button.font_size, "fontSize", true)?;
    check_finite_non_neg(button.numbers.border_width, "borderWidth", false)?;
    check_duration(button.numbers.transition_duration, "transitionDuration")?;
    if let Some(action) = &button.action {
        let method = action.strip_prefix("actions.").unwrap_or("");
        let mut chars = method.chars();
        let valid = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err("'clicked' requires actions.<name>() without arguments".into());
        }
    }
    button.radius = button.radius.min(button.width.min(button.height) / 2.);
    Ok(())
}

fn validate_modal_spec(
    modal: &mut ModalSpec,
    scene_width: f32,
    scene_height: f32,
    button_count: usize,
) -> Result<(), String> {
    if modal.controls.end > button_count || modal.controls.start > modal.controls.end {
        return Err("Invalid Modal control range".into());
    }
    check_finite_non_neg(modal.x, "x", false)?;
    check_finite_non_neg(modal.y, "y", false)?;
    if !modal.width.is_finite()
        || modal.width < 0.
        || !modal.height.is_finite()
        || modal.height < 0.
    {
        return Err("Modal size must be finite and non-negative".into());
    }
    if modal.width <= 0. {
        modal.width = scene_width;
    }
    if modal.height <= 0. {
        modal.height = scene_height;
    }
    if let Some(action) = &modal.action {
        let method = action.strip_prefix("actions.").unwrap_or("");
        let mut chars = method.chars();
        let valid = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err("'dismissed' requires actions.<name>() without arguments".into());
        }
    }
    Ok(())
}

/// Validates a typed [`Scene`] against the exact invariants enforced by `markup::parse`.
pub fn validate_scene_spec(scene: &mut Scene) -> Result<(), String> {
    check_finite_non_neg(scene.width, "width", true)?;
    check_finite_non_neg(scene.height, "height", true)?;
    check_finite_non_neg(scene.gap, "gap", false)?;
    check_finite_non_neg(scene.radius, "radius", false)?;
    for p in scene.padding {
        check_finite_non_neg(p, "padding", false)?;
    }
    scene.radius = scene.radius.min(scene.width.min(scene.height) / 2.);
    if scene.buttons.len() > 1024 {
        return Err("At most 1024 controls per scene".into());
    }
    if !scene.overflow.is_empty() && !matches!(scene.overflow.as_str(), "visible" | "hidden") {
        return Err(
            "overflow: expected visible or hidden; use a Scroll element for scrolling".into(),
        );
    }
    if scene.overflow == "hidden" {
        scene.clip = true;
    }

    let mut modal_keys = HashSet::new();
    let button_count = scene.buttons.len();
    for modal in &mut scene.modals {
        if !modal.key.is_empty() && !modal_keys.insert(modal.key.clone()) {
            return Err("Duplicate Modal key".into());
        }
        validate_modal_spec(modal, scene.width, scene.height, button_count)?;
    }

    let mut keys = HashSet::new();
    let base = scene
        .modals
        .first()
        .map_or(scene.buttons.len(), |m| m.controls.start);
    let mut y = scene.padding[0];
    let mut used_width = scene.width;
    let mut used_height = scene.height;
    for button in &mut scene.buttons[..base] {
        validate_button_spec(button)?;
        if !button.key.is_empty() && !keys.insert(button.key.clone()) {
            return Err("Duplicate control key".into());
        }
        if !button.specified.contains("x") {
            button.x = scene.padding[3];
        }
        if !button.specified.contains("y") {
            button.y = y;
        }
        y = button.y + button.height + scene.gap;
        used_width = used_width.max(button.x + button.width + scene.padding[1]);
        used_height = used_height.max(button.y + button.height + scene.padding[2]);
    }
    for modal in &scene.modals {
        for button in &mut scene.buttons[modal.controls.clone()] {
            validate_button_spec(button)?;
            if !button.key.is_empty() && !keys.insert(button.key.clone()) {
                return Err("Duplicate control key".into());
            }
        }
    }
    scene.content_width = used_width.min(f32::MAX).max(scene.width);
    scene.content_height = used_height.min(f32::MAX).max(scene.height);
    scene.button = scene.buttons.first().cloned().unwrap_or_default();
    Ok(())
}

/// Validates a typed [`Template`] against the exact invariants enforced by `template::Parser`.
pub fn validate_template(template: &Template) -> Result<(), String> {
    let mut props = template.props.clone();
    validate_button_spec(&mut props)?;
    check_scalar_4096(template.radius, "radius", false)?;
    if let Some(Brush { duration_ms, .. }) = &template.fill {
        check_duration(*duration_ms, "transition")?;
    }
    if let Some(Border { width, brush }) = &template.border {
        check_scalar_4096(*width, "width", false)?;
        check_duration(brush.duration_ms, "transition")?;
    }
    if let Some(reveal) = &template.reveal {
        if let Some(tx) = reveal.target_x {
            check_scalar_4096(tx, "targetX", false)?;
        }
        if let Some(ty) = reveal.target_y {
            check_scalar_4096(ty, "targetY", false)?;
        }
        if let Some(tw) = reveal.target_width {
            check_scalar_4096(tw, "targetWidth", true)?;
        }
        if let Some(th) = reveal.target_height {
            check_scalar_4096(th, "targetHeight", true)?;
        }
        if let Some(tr) = reveal.target_radius {
            check_scalar_4096(tr, "targetRadius", false)?;
        }
        check_scalar_4096(reveal.width, "width", false)?;
        check_scalar_4096(reveal.radius, "radius", true)?;
        check_scalar_4096(reveal.active_radius, "activeRadius", true)?;
        check_duration(reveal.duration_ms, "transition")?;
        if !reveal.stop.is_finite()
            || reveal.stop <= 0.
            || reveal.stop > 1.
            || !reveal.active_stop.is_finite()
            || reveal.active_stop <= 0.
            || reveal.active_stop > 1.
        {
            return Err("Reveal stop requires a unitless fraction above 0 and at most 1".into());
        }
    }
    if let Some(text) = &template.text {
        check_scalar_4096(text.font_size, "fontSize", true)?;
    }
    check_content_list(&template.content, false)?;
    if template.inputs.len() > 1 {
        return Err("One TextInput per control is supported".into());
    }
    for Input {
        bounds, font_size, ..
    } in &template.inputs
    {
        check_scalar_4096(bounds[0], "x", false)?;
        check_scalar_4096(bounds[1], "y", false)?;
        check_scalar_4096(bounds[2], "width", false)?;
        check_scalar_4096(bounds[3], "height", false)?;
        check_scalar_4096(*font_size, "fontSize", true)?;
    }
    if let Some(CanvasSlot {
        x,
        y,
        width,
        height,
    }) = template.canvas
    {
        check_scalar_4096(x, "x", false)?;
        check_scalar_4096(y, "y", false)?;
        if let Some(w) = width {
            check_scalar_4096(w, "width", false)?;
        }
        if let Some(h) = height {
            check_scalar_4096(h, "height", false)?;
        }
    }
    if template.links.len() > 4096 {
        return Err("Too many link regions".into());
    }
    for TextLink { bounds, id } in &template.links {
        if id.is_empty() {
            return Err("ContentLink requires a link id".into());
        }
        if !bounds[0].is_finite()
            || !bounds[1].is_finite()
            || bounds[0].abs() > 100_000.
            || bounds[1].abs() > 100_000.
        {
            return Err("Invalid content coordinate".into());
        }
        check_scalar_4096(bounds[2], "width", false)?;
        check_scalar_4096(bounds[3], "height", false)?;
    }
    if let Some(
        range @ RangeInput {
            value,
            step,
            min,
            max,
            minimum,
            maximum,
        },
    ) = &template.range
    {
        if !value.is_finite() || !(0. ..=1.).contains(value) {
            return Err("RangeInput value must be between 0 and 1".into());
        }
        if !step.is_finite() || !(0. ..=1.).contains(step) {
            return Err("RangeInput step must be between 0 and 1".into());
        }
        if !min.is_finite() || !max.is_finite() || min >= max || !(max - min).is_finite() {
            return Err("RangeInput min must be less than max".into());
        }
        check_content_list(minimum, true)?;
        check_content_list(maximum, true)?;
        if !range.compatible() {
            return Err("Range endpoints must have matching geometry".into());
        }
    }
    Ok(())
}
