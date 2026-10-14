use anyhow::Result;
use clap::Parser;
use fast_spektrafilm::gui::{Options, run};
use std::{num::NonZeroUsize, path::PathBuf};

/// A native Slint darkroom for fast-spektrafilm.
#[derive(Parser)]
#[command(name = "fast-spektrafilm-gui", version, about)]
struct Cli {
    /// Open a camera RAW or RGB NPY file on launch.
    input: Option<PathBuf>,
    #[arg(long, default_value_os_t = fast_spektrafilm::default_data_dir())]
    data_dir: PathBuf,
    /// Load the same JSON settings accepted by the CLI.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Processing threads (defaults to available CPUs; respects RAYON_NUM_THREADS).
    #[arg(long)]
    threads: Option<NonZeroUsize>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    run(Options {
        data_dir: cli.data_dir,
        input: cli.input,
        config: cli.config,
        threads: cli.threads.map_or(0, NonZeroUsize::get),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_default_to_automatic_and_accept_an_explicit_limit() {
        assert_eq!(Cli::try_parse_from(["gui"]).unwrap().threads, None);
        assert_eq!(
            Cli::try_parse_from(["gui", "--threads", "12"])
                .unwrap()
                .threads,
            NonZeroUsize::new(12)
        );
        assert!(Cli::try_parse_from(["gui", "--threads", "0"]).is_err());
    }
}
