//! The kind of a document node.

use std::sync::Arc;

/// Every node name defined natively by the `.ui` language or its template target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuiltinKind {
    Frame,
    Row,
    Column,
    Grid,
    Stack,
    Scroll,
    Modal,
    Text,
    TextInput,
    Canvas,
    Image,
    Rectangle,
    ContentPresenter,
    Brush,
    Border,
    Reveal,
    PointerArea,
    ContentText,
    ContentShape,
    ContentClip,
    ContentClipEnd,
    ContentInput,
    ContentLink,
    RangeInput,
    Minimum,
    Maximum,
    Button,
    Slider,
    If,
    Else,
    For,
    Empty,
    Let,
    Match,
    Branch,
}

impl BuiltinKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frame => "Frame",
            Self::Row => "Row",
            Self::Column => "Column",
            Self::Grid => "Grid",
            Self::Stack => "Stack",
            Self::Scroll => "Scroll",
            Self::Modal => "Modal",
            Self::Text => "Text",
            Self::TextInput => "TextInput",
            Self::Canvas => "Canvas",
            Self::Image => "Image",
            Self::Rectangle => "Rectangle",
            Self::ContentPresenter => "ContentPresenter",
            Self::Brush => "Brush",
            Self::Border => "Border",
            Self::Reveal => "Reveal",
            Self::PointerArea => "PointerArea",
            Self::ContentText => "ContentText",
            Self::ContentShape => "ContentShape",
            Self::ContentClip => "ContentClip",
            Self::ContentClipEnd => "ContentClipEnd",
            Self::ContentInput => "ContentInput",
            Self::ContentLink => "ContentLink",
            Self::RangeInput => "RangeInput",
            Self::Minimum => "Minimum",
            Self::Maximum => "Maximum",
            Self::Button => "Button",
            Self::Slider => "Slider",
            Self::If => "If",
            Self::Else => "Else",
            Self::For => "For",
            Self::Empty => "Empty",
            Self::Let => "Let",
            Self::Match => "Match",
            Self::Branch => "Branch",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "Frame" => Self::Frame,
            "Row" => Self::Row,
            "Column" => Self::Column,
            "Grid" => Self::Grid,
            "Stack" => Self::Stack,
            "Scroll" => Self::Scroll,
            "Modal" => Self::Modal,
            "Text" => Self::Text,
            "TextInput" => Self::TextInput,
            "Canvas" => Self::Canvas,
            "Image" => Self::Image,
            "Rectangle" => Self::Rectangle,
            "ContentPresenter" => Self::ContentPresenter,
            "Brush" => Self::Brush,
            "Border" => Self::Border,
            "Reveal" => Self::Reveal,
            "PointerArea" => Self::PointerArea,
            "ContentText" => Self::ContentText,
            "ContentShape" => Self::ContentShape,
            "ContentClip" => Self::ContentClip,
            "ContentClipEnd" => Self::ContentClipEnd,
            "ContentInput" => Self::ContentInput,
            "ContentLink" => Self::ContentLink,
            "RangeInput" => Self::RangeInput,
            "Minimum" => Self::Minimum,
            "Maximum" => Self::Maximum,
            "Button" => Self::Button,
            "Slider" => Self::Slider,
            "If" => Self::If,
            "Else" => Self::Else,
            "For" => Self::For,
            "Empty" => Self::Empty,
            "Let" => Self::Let,
            "Match" => Self::Match,
            "Branch" => Self::Branch,
            _ => return None,
        })
    }

    pub const fn is_layout(self) -> bool {
        matches!(
            self,
            Self::Frame | Self::Row | Self::Column | Self::Grid | Self::Stack | Self::Scroll
        )
    }

    pub const fn is_control_flow(self) -> bool {
        matches!(
            self,
            Self::If
                | Self::Else
                | Self::For
                | Self::Empty
                | Self::Let
                | Self::Match
                | Self::Branch
        )
    }

    pub const fn is_flattenable(self) -> bool {
        matches!(
            self,
            Self::Frame
                | Self::Row
                | Self::Column
                | Self::Grid
                | Self::Stack
                | Self::Text
                | Self::TextInput
                | Self::Image
                | Self::Rectangle
        )
    }

    pub const fn is_primitive(self) -> bool {
        (self.is_layout() && !matches!(self, Self::Scroll))
            || matches!(
                self,
                Self::Text
                    | Self::TextInput
                    | Self::Canvas
                    | Self::Image
                    | Self::Rectangle
                    | Self::ContentPresenter
                    | Self::Brush
                    | Self::Border
                    | Self::Reveal
                    | Self::PointerArea
                    | Self::ContentText
                    | Self::ContentShape
                    | Self::ContentClip
                    | Self::ContentClipEnd
            )
    }

    pub const fn is_intrinsic_ast_node(self) -> bool {
        matches!(
            self,
            Self::Frame
                | Self::Scroll
                | Self::Row
                | Self::Column
                | Self::Grid
                | Self::Stack
                | Self::If
                | Self::Else
                | Self::For
                | Self::Empty
                | Self::Modal
                | Self::Rectangle
                | Self::Text
                | Self::TextInput
                | Self::Image
                | Self::PointerArea
                | Self::RangeInput
                | Self::ContentPresenter
                | Self::Let
                | Self::Match
                | Self::Branch
        )
    }
}

/// A node's kind: a built-in name or the name of a component. Reads as a string.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeKind(Name);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Name {
    Builtin(BuiltinKind),
    Component(Arc<str>),
}

impl NodeKind {
    pub const fn from_builtin(kind: BuiltinKind) -> Self {
        Self(Name::Builtin(kind))
    }

    pub const fn builtin(&self) -> Option<BuiltinKind> {
        match self.0 {
            Name::Builtin(kind) => Some(kind),
            Name::Component(_) => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match &self.0 {
            Name::Builtin(kind) => kind.as_str(),
            Name::Component(name) => name,
        }
    }

    pub const fn is_layout(&self) -> bool {
        matches!(self.0, Name::Builtin(kind) if kind.is_layout())
    }

    pub const fn is_control_flow(&self) -> bool {
        matches!(self.0, Name::Builtin(kind) if kind.is_control_flow())
    }

    pub const fn is_flattenable(&self) -> bool {
        matches!(self.0, Name::Builtin(kind) if kind.is_flattenable())
    }

    pub const fn is_primitive(&self) -> bool {
        matches!(self.0, Name::Builtin(kind) if kind.is_primitive())
    }
}

impl From<BuiltinKind> for NodeKind {
    fn from(kind: BuiltinKind) -> Self {
        Self::from_builtin(kind)
    }
}

impl From<&str> for NodeKind {
    fn from(name: &str) -> Self {
        Self(match BuiltinKind::parse(name) {
            Some(kind) => Name::Builtin(kind),
            None => Name::Component(name.into()),
        })
    }
}

impl From<String> for NodeKind {
    fn from(name: String) -> Self {
        name.as_str().into()
    }
}

impl From<&String> for NodeKind {
    fn from(name: &String) -> Self {
        name.as_str().into()
    }
}

impl std::ops::Deref for NodeKind {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<BuiltinKind> for NodeKind {
    fn eq(&self, other: &BuiltinKind) -> bool {
        self.builtin() == Some(*other)
    }
}

impl PartialEq<str> for NodeKind {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for NodeKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for NodeKind {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Debug for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}

impl serde::Serialize for NodeKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for NodeKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_reads_as_its_name_whether_built_in_or_a_component() {
        for name in ["Frame", "Else", "Slider", "ContentInput", "TaskButton", ""] {
            let kind = NodeKind::from(name);
            assert_eq!(kind.as_str(), name);
            assert_eq!(kind, name);
            assert_eq!(kind, name.to_string());
            assert_eq!(kind, NodeKind::from(name.to_string()));
            assert_eq!(kind.to_string(), name);
            assert_eq!(format!("{kind:?}"), format!("{name:?}"));
            assert!(kind.starts_with(name));
        }
        assert_ne!(NodeKind::from("Frame"), NodeKind::from("frame"));
        assert_ne!(NodeKind::from("Frame"), "Row");
        assert_eq!(NodeKind::from("Frame"), BuiltinKind::Frame);
        assert!(NodeKind::from("Row").is_layout());
        assert!(NodeKind::from("If").is_control_flow());
        assert!(NodeKind::from("Text").is_flattenable());
        assert!(!NodeKind::from("Scroll").is_primitive());
    }

    #[test]
    fn built_in_names_are_shared_and_component_names_are_counted() {
        assert!(matches!(NodeKind::from("Frame").0, Name::Builtin(_)));
        let component = NodeKind::from("TaskButton");
        let Name::Component(name) = &component.0 else {
            panic!("a component name was taken for a built-in");
        };
        let copy = component.clone();
        assert_eq!(Arc::strong_count(name), 2);
        drop(copy);
    }

    #[test]
    fn a_kind_serializes_as_a_plain_string() {
        for name in ["Frame", "TaskButton"] {
            let json = serde_json::to_string(&NodeKind::from(name)).unwrap();
            assert_eq!(json, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<NodeKind>(&json).unwrap(), name);
        }
    }
}
