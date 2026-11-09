//! Errors returned by the control models.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    InvalidRange,
    NonFiniteValue,
    DuplicateItem(usize),
    UnknownItem(usize),
    InvalidTreeOrder(usize),
    InvalidTextBoundary(usize),
    MultilineText,
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRange => {
                write!(f, "range needs finite min <= max and positive finite step")
            }
            Self::NonFiniteValue => write!(f, "value must be finite"),
            Self::DuplicateItem(id) => write!(f, "duplicate item {id}"),
            Self::UnknownItem(id) => write!(f, "unknown item {id}"),
            Self::InvalidTreeOrder(id) => {
                write!(f, "tree item {id} must follow its parent in preorder")
            }
            Self::InvalidTextBoundary(offset) => write!(f, "invalid UTF-8 text boundary {offset}"),
            Self::MultilineText => write!(f, "single-line text cannot contain CR or LF"),
        }
    }
}
impl std::error::Error for ModelError {}
