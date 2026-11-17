use std::fmt;
use std::io;

const MAX_MSG_CHARS: usize = 200;

fn truncate_msg(msg: &str) -> String {
    if msg.chars().count() <= MAX_MSG_CHARS {
        msg.to_string()
    } else {
        let mut s: String = msg.chars().take(MAX_MSG_CHARS - 1).collect();
        s.push('\u{2026}');
        s
    }
}

/// Error returned by log viewer operations.
///
/// Every variant provides a stable machine-readable [`ViewError::code`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ViewError {
    /// Underlying storage or I/O error.
    Io(String),
    /// Invalid filter argument or syntax.
    InvalidFilter(String),
    /// Requested byte offset exceeds the source length or options are invalid.
    OffsetOutOfRange(String),
}

impl ViewError {
    /// Constructs an I/O error with a sanitized message.
    #[must_use]
    pub fn io(msg: impl AsRef<str>) -> Self {
        Self::Io(truncate_msg(msg.as_ref()))
    }

    /// Constructs an invalid filter error with a sanitized message.
    #[must_use]
    pub fn invalid_filter(msg: impl AsRef<str>) -> Self {
        Self::InvalidFilter(truncate_msg(msg.as_ref()))
    }

    /// Constructs an offset-out-of-range error with a sanitized message.
    #[must_use]
    pub fn offset_out_of_range(msg: impl AsRef<str>) -> Self {
        Self::OffsetOutOfRange(truncate_msg(msg.as_ref()))
    }

    /// Stable machine-readable error code (`"io"`, `"invalid_filter"`, or `"offset_out_of_range"`).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::InvalidFilter(_) => "invalid_filter",
            Self::OffsetOutOfRange(_) => "offset_out_of_range",
        }
    }

    /// Human-readable error detail, capped at 200 characters.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Io(m) | Self::InvalidFilter(m) | Self::OffsetOutOfRange(m) => m,
        }
    }
}

impl fmt::Display for ViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for ViewError {}

impl From<io::Error> for ViewError {
    fn from(err: io::Error) -> Self {
        Self::io(err.to_string())
    }
}
