//! Writes the component gallery to static HTML files with the assets next to
//! them, to open in a browser (or screenshot):
//!
//! ```text
//! cargo run -p navree-ui --example gallery -- target/navree-ui-gallery
//! ```

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/navree-ui-gallery"));
    navree_ui::gallery::write(&dir)?;
    for page in navree_ui::gallery::PAGES {
        println!("{}", dir.join(page.file).display());
    }
    Ok(())
}
