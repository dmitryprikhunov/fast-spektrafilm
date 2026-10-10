# Slint darkroom

The optional `fast-spektrafilm-gui` binary uses [Slint](https://github.com/slint-ui/slint)
1.18.1 and calls the Rust `Simulator` directly. It does not invoke Python or the
CLI. Build/run commands are in [README.md](README.md).

Open a camera RAW supported by the bundled LibRaw decoder or an H×W×3 RGB NPY
file. The command line also accepts an input path, `--config`, `--data-dir` and
`--threads` (four by default). NPY values use the input colour space and transfer
function in the settings, matching the CLI. RAW uses as-shot white balance and
is converted from linear ACES2065-1 to the configured input representation.

Choose a measured film and print profile, then adjust camera exposure, metering,
physical film format, print exposure/filter shifts, grain, halation, glare,
development gamma, scanner blur/sharpening and output space. The three supported
routes are Film → Print → Scan, Film → Scan and colour-managed passthrough.
The Light, Texture and Output tabs expose common controls. The JSON editor
exposes every supported `Params` field; press **Apply JSON** to use its changes.
Settings files are the same partial/default-filled JSON accepted by the CLI.
Unknown fields, unsupported algorithms and processing failures appear in the
status bar. **Reset** restores all processing defaults.

Save writes applied settings, not unapplied JSON editor text. Changing a widget
updates only its own parameter: hidden fields and untouched float64 values
survive loading, editing and saving without being rounded through Slint's
float32 controls.

The GUI caches a full-frame source preview with a 768-pixel long side and
debounces automatic updates by 300 ms. Decoding, simulator construction,
processing and export run on a dedicated worker with its own Rayon pool.
Superseded preview requests/results are discarded. Disable automatic updates
to edit several settings, then press **Update preview**. The pending label marks
an image rendered with older settings. An update already in progress follows
the latest settings. Preview images are converted to encoded sRGB for display,
including when the chosen export space is wide gamut or linear.

RAW previews area-average the entire decoded bitmap directly into a bounded
float64 buffer. LibRaw still allocates its full demosaiced native bitmap, but
the preview does not allocate a full-resolution float64 RGB image. NPY inputs
are read fully before preview reduction. Processing uses the same film model
and physical format at preview resolution, so metering and fine spatial/grain
detail can differ from an original-resolution export. Preview speed is not a
full-resolution throughput measurement.

Choose the file format in the Output tab (PNG by default). **Export** reopens and processes the original image at full resolution with
the current settings, including framing/upscale settings. PNG is clipped,
8-bit encoded sRGB. TIFF is float32 RGB and NPY is float64 RGB in the configured
output colour space and transfer function, matching CLI export. These files do
not embed an ICC profile. Settings edits are disabled while exporting. Export
refuses the source path; settings/export files are written to a temporary file
in the destination directory and replaced only after success.

The GUI currently does not open ordinary JPEG/PNG/TIFF images, apply ICC monitor
profiles, provide custom RAW temperature/tint or lens correction, or expose
unsupported scan-inverse/calibration routes. The core numerical contract and
remaining work remain documented in [MIGRATION.md](MIGRATION.md).

## Verification

```sh
cargo test --release --locked -j4
cargo test --features gui --locked --lib -j4
# Optional real-RAW parity check and offscreen UI screenshots:
GUI_QA_RAW="$PWD/samples/RAW_CANON_EOS_5DMARK3.CR2" \
GUI_QA_DIR="$PWD/artifacts/gui" cargo test --features gui --locked --lib -j4
```

GUI tests compare original-resolution NPY export exactly with direct simulator
output, verify float TIFF and sRGB PNG pixels, preserve files after errors,
check settings precision and full-frame preview reduction, exercise actual
Slint callbacks and render the developed/comparison/empty states at 1240×850
and a compact window at 980×700 using Slint's software renderer. Camera samples
and a display server are not required for the normal tests.
