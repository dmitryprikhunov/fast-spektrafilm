# Slint branch review

This is a historical record of fixes, not a complete parity sign-off. The
2026-10-13 audit of `develop` found remaining calibration, preview, runtime and
I/O gaps; [MIGRATION.md](MIGRATION.md) is the current backlog. In particular,
"core controls" below means controls represented by the Rust parameter schema.

Review target: `feature/slint-gui`. Baseline: `codex/core-coverage` (`f70341a`).
Reference: experimental `28bf883e1672e884307edc75852549376e13644e`, especially
`params_manifest.py`, `params_mapper.py`, `widget_sections.py` and the supplied
GUI screenshot. The original checkout was not modified.

## Fixed findings

- **P1 — Input decoding and stale RAW cache.** Every non-NPY file was treated as
  RAW, and RAW always used as-shot settings. Shared I/O now handles the actual
  formats, ICC and orientation. The cache includes RAW and input-colour settings.
- **P1 — Export changed the requested colour representation.** PNG forced sRGB8
  and all exports omitted ICC. Shared encoders now write PNG16/JPEG/TIFF/EXR/NPY
  with configured colour and available profiles. Metadata carries over without
  replacing TIFF's output profile. Failed GUI saves preserve existing files.
- **P1 — Core features required editing JSON or were unreachable.** Native
  controls now cover the core parameters, six routes, calibration, crop, gamut,
  diffusion and RAW settings. A coverage test rejects missing/duplicated editors.
  The fixed input algorithm, CLI LUT mode and upstream no-op enlarger blur are
  not shown as photo controls; unsupported print-base tilt is omitted.
- **P1 — Manual preset-backed edits could have no effect.** Controls receive
  the resolved stock values; editing grain, couplers or neutral filters disables
  their automatic preset selection.
- **P2 — Original dye permutations were absent from the processing schema.**
  Film and print dye spectra can now be permuted independently, before parameter
  digestion. Reference-generated RGB fixtures verify that sensitivity and
  development channels are not mistakenly permuted.
- **P2 — No full-resolution display operation.** Scan now processes the original
  resolution; zoom shows actual pixel scale and scrolls large results. Preview
  remains bounded and Save always uses current full-resolution settings.
- **P2 — Layout competed with the image and duplicated actions.** Removed the
  slogan, persistent JSON panel and duplicate Open/Export controls. The photo,
  grey canvas, paper border, right-side tabs/collapsible groups and bottom action
  bar follow the original UI's arrangement.
- **P2 — No monitor transform or persisted startup defaults.** Monitor ICC is
  isolated from export colour; Config can save current settings as startup defaults.

## Validation and limits

Core release tests (20), GUI/metadata tests (7), and strict core/GUI Clippy
passed before integration. Optional GExiv2 and Lensfun checks were enabled. UI checks use Slint's actual software renderer and pointer events,
including normal/compact windows and all five tabs. The test suite exercises
stale-result rejection, failure recovery and closing the window while the
worker exists. Real RAW decoding was checked separately against full-frame
area reduction. See GUI.md for commands and intentional application differences.

## Import/Save review, 2026-10-13

Review target: `fix/gui-import-save-threading`, PR #1 against `develop`
(`23cf70d`). Two additional findings were reproduced and fixed:

- **P1 — Reopening a source could retain the old image.** A newer preview could
  discard the request carrying the one-time reload flag. When the same filename
  was opened after its contents changed, the cached preview and scan survived.
  Each preview now carries the source generation, so discarding an obsolete
  request cannot discard the reload intent. A worker regression queues both
  requests before starting the worker and requires the replacement dimensions.
- **P2 — An invalid monitor ICC discarded the completed scan.** Full-resolution
  rendering cleared the scan before opening or validating the monitor profile.
  A failed display update then forced Save to rerender stochastic grain. Monitor
  bytes and the transform are now checked on one pixel before releasing the
  scan. The regression checks missing and corrupt profiles, removes the source,
  and requires an export identical to the original scan.

Both regressions failed before the fixes and passed afterward. Native file
dialogs are covered through the shared asynchronous completion path with a
blocked picker and cancellation; interactive desktop dialogs were not manually
exercised in this review.

The complete release suite passed 32 tests with GUI enabled, including the pinned
reference stages, metadata round trips and Lensfun against the fixture database.
Strict release Clippy with all targets and GUI enabled, formatting checks and
all 235 copied-asset hashes passed.

The four cached 256×256 camera crops were benchmarked against the pinned reference
with identical Portra 400 → Endura → sRGB settings, autoexposure and spatial/
stochastic effects disabled, four threads and three repetitions. Median processing
times ranged from 60.52–61.01 ms for Rust and 450.07–470.60 ms for Python. Peak
process RSS was 36.11–36.22 MiB and 456.61–465.62 MiB respectively; it includes
startup and preparation. Maximum absolute output error was 2.1202e-14. These
measurements cover the core crop transform; they do not measure GUI scan-cache
memory or full-resolution RAW throughput. Detailed logs and benchmark provenance
remain under ignored `artifacts/`.

## Preview and parameter controls, 2026-10-16

Review target: `refactor/gui-preview-controls`, based on `develop` (`0ac296d`).
The pinned reference GUI and supplied screenshots guided the layout and help.

- Mouse-wheel zoom keeps the image point under the pointer fixed. Dragging pans
  the image; Original, Processed and Compare share navigation. Fit, pixel-scale
  presets and double-click reset the view without processing another image.
- Rotation uses two 44 × 32 pixel native buttons with 20-pixel SVG symbols in
  the preview footer, beside the zoom presets. Pointer checks exercise both
  directions at 1240 × 850 and 980 × 700, including from the Print tab.
- Numeric wheels use the reference's steps and bounds; dropdowns and checkboxes
  accept wheels as well. Small deltas accumulate, and unchanged edits preserve
  active work and custom lightness-compression knees. Output depth has discrete
  Automatic/8/16/32 choices. Text entry retains float64 precision.
- Delayed help covers labels, input fields and checkboxes. Help and numeric
  metadata are copied from the pinned Python manifests into embedded JSON,
  with source hashes and GPL attribution. The extraction script reproduces
  the JSON byte-for-byte. Native tooltip popups wrap long explanations; their
  hover observer precedes native editor input handling.
- Main/Film/Print follow the reference's group order, including nested Grain
  sections. Spectral upsampling and experimental illuminant/dye controls live
  in Advanced. Automatic preview and all six workflows remain in the footer.
- Queued previews immediately show their pending status. Active previews,
  scans, calibration and saves retain their processing message when settings
  notifications arrive. Invalid entries remain visible alongside that message;
  obsolete worker results cannot replace the current operation's status.

The cached-crop benchmark used Portra 400 → Endura → sRGB, four threads, three
timed repetitions after warmup, with autoexposure and spatial/stochastic effects
disabled identically in Rust and the pinned reference. All comparisons passed
the strict float64 contract.

| Cached 256 × 256 crop | Rust median | Reference median | Rust peak RSS | Reference peak RSS | Maximum absolute error |
| --- | ---: | ---: | ---: | ---: | ---: |
| Canon EOS 5D Mark III | 60.43 ms | 457.59 ms | 36.68 MiB | 468.20 MiB | 2.1202e-14 |
| Hasselblad H3D-39 II | 61.13 ms | 448.10 ms | 36.47 MiB | 468.28 MiB | 1.3993e-14 |
| Mamiya ZD | 60.82 ms | 452.79 ms | 36.42 MiB | 455.98 MiB | 1.8735e-15 |
| Nikon D750 | 60.66 ms | 448.20 ms | 36.30 MiB | 456.06 MiB | 1.5266e-15 |

RSS includes startup and preparation in both processes. These measurements cover
the core crop transform. GUI interaction checks use Slint's software renderer;
the table does not measure GUI resident memory or full-frame RAW throughput.
Run `.venv/bin/python tools/benchmark.py --threads 4 --repeat 3` to reproduce;
full provenance and per-run numbers are in ignored
`artifacts/benchmark/deterministic/report.json`. Interaction screenshots and
validation logs are under ignored `artifacts/gui-refactor/`.

Validation passed: 54 release tests (four optional/benchmark tests ignored),
strict all-target release Clippy with GUI enabled, formatting, all 235 asset
hashes and byte-for-byte regeneration of the GUI metadata. Release checks used
the CI settings: LTO disabled, 16 codegen units and four processing threads.
The offscreen GUI regression also passed in debug mode. The original sibling
checkout remains clean at the pinned commit.
