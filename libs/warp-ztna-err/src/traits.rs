use std::error::Error as StdError;

use orion_error::prelude::{SourceErr, SourceRawErr};

use crate::error::{ErrorExt, Result, explain};
use crate::immut_str::ImmutStr;
use crate::reason::Reason;

/// Lift `Result<T, E>` (`E: Error`) into [`Result<T>`] with reason + context.
///
/// For an error that is already [`Error`], use [`rewrap`] instead (coherence
/// prevents a second `OrErr` impl on `Result<T, Error>`).
pub trait OrErr<T> {
    /// Map `Err(E)` with context; `E` becomes the source cause.
    fn or_err(self, reason: Reason, context: impl Into<ImmutStr>) -> Result<T>;

    /// Like [`or_err`](OrErr::or_err) but builds context lazily.
    fn or_err_with<C, F>(self, reason: Reason, context: F) -> Result<T>
    where
        C: Into<ImmutStr>,
        F: FnOnce() -> C;
}

impl<T, E> OrErr<T> for std::result::Result<T, E>
where
    E: StdError + Send + Sync + 'static,
{
    fn or_err(self, reason: Reason, context: impl Into<ImmutStr>) -> Result<T> {
        self.source_raw_err(reason, context.into().into_string())
    }

    fn or_err_with<C, F>(self, reason: Reason, context: F) -> Result<T>
    where
        C: Into<ImmutStr>,
        F: FnOnce() -> C,
    {
        self.source_raw_err(reason, context().into().into_string())
    }
}

/// Rewrap [`Result<T, Error>`] with a new reason (previous `Error` kept as source).
pub fn rewrap<T>(result: Result<T>, reason: Reason, context: impl Into<ImmutStr>) -> Result<T> {
    result.source_err(reason, context.into().into_string())
}

/// Lazy-context variant of [`rewrap`].
pub fn rewrap_with<T, C, F>(result: Result<T>, reason: Reason, context: F) -> Result<T>
where
    C: Into<ImmutStr>,
    F: FnOnce() -> C,
{
    result.source_err(reason, context().into().into_string())
}

/// Map `Err(E)` without attaching `E` as source (text-only context).
pub trait ExplainErr<T, E> {
    fn explain_err<C, F>(self, reason: Reason, context: F) -> Result<T>
    where
        C: Into<ImmutStr>,
        F: FnOnce(E) -> C;

    /// Surface as [`Reason::Internal`] with empty context (cause preserved).
    fn or_fail(self) -> Result<T>
    where
        E: StdError + Send + Sync + 'static;
}

impl<T, E> ExplainErr<T, E> for std::result::Result<T, E> {
    fn explain_err<C, F>(self, reason: Reason, context: F) -> Result<T>
    where
        C: Into<ImmutStr>,
        F: FnOnce(E) -> C,
    {
        self.map_err(|err| explain(reason, context(err)))
    }

    fn or_fail(self) -> Result<T>
    where
        E: StdError + Send + Sync + 'static,
    {
        OrErr::or_err(self, Reason::Internal, "")
    }
}

/// Convert `Option<T>` into [`Result<T>`] with explain-style errors.
pub trait OkOrErr<T> {
    fn or_err(self, reason: Reason, context: impl Into<ImmutStr>) -> Result<T>;

    fn or_err_with<C, F>(self, reason: Reason, context: F) -> Result<T>
    where
        C: Into<ImmutStr>,
        F: FnOnce() -> C;
}

impl<T> OkOrErr<T> for Option<T> {
    fn or_err(self, reason: Reason, context: impl Into<ImmutStr>) -> Result<T> {
        self.ok_or_else(|| explain(reason, context))
    }

    fn or_err_with<C, F>(self, reason: Reason, context: F) -> Result<T>
    where
        C: Into<ImmutStr>,
        F: FnOnce() -> C,
    {
        self.ok_or_else(|| explain(reason, context()))
    }
}

/// Add context to an existing [`Result<T, Error>`].
pub trait ErrContext<T> {
    fn err_context<C, F>(self, context: F) -> Result<T>
    where
        C: Into<ImmutStr>,
        F: FnOnce() -> C;
}

impl<T> ErrContext<T> for Result<T> {
    fn err_context<C, F>(self, context: F) -> Result<T>
    where
        C: Into<ImmutStr>,
        F: FnOnce() -> C,
    {
        self.map_err(|err| err.more_context(context()))
    }
}
