use anyhow::{Result, ensure};
use clap::Parser;
use fast_spektrafilm::gui::{Options, run};
use std::path::PathBuf;

/// A native Slint darkroom for fast-spektrafilm.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Open a camera RAW or RGB NPY file on launch.
    input: Option<PathBuf>,
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/data"))]
    data_dir: PathBuf,
    /// Load the same JSON settings accepted by the CLI.
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, default_value_t = 4)]
    threads: usize,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    ensure!(cli.threads > 0, "Thread count must be positive");
    run(Options {
        data_dir: cli.data_dir,
        input: cli.input,
        config: cli.config,
        threads: cli.threads,
    })
}
