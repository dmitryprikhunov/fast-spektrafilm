# fast-spektrafilm

Rust implementation of [spektrafilm](https://github.com/andreavolpato/spektrafilm)'s
experimental processing pipeline, with a CLI, library and optional Slint GUI.

The film model, processing methods and original profiles/data are the work of
**Andrea Volpato and the spektrafilm contributors**. This repository ports that
work to Rust. Its reference is experimental commit
[`28bf883`](https://github.com/andreavolpato/spektrafilm/tree/28bf883e1672e884307edc75852549376e13644e).
Upstream attribution and licenses are retained; this derivative is GPLv3.

## Build

Rust 1.93+, a C/C++ compiler and pkg-config are required. LibRaw is bundled;
Linux builds need the OpenMP runtime. The optional Linux GUI also needs
Fontconfig development files and an XDG portal for file dialogs.

```sh
cargo build --release --locked
cargo test --release --locked
cargo run --release --locked -- profiles
cargo run --release --locked -- process input.tiff output.tiff
# Optional desktop GUI:
cargo run --release --locked --features gui --bin fast-spektrafilm-gui
```

Run from the repository root so `data/` is found, or pass `--data-dir`.
Metadata copying requires GExiv2 0.14; disable `io.copy_metadata` if unavailable.
Lens correction requires Lensfun 0.3 and its database.

## Development

`develop` is the integration branch. See [CONTRIBUTING.md](CONTRIBUTING.md)
for checks and [DEVELOPMENT.md](DEVELOPMENT.md) for CLI/library usage,
dependencies and reference tooling.

- [Coverage and remaining differences](MIGRATION.md)
- [Numerical validation](VALIDATION.md) and [measured benchmarks](BENCHMARKS.md)
- [GUI behavior](GUI.md) and [branch review](GUI_REVIEW.md)
- [Original asset hashes](BASELINE.json), [GPLv3](LICENSE) and [third-party licenses](licenses/)
