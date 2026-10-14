# Migration status

Baseline: experimental commit `28bf883e1672e884307edc75852549376e13644e`.
Original audit: **2026-10-13**, closure: **2026-10-14**. Rust source audited: `b83e4d1` (`develop`, after PR #1).
The comparison uses the pinned Git archive, not the sibling checkout's current
HEAD or today's experimental branch. All 235 copied assets pass `BASELINE.json`.
The original checkout remains untouched.

The nine findings from the 2026-10-13 audit are addressed in this branch.
Closure evidence is listed below; numerical coverage remains a tested set of
settings, not an exhaustive Cartesian product. [GUI_REVIEW.md](GUI_REVIEW.md)
records the preceding GUI review.

## Implemented processing and application features

- Rust library, JSON configuration, CLI, NPY and selectable image export and `.cube` baking.
- All copied film/print profiles, Gaussian and septic density models, BW
  development-time selection, film/print coupled gamma and developer exhaustion.
- Six spectral upsampling methods, adaptation windows and surfaces, spectral
  smoothing, camera colour filters, input locus/adjustable hull compression,
  all six output gamut algorithms and seven RGB
  spaces with matching chromatic adaptation and transfer functions.
- Film/print/scan, film/scan and colour-managed passthrough routes; intermediate
  film and print exposure/density taps.
- Neutral filter database, print exposure compensation/normalization, preflash,
  film/print base controls and highlight reconstruction boost.
- Scanner black/white luminance correction with matching upstream exposure
  anchors; optional tetrahedral colour-density and linear BW acceleration LUTs.
- Seven autoexposure meters and anti-aliased full-frame metering previews.
- Reference crop rounding, cubic B-spline up/downscaling, anti-aliasing and
  framing-independent metering and physical pixel scale.
- Camera Gaussian blur, scattering, halation bounces, DIR donor diffusion and
  receiver saturation, scanner blur and sharpening.
- Streaming multilayer grain, stock grain presets, particle statistics, dye-cloud
  blur, microstructure and mass-preserving multiplicative sharpening; glare.
- Pinned LibRaw 0.22.0 decoding, including rawpy's parameter application order.
  Default RAW test crops (lens correction off) avoid full-frame floating-point
  conversion and simulation.
- All three convert-film routes, including base-free film scans; bounded inverse
  scan solver, optional tetrahedral inverse LUT, device calibration and clear-base
  detection/fitting. CLI `calibrate` saves the fitted settings.
- All four optical diffusion families in both camera and enlarger, including
  strength, warmth and per-component overrides. Blackbody scan/enlarger lamps.
- PNG/JPEG/TIFF/WebP/EXR input; PNG8/16, JPEG8, TIFF8/16/32, EXR16/32 and NPY64 output;
  embedded RGB ICC conversion and output profile embedding where available.
- Film/print dye-spectrum permutations, matching the original GUI special controls.
- EXIF/IPTC/XMP carryover through GExiv2, corrected output tags and preserved ICC.
- As-shot, daylight, tungsten and custom temperature/tint RAW white balance;
  optional Lensfun vignetting, TCA, distortion, geometry and scale correction.

## Numerical contract

No float32 conversion is inserted into the deterministic film/print/scan math
except where the pinned model itself explicitly uses it (optical diffusion
and inverse scanning).
The original grain model's float32 stores and pairwise sums are retained. Strict
tests compare stage arrays with `atol=1e-9`, `rtol=1e-8` and reject nonfinite
results. Legacy seeded grain is compared pixel by pixel. Default approximate
sampler formulas have reference-generated stream fixtures and controlled-seed
whole-pipeline tests. Legacy and controlled fast grain densities match exactly
in tested cases. Glare is also compared with a controlled one-thread reference
seed. Arbitrary Numba thread-local stochastic patterns are not reproduced.

Timing claims must name the enabled effects, crop size and thread count. RSS
numbers include runtime startup and table/JIT preparation for both processors.
They must not be extrapolated to full-resolution RAW throughput.

## Compatibility boundaries

| Area | Current boundary | Evidence / remaining work |
|---|---|---|
| Forward film/print/scan | Strict float64 stage comparisons on the tested settings; six routes exist | `tests/reference.rs`, `tools/validate.py`; coverage is not the Cartesian product of stocks and parameters |
| Optical diffusion / inverse scanning | Source float32 operations retained; separate error bounds | `tests/core_coverage.rs`; [VALIDATION.md](VALIDATION.md) lists bounds and measured errors |
| Clear-base / device calibration | Pinned mixed precision, MINPACK finite differences and BLAS reduction order | D1 below; strict fitted parameters/residual/density/RGB fixtures |
| Grain / glare | Controlled seeds match covered reference cases | Arbitrary Numba thread-local random patterns are not reproduced |
| RAW / Lensfun | Four supplied RAW formats; optional native lens test | Decoder and database versions matter; an ordinary test run skips Lensfun unless configured |
| RGB file I/O / export | Float64 integer normalization and selectable precision | D6; OpenImageIO fixtures plus ICC/orientation tests |
| Preview / runtime APIs | Pinned reducer/preview digestion; injection, update and timings | D2/D5; injection spacing is explicit, update rebuilds tables |
| GUI | Rotation, independent delivery colour, calibration and neutral filters | [GUI.md](GUI.md); presentation boundaries remain explicit |

Lens correction requires Lensfun 0.3 and its database. No matching lens leaves
the image unchanged, as in the original; an unavailable requested library is
an error. The inverse converter requires three dye channels; BW conversion is
rejected. The experimental enlarger `lens_blur` field remains a no-op because
the pinned printing stage does not use it.

Unknown JSON fields are rejected and supported algorithm names are validated.
This is not a claim that every numeric parameter, custom profile or parameter
combination has a regression test. Preset-backed values can be replaced during
parameter digestion, as documented in [GUI.md](GUI.md).

## Audit closure

The original audit is preserved in commit `6bcaeb2`. The tests below use fixtures
extracted from the pinned source, never Rust-generated expected values.

| Finding | Change | Verification |
|---|---|---|
| D1: calibration | Rust MINPACK LM; source mixed float32/64 arithmetic, finite differences and BLAS reduction order; shared full-input GUI/CLI sampling | `tests/debt.rs`: five stock/base/exposure/degenerate cases, coefficients <1e-4, full residual <1e-5, fitted density/RGB <1e-4; original core matrix also consumed. GUI uses shared calibration helper. |
| D2: preview | Anti-aliased order-1 mirror reducer, truncated dimensions, default edge 640; source preview-mode digestion | Odd aspect/bright-edge input and all five output stages; actual RAW preview versus full decode/reduce. Scan forces preview mode off. |
| D3: file preservation | Shared transactional saves; reject canonical and hard-link source aliases; replace distinct existing destinations only on success | Failed encoding/metadata leaves destination unchanged, CLI failure test, symlink/hard-link tests. |
| D4: neutral filters | Rust neutral-M/Y helper and GUI action | Three stock/base cases compare fitted shifts and final neutral RGB. |
| D5: runtime APIs | `process_from`, `process_profiled`, validated atomic `update`, CLI stage timing | Film/print exposure/density injection with physical spacing and diffusion; fused/profiled output equality; invalidation and failed-update tests. Update rebuilds tables; separate soft-update semantics are deliberately omitted. |
| D6: file precision | Direct float64 integer normalization; PNG8/16, TIFF8/16/32, EXR16/32, ZIP/none; truncation and explicit ICC errors | Pinned OpenImageIO input/export fixtures, wide-gamut ICC and orientation, saving-colour differential fixture. |
| D7: reproducibility | Full HWC/finiteness assertions; 27 exact grain comparisons; benchmark parity gates/provenance; copied/derived/reference-code hash checks; Cargo CI | `REFERENCE_MANIFEST.json`, `tools/check_baseline.py`, explicit ignored native tests and CI native job. Frozen development-only reference environment. |
| D8: measurements | Actual Session Preview → Scan → Save benchmark, setup/load/cache-save timings and process peak RSS; core inverse/diffusion stage measurements | [BENCHMARKS.md](BENCHMARKS.md). Python defaults to cached ≤256px crops; explicit full RAW run is Rust-only. GUI event painting and OS dialogs are outside backend timings. |
| D9: rotation/save colour | Input quarter-turn state before crop/preview/scan; independent delivery conversion | Source/scan invalidation and cached-pixel preservation tests; source colour-conversion fixture. Monitor ICC remains display-only. |

[VALIDATION.md](VALIDATION.md) records the measured errors and reproduction
commands. [BENCHMARKS.md](BENCHMARKS.md) records timing/RSS with identical
settings and their limits. Fixtures do not prove every custom profile or
illuminant combination. Bitwise fit portability across arbitrary NumPy/BLAS
builds is outside the pinned compatibility contract.

## Schema and intentional application differences

Rust settings are not an import format for Python runtime or Qt GUI state.
The main mappings are:

| Pinned runtime parameter | Rust representation |
|---|---|
| `film` / `print` profile objects | `film_profile` / `print_profile` stock names |
| `film_render.chemistry`, `.base`, `.convert` | `film_chemistry`, `film_base`, `convert` |
| `film_render.grain`, `.halation`, `.dir_couplers` | Top-level `grain`, `halation`, `dir_couplers` |
| `print_render.chemistry`, `.base`, `.glare` | `print_chemistry`, `print_base`, `print_glare` |
| `convert.calibration` nine-number string | Nested numeric 3×3 array |
| `taps.collect` | Library `Tap` / CLI `--collect` |
| GUI `workflow.saving_color_space` / `saving_cctf_encoding` | Same fields; null space retains processing representation |
| `taps.inject` / `debug.print_timings` | Library injection/profiled APIs and CLI `--stage-timings` |
| `settings.preview_mode` / `settings.preview_max_size` | Same settings; GUI selects preview/scan mode automatically |

`grain.micro_sublayers` and `film_render.glare` appear in the upstream schema but
have no active consumer in the pinned processing path; their absence is not an
unimplemented working grain/glare stage. Do not infer feature coverage from
schema names alone. Print-base tilt is rejected; upstream has no such control.

Napari layer tooling, decorative Polaroid/paper-back presentation, Qt state-file
compatibility and the legacy Python/ART facade are not reproduced. Rust's seeded
noise is reproducible across its tested thread counts; arbitrary unseeded Numba
noise patterns are outside the parity claim. These explicit boundaries remain outside the migration scope.
