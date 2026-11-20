//! Structured errors produced while evaluating `.ui` expressions.

/// A structured expression-evaluation failure, preserving error kind for optional chaining and
/// missing-state detection without inspecting formatted error strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::form_document) enum EvalError {
    MissingState(String),
    UnknownProperty(String),
    UnknownToken(String),
    UnknownI18nKey(String),
    UnknownEnumVariant(String),
    UnknownMember(String),
    UnknownListIndex(String),
    CannotReadMember(String),
    UnavailableMember(String),
    CyclicProperty(String),
    DivisionByZero,
    RangeExceedsLimit,
    NonFiniteNumber,
    NoMatchingBranch,
    TypeError(String),
    Message(String),
}

impl EvalError {
    pub(in crate::form_document) fn is_missing_state(&self) -> bool {
        matches!(self, Self::MissingState(_))
    }

    pub(in crate::form_document) fn is_optional_absent(&self) -> bool {
        matches!(
            self,
            Self::MissingState(_)
                | Self::UnknownProperty(_)
                | Self::UnknownMember(_)
                | Self::UnknownListIndex(_)
                | Self::CannotReadMember(_)
        )
    }

    pub(in crate::form_document) fn into_message(self) -> String {
        match self {
            Self::MissingState(key) => format!("Missing state.{key}"),
            Self::UnknownProperty(path) => format!("Unknown property {path}"),
            Self::UnknownToken(path) => format!("Unknown token {path}"),
            Self::UnknownI18nKey(path) => format!("Unknown i18n key {path}"),
            Self::UnknownEnumVariant(path) => format!("Unknown enum variant {path}"),
            Self::UnknownMember(member) => format!("Unknown member {member}"),
            Self::UnknownListIndex(idx) => format!("Unknown list index {idx}"),
            Self::CannotReadMember(member) => format!("Cannot read member {member}"),
            Self::UnavailableMember(member) => format!("Unavailable member {member}"),
            Self::CyclicProperty(key) => format!("Cyclic property {key}"),
            Self::DivisionByZero => "Division by zero".into(),
            Self::RangeExceedsLimit => "Range exceeds 4096 items".into(),
            Self::NonFiniteNumber => "Expression produced a non-finite number".into(),
            Self::NoMatchingBranch => "No matching branch; add _ => ...".into(),
            Self::TypeError(msg) | Self::Message(msg) => msg,
        }
    }
}

impl From<String> for EvalError {
    fn from(msg: String) -> Self {
        Self::Message(msg)
    }
}

impl From<&str> for EvalError {
    fn from(msg: &str) -> Self {
        Self::Message(msg.to_owned())
    }
}
