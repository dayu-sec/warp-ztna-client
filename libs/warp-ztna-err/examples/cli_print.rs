//! Human-oriented stderr printing for CLI binaries.
//!
//! Prefer `tracing` in long-running services; use `cli::print_error` in tools.
//!
//! ```bash
//! cargo run -p warp-ztna-err --example cli_print
//! ```

use std::io;

use warp_ztna_err::cli::print_error;
use warp_ztna_err::prelude::*;

fn load_key() -> Result<String> {
    Err(io::Error::other("permission denied")).or_err(Reason::Forbidden, "read setup key file")
}

fn main() {
    match load_key() {
        Ok(key) => println!("key={key}"),
        Err(err) => {
            eprintln!("failed to load setup key\n");
            print_error(&err);
            std::process::exit(1);
        }
    }
}
