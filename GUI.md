# Slint darkroom

The optional `fast-spektrafilm-gui` binary uses Slint 1.18.1 and the Rust
Simulator directly. Build/run commands are in [DEVELOPMENT.md](DEVELOPMENT.md).

The photo occupies the left canvas, with the original Main, Film, Print,
Advanced and Config groups on the right. Click an orange heading to expand
its controls. There is one editor per parameter; no JSON editor or duplicated
exposure/export controls. Numeric fields accept float64 values: press Enter or
leave the field to apply. Invalid input appears in the status bar.

Main contains profiles, all six workflows, RAW white balance/lens correction,
input colour, crop, camera, enlarger, scanner and output colour. Film and Print
contain their chemistry, base, texture and optical diffusion controls. Advanced
contains negative calibration, film/print dye permutations and gamut controls.
Config contains processing settings, monitor ICC selection, automatic preview,
file format, settings load/save/reset and startup defaults. JSON remains the
portable settings-file format shared with the CLI, not a user-facing editor.
Stock grain/coupler values and neutral filters are resolved into their controls;
manual changes switch off the corresponding automatic preset so they take effect.

Open RAW, PNG, JPEG, TIFF, WebP, EXR or RGB NPY. Embedded RGB profiles are honoured;
untagged PNG/JPEG/WebP use sRGB. Untagged TIFF/EXR and NPY use the selected input
interpretation. RAW supports as-shot, daylight, tungsten and custom temperature/
tint. Lens correction uses Lensfun 0.3 and its database, as described in README.

**Preview** processes a full-frame source reduced to a 768-pixel long side.
Automatic updates debounce edits by 300 ms. **Scan** processes and displays the
original resolution. **Save** always processes the original with the current
settings. Fit and 100/200/400% views support scrolling, and Original/Processed/
Compare use the same display transform. Fine grain and spatial detail can differ
at preview scale; use Scan to inspect the full-resolution result.

Image decoding, simulation, calibration, metadata and export run on the worker,
not the UI thread. Obsolete results are discarded, including after opening a
new source. RAW/input interpretation changes invalidate the source cache. The
common linear as-shot RAW preview uses a bounded floating-point buffer; custom
white balance, lens correction and other input paths decode before reduction.

PNG is 16-bit, JPEG is quality 95, TIFF/EXR are float32 and NPY is float64.
Exports retain the selected output colour representation and embed its ICC
profile where the reference ships one. The monitor ICC profile affects display
only. Image saves refuse the source path and replace the destination only after
successful processing, encoding and metadata copying.

Copy metadata preserves source EXIF/IPTC/XMP for PNG/JPEG/TIFF and corrects the
orientation, dimensions, software, date and colour tags. This option requires
GExiv2 0.14 or later (Exiv2 through its C ABI); a missing library is an explicit
error. Turn off Copy metadata under Main → Output to export without it.
`SPEKTRAFILM_GEXIV2_LIBRARY` can select a nonstandard library path. NPY/EXR skip
content metadata. The output ICC remains intact, including TIFF's ICC IFD tag.

Startup defaults live in `fast-spektrafilm/default.json` under the user config
directory (`XDG_CONFIG_HOME`, `APPDATA`, or `$HOME/.config`). An explicit CLI
`--config` takes precedence. Reset affects current settings; saving them as the
startup default is an explicit separate action.

## Verification

```sh
cargo test --release --locked -j4
cargo test --features gui --locked --lib -j4
# Exercise installed GExiv2, including repeated exports and ICC retention:
SPEKTRAFILM_TEST_METADATA=1 cargo test --features gui --locked --lib -j4
# Optional real-RAW check and offscreen software-renderer screenshots:
GUI_QA_RAW="$PWD/samples/RAW_CANON_EOS_5DMARK3.CR2" \
GUI_QA_DIR="$PWD/artifacts/gui" cargo test --features gui --locked --lib -j4
```

Tests cover export/core equality, float TIFF and PNG16 values, full-resolution
Scan, source-file protection, invalid settings, float64 edits, every parameter's
editor coverage and uniqueness, cache invalidation, monitor/export separation,
stale worker results, recovery after failed loading, native tab pointer input
and screenshots of every tab at normal/compact sizes. Camera samples and a
display server are not required for the normal tests.

The Slint application replaces the photo-editing controls and processing
workflows. It does not recreate Napari's general layer tooling, the decorative
Polaroid animation/paper-back view, or import the original Qt GUI-state JSON
schema. Its settings files use the Rust processing schema. These application
presentation differences are separate from numerical core parity.
