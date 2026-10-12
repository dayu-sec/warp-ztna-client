use std::error::Error as StdError;

use orion_error::StructError;
use orion_error::prelude::{SourceErr, SourceRawErr};

use crate::immut_str::ImmutStr;
use crate::reason::Reason;

/// Operation error carrier (orion `StructError` specialized to [`Reason`]).
pub type Error = StructError<Reason>;

/// Default result type for application / orchestration code.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Complete diagnostic information carried to an outer logging boundary.
///
/// The error chain is rendered by [`display_chain`] and is intentionally kept
/// intact. Callers decide at the logging boundary whether the diagnostic is
/// appropriate for a particular sink; this type does not alter or truncate the
/// chain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorDiagnostic {
    pub code: String,
    pub error_chain: String,
}

impl ErrorDiagnostic {
    /// Build a diagnostic from the canonical operation-error chain.
    #[must_use]
    pub fn from_error(code: impl Into<String>, err: &Error) -> Self {
        Self { code: code.into(), error_chain: display_chain(err) }
    }

    /// Build a diagnostic from already-rendered error text.
    #[must_use]
    pub fn from_message(code: impl Into<String>, message: &str) -> Self {
        Self { code: code.into(), error_chain: message.to_owned() }
    }
}

/// Extension methods on [`Error`].
pub trait ErrorExt {
    /// Stack another context string, keeping this error as the structured source.
    fn more_context(self, context: impl Into<ImmutStr>) -> Error
    where
        Self: Sized;
}

impl ErrorExt for Error {
    fn more_context(self, context: impl Into<ImmutStr>) -> Error {
        let reason = self.reason().clone();
        let detail = context.into().into_string();
        let as_result: Result<()> = Err(self);
        match as_result.source_err(reason, detail) {
            Err(err) => err,
            Ok(()) => unreachable!("input was Err"),
        }
    }
}

/// Create an error with only a reason (no context).
#[must_use]
pub fn new(reason: Reason) -> Error {
    StructError::from(reason)
}

/// Create an error with reason and context (no cause).
#[must_use]
pub fn explain(reason: Reason, context: impl Into<ImmutStr>) -> Error {
    StructError::from(reason).with_detail(context.into().into_string())
}

/// Short for `Err(explain(...))`.
pub fn e_explain<T>(reason: Reason, context: impl Into<ImmutStr>) -> Result<T> {
    Err(explain(reason, context))
}

/// Create an error with reason, context, and a causing error.
#[must_use]
pub fn because<E>(reason: Reason, context: impl Into<ImmutStr>, cause: E) -> Error
where
    E: StdError + Send + Sync + 'static,
{
    let detail = context.into().into_string();
    let as_result: std::result::Result<(), E> = Err(cause);
    match as_result.source_raw_err(reason, detail) {
        Err(err) => err,
        Ok(()) => unreachable!("input was Err"),
    }
}

/// Short for `Err(because(...))`.
pub fn e_because<T, E>(reason: Reason, context: impl Into<ImmutStr>, cause: E) -> Result<T>
where
    E: StdError + Send + Sync + 'static,
{
    Err(because(reason, context, cause))
}

/// Human-readable multi-line chain (for logs or CLI).
#[must_use]
pub fn display_chain(err: &Error) -> String {
    err.display_chain()
}
