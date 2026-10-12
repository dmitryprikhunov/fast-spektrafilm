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
