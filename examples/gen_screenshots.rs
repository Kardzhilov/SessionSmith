//! Generate the README screenshots as SVG files by rendering the real TUI.
//!
//! Run from the repository root so campaign/output data is picked up:
//!
//! ```sh
//! cargo run --example gen_screenshots
//! ```

use std::path::Path;

fn main() {
    let out = Path::new("docs/screenshots");
    sessionsmith::tui::generate_screenshots(out).expect("generate screenshots");
    println!("Wrote screenshots to {}", out.display());
}
