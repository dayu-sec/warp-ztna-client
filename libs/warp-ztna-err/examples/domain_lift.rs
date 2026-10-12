//! Domain `thiserror` enum stays in the library; the app lifts it with `or_err`.
//!
//! ```bash
//! cargo run -p warp-ztna-err --example domain_lift
//! ```

use std::fmt;

use warp_ztna_err::prelude::*;

// --- domain layer (would live in e.g. warp-ztna-ag) -------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum GraphBuildError {
    DuplicateAssetId(String),
    LayerViolation { from_id: String, to_id: String },
}

impl fmt::Display for GraphBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateAssetId(id) => write!(f, "duplicate asset id: {id}"),
            Self::LayerViolation { from_id, to_id } => {
                write!(f, "layer violation: {from_id} -> {to_id}")
            }
        }
    }
}

impl std::error::Error for GraphBuildError {}

fn build_graph(dup: bool) -> std::result::Result<(), GraphBuildError> {
    if dup {
        return Err(GraphBuildError::DuplicateAssetId("svc-finance".into()));
    }
    Err(GraphBuildError::LayerViolation { from_id: "node-1".into(), to_id: "app-1".into() })
}

// --- application layer ---------------------------------------------------

fn sync_snapshot() -> Result<()> {
    // Application chooses operation Reason + context; domain error becomes cause.
    build_graph(true).or_err(Reason::InvalidAssetGraph, "sync asset snapshot build")?;
    Ok(())
}

fn main() {
    match sync_snapshot() {
        Ok(()) => println!("unexpected success"),
        Err(err) => {
            println!("operation reason: {:?} ({})", err.reason(), err.reason().code());
            println!("detail:           {:?}", err.detail());
            println!("has source:       {}", err.source_ref().is_some());
            println!("\n{}", display_chain(&err));
        }
    }
}
