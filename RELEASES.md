# Binary releases

The first release provides the CLI and Slint desktop app for **Linux x86_64**.
Download the Linux archive and `SHA256SUMS` from
[GitHub Releases](https://github.com/dmitryprikhunov/fast-spektrafilm/releases).
The archive includes both executables, the original film/filter/LUT/ICC data,
license notices, guides and a build manifest. A matching source archive includes
the pinned LibRaw source and Cargo lockfile.

## Run

The binaries are built on Ubuntu 24.04 with Rust 1.93.1, using the normal release
profile (thin LTO, one codegen unit) and a generic x86_64 CPU target. They require
glibc 2.39 or newer and system LittleCMS, OpenMP and C++ runtimes. GUI startup
also needs Fontconfig and an X11 or Wayland desktop; native file dialogs use the
desktop's XDG portal. `DYNAMIC-LIBRARIES.txt` lists linked libraries.
Windows, macOS and Linux ARM packages are not included in v0.1.0.

On Ubuntu 24.04, the runtime packages are:

```sh
sudo apt-get install liblcms2-2 libgomp1 libstdc++6 libfontconfig1 libgexiv2-2 liblensfun1
```

GExiv2 is used for PNG/JPEG/TIFF metadata copying, enabled by default. The GUI's
Main → Output panel can disable **Copy metadata**, or set `io.copy_metadata` to
`false` in a CLI config. Lensfun and its database are used only when lens
correction is enabled. See [DEVELOPMENT.md](DEVELOPMENT.md) for user-local native
library overrides. No Python interpreter is needed to run the release.

```sh
# Keep both archives alongside SHA256SUMS to verify all release files.
sha256sum -c SHA256SUMS
tar -xzf fast-spektrafilm-0.1.0-linux-x86_64.tar.gz
cd fast-spektrafilm-0.1.0-linux-x86_64
./fast-spektrafilm-gui
./fast-spektrafilm profiles
./fast-spektrafilm process /path/to/input.tiff /path/to/output.tiff
```

Both executables find `data/` next to themselves, so they can be launched from
another working directory. Keep that directory with the executables, or supply
`--data-dir /path/to/data`. Cargo builds fall back to the source checkout's data.

## Create the next release

Integrate release preparation into `develop`, and verify its Rust and Release
workflows. Keep the Cargo version and `releases/vVERSION.md` notes in sync, then
push an annotated `vVERSION` tag pointing to that verified commit. Promotion to
`main` remains a separate step.

The Release workflow builds both executables and tests processing/library/CLI
with the normal release profile on Ubuntu 24.04. Rust CI covers the GUI callback
suite and explicit native checks. Release checks verify original asset hashes,
data discovery, CLI rendering and GUI startup from an unpacked archive outside
the checkout. The workflow uploads the
Linux archive, matching source archive and SHA256 checksums as Actions artifacts.
Tag runs publish these files to a GitHub release only after all checks pass;
pull requests and manual workflow runs build artifacts without publishing.

Local packaging after `cargo build --release --locked --features gui --bins`:
use a rustup toolchain, which includes the standard library's copyright and
license files. For another compiler installation, `SPEKTRAFILM_RUST_NOTICES` can
point to its matching `share/doc/rust` notice directory.

```sh
bash scripts/package-linux.sh v0.1.0
bash scripts/smoke-release.sh artifacts/release/dist/fast-spektrafilm-0.1.0-linux-x86_64.tar.gz
```

Local artifacts inherit the host's glibc requirements. Use the workflow artifacts
for the Ubuntu 24.04 compatibility boundary.
