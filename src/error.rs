//! Internal drop-in replacement for `anyhow`.
//!
//! Provides a small, owned error type with `?` conversions and a
//! `Context` trait that mirrors `anyhow::Context` for `Result` and
//! `Option`.

use std::fmt;

/// Fract's internal error type.
#[derive(Debug)]
pub struct Error {
    message: String,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

/// Convenience alias used throughout `fract`.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Construct a new error from a message.
    pub fn new<M: Into<String>>(msg: M) -> Self {
        Self {
            message: msg.into(),
            source: None,
        }
    }

    /// Construct an error with a message and a source error.
    pub fn from_source<M: Into<String>>(
        msg: M,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            message: msg.into(),
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(source) = &self.source {
            write!(f, ": {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Self::new(msg)
    }
}

impl From<&str> for Error {
    fn from(msg: &str) -> Self {
        Self::new(msg.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::from_source(err.to_string(), err)
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Self::from_source(err.to_string(), err)
    }
}

impl From<toml::ser::Error> for Error {
    fn from(err: toml::ser::Error) -> Self {
        Self::from_source(err.to_string(), err)
    }
}

impl From<notify::Error> for Error {
    fn from(err: notify::Error) -> Self {
        Self::from_source(err.to_string(), err)
    }
}

impl From<git2::Error> for Error {
    fn from(err: git2::Error) -> Self {
        Self::from_source(err.to_string(), err)
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::from_source(err.to_string(), err)
    }
}

impl From<crate::cli::Error> for Error {
    fn from(err: crate::cli::Error) -> Self {
        Self::new(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::new(format!("JSON error: {}", err))
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(err: serde_yaml::Error) -> Self {
        Self::new(format!("YAML error: {}", err))
    }
}

/// Mirrors `anyhow::Context` for `fract::Result`.
pub trait Context<T> {
    /// Wrap a potential error with a static message.
    ///
    /// # Errors
    /// Returns an error carrying `msg` (with the original error as source)
    /// when `self` is `Err` or `None`.
    fn context<M: Into<String>>(self, msg: M) -> Result<T>;

    /// Lazily construct the context message only on error.
    ///
    /// # Errors
    /// Returns an error carrying `f()` (with the original error as source)
    /// when `self` is `Err` or `None`.
    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> Context<T> for std::result::Result<T, E> {
    fn context<M: Into<String>>(self, msg: M) -> Result<T> {
        self.map_err(|err| Error {
            message: msg.into(),
            source: Some(Box::new(err)),
        })
    }

    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T> {
        self.map_err(|err| Error {
            message: f(),
            source: Some(Box::new(err)),
        })
    }
}

impl<T> Context<T> for Option<T> {
    fn context<M: Into<String>>(self, msg: M) -> Result<T> {
        self.ok_or_else(|| Error::new(msg))
    }

    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T> {
        self.ok_or_else(|| Error::new(f()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Context, Error, Result};
    use std::error::Error as _;

    #[test]
    fn error_from_string() {
        let err: Error = "plain message".into();
        assert_eq!(err.to_string(), "plain message");
    }

    #[test]
    fn io_error_converts() {
        fn fallible() -> Result<()> {
            std::fs::read_to_string("/tmp/fract_error_test_nonexistent_12345")?;
            Ok(())
        }
        let err = fallible().unwrap_err();
        assert!(err.to_string().contains("No such file"));
    }

    #[test]
    fn toml_de_error_converts() {
        fn fallible() -> Result<()> {
            let _: std::collections::HashMap<String, String> = toml::from_str("not valid")?;
            Ok(())
        }
        let err = fallible().unwrap_err();
        assert!(err.to_string().contains("expected"));
    }

    #[test]
    fn toml_ser_error_converts() {
        #[derive(serde::Serialize)]
        struct Bad;
        fn fallible() -> Result<()> {
            toml::to_string_pretty(&Bad)?;
            Ok(())
        }
        let err = fallible().unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn result_context_overrides_message() {
        fn fallible() -> Result<()> {
            std::fs::read_to_string("/tmp/fract_error_test_missing_12345")
                .context("loading config")?;
            Ok(())
        }
        let err = fallible().unwrap_err();
        assert!(err.to_string().starts_with("loading config"));
        assert!(err.source().is_some());
    }

    #[test]
    fn result_with_context_lazy() {
        fn fallible() -> std::result::Result<(), std::io::Error> {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
        }
        let err = fallible()
            .with_context(|| "lazy context".to_string())
            .unwrap_err();
        assert_eq!(err.to_string(), "lazy context: missing");
    }

    #[test]
    fn option_context_provides_message() {
        let opt: Option<i32> = None;
        let err = opt.context("value missing").unwrap_err();
        assert_eq!(err.to_string(), "value missing");
        assert!(err.source().is_none());
    }

    #[test]
    fn ok_result_unwraps() {
        let result: std::result::Result<i32, std::io::Error> = Ok(42);
        assert_eq!(result.context("never shown").unwrap(), 42);
    }

    #[test]
    fn some_option_unwraps() {
        let opt: Option<i32> = Some(7);
        assert_eq!(opt.context("never shown").unwrap(), 7);
    }

    #[test]
    fn error_chain_source() {
        let inner = std::io::Error::other("inner");
        let outer = Error::from_source("outer", inner);
        assert_eq!(outer.to_string(), "outer: inner");
        assert!(outer.source().is_some());
    }
}
