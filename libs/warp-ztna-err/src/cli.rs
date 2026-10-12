//! CLI / human diagnostic helpers.
//!
//! Prefer structured logging (`tracing`) in long-running services. Use this
//! module from CLI binaries such as OPE tooling.

use crate::error::Error;

/// Print the full error chain to stderr.
///
/// Not part of [`crate::prelude`] so server code does not reach for it by default.
pub fn print_error(err: &Error) {
    eprintln!("{}", err.display_chain());
}
