//! Colour builtins over hex colours.

use super::*;

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match name {
        "alpha" if arguments.len() == 2 => {
            let (r, g, b, _) = parse_hex_color(&arguments[0])?;
            let a = (expression_number(&arguments[1])?.clamp(0., 1.) * 255.).round() as u8;
            Ok(format_hex_color(
                r,
                g,
                b,
                a,
                matches!(arguments[0], Value::Expr(_)),
            ))
        }
        "lighten" | "darken" if arguments.len() == 2 => {
            let (r, g, b, a) = parse_hex_color(&arguments[0])?;
            let amt = expression_number(&arguments[1])?.clamp(0., 1.);
            let shift = |c: u8| -> u8 {
                if name == "lighten" {
                    (c as f64 + (255. - c as f64) * amt).round() as u8
                } else {
                    (c as f64 * (1. - amt)).round() as u8
                }
            };
            Ok(format_hex_color(
                shift(r),
                shift(g),
                shift(b),
                a,
                matches!(arguments[0], Value::Expr(_)),
            ))
        }
        "saturate" | "desaturate" if arguments.len() == 2 => {
            let (r, g, b, a) = parse_hex_color(&arguments[0])?;
            let amt = expression_number(&arguments[1])?.clamp(0., 1.);
            let (rf, gf, bf) = (r as f64 / 255., g as f64 / 255., b as f64 / 255.);
            let max = rf.max(gf).max(bf);
            let min = rf.min(gf).min(bf);
            let l = (max + min) * 0.5;
            if (max - min).abs() < f64::EPSILON {
                Ok(format_hex_color(
                    r,
                    g,
                    b,
                    a,
                    matches!(arguments[0], Value::Expr(_)),
                ))
            } else {
                let d = max - min;
                let s = if l > 0.5 {
                    d / (2. - max - min)
                } else {
                    d / (max + min)
                };
                let new_s = if name == "saturate" {
                    (s + (1. - s) * amt).clamp(0., 1.)
                } else {
                    (s * (1. - amt)).clamp(0., 1.)
                };
                let factor = if s > 0. { new_s / s } else { 0. };
                let adj = |c: f64| ((l + (c - l) * factor).clamp(0., 1.) * 255.).round() as u8;
                Ok(format_hex_color(
                    adj(rf),
                    adj(gf),
                    adj(bf),
                    a,
                    matches!(arguments[0], Value::Expr(_)),
                ))
            }
        }
        "invert" if arguments.len() == 1 => {
            let (r, g, b, a) = parse_hex_color(&arguments[0])?;
            Ok(format_hex_color(
                255 - r,
                255 - g,
                255 - b,
                a,
                matches!(arguments[0], Value::Expr(_)),
            ))
        }
        "contrast" if arguments.len() == 2 => {
            let (r1, g1, b1, _) = parse_hex_color(&arguments[0])?;
            let (r2, g2, b2, _) = parse_hex_color(&arguments[1])?;
            let lum = |r: u8, g: u8, b: u8| -> f64 {
                let ch = |c: u8| -> f64 {
                    let s = c as f64 / 255.;
                    if s <= 0.03928 {
                        s / 12.92
                    } else {
                        ((s + 0.055) / 1.055).powf(2.4)
                    }
                };
                0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
            };
            let l1 = lum(r1, g1, b1);
            let l2 = lum(r2, g2, b2);
            let ratio = (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05);
            Ok(number((ratio * 100.).round() / 100.))
        }
        "mix" if arguments.len() == 2 || arguments.len() == 3 => {
            let (r1, g1, b1, a1) = parse_hex_color(&arguments[0])?;
            let (r2, g2, b2, a2) = parse_hex_color(&arguments[1])?;
            let w = if arguments.len() == 3 {
                expression_number(&arguments[2])?.clamp(0., 1.)
            } else {
                0.5
            };
            let lerp =
                |c1: u8, c2: u8| -> u8 { (c1 as f64 * (1. - w) + c2 as f64 * w).round() as u8 };
            Ok(format_hex_color(
                lerp(r1, r2),
                lerp(g1, g2),
                lerp(b1, b2),
                lerp(a1, a2),
                matches!(arguments[0], Value::Expr(_)) || matches!(arguments[1], Value::Expr(_)),
            ))
        }
        _ => Err(unknown(name)),
    }
}
