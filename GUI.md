# Slint darkroom

The optional `fast-spektrafilm-gui` binary uses Slint 1.18.1 and the Rust
Simulator directly. Build/run commands are in [DEVELOPMENT.md](DEVELOPMENT.md).

The photo occupies the left canvas, with the original Main, Film, Print,
Advanced and Config groups on the right. Click an orange heading to expand
its controls. There is one editor per parameter; no JSON editor or duplicated
exposure/export controls. Numeric fields accept float64 values: press Enter or
leave the field to apply. Scroll over an editor to change its value; numeric
increments and wheel bounds come from the pinned reference GUI. Dropdowns and
checkboxes also accept wheel input. Numeric wheel edits first apply pending
text; invalid text stays editable and reports an error. Small wheel movements
accumulate across successive edits. Scroll over the labels to move through a
long parameter panel. Hover over a label or editor for delayed, wrapped help.
Invalid input appears in the status bar.

Main contains profiles, RAW white balance/lens correction,
input colour, crop, camera, enlarger, scanner and output colour. Film and Print
contain their chemistry, base, texture and optical diffusion controls, including
the reference's nested Grain sections. Advanced contains spectral upsampling,
input/output gamut compression, experimental illuminant/dye permutations and
negative calibration. Config contains processing settings, stock presets, print
balance, monitor ICC selection, file format, settings load/save/reset and startup
defaults. Automatic preview and Workflow stay above Preview/Scan/Save on every
tab. JSON remains the portable settings-file format shared with the CLI, not a user-facing editor.

Detect base and device calibration use the full decoded input through the same
helper as the CLI. Device calibration applies the source sampling caps.
Advanced also provides Neutralize print filters, which fits M/Y shifts after
stock/base edits.

Stock grain/coupler values and neutral filters are resolved into their controls;
manual changes switch off the corresponding automatic preset so they take effect.

Open RAW, PNG, JPEG, TIFF, WebP, EXR or RGB NPY. Embedded RGB profiles are honoured;
untagged PNG/JPEG/WebP use sRGB. Untagged TIFF/EXR and NPY use the selected input
interpretation. RAW supports as-shot, daylight, tungsten and custom temperature/
tint. Lens correction uses Lensfun 0.3 and its database; see
[DEVELOPMENT.md](DEVELOPMENT.md).

**Preview** reduces the full source with the pinned anti-aliased linear reducer;
`settings.preview_max_size` defaults to a 640-pixel long side. Automatic edits
debounce by 300 ms. Preview mode disables grain, lens/scanner sharpening and
selected diffusion/blur operations exactly as source digestion does; halation
and scattering remain enabled. **Scan** runs at original resolution with preview
mode off. **Save** reuses a matching scan or processes the original if needed.
Use the mouse wheel over the image to zoom toward or away from the pointer,
and drag with the left mouse button to pan. Original/Processed/Compare share
zoom and pan.
Fit and the 100/200/400% buttons center the image; double-clicking the canvas
also restores Fit. Navigation does not trigger processing. Use Scan to inspect
full-resolution grain and detail. The status bar shows queued previews during
the debounce, then the active operation until the current worker result arrives;
saving settings cannot replace a processing status with an idle message.

File dialogs run independently of the UI event loop. Image decoding, simulation,
calibration, metadata and export run on the processing worker. The GUI uses
available CPU threads by default (or `RAYON_NUM_THREADS`); `--threads N` overrides
it. Obsolete results are discarded, including after opening a new source.
RAW/input interpretation changes invalidate the source cache. The common
linear as-shot RAW preview uses a bounded floating-point buffer; custom
white balance, lens correction and other input paths decode before reduction.

The preview footer supplies compact clockwise/counterclockwise rotation icons
beside the zoom buttons, on every tab. Rotation applies before cropping,
Preview and Scan, invalidates the source/scan cache, and restores Fit.
Main → Output exposes output depth (PNG8/16, TIFF8/16/32, EXR16/32) and TIFF/EXR
compression. Defaults are PNG16 and TIFF/EXR32; JPEG is quality 95 and NPY64.
Independent saving-colour settings convert completed pixels without invalidating
the scan. Output colour controls still affect processing. Monitor ICC affects
display only; an invalid profile preserves the scan. Supported output ICC profiles
are embedded. Saves refuse source aliases and replace a destination only after
encoding and metadata copying succeed.

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
SPEKTRAFILM_TEST_METADATA=1 cargo test --release --features gui --locked --lib -j4 metadata -- --ignored
# Optional real-RAW decode check:
GUI_QA_RAW="$PWD/samples/RAW_CANON_EOS_5DMARK3.CR2" \
cargo test --release --features gui --locked --lib -j4 raw_preview -- --ignored
# Offscreen interaction checks and screenshots using a cached decoded crop:
GUI_QA_RAW="$PWD/artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy" \
GUI_QA_DIR="$PWD/artifacts/gui" cargo test --features gui --locked --lib -j4 offscreen_darkroom_callbacks_and_layout
```

Tests cover export/core equality, float TIFF and PNG16 values, full-resolution
Scan, source-file protection, invalid settings, float64 edits, every parameter's
editor coverage and uniqueness, cache invalidation, monitor/export separation,
stale worker results, queued/active preview and scan statuses, recovery after
failed loading, native wheel/drag/checkbox/tab/rotation input, delayed help
and screenshots of every tab at normal/compact sizes. Reference tooltip text
and numeric wheel metadata are embedded from
`src/gui/reference_parameters.json`; regenerate it with
`python3 tools/extract_gui_metadata.py` against the pinned sibling checkout.
The matrix tooltip is adapted for separate numeric coefficient editors, and
Rust-specific controls have their own help. Camera samples and a
display server are not required for the normal tests.

The Slint application replaces the photo-editing controls and processing
workflows. It does not recreate Napari's general layer tooling, the decorative
Polaroid animation/paper-back view, or import the original Qt GUI-state JSON
schema. Its settings files use the Rust processing schema. These are intentional
presentation/schema boundaries. Audit fixes and verification boundaries are recorded in [MIGRATION.md](MIGRATION.md);
the native-control coverage test checks the Rust schema only.
