//! The deliberately small, shared Forma markup frontend for the vector demo.
//! Unsupported syntax is an error rather than an invisible no-op.

use std::collections::{HashSet,HashMap};

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub name: String,
    pub width: f32,
    pub height: f32,
    pub background: [u8; 4],
    pub overflow: String,
    pub clip: bool,
    pub radius: f32,
    pub scroll: bool,
    pub padding: [f32;4],
    pub content_width: f32,
    pub content_height: f32,
    pub button: ButtonSpec,
    pub buttons: Vec<ButtonSpec>,
    pub gap: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonSpec {
    pub specified: HashSet<String>,
    pub colors: HashMap<String,[u8;4]>,
    pub numbers: HashMap<String,f32>,
    pub key: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub background: [u8; 4],
    pub color: [u8; 4],
    pub font_size: f32,
    pub text: String,
    pub disabled: bool,
    pub action: Option<String>,
}

impl Default for ButtonSpec {
    fn default() -> Self {
        Self {
            specified:HashSet::new(),
            colors:HashMap::from([("hoverBackground".into(),[168,186,255,255]),("pressedBackground".into(),[106,131,218,255]),("disabledBackground".into(),[89,98,115,255]),("borderColor".into(),[190,208,255,255]),("focusBorderColor".into(),[255;4])]),
            numbers:HashMap::from([("borderWidth".into(),1.),("transitionDuration".into(),140.)]),
            key: String::new(),
            x: 0.,
            y: 0.,
            width: 200.,
            height: 70.,
            radius: 16.,
            background: [140, 165, 255, 255],
            color: [20, 33, 59, 255],
            font_size: 18.,
            text: String::new(),
            disabled: false,
            action: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Ident(String),
    String(String),
    Number(f32),
    Duration(f32),
    Hex(String),
    Open,
    Close,
    Colon,
    Semi,
    LeftParen,
    RightParen,
    Arrow,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    offset: usize,
}

struct Lexer<'a> {
    source: &'a str,
    offset: usize,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.source[self.offset..].chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }
    fn rest(&self) -> &str {
        &self.source[self.offset..]
    }
    fn error(&self, message: &str) -> String {
        format!("{message} (byte {})", self.offset)
    }

    fn next(&mut self) -> Result<Token, String> {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.bump();
            }
            if self.rest().starts_with("//") {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.bump();
                }
            } else if self.rest().starts_with("/*") {
                let start = self.offset;
                self.offset += 2;
                let mut depth = 1;
                while depth > 0 {
                    if self.rest().starts_with("/*") {
                        depth += 1;
                        self.offset += 2;
                    } else if self.rest().starts_with("*/") {
                        depth -= 1;
                        self.offset += 2;
                    } else if self.bump().is_none() {
                        return Err(format!("Unclosed block comment (byte {start})"));
                    }
                }
            } else {
                break;
            }
        }
        let offset = self.offset;
        let Some(ch) = self.bump() else {
            return Ok(Token {
                kind: Kind::Eof,
                offset,
            });
        };
        let kind = match ch {
            '{' => Kind::Open, '}' => Kind::Close, ':' => Kind::Colon,
            ';' => Kind::Semi, '(' => Kind::LeftParen, ')' => Kind::RightParen,
            '-' if self.peek() == Some('>') => { self.bump(); Kind::Arrow }
            '\'' | '"' => {
                let mut value = String::new();
                loop {
                    match self.bump() {
                        Some(c) if c == ch => break,
                        Some('\\') => {
                            let escaped = match self.bump() {
                                Some('n') => '\n', Some('r') => '\r', Some('t') => '\t',
                                Some('\\') => '\\', Some('\'') => '\'', Some('"') => '"',
                                Some(c) => return Err(self.error(&format!("Unsupported string escape \\{c}"))),
                                None => return Err(format!("Unclosed string (byte {offset})")),
                            };
                            value.push(escaped);
                        }
                        Some(c) => value.push(c),
                        None => return Err(format!("Unclosed string (byte {offset})")),
                    }
                }
                Kind::String(value)
            }
            '#' => {
                let start = self.offset;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric()) { self.bump(); }
                Kind::Hex(self.source[start..self.offset].to_owned())
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') { self.bump(); }
                Kind::Ident(self.source[offset..self.offset].to_owned())
            }
            c if c.is_ascii_digit() || matches!(c, '.' | '-' | '+') => {
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') { self.bump(); }
                if self.peek().is_some_and(|c| c == 'e' || c == 'E') {
                    self.bump();
                    if self.peek().is_some_and(|c| c == '+' || c == '-') { self.bump(); }
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) { self.bump(); }
                }
                let raw = &self.source[offset..self.offset];
                let value: f32 = raw.parse().map_err(|_| format!("Invalid number '{raw}' (byte {offset})"))?;
                if !value.is_finite() { return Err(format!("Number must be finite (byte {offset})")); }
                if self.rest().starts_with("ms") { self.offset+=2; Kind::Duration(value) }
                else {if self.rest().starts_with("px") { self.offset += 2; } Kind::Number(value)}
            }
            c => return Err(format!("Unsupported character '{c}' (byte {offset}); this vector demo does not support bindings or expressions")),
        };
        Ok(Token { kind, offset })
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Result<Self, String> {
        let mut lexer = Lexer { source, offset: 0 };
        let current = lexer.next()?;
        Ok(Self { lexer, current })
    }
    fn advance(&mut self) -> Result<Kind, String> {
        let next = self.lexer.next()?;
        Ok(std::mem::replace(&mut self.current, next).kind)
    }
    fn error(&self, message: &str) -> String {
        format!("{message} (byte {})", self.current.offset)
    }
    fn expect(&mut self, kind: Kind, description: &str) -> Result<(), String> {
        if self.current.kind != kind {
            return Err(self.error(&format!(
                "Expected {description}, found {:?}",
                self.current.kind
            )));
        }
        self.advance()?;
        Ok(())
    }
    fn ident(&mut self) -> Result<String, String> {
        if let Kind::Ident(value) = &self.current.kind {
            let value = value.clone();
            self.advance()?;
            Ok(value)
        } else {
            Err(self.error("Expected an identifier"))
        }
    }
    fn string(&mut self, property: &str) -> Result<String, String> {
        if let Kind::String(value) = &self.current.kind {
            let value = value.clone();
            self.advance()?;
            Ok(value)
        } else {
            Err(self.error(&format!("'{property}' requires a quoted string; bindings and expressions are not supported in the vector demo")))
        }
    }
    fn number(&mut self, property: &str, zero_allowed: bool) -> Result<f32, String> {
        if let Kind::Number(value) = self.current.kind {
            if value < 0. || (!zero_allowed && value == 0.) {
                return Err(self.error(&format!(
                    "'{property}' must be {}",
                    if zero_allowed {
                        "non-negative"
                    } else {
                        "positive"
                    }
                )));
            }
            self.advance()?;
            Ok(value)
        } else {
            Err(self.error(&format!("'{property}' requires a finite pixel number")))
        }
    }
    fn color(&mut self, property: &str) -> Result<[u8; 4], String> {
        let Kind::Hex(raw) = &self.current.kind else {
            return Err(self.error(&format!(
                "'{property}' requires an unquoted #RGB, #RRGGBB or #RRGGBBAA color"
            )));
        };
        if !matches!(raw.len(), 3 | 6 | 8) || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(self.error(&format!(
                "Invalid hex color '#{raw}': expected #RGB, #RRGGBB or #RRGGBBAA"
            )));
        }
        let mut color = [0, 0, 0, 255];
        if raw.len() == 3 {
            for (i, c) in raw.chars().enumerate() {
                color[i] = c.to_digit(16).unwrap() as u8 * 17;
            }
        } else {
            for i in 0..raw.len() / 2 {
                color[i] = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).unwrap();
            }
        }
        self.advance()?;
        Ok(color)
    }
    fn padding(&mut self) -> Result<[f32; 4], String> {
        let mut values = Vec::new();
        while matches!(self.current.kind, Kind::Number(_)) {
            values.push(self.number("padding", true)?);
            if values.len() > 4 {
                return Err(self.error("'padding' accepts 1, 2 or 4 values"));
            }
        }
        match values.as_slice() {
            [all] => Ok([*all; 4]),
            [vertical, horizontal] => Ok([*vertical, *horizontal, *vertical, *horizontal]),
            [top, right, bottom, left] => Ok([*top, *right, *bottom, *left]),
            _ => Err(self.error("'padding' accepts 1, 2 or 4 non-negative pixel values")),
        }
    }
    fn button(&mut self) -> Result<ButtonSpec, String> {
        self.expect(Kind::Open, "'{' after Button")?;
        let mut button = ButtonSpec::default();
        let mut seen = HashSet::new();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            if !seen.insert(name.clone()) {
                return Err(self.error(&format!("Duplicate Button property '{name}'")));
            }
            if name == "clicked" {
                self.expect(Kind::Arrow, "'->' after clicked")?;
                let action = self.ident()?;
                let method = action.strip_prefix("actions.").unwrap_or("");
                if !valid_identifier(method) {
                    return Err(self.error("'clicked' requires actions.<name>() without arguments"));
                }
                self.expect(Kind::LeftParen, "'(' after action name")?;
                self.expect(Kind::RightParen, "')'; action arguments are not supported")?;
                button.action = Some(action);
            } else {
                if !matches!(
                    name.as_str(),
                    "key" | "x" | "y"
                        | "width"
                        | "height"
                        | "radius"
                        | "background"
                        | "color"
                        | "font.size"
                        | "fontSize" | "hoverBackground" | "pressedBackground" | "disabledBackground" | "borderColor" | "focusBorderColor" | "borderWidth" | "transitionDuration"
                        | "text"
                        | "disabled"
                ) {
                    return Err(
                        self.error(&format!("Unsupported Button property or child '{name}'"))
                    );
                }
                self.expect(
                    Kind::Colon,
                    "':' after property name; bindings are not supported",
                )?;
                match name.as_str() {
                    "key" => {
                        button.key = self.string("key")?;
                        if button.key.is_empty() {
                            return Err(self.error("Explicit 'key' must not be empty"));
                        }
                    }
                    "x" => button.x = self.number("x", true)?,
                    "y" => button.y = self.number("y", true)?,
                    "width" => button.width = self.number("width", false)?,
                    "height" => button.height = self.number("height", false)?,
                    "radius" => button.radius = self.number("radius", true)?,
                    "background" => button.background = self.color("background")?,
                    "color" => button.color = self.color("color")?,
                    "font.size" | "fontSize" => {if seen.contains("font.size")&&seen.contains("fontSize"){return Err(self.error("fontSize and font.size cannot both be specified"));}button.font_size = self.number("fontSize", false)?;},
                    "hoverBackground" | "pressedBackground" | "disabledBackground" | "borderColor" | "focusBorderColor" => {let color=self.color(&name)?;button.colors.insert(name.clone(),color);},
                    "borderWidth" => {let n=self.number(&name,true)?;button.numbers.insert(name.clone(),n);},
                    "transitionDuration" => {let Kind::Duration(n)=self.current.kind else{return Err(self.error("transitionDuration requires ms"));};if !(0. ..=2000.).contains(&n){return Err(self.error("transitionDuration must be 0..2000ms"));}self.advance()?;button.numbers.insert(name.clone(),n);},
                    "text" => button.text = self.string("text")?,
                    "disabled" => {
                        button.disabled = match self.current.kind.clone() {
                            Kind::Ident(value) if value == "true" => true,
                            Kind::Ident(value) if value == "false" => false,
                            _ => return Err(self.error("'disabled' requires true or false")),
                        };
                        self.advance()?;
                    }
                    _ => unreachable!(),
                }
            }
            self.expect(Kind::Semi, "';' after Button property")?;
        }
        self.expect(Kind::Close, "'}' after Button")?;
        // Like CSS rounded corners, a radius may be larger than half the box.
        button.specified=seen.into_iter().map(|n|if n=="font.size"{"fontSize".into()}else{n}).collect();
        button.radius = button.radius.min(button.width.min(button.height) / 2.);
        Ok(button)
    }
    fn scene(&mut self) -> Result<Scene, String> {
        self.expect(Kind::Ident("component".into()), "'component'")?;
        let name = self.ident()?;
        if !valid_identifier(&name) {
            return Err(self.error("Invalid component name"));
        }
        self.expect(Kind::Open, "'{' after component name")?;
        self.expect(Kind::Ident("Frame".into()), "exactly one root Frame")?;
        self.expect(Kind::Open, "'{' after Frame")?;
        let mut width = 360.;
        let mut height = 220.;
        let mut background = [24, 30, 42, 255];
        let mut padding = [0.; 4];
        let mut overflow = String::from("visible");
        let mut clip=false;let mut radius=0.;
        let mut scroll=false;
        let mut buttons = Vec::new();
        let mut gap = 0.;
        let mut seen = HashSet::new();
        while self.current.kind != Kind::Close {
            let property = self.ident()?;
            if property == "Button" || property == "Scroll" {
                if property=="Scroll" {
                    if scroll || !buttons.is_empty() { return Err(self.error("Scroll must be the only Frame child")); }
                    scroll=true;
                    self.expect(Kind::Open,"'{' after Scroll")?;
                    while self.current.kind != Kind::Close {
                        self.expect(Kind::Ident("Button".into()),"Button inside Scroll")?;
                        buttons.push(self.button()?);
                    }
                    self.expect(Kind::Close,"'}' after Scroll")?;
                } else {
                    if scroll { return Err(self.error("Scroll must be the only Frame child")); }
                    buttons.push(self.button()?);
                }
                continue;
            }
            if !matches!(
                property.as_str(),
                "gap" | "width" | "height" | "padding" | "background" | "overflow" | "clip" | "radius"
            ) {
                return Err(
                    self.error(&format!("Unsupported Frame property or child '{property}'"))
                );
            }
            if !seen.insert(property.clone()) {
                return Err(self.error(&format!("Duplicate Frame property '{property}'")));
            }
            self.expect(Kind::Colon, "':' after Frame property")?;
            match property.as_str() {
                "gap" => gap = self.number("gap", true)?,
                "width" => width = self.number("width", false)?,
                "height" => height = self.number("height", false)?,
                "padding" => padding = self.padding()?,
                "background" => background = self.color("background")?,
                "radius" => radius=self.number("radius",true)?,
                "clip" => {clip=match self.ident()?.as_str(){"true"=>true,"false"=>false,_=>return Err(self.error("clip requires true or false"))};},
                "overflow" => { overflow=self.ident()?; if !matches!(overflow.as_str(),"visible"|"hidden"){return Err(self.error("overflow: expected visible or hidden; use a Scroll element for scrolling"));} },
                _ => unreachable!(),
            }
            self.expect(Kind::Semi, "';' after Frame property")?;
        }
        self.expect(Kind::Close, "'}' after Frame")?;
        self.expect(Kind::Close, "'}' after component")?;
        if self.current.kind != Kind::Eof {
            return Err(
                self.error("Unexpected content after component; only one component is supported")
            );
        }
        if buttons.is_empty() { return Err(self.error("Frame must contain at least one Button")); }
        if buttons.len()>256 { return Err(self.error("At most 256 controls per scene")); }
        if seen.contains("clip")&&seen.contains("overflow"){return Err(self.error("Use clip or legacy overflow, not both"));}
        if overflow=="hidden"{clip=true;}
        let mut keys=HashSet::new();
        let mut y=padding[0];
        let mut used_width=width;
        let mut used_height=height;
        for button in &mut buttons {
            if !button.key.is_empty() && !keys.insert(button.key.clone()) { return Err(self.error("Duplicate control key")); }
            if !button.specified.contains("x") {button.x=padding[3];}
            if !button.specified.contains("y") {button.y=y;}
            y=button.y+button.height+gap;
            used_width=used_width.max(button.x+button.width+padding[1]);
            used_height=used_height.max(button.y+button.height+padding[2]);
        }
        let button=buttons[0].clone();
        Ok(Scene {
            name,
            width,
            height,
            background,
            overflow,
            clip,
            radius:radius.min(width.min(height)/2.),
            scroll,
            padding,
            content_width:used_width.min(f32::MAX).max(width),
            content_height:used_height.min(f32::MAX).max(height),
            button, buttons, gap,
        })
    }
}

fn valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn parse(source: &str) -> Result<Scene, String> {
    Parser::new(source)?.scene()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = "component SearchWindow { Frame { width: 360; height: 220; padding: 65 80; background: #181e2a; Button { key: 'run'; width: 200; height: 70; radius: 16; background: #8ca5ff; color: #14213b; font.size: 18; text: 'Найти'; disabled: false; clicked -> actions.search(); } } }";
    fn with_button(props: &str) -> Result<Scene, String> {
        parse(&format!(
            "component Demo {{ Frame {{ Button {{ {props} }} }} }}"
        ))
    }

    #[test]
    fn parses_the_actual_forma_button_and_layout() {
        let scene = parse(DEMO).unwrap();
        assert_eq!(scene.name, "SearchWindow");
        assert_eq!((scene.width, scene.height), (360., 220.));
        assert_eq!(scene.background, [24, 30, 42, 255]);
        assert_eq!((scene.button.x, scene.button.y), (80., 65.));
        assert_eq!((scene.button.width, scene.button.height), (200., 70.));
        assert_eq!(scene.button.radius, 16.);
        assert_eq!(scene.button.font_size, 18.);
        assert_eq!(scene.button.key, "run");
        assert_eq!(scene.button.text, "Найти");
        assert_eq!(scene.button.color, [20, 33, 59, 255]);
        assert_eq!(scene.button.background, [140, 165, 255, 255]);
        assert_eq!(scene.button.action.as_deref(), Some("actions.search"));
        assert!(!scene.button.disabled);
    }

    #[test]
    fn defaults_are_explicit_and_deterministic() {
        let scene = with_button("").unwrap();
        assert_eq!(scene.button, ButtonSpec::default());
        assert_eq!((scene.width, scene.height), (360., 220.));
    }

    #[test]
    fn comments_quotes_and_unicode_are_preserved() {
        let scene = parse("// hello\ncomponent Demo /* nested /* comment */ ok */ { Frame { Button { text: \"Найти 'текст' \\\"ещё\\\"\\n\\t\\\\\"; } } } // end").unwrap();
        assert_eq!(scene.button.text, "Найти 'текст' \"ещё\"\n\t\\");
        assert_eq!(
            with_button("text: 'it\\'s // not a comment';")
                .unwrap()
                .button
                .text,
            "it's // not a comment"
        );
    }

    #[test]
    fn css_hex_colors_include_alpha_and_uppercase() {
        assert_eq!(
            with_button("color: #AbC;").unwrap().button.color,
            [170, 187, 204, 255]
        );
        assert_eq!(
            with_button("background: #01020304;")
                .unwrap()
                .button
                .background,
            [1, 2, 3, 4]
        );
        for value in [
            "#abcd",
            "#12",
            "#123456789",
            "#xyzxyz",
            "'#ffffff'",
            "rgb(1,2,3)",
        ] {
            assert!(
                with_button(&format!("color: {value};")).is_err(),
                "accepted {value}"
            );
        }
    }

    #[test]
    fn padding_uses_top_right_bottom_left() {
        for (value, position) in [
            ("10", (10., 10.)),
            ("10px 20px", (20., 10.)),
            ("10 20 30 40", (40., 10.)),
        ] {
            let scene = parse(&format!(
                "component Demo {{ Frame {{ padding: {value}; Button {{ }} }} }}"
            ))
            .unwrap();
            assert_eq!((scene.button.x, scene.button.y), position);
        }
        for value in ["", "1 2 3", "1 2 3 4 5", "-1", "1, 2", "auto"] {
            assert!(parse(&format!(
                "component Demo {{ Frame {{ padding: {value}; Button {{ }} }} }}"
            ))
            .is_err());
        }
    }

    #[test]
    fn accepts_fractional_and_px_sizes_and_clamps_rounding() {
        let scene =
            with_button("width: 20.5px; height: 1e1; radius: 100; font.size: .5; disabled: true;")
                .unwrap();
        assert_eq!(
            (
                scene.button.width,
                scene.button.height,
                scene.button.radius,
                scene.button.font_size
            ),
            (20.5, 10., 5., 0.5)
        );
        assert!(scene.button.disabled);
    }

    #[test]
    fn rejects_nonpositive_or_nonfinite_sizes() {
        for value in [
            "0", "-1", "1e999", "NaN", "Infinity", "10%", "*", "1.2.3", "1em",
        ] {
            assert!(
                with_button(&format!("width: {value};")).is_err(),
                "accepted {value}"
            );
        }
        assert!(with_button("radius: -1;").is_err());
        assert!(with_button("font.size: 0;").is_err());
        assert!(with_button("radius: 0;").is_ok());
    }

    #[test]
    fn rejects_invalid_frame_sizes() {
        for source in [
            "component X { Frame { width: 0; Button {} } }",
            "component X { Frame { height: -1; Button {} } }",
        ] {
            assert!(parse(source).is_err(), "accepted {source}");
        }
    }

    #[test]
    fn rejects_duplicates_unsupported_properties_and_bindings() {
        for props in [
            "text:'a'; text:'b';",
            "key:'';",
            "row: 1;",
            "font.weight: 700;",
            "text: state.label;",
            "text <-> state.label;",
            "Text {}",
            "disabled: 1;",
            "clicked -> actions.a(); clicked -> actions.b();",
        ] {
            assert!(with_button(props).is_err(), "accepted {props}");
        }
        for contents in [
            "padding:0; padding:1; Button{}",
            "width:360; width:360; Button{}",
            "rows:[*]; Button{}",
            "Frame{}",
            "Text{}",
        ] {
            assert!(
                parse(&format!("component X {{ Frame {{ {contents} }} }}")).is_err(),
                "accepted {contents}"
            );
        }
    }

    #[test]
    fn actions_must_be_explicit_and_have_no_arguments() {
        for action in [
            "search()",
            "actions.search(1)",
            "actions.search",
            "state.search()",
            "actions.nested.search()",
            "actions.()",
        ] {
            assert!(
                with_button(&format!("clicked -> {action};")).is_err(),
                "accepted {action}"
            );
        }
        assert_eq!(
            with_button("clicked -> actions._run2();")
                .unwrap()
                .button
                .action
                .as_deref(),
            Some("actions._run2")
        );
    }

    #[test]
    fn rejects_incomplete_documents_and_trailing_content() {
        for source in [
            "",
            "component X {}",
            "component X { Frame {} }",
            "component X { Frame { Button {} }",
            "component X { Frame { Button {} } } Button {}",
            "component X { Frame { Button {} } Frame {} }",
            "component X { Frame { Button { text: 'x' } } }",
            "component X { Frame { Button { text: 'unterminated; } } }",
            "/* unclosed",
            "component X { Frame { Button{} } } /* unclosed",
        ] {
            let error = parse(source).unwrap_err();
            assert!(error.contains("byte"), "missing location: {error}");
        }
    }
}
