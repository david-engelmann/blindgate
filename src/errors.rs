use std::fmt;

/// An error the tool reports in words instead of a traceback.
///
/// This is the counterpart of the Python BlindgateError, and the
/// message it carries is the whole of what a caller prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlindgateError {
    message: String,
}

impl BlindgateError {
    pub fn new(message: impl Into<String>) -> Self {
        BlindgateError {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for BlindgateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BlindgateError {}
