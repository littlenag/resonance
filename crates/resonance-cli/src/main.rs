//! The `resonance` binary: parses the command line and plugs `SqliteStore` into the core API (§11, FR-250).

mod cli;
mod commands;
mod render;
mod tui;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    if let Err(e) = commands::run(cli) {
        eprintln!("resonance: {e}");
        std::process::exit(e.exit_code());
    }
}
