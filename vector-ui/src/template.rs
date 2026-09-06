//! The first explicit primitive-composition subset of a Forma control template.
//! The host supplies the bounds; every painted primitive comes from this file.

use crate::markup::ButtonSpec;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub props:ButtonSpec,
    pub radius: f32,
    pub fill: Option<Brush>,
    pub border: Option<Border>,
    pub text: Option<Text>,
    pub clickable: bool,
    pub content: Vec<Content>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Clip { bounds: [f32;4], radius:f32 },
    ClipEnd,
    Text { bounds: [f32;4], text: Text },
    Shape { points: Vec<[f32;2]>, color: [u8;4] },
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
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Unit {
    Logical,
    Px,
    Ms,
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Ident(String),
    String(String),
    Number(f32, Unit),
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
    fn rest(&self) -> &str {
        &self.source[self.offset..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        Some(ch)
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
            '{' => Kind::Open,
            '}' => Kind::Close,
            ':' => Kind::Colon,
            ';' => Kind::Semi,
            '(' => Kind::LeftParen,
            ')' => Kind::RightParen,
            '-' if self.peek() == Some('>') => {
                self.bump();
                Kind::Arrow
            }
            '\'' | '"' => {
                let mut value = String::new();
                loop {
                    match self.bump() {
                        Some(c) if c == ch => break,
                        Some('\\') => {
                            let escaped = match self.bump() {
                                Some('n') => '\n',
                                Some('r') => '\r',
                                Some('t') => '\t',
                                Some('\\') => '\\',
                                Some('\'') => '\'',
                                Some('"') => '"',
                                Some(c) => {
                                    return Err(format!(
                                        "Unsupported escape \\{c} (byte {})",
                                        self.offset
                                    ))
                                }
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
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric()) {
                    self.bump();
                }
                Kind::Hex(self.source[start..self.offset].to_owned())
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                {
                    self.bump();
                }
                Kind::Ident(self.source[offset..self.offset].to_owned())
            }
            c if c.is_ascii_digit() || matches!(c, '.' | '-' | '+') => {
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.bump();
                }
                if self.peek().is_some_and(|c| c == 'e' || c == 'E') {
                    self.bump();
                    if self.peek().is_some_and(|c| c == '+' || c == '-') {
                        self.bump();
                    }
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        self.bump();
                    }
                }
                let raw = &self.source[offset..self.offset];
                let value: f32 = raw
                    .parse()
                    .map_err(|_| format!("Invalid number '{raw}' (byte {offset})"))?;
                if !value.is_finite() {
                    return Err(format!("Number must be finite (byte {offset})"));
                }
                let unit = if self.rest().starts_with("px") {
                    self.offset += 2;
                    Unit::Px
                } else if self.rest().starts_with("ms") {
                    self.offset += 2;
                    Unit::Ms
                } else {
                    Unit::Logical
                };
                Kind::Number(value, unit)
            }
            c => {
                return Err(format!(
                    "Unsupported template character '{c}' (byte {offset})"
                ))
            }
        };
        Ok(Token { kind, offset })
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
    props: ButtonSpec,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, props: &'a ButtonSpec) -> Result<Self, String> {
        let mut lexer = Lexer { source, offset: 0 };
        let current = lexer.next()?;
        Ok(Self {
            lexer,
            current,
            props:props.clone(),
        })
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
            Err(self.error("Expected a primitive or property name"))
        }
    }
    fn colon(&mut self) -> Result<(), String> {
        self.expect(Kind::Colon, "':'")
    }
    fn semi(&mut self) -> Result<(), String> {
        self.expect(Kind::Semi, "';'")
    }
    fn open(&mut self) -> Result<(), String> {
        self.expect(Kind::Open, "'{'")
    }
    fn close(&mut self) -> Result<(), String> {
        self.expect(Kind::Close, "'}'")
    }
    fn unique(&self, seen: &mut HashSet<String>, name: &str) -> Result<(), String> {
        if !seen.insert(name.to_owned()) {
            return Err(self.error(&format!("Duplicate template field or primitive '{name}'")));
        }
        Ok(())
    }
    fn scalar(&mut self, name: &str, positive: bool) -> Result<f32, String> {
        let value = match &self.current.kind {
            Kind::Number(value, Unit::Logical | Unit::Px) => *value,
            Kind::Ident(reference) => match reference.as_str() {
                "props.radius" => self.props.radius,
                "props.width" => self.props.width,
                "props.height" => self.props.height,
                "props.font.size" | "props.fontSize" => self.props.font_size,
                "props.borderWidth" => self.props.numbers["borderWidth"],
                _ => {
                    return Err(self.error(&format!(
                        "Unknown or non-numeric props reference '{reference}' for {name}"
                    )))
                }
            },
            _ => {
                return Err(self.error(&format!(
                    "'{name}' requires a pixel number or numeric props reference"
                )))
            }
        };
        if !value.is_finite() || value < 0. || value > 4096. || (positive && value == 0.) {
            return Err(self.error(&format!(
                "'{name}' must be finite, {} and at most 4096",
                if positive { "positive" } else { "non-negative" }
            )));
        }
        self.advance()?;
        Ok(value)
    }
    fn duration(&mut self) -> Result<f32, String> {
        let value=match &self.current.kind {Kind::Number(v,Unit::Ms)=>*v,Kind::Ident(r) if r=="props.transitionDuration"=>self.props.numbers["transitionDuration"],_=>return Err(self.error("'transition' requires ms or props.transitionDuration"))};
        if !(0. ..=2000.).contains(&value) {
            return Err(self.error("'transition' must be between 0ms and 2000ms"));
        }
        self.advance()?;
        Ok(value)
    }
    fn color(&mut self, name: &str) -> Result<[u8; 4], String> {
        let color = match &self.current.kind {
            Kind::Ident(reference) => match reference.as_str() {
                "props.background" => self.props.background,
                "props.color" => self.props.color,
                r if r.strip_prefix("props.").is_some_and(|k|self.props.colors.contains_key(k)) => self.props.colors[r.strip_prefix("props.").unwrap()],
                _ => {
                    return Err(self.error(&format!(
                        "Unknown or non-color props reference '{reference}' for {name}"
                    )))
                }
            },
            Kind::Hex(raw) => {
                if !matches!(raw.len(), 3 | 6 | 8) || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(self.error("Expected #RGB, #RRGGBB or #RRGGBBAA color"));
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
                color
            }
            _ => {
                return Err(self.error(&format!(
                    "'{name}' requires an unquoted hex color or color props reference"
                )))
            }
        };
        self.advance()?;
        Ok(color)
    }
    fn text_value(&mut self) -> Result<String, String> {
        let value = match &self.current.kind {
            Kind::String(value) => value.clone(),
            Kind::Ident(reference) => match reference.as_str() {
                "props.text" => self.props.text.clone(),
                "props.key" => self.props.key.clone(),
                _ => {
                    return Err(self.error(&format!(
                        "Unknown or non-string props reference '{reference}' for text"
                    )))
                }
            },
            _ => {
                return Err(self.error("'text' requires a quoted string or string props reference"))
            }
        };
        self.advance()?;
        Ok(value)
    }
    fn brush(&mut self) -> Result<Brush, String> {
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
        let mut seen = HashSet::new();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            if !matches!(
                name.as_str(),
                "color" | "hover" | "pressed" | "disabled" | "focus" | "transition"
            ) {
                return Err(self.error(&format!("Unsupported Brush field or primitive '{name}'")));
            }
            self.colon()?;
            match name.as_str() {
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
    fn brush_value(&mut self) -> Result<Brush, String> {
        if self.current.kind == Kind::Ident("Brush".into()) {
            self.advance()?;
            self.brush()
        } else {
            Ok(Brush { color:self.color("brush")?, hover:None, pressed:None, disabled:None, focus:None, duration_ms:0. })
        }
    }
    fn border(&mut self) -> Result<Border, String> {
        self.open()?;
        let mut width = 1.;
        let mut brush = None;
        let mut seen = HashSet::new();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            match name.as_str() {
                "width" => {
                    self.colon()?;
                    width = self.scalar("width", false)?;
                    self.semi()?;
                }
                "background" => { self.colon()?; brush = Some(self.brush_value()?); self.semi()?; },
                _ => {
                    return Err(
                        self.error(&format!("Unsupported Border field or primitive '{name}'"))
                    )
                }
            }
        }
        self.close()?;
        Ok(Border {
            width,
            brush: brush.ok_or_else(|| self.error("Border requires background: Brush { ... };"))?,
        })
    }
    fn text(&mut self) -> Result<Text, String> {
        self.open()?;
        let mut text = Text {
            text: String::new(),
            color: [0, 0, 0, 255],
            font_size: 16.,
        };
        let mut seen = HashSet::new();
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            self.unique(&mut seen, &name)?;
            if !matches!(name.as_str(), "text" | "color" | "font.size" | "fontSize") {
                return Err(self.error(&format!("Unsupported Text field or primitive '{name}'")));
            }
            self.colon()?;
            match name.as_str() {
                "text" => text.text = self.text_value()?,
                "color" => text.color = self.color("color")?,
                "font.size" | "fontSize" => text.font_size = self.scalar("fontSize", true)?,
                _ => unreachable!(),
            }
            self.semi()?;
        }
        self.close()?;
        Ok(text)
    }
    fn pointer_area(&mut self) -> Result<bool, String> {
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
                Kind::Ident("events.clicked".into()),
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
    fn rectangle(&mut self) -> Result<Template, String> {
        self.open()?;
        let mut template = Template {
            props:self.props.clone(),
            radius: 0.,
            fill: None,
            border: None,
            text: None,
            clickable: false,
            content: Vec::new(),
        };
        let mut seen = HashSet::new();let mut clip_depth=0;
        while self.current.kind != Kind::Close {
            let name = self.ident()?;
            if name=="ContentText"||name=="ContentShape"||name=="ContentClip"||name=="ContentClipEnd" {
                if name=="ContentClip"{clip_depth+=1;if clip_depth>16{return Err(self.error("Clip nesting limit is 16"));}}
                if name=="ContentClipEnd"{if clip_depth==0{return Err(self.error("Unmatched clip end"));}clip_depth-=1;}
                if template.content.len()>=4096 {return Err(self.error("Too many content primitives"));}
                template.content.push(self.content(&name)?);continue;
            }
            self.unique(&mut seen, &name)?;
            match name.as_str() {
                "radius" => {
                    self.colon()?;
                    template.radius = self.scalar("radius", false)?;
                    self.semi()?;
                }
                "background" => { self.colon()?; template.fill = Some(self.brush_value()?); self.semi()?; },
                "Border" => template.border = Some(self.border()?),
                "Text" => template.text = Some(self.text()?),
                "PointerArea" => template.clickable = self.pointer_area()?,
                _ => {
                    return Err(self.error(&format!(
                        "Unsupported Rectangle field or primitive '{name}'"
                    )))
                }
            }
        }
        self.close()?;
        if clip_depth!=0{return Err(self.error("Unclosed content clip"));}
        Ok(template)
    }
    fn content(&mut self, kind: &str) -> Result<Content,String> {
        self.open()?;
        if kind=="ContentClipEnd"{self.close()?;return Ok(Content::ClipEnd);}
        let mut bounds=[0.;4];let mut text=Text{text:String::new(),color:[255;4],font_size:16.};let mut points=None;
        let mut radius=0.;
        let mut seen=HashSet::new();
        while self.current.kind!=Kind::Close {
            let name=self.ident()?;self.unique(&mut seen,&name)?;self.colon()?;
            match (kind,name.as_str()) {
                ("ContentText"|"ContentClip","x"|"y")=>{let v=match self.current.kind {Kind::Number(v,Unit::Logical|Unit::Px) if v.is_finite()&&v.abs()<=100_000.=>v,_=>return Err(self.error("Invalid content coordinate"))};bounds[if name=="x"{0}else{1}]=v;self.advance()?;},
                ("ContentText"|"ContentClip","width"|"height")=>bounds[if name=="width"{2}else{3}]=self.scalar(&name,false)?,
                ("ContentClip","radius")=>radius=self.scalar(&name,false)?,
                ("ContentText","text")=>text.text=self.text_value()?,
                ("ContentText","fontSize")=>text.font_size=self.scalar(&name,true)?,
                (_,"color")=>text.color=self.color("color")?,
                ("ContentShape","points")=>{
                    let raw=self.text_value()?;let values:Result<Vec<f32>,_>=raw.split_whitespace().map(str::parse::<f32>).collect();
                    let values=values.map_err(|_|self.error("Invalid polygon points"))?;
                    if values.len()<6||values.len()>8192||values.len()%2!=0||values.iter().any(|v|!v.is_finite()||v.abs()>100_000.){return Err(self.error("Invalid polygon points"));}
                    points=Some(values.chunks_exact(2).map(|v|[v[0],v[1]]).collect());
                },
                _=>return Err(self.error(&format!("Unsupported {kind} property {name}"))),
            }self.semi()?;
        }self.close()?;
        if kind=="ContentShape" {Ok(Content::Shape{points:points.ok_or_else(||self.error("Shape requires points"))?,color:text.color})}
        else {if !seen.contains("width")||!seen.contains("height"){return Err(self.error("Content requires width and height"));}if kind=="ContentClip"{Ok(Content::Clip{bounds,radius})}else{Ok(Content::Text{bounds,text})}}
    }
    fn template(&mut self) -> Result<Template, String> {
        self.expect(Kind::Ident("component".into()), "'component'")?;
        self.expect(Kind::Ident("Button".into()), "Button component name")?;
        self.open()?;
        let mut seen=HashSet::new();
        while self.current.kind!=Kind::Ident("Rectangle".into()) {
            let name=self.ident()?;let name=if name=="font.size"{"fontSize".to_string()}else{name};
            self.unique(&mut seen,&name)?;self.colon()?;
            // Defaults are literals: no ordering-dependent references or cycles.
            if matches!(&self.current.kind,Kind::Ident(r) if r.starts_with("props.")){return Err(self.error("Component defaults must be literals"));}
            let apply=!self.props.specified.contains(&name);
            match name.as_str(){
                "width"|"height"|"radius"|"fontSize"|"borderWidth"=>{let v=self.scalar(&name,matches!(name.as_str(),"width"|"height"|"fontSize"))?;if apply{match name.as_str(){"width"=>self.props.width=v,"height"=>self.props.height=v,"radius"=>self.props.radius=v,"fontSize"=>self.props.font_size=v,_=>{self.props.numbers.insert(name.clone(),v);}}}},
                "background"|"color"|"hoverBackground"|"pressedBackground"|"disabledBackground"|"borderColor"|"focusBorderColor"=>{let v=self.color(&name)?;if apply{match name.as_str(){"background"=>self.props.background=v,"color"=>self.props.color=v,_=>{self.props.colors.insert(name.clone(),v);}}}},
                "text"=>{let v=self.text_value()?;if apply{self.props.text=v;}},
                "disabled"=>{let v=match &self.current.kind{Kind::Ident(v)if v=="true"=>true,Kind::Ident(v)if v=="false"=>false,_=>return Err(self.error("disabled requires true or false"))};self.advance()?;if apply{self.props.disabled=v;}},
                "transitionDuration"=>{let v=self.duration()?;if apply{self.props.numbers.insert(name.clone(),v);}},
                _=>return Err(self.error(&format!("Unsupported component property '{name}'"))),
            }
            self.semi()?;
        }
        self.expect(
            Kind::Ident("Rectangle".into()),
            "exactly one Rectangle root",
        )?;
        let template = self.rectangle()?;
        self.close()?;
        if self.current.kind != Kind::Eof {
            return Err(self.error("Unexpected content after Button template"));
        }
        Ok(template)
    }
}

pub fn parse(source: &str, props: &ButtonSpec) -> Result<Template, String> {
    Parser::new(source, props)?.template()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "component Button { Rectangle { radius: props.radius; background: Brush { color: props.background; hover: #a8baff; pressed: #6a83da; disabled: #596273; transition: 140ms; }; Border { width: 1; background: Brush { color: #bed0ff; focus: #ffffff; transition: 100ms; }; } Text { text: props.text; color: props.color; font.size: props.font.size; } PointerArea { clicked -> events.clicked(); } } }";
    fn props() -> ButtonSpec {
        ButtonSpec {
            text: "Найти".into(),
            key: "run".into(),
            ..ButtonSpec::default()
        }
    }
    #[test]
    fn component_defaults_and_explicit_instance_overrides() {
        let source = include_str!("../examples/Button.component.ui");
        let scene = crate::markup::parse("component Demo { Frame { Button {} } }").unwrap();
        let t = parse(source, &scene.button).unwrap();
        assert_eq!((t.props.width, t.props.height), (100., 40.));
        assert_eq!(t.text.unwrap().text, "Кнопка");
        assert_eq!(t.border.unwrap().brush.duration_ms, 140.);
        let scene = crate::markup::parse("component Demo { Frame { Button { width: 60; fontSize: 22; text: 'Моя'; borderWidth: 0; hoverBackground: #123456; transitionDuration: 80ms; disabled: false; } } }").unwrap();
        let t = parse(&source.replace("disabled: false;", "disabled: true;"), &scene.button).unwrap();
        assert_eq!(t.props.width, 60.);
        assert!(!t.props.disabled);
        assert_eq!(t.text.as_ref().unwrap().font_size, 22.);
        assert_eq!(t.text.unwrap().text, "Моя");
        assert_eq!(t.border.unwrap().width, 0.);
        let fill = t.fill.unwrap();
        assert_eq!(fill.hover, Some([18, 52, 86, 255]));
        assert_eq!(fill.duration_ms, 80.);
    }

    #[test]
    fn invalid_component_defaults_are_diagnostics() {
        for fields in ["width: 0;", "width: 10; width: 20;", "fontSize: 12; font.size: 14;", "width: props.width;", "unknown: 1;", "disabled: 1;", "transitionDuration: 20;"] {
            assert!(parse(&format!("component Button {{ {fields} Rectangle {{}} }}"), &props()).is_err(), "{fields}");
        }
    }
    fn body(source: &str) -> Result<Template, String> {
        parse(
            &format!("component Button {{ Rectangle {{ {source} }} }}"),
            &props(),
        )
    }
    #[test]
    fn brushes_are_property_values_with_color_shorthand() {
        let t=body("background: #123456; Border { background: #ffffff; }").unwrap();
        assert_eq!(t.fill.unwrap().color,[18,52,86,255]);
        assert_eq!(t.border.unwrap().brush.color,[255;4]);
        assert!(body("background: Brush { color:#fff; }").is_err());
        assert!(body("Border { Brush { color:#fff; } }").is_err());
    }

    #[test]
    fn evaluates_the_complete_primitive_template() {
        let props = props();
        let template = parse(SOURCE, &props).unwrap();
        assert_eq!(template.radius, props.radius);
        assert!(template.clickable);
        let fill = template.fill.unwrap();
        assert_eq!(fill.color, props.background);
        assert_eq!(fill.hover, Some([168, 186, 255, 255]));
        assert_eq!(fill.pressed, Some([106, 131, 218, 255]));
        assert_eq!(fill.disabled, Some([89, 98, 115, 255]));
        assert_eq!(fill.duration_ms, 140.);
        assert_eq!(fill.focus, None);
        let border = template.border.unwrap();
        assert_eq!(border.width, 1.);
        assert_eq!(border.brush.color, [190, 208, 255, 255]);
        assert_eq!(border.brush.focus, Some([255; 4]));
        assert_eq!(border.brush.duration_ms, 100.);
        let text = template.text.unwrap();
        assert_eq!(text.text, "Найти");
        assert_eq!(text.color, props.color);
        assert_eq!(text.font_size, props.font_size);
    }

    #[test]
    fn literals_comments_and_css_colors_work() {
        let template = body("// shape\nradius: 12px; /* a /* nested */ comment */ background: Brush { color: #AbC; hover: #12345678; transition: 0ms; }; Text { text: \"Кнопка \\\"ОК\\\"\\n\"; font.size: 2e1; color: #000; }").unwrap();
        assert_eq!(template.radius, 12.);
        assert_eq!(template.fill.as_ref().unwrap().color, [170, 187, 204, 255]);
        assert_eq!(template.fill.unwrap().hover, Some([18, 52, 86, 120]));
        assert_eq!(template.text.as_ref().unwrap().text, "Кнопка \"ОК\"\n");
        assert_eq!(template.text.unwrap().font_size, 20.);
    }

    #[test]
    fn omitted_primitives_are_absent_and_do_not_get_painted() {
        assert_eq!(
            body("").unwrap(),
            Template {
                props: props(),
                content: Vec::new(),
                radius: 0.,
                fill: None,
                border: None,
                text: None,
                clickable: false
            }
        );
        assert!(!body("PointerArea {}").unwrap().clickable);
        assert!(
            body("PointerArea { clicked -> events.clicked(); }")
                .unwrap()
                .clickable
        );
        let border = body("Border { background: Brush { color: #fff; }; }")
            .unwrap()
            .border
            .unwrap();
        assert_eq!(border.width, 1.);
        assert_eq!(border.brush.duration_ms, 0.);
    }

    #[test]
    fn references_are_typed_and_re_evaluated_for_each_instance() {
        let mut props = props();
        props.text = "Другая кнопка".into();
        props.radius = 8.;
        let template = parse(SOURCE, &props).unwrap();
        assert_eq!(template.text.unwrap().text, "Другая кнопка");
        assert_eq!(template.radius, 8.);
        assert_eq!(
            body("Text { text: props.key; }")
                .unwrap()
                .text
                .unwrap()
                .text,
            "run"
        );
        for source in [
            "radius: props.color;",
            "radius: state.radius;",
            "radius: props.missing;",
            "Text { text: props.radius; }",
            "background: Brush { color: props.text; };",
            "background: Brush { color: state.background; };",
            "Text { font.size: props.font.weight; }",
            "Text { text: props.text(); }",
        ] {
            assert!(body(source).is_err(), "accepted {source}");
        }
    }

    #[test]
    fn rejects_duplicate_primitives_properties_and_missing_required_values() {
        for source in [
            "background: Brush { color:#fff; }; background: Brush { color:#000; };",
            "Text {} Text {}",
            "Border { background: Brush {color:#fff;}; } Border { background: Brush {color:#fff;}; }",
            "PointerArea {} PointerArea {}",
            "radius:1; radius:2;",
            "background: Brush { color:#fff; color:#000; };",
            "background: Brush { color:#fff; hover:#fff; hover:#000; };",
            "Border { width:1; width:2; background: Brush {color:#fff;}; }",
            "Border { background: Brush {color:#fff;}; background: Brush {color:#000;}; }",
            "Text { text:'a'; text:'b'; }",
            "background: Brush {};",
            "Border {}",
        ] {
            assert!(body(source).is_err(), "accepted {source}");
        }
    }

    #[test]
    fn rejects_nonfinite_negative_oversize_and_wrong_unit_values() {
        for value in ["-1", "4097", "1e999", "NaN", "20ms", "10%", "*"] {
            assert!(
                body(&format!("radius:{value};")).is_err(),
                "accepted {value}"
            );
        }
        for value in ["-1ms", "2001ms", "140", "1px", "1s", "props.radius"] {
            assert!(
                body(&format!("background: Brush {{ color:#fff; transition:{value}; }};")).is_err(),
                "accepted {value}"
            );
        }
        assert!(
            body("radius:4096; Border { width:0; background: Brush { color:#fff; transition:2000ms; }; }")
                .is_ok()
        );
        assert!(body("Text { font.size:0; }").is_err());
        assert!(body("Border { width:-1; background: Brush {color:#fff;}; }").is_err());
        let mut props = props();
        props.radius = f32::NAN;
        assert!(parse(SOURCE, &props).is_err());
        props.radius = 1.;
        props.font_size = f32::INFINITY;
        assert!(parse(SOURCE, &props).is_err());
    }

    #[test]
    fn events_only_forward_the_declared_component_event() {
        for event in [
            "hovered -> events.clicked();",
            "clicked -> actions.search();",
            "clicked -> events.other();",
            "clicked -> events.clicked(1);",
            "clicked -> events.clicked(); clicked -> events.clicked();",
            "clicked => events.clicked();",
            "clicked: true;",
        ] {
            assert!(
                body(&format!("PointerArea {{ {event} }}")).is_err(),
                "accepted {event}"
            );
        }
    }

    #[test]
    fn unsupported_primitives_and_fields_are_not_silent() {
        for source in [
            "Rectangle {}",
            "Frame {}",
            "width:200;",
            "Brush { color:#fff; }",
            "background: Brush { color:#fff; easing:'spring'; };",
            "Border { radius:4; background: Brush {color:#fff;}; }",
            "Text { font.weight:700; }",
            "PointerArea { enabled:true; }",
        ] {
            let error = body(source).unwrap_err();
            assert!(error.contains("Unsupported"), "{source}: {error}");
        }
    }

    #[test]
    fn rejects_malformed_documents_and_colors() {
        for source in [
            "",
            "component Other { Rectangle {} }",
            "component Button {}",
            "component Button { Rectangle {} Rectangle {} }",
            "component Button { Rectangle {} } junk",
            "component Button { Rectangle {",
            "/* unclosed",
            "component Button { Rectangle {} } /* unclosed",
        ] {
            let error = parse(source, &props()).unwrap_err();
            assert!(error.contains("byte"), "no location: {error}");
        }
        for color in ["#ab", "#abcd", "#gggggg", "'#fff'", "rgb(0,0,0)"] {
            assert!(
                body(&format!("background: Brush {{ color:{color}; }};")).is_err(),
                "accepted {color}"
            );
        }
        assert!(body("Text { text:'unclosed; }").is_err());
        assert!(body("Text { text:'x' }").is_err());
    }
}
