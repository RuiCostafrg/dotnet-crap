//! Typed library errors.
//!
//! Every fallible library operation returns [`Error`]. Match on
//! [`Error::kind`] for the broad cause; message text is diagnostic and may
//! change. `Display` writes the outermost message. The alternate form
//! (`{:#}`) appends each `source()` separated by `": "`, which is identical to
//! the `anyhow` rendering used by the CLI, so diagnostics are unchanged when an
//! `Error` is converted into `anyhow::Error`.
use std::{error::Error as StdError, fmt};

/// Boxed underlying cause.
type Source = Box<dyn StdError + Send + Sync + 'static>;

/// Result alias for library operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Broad, matchable cause of an [`Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Invalid caller-supplied options, such as the threshold, root, input
    /// paths or collection policy.
    InvalidInput,
    /// A filesystem or other I/O operation failed.
    Io,
    /// A selected source could not be read as text or named as a native coverage
    /// path, or the C# parser could not be initialized.
    Source,
    /// A selected source is not syntactically valid C#.
    Syntax,
    /// Selected source bytes changed after they were scanned.
    SourceChanged,
    /// Native coverage evidence is malformed, stale or inconsistent with sources.
    Coverage,
    /// A Cobertura report was rejected by the strict importer.
    Cobertura,
    /// A syntax traversal or complexity budget was exceeded.
    ResourceLimit,
    /// The trusted test command failed, timed out or produced unexpected results.
    Collection,
    /// An output destination was rejected or could not be written.
    Output,
    /// A report could not be serialized.
    Render,
}

/// Error returned by `dotnet_crap` operations.
pub struct Error {
    kind: ErrorKind,
    repr: Repr,
}

/// Message-bearing errors own their text; transparent errors display their cause.
enum Repr {
    Message {
        message: String,
        source: Option<Source>,
    },
    Transparent(Source),
}

impl Error {
    /// Create an error with a message and no underlying cause.
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            repr: Repr::Message {
                message: message.into(),
                source: None,
            },
        }
    }

    /// Return the broad cause of this error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Return the wrapped underlying error, if any, for inspection or downcasting.
    ///
    /// A classified foreign error (for example a `std::io::Error`) is returned
    /// even though `Display` already shows its text and `source()` therefore
    /// starts below it. A message with a cause returns that cause.
    pub fn get_ref(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        match &self.repr {
            Repr::Message { source, .. } => source.as_deref(),
            Repr::Transparent(inner) => Some(inner.as_ref()),
        }
    }

    /// Wrap this error under `message`, keeping its kind.
    pub(crate) fn wrap(self, message: impl Into<String>) -> Self {
        Self {
            kind: self.kind,
            repr: Repr::Message {
                message: message.into(),
                source: Some(Box::new(self)),
            },
        }
    }
}

impl fmt::Display for Error {
    /// Write the outermost message; the alternate form appends the source chain.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.repr {
            Repr::Message { message, .. } => f.write_str(message)?,
            Repr::Transparent(inner) => write!(f, "{inner}")?,
        }
        if f.alternate() {
            let mut cause = self.source();
            while let Some(error) = cause {
                write!(f, ": {error}")?;
                cause = error.source();
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Error {
    /// Show the kind and the complete diagnostic chain.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("message", &format!("{self:#}"))
            .finish()
    }
}

impl StdError for Error {
    /// Return the wrapped cause; a transparent error skips the cause it already
    /// displays, matching `thiserror`'s `#[error(transparent)]`. Use
    /// [`Error::get_ref`] to inspect or downcast that cause.
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.repr {
            Repr::Message { source, .. } => source.as_deref().map(|s| s as _),
            Repr::Transparent(inner) => inner.source(),
        }
    }
}

/// Return early with a message-only [`Error`] of the given kind.
macro_rules! fail {
    ($kind:ident, $($arg:tt)+) => {
        return Err($crate::error::Error::new(
            $crate::error::ErrorKind::$kind,
            format!($($arg)+),
        ))
    };
}
pub(crate) use fail;

/// Attach a kind and optional context message to foreign or library failures.
pub(crate) trait ResultExt<T> {
    /// Wrap the failure under `message`, keeping it as the source.
    fn context(self, kind: ErrorKind, message: &str) -> Result<T>;
    /// Like [`ResultExt::context`] with a lazily built message.
    fn with_context(self, kind: ErrorKind, message: impl FnOnce() -> String) -> Result<T>;
    /// Classify the failure without adding a message; its text is unchanged.
    fn kind(self, kind: ErrorKind) -> Result<T>;
}

impl<T, E: StdError + Send + Sync + 'static> ResultExt<T> for std::result::Result<T, E> {
    fn context(self, kind: ErrorKind, message: &str) -> Result<T> {
        self.with_context(kind, || message.to_owned())
    }
    fn with_context(self, kind: ErrorKind, message: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|error| Error {
            kind,
            repr: Repr::Message {
                message: message(),
                source: Some(Box::new(error)),
            },
        })
    }
    fn kind(self, kind: ErrorKind) -> Result<T> {
        self.map_err(|error| Error {
            kind,
            repr: Repr::Transparent(Box::new(error)),
        })
    }
}

/// Convert an absent value into a message-only error.
pub(crate) trait OptionExt<T> {
    /// Fail with `message` when the value is absent.
    fn context(self, kind: ErrorKind, message: &str) -> Result<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn context(self, kind: ErrorKind, message: &str) -> Result<T> {
        self.ok_or_else(|| Error::new(kind, message))
    }
}

impl From<std::io::Error> for Error {
    /// Classify an uncontextualized I/O failure without changing its text.
    fn from(error: std::io::Error) -> Self {
        Self {
            kind: ErrorKind::Io,
            repr: Repr::Transparent(Box::new(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Leaf message used to verify chain rendering.
    #[derive(Debug)]
    struct Leaf;
    impl fmt::Display for Leaf {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("leaf")
        }
    }
    impl StdError for Leaf {}

    /// Foreign error with its own cause, as `std::io::Error` or `tempfile` may have.
    #[derive(Debug)]
    struct Middle(Leaf);
    impl fmt::Display for Middle {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("middle")
        }
    }
    impl StdError for Middle {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    /// Alternate display must equal anyhow's `{:#}` for every representation.
    fn chains_render_like_anyhow() {
        let cases: Vec<(Error, anyhow::Error)> = vec![
            (
                Error::new(ErrorKind::Coverage, "outer"),
                anyhow::anyhow!("outer"),
            ),
            (
                Err::<(), _>(Middle(Leaf)).kind(ErrorKind::Io).unwrap_err(),
                anyhow::Error::new(Middle(Leaf)),
            ),
            (
                Err::<(), _>(Middle(Leaf))
                    .context(ErrorKind::Io, "ctx")
                    .unwrap_err(),
                anyhow::Error::new(Middle(Leaf)).context("ctx"),
            ),
            (
                Err::<(), _>(Middle(Leaf))
                    .kind(ErrorKind::Io)
                    .with_context(ErrorKind::Cobertura, || "outer".into())
                    .unwrap_err(),
                anyhow::Error::new(Middle(Leaf)).context("outer"),
            ),
            (
                None::<()>
                    .context(ErrorKind::Source, "absent")
                    .with_context(ErrorKind::Source, || "wrapped".into())
                    .unwrap_err(),
                anyhow::anyhow!("absent").context("wrapped"),
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(format!("{error:#}"), format!("{expected:#}"));
            assert_eq!(error.to_string(), expected.to_string());
            let converted = anyhow::Error::from(error);
            assert_eq!(format!("{converted:#}"), format!("{expected:#}"));
        }
    }

    #[test]
    /// Kinds are retained by every constructor, and Debug carries the full chain.
    fn kinds_and_debug_are_preserved() {
        let io = Error::from(std::io::Error::other("disk"));
        assert_eq!(io.kind(), ErrorKind::Io);
        assert_eq!(io.to_string(), "disk");
        let wrapped = Err::<(), _>(io)
            .context(ErrorKind::Output, "writing")
            .unwrap_err();
        assert_eq!(wrapped.kind(), ErrorKind::Output);
        assert_eq!(
            format!("{wrapped:?}"),
            r#"Error { kind: Output, message: "writing: disk" }"#
        );
        let failed = (|| -> Result<()> { fail!(ResourceLimit, "limit {}", 1) })().unwrap_err();
        assert_eq!(failed.kind(), ErrorKind::ResourceLimit);
        assert_eq!(failed.to_string(), "limit 1");
        assert!(failed.source().is_none());
        assert!(failed.get_ref().is_none());
    }

    #[test]
    /// Callers can downcast the classified or contextual cause; wrapping keeps the kind.
    fn get_ref_exposes_the_wrapped_cause() {
        let io = Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        let inner = io
            .get_ref()
            .and_then(|e| e.downcast_ref::<std::io::Error>());
        assert_eq!(
            inner.map(std::io::Error::kind),
            Some(std::io::ErrorKind::NotFound)
        );
        let wrapped = io.wrap("outer");
        assert_eq!(wrapped.kind(), ErrorKind::Io);
        assert_eq!(format!("{wrapped:#}"), "outer: gone");
        assert!(wrapped
            .get_ref()
            .and_then(|e| e.downcast_ref::<Error>())
            .is_some_and(|e| e.to_string() == "gone"));
    }
}
