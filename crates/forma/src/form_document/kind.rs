//! The kind of a document node.

use std::sync::Arc;

/// The names the language itself defines. A kind with one of them holds no allocation, so it is
/// free to copy; expansion and rendering copy the kind of every node they emit.
const BUILTIN: [&str; 29] = [
    "Frame",
    "Row",
    "Column",
    "Grid",
    "Stack",
    "Scroll",
    "Text",
    "TextInput",
    "Canvas",
    "Image",
    "Rectangle",
    "ContentPresenter",
    "Brush",
    "Border",
    "Reveal",
    "PointerArea",
    "ContentText",
    "ContentShape",
    "ContentClip",
    "ContentClipEnd",
    "Button",
    "Slider",
    "If",
    "Else",
    "For",
    "Empty",
    "Let",
    "Match",
    "Branch",
];

/// A node's kind: a built-in name or the name of a component. Reads as a string.
#[derive(Clone, PartialEq, Eq)]
pub struct NodeKind(Name);

#[derive(Clone, PartialEq, Eq)]
enum Name {
    Builtin(&'static str),
    Component(Arc<str>),
}

impl NodeKind {
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Name::Builtin(name) => name,
            Name::Component(name) => name,
        }
    }
}

impl From<&str> for NodeKind {
    fn from(name: &str) -> Self {
        Self(match BUILTIN.iter().find(|builtin| **builtin == name) {
            Some(builtin) => Name::Builtin(builtin),
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
        for name in ["Frame", "Else", "Slider", "TaskButton", ""] {
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
