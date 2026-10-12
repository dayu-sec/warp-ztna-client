//! Stack context and rewrap an existing `Error` with a new `Reason`.
//!
//! ```bash
//! cargo run -p warp-ztna-err --example rewrap_and_context
//! ```

use warp_ztna_err::prelude::*;

fn step_validate() -> Result<()> {
    e_explain(Reason::InvalidArgument, "cidr must be host /32")
}

fn step_build() -> Result<()> {
    // Already `Result<_, Error>`: use `rewrap` (not `or_err`) when changing Reason.
    rewrap(step_validate(), Reason::InvalidAssetGraph, "build graph from snapshot")
}

fn step_persist() -> Result<()> {
    step_build().err_context(|| "persist authoritative JSONB snapshot")
}

fn step_sync() -> Result<()> {
    // `more_context` on the error value itself.
    step_persist().map_err(|err| err.more_context("OPE asset sync job"))
}

fn option_path(id: Option<&str>) -> Result<&str> {
    id.or_err(Reason::NotFound, "device id missing from request")
}

fn main() {
    println!("== rewrap + err_context + more_context ==");
    if let Err(err) = step_sync() {
        println!("final reason: {}", err.reason().code());
        println!("final detail: {:?}", err.detail());
        println!("\n{}", display_chain(&err));
    }

    println!("\n== Option::or_err ==");
    if let Err(err) = option_path(None) {
        println!("{err}");
    }

    println!("\n== explain_err (no cause chain) ==");
    let raw: std::result::Result<(), &str> = Err("opaque failure");
    // `&str` is not `std::error::Error` — use explain_err for text-only lift.
    let err =
        raw.explain_err(Reason::Internal, |msg| format!("handler failed: {msg}")).unwrap_err();
    println!("detail={:?} source={:?}", err.detail(), err.source_ref().is_some());
}
