//! Lift `std` failures with `or_err` / `because` / `explain`.
//!
//! ```bash
//! cargo run -p warp-ztna-err --example basic_or_err
//! ```

use std::fs;
use std::path::Path;

use warp_ztna_err::prelude::*;

fn read_config(path: &Path) -> Result<String> {
    fs::read_to_string(path).or_err(Reason::External, "read config file")
}

fn parse_port(raw: &str) -> Result<u16> {
    raw.trim().parse::<u16>().or_err(Reason::InvalidArgument, "parse listen port")
}

fn demo_explain() -> Result<()> {
    // No underlying cause — pure validation / policy failure.
    if true {
        return e_explain(Reason::Forbidden, "operator role required");
    }
    Ok(())
}

fn demo_because() -> Result<()> {
    let io_err = std::io::Error::other("connection reset");
    Err(because(Reason::Unavailable, "netbird list peers", io_err))
}

fn main() {
    println!("== or_err (missing file) ==");
    match read_config(Path::new("/no/such/warp-ztna.conf")) {
        Ok(body) => println!("unexpected ok: {body}"),
        Err(err) => {
            println!("reason code: {}", err.reason().code());
            println!("display:     {err}");
            println!("chain:\n{}", display_chain(&err));
        }
    }

    println!("\n== or_err (parse) ==");
    match parse_port("not-a-port") {
        Ok(port) => println!("unexpected ok: {port}"),
        Err(err) => println!("{err} (code={})", err.reason().code()),
    }

    println!("\n== explain ==");
    if let Err(err) = demo_explain() {
        println!("{err}");
    }

    println!("\n== because ==");
    if let Err(err) = demo_because() {
        println!("{}", display_chain(&err));
    }
}
