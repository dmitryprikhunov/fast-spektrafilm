# fast-spektrafilm

Film and print simulations for RAW and RGB images, with a Rust CLI and desktop app.

Based on [spektrafilm](https://github.com/andreavolpato/spektrafilm/tree/28bf883e1672e884307edc75852549376e13644e)
by **Andrea Volpato and the spektrafilm contributors**. Original attribution,
data and licenses are retained; this port is GPLv3.

![Desktop interface showing a processed RAW photo and film controls](docs/images/interface.png)

## Get started

Linux x86_64 binaries for the CLI and desktop app are available in
[GitHub Releases](https://github.com/dmitryprikhunov/fast-spektrafilm/releases).
See [RELEASES.md](RELEASES.md) for installation and runtime requirements.

Run from the repository root:

```sh
# Desktop app
cargo run --release --locked --features gui --bin fast-spektrafilm-gui

# Command line
cargo run --release --locked -- process input.tiff output.tiff
```

Open an image, adjust settings with **Preview**, then **Scan** at full resolution.
**Save** reuses the scan when settings match. CPU threads are selected automatically;
use `--threads N` to set a limit.

[App guide](GUI.md) · [Build requirements and CLI/library usage](DEVELOPMENT.md) ·
[Migration status and technical debt](MIGRATION.md) · [Validation](VALIDATION.md) ·
[Contributing](CONTRIBUTING.md) · [Benchmarks](BENCHMARKS.md) · [License](LICENSE)

This product includes software developed by the University of Chicago, as
Operator of Argonne National Laboratory (MINPACK). SciPy and OpenBLAS arithmetic
and CORE-MATH attribution and license notices are preserved in [licenses/](licenses).
