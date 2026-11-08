use core::fmt;
use ghostty_sys as sys;

/// Errors reported by libghostty-vt (`GhosttyResult` codes other than
/// `GHOSTTY_SUCCESS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// An allocation failed.
    OutOfMemory,
    /// An argument was not valid.
    InvalidValue,
    /// A buffer was too small.
    OutOfSpace,
    /// The requested value has no value.
    NoValue,
    /// Reading from or writing to external I/O failed.
    Io,
    /// Encoded input exceeded a configured limit.
    LimitExceeded,
    /// A safety check rejected the operation (e.g. unsafe paste data).
    Rejected,
    /// The library cannot do what was asked, for example because the
    /// feature was left out of this build.
    Unsupported,
    /// A result code this crate doesn't know.
    Unknown(i32),
}

/// Result type of fallible libghostty-vt operations.
pub type Result<T, E = Error> = core::result::Result<T, E>;

impl Error {
    /// Converts a `GhosttyResult` into `Ok(())` or the matching error.
    pub(crate) fn check(code: sys::GhosttyResult) -> Result<()> {
        Err(match code {
            sys::GHOSTTY_SUCCESS => return Ok(()),
            sys::GHOSTTY_OUT_OF_MEMORY => Self::OutOfMemory,
            sys::GHOSTTY_INVALID_VALUE => Self::InvalidValue,
            sys::GHOSTTY_OUT_OF_SPACE => Self::OutOfSpace,
            sys::GHOSTTY_NO_VALUE => Self::NoValue,
            sys::GHOSTTY_IO_ERROR => Self::Io,
            sys::GHOSTTY_LIMIT_EXCEEDED => Self::LimitExceeded,
            sys::GHOSTTY_REJECTED => Self::Rejected,
            sys::GHOSTTY_UNSUPPORTED => Self::Unsupported,
            other => Self::Unknown(other),
        })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfMemory => f.write_str("out of memory"),
            Self::InvalidValue => f.write_str("invalid value"),
            Self::OutOfSpace => f.write_str("buffer too small"),
            Self::NoValue => f.write_str("no value"),
            Self::Io => f.write_str("I/O error"),
            Self::LimitExceeded => f.write_str("limit exceeded"),
            Self::Rejected => f.write_str("rejected by safety check"),
            Self::Unsupported => f.write_str("unsupported"),
            Self::Unknown(code) => write!(f, "unknown libghostty-vt error {code}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for std::io::Error {
    fn from(err: Error) -> Self {
        let kind = match err {
            Error::OutOfMemory => std::io::ErrorKind::OutOfMemory,
            Error::InvalidValue => std::io::ErrorKind::InvalidInput,
            Error::Unsupported => std::io::ErrorKind::Unsupported,
            _ => std::io::ErrorKind::Other,
        };
        std::io::Error::new(kind, err)
    }
}
