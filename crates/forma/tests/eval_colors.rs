//! Colour arguments that are not plain hex digits must be rejected, never crash or slip through.

use forma::form_document::{Expression, Properties, Value};

fn lighten(colour: &str) -> Result<Value, String> {
    Value::Expression(Box::new(Expression::Call(
        "lighten".into(),
        vec![Value::Text(colour.into()), Value::Number(0.1)],
    )))
    .evaluate(&Properties::new(), &Properties::new())
}

#[test]
fn a_non_ascii_colour_is_an_error_not_a_panic() {
    // "é" is two bytes, so each of these has a byte length the parser accepts (3, 6 or 8)
    // while a digit boundary falls inside a character.
    for colour in ["#é1", "#éé11", "#ééé11"] {
        assert_eq!(lighten(colour), Err("Invalid hex color".into()), "{colour}");
    }
}

#[test]
fn a_sign_is_not_a_hex_digit() {
    assert_eq!(lighten("#+f+f+f"), Err("Invalid hex color".into()));
    assert_eq!(lighten("#+f+f+f+f"), Err("Invalid hex color".into()));
}
