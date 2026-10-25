//! # keel
//!
//! The `keel` command-line tool: `keel new`, `keel make:*`, `keel dev`, and
//! forwarding of runtime commands (`migrate`, `queue:work`, ...) to the app
//! binary.
#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;
use keel_cli::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = std::env::current_dir()
        .map_err(anyhow::Error::from)
        .and_then(|cwd| keel_cli::commands::execute(cli, &cwd));
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
