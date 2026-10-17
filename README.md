<div align="center">

# fast-spektrafilm

[![Release](https://img.shields.io/github/v/release/dmitryprikhunov/fast-spektrafilm?label=release&style=flat-square&color=ff985a)](https://github.com/dmitryprikhunov/fast-spektrafilm/releases/latest)
[![Linux x86_64](https://img.shields.io/badge/Linux-x86__64-ff985a?style=flat-square)](RELEASES.md)
[![License: GPL-3.0-only](https://img.shields.io/badge/license-GPL--3.0--only-ff985a?style=flat-square)](LICENSE)

[🇷🇺 Русский](README.ru.md)

Film and print simulations for RAW and RGB images, with a Rust library, CLI and
Slint desktop app. Process an image through film exposure, development, printing
and scanning, with controls for colour, grain and optical effects.

Based on [spektrafilm](https://github.com/andreavolpato/spektrafilm/tree/28bf883e1672e884307edc75852549376e13644e)
by **Andrea Volpato and the spektrafilm contributors**. Original attribution,
data and licenses are retained; this port is GPLv3.

![Desktop interface showing a processed RAW photo and film controls](docs/images/interface.png)

The photo shown in the preview comes from
[Signature Edits' free RAW photo collection](https://www.signatureedits.com/free-raw-photos/).
It belongs to its original photographer, not the fast-spektrafilm project.

## Get started

Linux x86_64 binaries for the CLI and desktop app are available in
[GitHub Releases](https://github.com/dmitryprikhunov/fast-spektrafilm/releases).
See [RELEASES.md](RELEASES.md) for installation and runtime requirements.

To build from source, see the prerequisites in [DEVELOPMENT.md](DEVELOPMENT.md),
then run from the repository root:

```sh
# Desktop app
cargo run --release --locked --features gui --bin fast-spektrafilm-gui

# Command line
cargo run --release --locked -- process input.tiff output.tiff
```

Open an image, adjust settings with **Preview**, then **Scan** at full resolution.
**Save** reuses the scan when settings match. CPU threads are selected automatically;
use `--threads N` to set a limit.

Supported inputs include RAW, PNG, JPEG, TIFF, WebP, EXR and RGB NPY. The desktop
app and CLI share JSON processing settings.

[App guide](GUI.md) · [Build requirements and CLI/library usage](DEVELOPMENT.md) ·
[Migration status and technical debt](MIGRATION.md) · [Validation](VALIDATION.md) ·
[Contributing](CONTRIBUTING.md) · [Benchmarks](BENCHMARKS.md) · [License](LICENSE)

This product includes software developed by the University of Chicago, as
Operator of Argonne National Laboratory (MINPACK). SciPy and OpenBLAS arithmetic
and CORE-MATH attribution and license notices are preserved in [licenses/](licenses).

</div>
