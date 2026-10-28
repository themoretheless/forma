/// Shared geometry error. Codes stay crate-specific; the type is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}
impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn contains(&self, needle: &str) -> bool {
        self.message.contains(needle)
    }
    /// Shared precondition check. Codes stay crate-specific at the call site.
    #[inline]
    pub fn ensure(ok: bool, code: &'static str, message: impl Into<String>) -> Result<()> {
        if ok {
            Ok(())
        } else {
            Err(Self::new(code, message))
        }
    }
}
/// Same as [`Error::ensure`]; free function for crate `check` wrappers.
#[inline]
pub fn ensure(ok: bool, code: &'static str, message: impl Into<String>) -> Result<()> {
    Error::ensure(ok, code, message)
}
impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.message
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
