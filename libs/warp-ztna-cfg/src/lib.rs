//! Process configuration loaded from environment variables.
//!
//! ```rust
//! use warp_ztna_cfg::prelude::*;
//! ```

#![allow(clippy::missing_errors_doc)]

mod env;

/// Public import path for consumers.
pub mod prelude {
    pub use crate::env::*;
}
