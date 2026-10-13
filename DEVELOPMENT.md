# Developer guide

Build details, processing conventions and reference validation for the Rust
implementation. See [README.md](README.md) for upstream attribution.

## Build and run

Requires Rust 1.93 and a C/C++ compiler. On Linux, GCC supplies the OpenMP runtime.
LibRaw 0.22.0 source is bundled and pinned to the reference rawpy 0.26.1 build;
no Python interpreter is used by the Rust processor. `--no-default-features`
instead links system `libraw_r` through pkg-config. A different decoder version
requires its own parity check.

```sh
cargo build --release --locked -j4
cargo test --release --locked -j4
target/release/fast-spektrafilm profiles
target/release/fast-spektrafilm defaults > settings.json

# Crop a RAW before converting pixels to floating-point RGB.
target/release/fast-spektrafilm --threads 4 process \
  samples/RAW_CANON_EOS_5DMARK3.CR2 crop.tiff --crop-size 256 --deterministic

# Preserve float64 stage output for strict comparisons.
target/release/fast-spektrafilm --threads 4 process \
  samples/RAW_CANON_EOS_5DMARK3.CR2 film-density.npy \
  --crop-size 256 --deterministic --collect cmy-film

target/release/fast-spektrafilm bake transform.cube --resolution 33
```

The Slint desktop darkroom is an optional build, so library and CLI users do
not need GUI dependencies. Linux GUI builds additionally require Fontconfig
development files and pkg-config. Native file dialogs use the desktop's XDG
portal. Slint uses its Winit backend and software renderer.

```sh
cargo build --release --locked --features gui -j4
target/release/fast-spektrafilm-gui
target/release/fast-spektrafilm-gui samples/RAW_CANON_EOS_5DMARK3.CR2 --threads 4
# Settings files are shared with the CLI.
target/release/fast-spektrafilm-gui input.npy --config settings.json --data-dir data
```

LUT baking disables framing, resampling and internal scanner/enlarger
acceleration LUTs, preserving the cube domain and sampling the exact spectral
transform even when reusing a render configuration.

`--deterministic` disables grain and glare just as the reference debug switch
does. Spatial effects and autoexposure remain enabled; use `--no-spatial` and
`--no-auto-exposure` explicitly when comparing a per-pixel transform.
`--crop-size` selects a test input crop before simulation. This is distinct from
the configurable framing stage and meters the selected test crop.
LibRaw still unpacks the camera frame to demosaic it correctly; cropping bounds
floating-point conversion and all later processing.

The `io.crop`, `io.crop_center`, `io.crop_size` and `io.upscale_factor` settings
reproduce the reference framing stage. Crop sizes are fractions of the input's
long side; coordinates are ordered x, y. Metering uses the full test input before
framing, and spatial effects retain its physical pixel scale after cropping.

Configuration is JSON, with partial groups filled from defaults. Unknown fields
and unsupported algorithms produce errors. The schema is not compatible with
Python runtime/Qt state JSON; see the mapping and open gaps in
[MIGRATION.md](MIGRATION.md). Profile names select the original
JSON measurement data. `--data-dir` overrides the data location. RAW input defaults to
as-shot white balance; `raw.white_balance` selects the alternatives described
below. `decode` also supports half-size demosaicing.

Use a distinct new output path for CLI processing: unlike GUI Save, CLI writes
directly to its destination before metadata copying and does not protect the
source path. A failed export can leave a changed destination (migration debt D3).

NPY input is an H×W×C little-endian float16/32/64 array in the configured input
space. Simulation and NPY export use float64; TIFF export uses float32 RGB.

## Library

```rust
use fast_spektrafilm::{Image, Params, Simulator, Tap};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let mut params = Params::default();
    params.debug.deactivate_stochastic_effects = true;
    let sim = Simulator::new(Path::new("data"), params)?;
    let input = Image::read_npy(Path::new("input.npy"))?;
    let output = sim.process_owned(input, Tap::RgbOut)?;
    output.write_npy(Path::new("output.npy"))?;
    Ok(())
}
```

Reuse the immutable simulator to reuse spectral and gamut tables. `process_owned`
consumes the input and reuses its storage on the per-pixel path. `process` keeps
the caller's input. Construct a new simulator when settings change. Upstream tap
injection, soft updates and per-stage timing do not yet have equivalent Rust
APIs. Rayon handles
pixel work; a local thread pool can set the library's thread count.

Grain uses the reference's float32 working precision. With
`settings.use_fast_stats=false`, the seeded NumPy RandomState grain stream is
reproduced. The default approximate samplers and glare use Rust random streams;
their formulas are preserved, but their pixel patterns differ from Numba's
thread-local random streams. Set `settings.random_seed` to reproduce a Rust
render across runs and thread counts. Without an explicit seed, fast grain and
glare vary on each render. Controlled one-thread reference tests also compare
fast grain and random glare pixel by pixel. Use deterministic mode for strict
non-stochastic image comparisons.

## Reference validation

Checked-in small golden fixtures let `cargo test --release` run without Python or
the original repository. Development tools compare five processing taps against
the pinned Python implementation with `atol=1e-9`, `rtol=1e-8`. They also compare
RAW decoding, metering, chemistry, colour spaces, spatial effects, grain and the
copied profiles. Python is confined to reference extraction and testing.

```sh
python3 tools/prepare_reference.py --source ../spektrafilm
# Use a Python 3.13 virtual environment with tools/requirements-reference.txt.
.venv/bin/python tools/validate.py --all-profiles --samples --decode \
  --spatial --variants --grain --spectral --gamut --lut --framing --crop-size 256
.venv/bin/python tools/benchmark.py --threads 4 --repeat 3
.venv/bin/python tools/benchmark.py --mode spatial --threads 4 --repeat 3
.venv/bin/python tools/benchmark.py --mode legacy-grain --threads 4 --repeat 3
# A 1.5 mm crop window gives 5.86 um pixel spacing at 256px.
.venv/bin/python tools/benchmark.py --mode spatial --film-format-mm 1.5 --threads 4 --repeat 3
```

The four sample files are decoded once and their center crops cached under
`artifacts/crops`. The reference simulation runs on these crops, not full-size
samples. Reports are written under `artifacts/parity` and `artifacts/benchmark`.
Benchmark processing times exclude preparation and RAW decoding. Peak RSS
includes process startup, Python imports/JIT, and table preparation; it is not
a measurement of pixel-buffer allocation alone.
`--film-format-mm` selects the physical long side represented by the crop.
The default 35 mm on 256px gives coarse 136.72 µm pixels; smaller physical crop
windows exercise blur kernels at finer, capture-like spacing without processing
an entire capture.

## Data and licensing

235 original assets (45,632,323 bytes) were copied byte-for-byte: profiles,
spectral LUTs, filters, presets, illuminants and ICC profiles. `BASELINE.json`
records their SHA-256 hashes. `data/reference_tables.json` contains immutable
colour-science tables extracted by `tools/export_reference_tables.py` from the
pinned reference environment; its algorithms run in Rust.

The rewrite retains the original GPLv3 license and attribution. Third-party
licenses are preserved in `licenses/` and `vendor/libraw/`. LibRaw's original
native C++ decoder is reused; `native/raw.c` only adapts its ABI. All new film
processing algorithms are Rust.

### Additional input and negative workflows

`process` opens RGB NPY, ordinary PNG/JPEG/TIFF/WebP/EXR, and camera RAW.
PNG/JPEG/WebP without an ICC profile are interpreted as sRGB; untagged TIFF/EXR
and NPY retain the explicitly configured input representation. RGB ICC profiles
are honoured. PNG export is 16-bit; TIFF/EXR remain float32 and NPY float64.
Output profiles are embedded for supported RGB spaces in PNG/JPEG/TIFF.
These encodings differ from upstream PNG8, selectable TIFF8/16/32 and EXR16/32;
Rust has no output bit-depth/compression selector. Ordinary image decoding
passes through float32; use NPY for strict float64 input and stage interchange.
One-channel BW density taps require NPY. See migration debt D6 for file-I/O
parity limits.

Set `workflow.route` to `input > convert-film > print > scan`,
`input > convert-film > scan`, or `input > convert-film > scan-minus-base`.
`convert` contains the scan lamp, exposure alignment and a 3×3 calibration matrix.
`calibrate input.tiff fitted.json --config settings.json` detects/fits clear base;
add `--device` to fit device calibration. Both operations save reusable settings.
The device optimizer differs from
SciPy and its fitted matrix is not reference-equivalent on the existing fixture;
see migration debt D1. The GUI calibrates its preview, while this CLI command
uses the original resolution. Neither action runs upstream's automatic neutral
print-balance solver, which is not yet ported; adjust M/Y shifts manually.

`raw.white_balance` accepts `as_shot`, `daylight`, `tungsten`, or `custom`;
custom uses `raw.temperature` (kelvin) and `raw.tint`. Enable
`raw.lens_correction` to use Lensfun 0.3 and its camera/lens database.
`SPEKTRAFILM_LENSFUN_LIBRARY` and `SPEKTRAFILM_LENSFUN_DATABASE` can point to
user-local installations. Missing required libraries report an error.
Camera and enlarger `diffusion_filter` expose all four original filter families.

Content metadata copying for PNG/JPEG/TIFF uses GExiv2 0.14 at runtime. Disable
`io.copy_metadata` if it is unavailable, or set `SPEKTRAFILM_GEXIV2_LIBRARY` to its
library path. The GUI provides this switch under Main → Output. NPY/EXR do not
require GExiv2. The Slint review and remaining application presentation
differences are recorded in [GUI_REVIEW.md](GUI_REVIEW.md) and [GUI.md](GUI.md).
