# fast-spektrafilm

Film and print simulations for RAW and RGB images, with a Rust CLI and desktop app.

Based on [spektrafilm](https://github.com/andreavolpato/spektrafilm/tree/28bf883e1672e884307edc75852549376e13644e)
by **Andrea Volpato and the spektrafilm contributors**. Original attribution,
data and licenses are retained; this port is GPLv3.

## Get started

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
[Contributing](CONTRIBUTING.md) · [Benchmarks](BENCHMARKS.md) · [License](LICENSE)
