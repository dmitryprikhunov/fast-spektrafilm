# Slint branch review

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
