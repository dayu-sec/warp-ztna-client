//! Operation-error facade for Warp ZTNA.
//!
//! Domain crates keep `thiserror` enums. Application code lifts failures here:
//!
//! ```rust
//! use warp_ztna_err::prelude::*;
//!
//! fn load() -> Result<u32> {
//!     std::fs::read_to_string("/no/such")
//!         .or_err(Reason::External, "read config")?;
//!     Ok(1)
//! }
//! ```
//!
//! Built on [`orion_error`] with simple style names (`or_err`, `because`,
//! `explain`, `err_context`). Public API is only via [`prelude`] (plus [`cli`]).
//!
//! # Examples
//!
//! Runnable samples under `examples/`:
//!
//! - `basic_or_err` — `or_err` / `because` / `explain`
//! - `domain_lift` — domain-style error → operation error
//! - `rewrap_and_context` — `rewrap` / `err_context` / `more_context` / `Option::or_err`
//! - `cli_print` — [`cli::print_error`]
//!
//! ```bash
//! cargo run -p warp-ztna-err --example basic_or_err
//! ```
//!
//! # Errors
//!
//! Fallible helpers always return the prelude [`Result`] type; individual items
//! omit per-function `# Errors` sections to avoid noise.

#![allow(clippy::missing_errors_doc)]

mod error;
mod immut_str;
mod reason;
mod traits;

/// CLI / diagnostic printing ([`cli::print_error`]). Not in [`prelude`].
pub mod cli;

/// Sole public import path for application code (`use warp_ztna_err::prelude::*`).
pub mod prelude {
    pub use crate::error::{
        Error, ErrorDiagnostic, ErrorExt, Result, because, display_chain, e_because, e_explain,
        explain, new,
    };
    pub use crate::immut_str::ImmutStr;
    pub use crate::reason::Reason;
    pub use crate::traits::{ErrContext, ExplainErr, OkOrErr, OrErr, rewrap, rewrap_with};
}

#[cfg(test)]
mod tests {
    use super::prelude::*;
    use std::io;

    #[test]
    fn explain_sets_detail() {
        let err = explain(Reason::NotFound, "device missing");
        assert_eq!(err.reason(), &Reason::NotFound);
        assert_eq!(err.detail().as_deref(), Some("device missing"));
        assert_eq!(err.reason().code(), "biz.not_found");
    }

    #[test]
    fn or_err_preserves_std_cause() {
        let result: std::result::Result<(), io::Error> = Err(io::Error::other("disk full"));
        let err = result.or_err(Reason::External, "write snapshot").unwrap_err();
        assert_eq!(err.reason(), &Reason::External);
        assert_eq!(err.detail().as_deref(), Some("write snapshot"));
        assert!(err.source_ref().is_some());
        let chain = display_chain(&err);
        assert!(chain.contains("write snapshot") || chain.contains("External"));
    }

    #[test]
    fn because_and_more_context() {
        let cause = io::Error::new(io::ErrorKind::NotFound, "gone");
        let err = because(Reason::External, "load peer", cause).more_context("during sync");
        assert_eq!(err.reason(), &Reason::External);
        assert_eq!(err.detail().as_deref(), Some("during sync"));
    }

    #[test]
    fn err_context_on_result() {
        let res: Result<()> = e_explain(Reason::Internal, "step a");
        let err = res.err_context(|| "step b").unwrap_err();
        assert_eq!(err.detail().as_deref(), Some("step b"));
    }

    #[test]
    fn explain_err_drops_cause() {
        let result: std::result::Result<(), io::Error> = Err(io::Error::other("secret"));
        let err = result.explain_err(Reason::Internal, |e| format!("wrapped: {e}")).unwrap_err();
        assert_eq!(err.detail().as_deref(), Some("wrapped: secret"));
        assert!(err.source_ref().is_none());
    }

    #[test]
    fn option_or_err() {
        let err = None::<u8>.or_err(Reason::NotFound, "id").unwrap_err();
        assert_eq!(err.reason(), &Reason::NotFound);
    }

    #[test]
    fn immut_str_static_and_owned() {
        let s: ImmutStr = "static".into();
        assert!(!s.is_owned());
        let o: ImmutStr = format!("x{}", 1).into();
        assert!(o.is_owned());
        assert_eq!(o.as_str(), "x1");
    }

    #[test]
    fn rewrap_structured_error() {
        let inner: Result<()> = e_explain(Reason::InvalidArgument, "bad cidr");
        let err = rewrap(inner, Reason::InvalidAssetGraph, "build graph").unwrap_err();
        assert_eq!(err.reason(), &Reason::InvalidAssetGraph);
        assert_eq!(err.detail().as_deref(), Some("build graph"));
    }

    #[test]
    fn diagnostic_is_built_from_the_operation_error_chain() {
        let err = because(Reason::Internal, "persist state", io::Error::other("disk offline"));
        let diagnostic = ErrorDiagnostic::from_error("database_error", &err);

        assert_eq!(diagnostic.code, "database_error");
        assert!(diagnostic.error_chain.contains("persist state"));
        assert!(diagnostic.error_chain.contains("disk offline"));
    }

    #[test]
    fn diagnostic_does_not_modify_the_complete_chain() {
        let err = because(
            Reason::External,
            "request https://example.test/resource",
            io::Error::other("deep root cause"),
        );
        let diagnostic = ErrorDiagnostic::from_error("external_error", &err);

        assert!(diagnostic.error_chain.contains("https://example.test/resource"));
        assert!(diagnostic.error_chain.contains("deep root cause"));
    }
}
