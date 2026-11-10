//! `ui-docs`: writes the website's UI docs data, or checks it is current.
//!
//! ```text
//! cargo run -p ui-docs              # write docs/ui-data
//! cargo run -p ui-docs -- --check   # exit 1 when a file there is stale
//! ```
#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let check = std::env::args().skip(1).any(|arg| arg == "--check");
    let files = ui_docs::generate();
    if check {
        let stale = ui_docs::stale(&files);
        if stale.is_empty() {
            println!("{} files in {} are current", files.len(), ui_docs::OUT);
            return ExitCode::SUCCESS;
        }
        for path in &stale {
            eprintln!("stale: {}/{path}", ui_docs::OUT);
        }
        eprintln!("{}", ui_docs::RERUN);
        return ExitCode::FAILURE;
    }
    match ui_docs::write(&files) {
        Ok(()) => {
            println!("wrote {} files to {}", files.len(), ui_docs::OUT);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ui-docs: {error}");
            ExitCode::FAILURE
        }
    }
}
