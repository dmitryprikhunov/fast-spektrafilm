# Migration status

Baseline: experimental commit `28bf883e1672e884307edc75852549376e13644e`.
Audit date: **2026-10-13**. Rust source reviewed: `b83e4d1` (`develop`, after PR #1).
The comparison uses the pinned Git archive, not the sibling checkout's current
HEAD or today's experimental branch. All 235 copied assets pass `BASELINE.json`.
The original checkout remains untouched.

The main film/print/scan model and six routes are implemented, but the migration
is **not complete**. Calibration, preview behavior, runtime APIs and file I/O
still differ. Passing the covered stage fixtures does not establish whole-app
parity. Open items and closure criteria below are the current migration backlog;
[GUI_REVIEW.md](GUI_REVIEW.md) records earlier fixes.

## Implemented processing and application features

- Rust library, JSON configuration, CLI, NPY/float TIFF export and `.cube` baking.
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
- PNG/JPEG/TIFF/WebP/EXR input; 16-bit PNG, JPEG, float TIFF/EXR and NPY output;
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
| Clear-base / device calibration | Model residuals ported; optimizer differs | D1 below; device matrix parity is not established |
| Grain / glare | Controlled seeds match covered reference cases | Arbitrary Numba thread-local random patterns are not reproduced |
| RAW / Lensfun | Four supplied RAW formats; optional native lens test | Decoder and database versions matter; an ordinary test run skips Lensfun unless configured |
| RGB file I/O / export | Supported formats work, but decoding precision and export options differ | D6; round trips are not differential OpenImageIO tests |
| Preview / runtime APIs | Slint preview and immutable Rust simulator have different contracts | D2 and D5; full-resolution stage tests do not verify preview equivalence |
| GUI | Native controls cover the Rust schema, not all upstream APIs/actions | [GUI.md](GUI.md); coverage tests cannot detect fields absent from `Params` |

Lens correction requires Lensfun 0.3 and its database. No matching lens leaves
the image unchanged, as in the original; an unavailable requested library is
an error. The inverse converter requires three dye channels; BW conversion is
rejected. The experimental enlarger `lens_blur` field remains a no-op because
the pinned printing stage does not use it.

Unknown JSON fields are rejected and supported algorithm names are validated.
This is not a claim that every numeric parameter, custom profile or parameter
combination has a regression test. Preset-backed values can be replaced during
parameter digestion, as documented in [GUI.md](GUI.md).

## Open technical debt

P1 means correctness or data preservation should be addressed first; P2 means a
missing capability, verification gap or performance work. These are open audit
findings, not fixes delivered by this documentation change. Upstream paths below
are relative to `src/spektrafilm/` at the pinned commit.

### D1 — P1: calibration is not numerically equivalent

Upstream `model/convert.py::fit_base_params` and `fit_blind_calibration` use
SciPy `least_squares(method="lm")`; Rust [src/convert.rs](src/convert.rs) uses
its own damped normal-equation solver with different finite differences and
termination. The device fit has 30 iterations, versus the reference's
`max_nfev=300` evaluation limit. Those limits are not interchangeable.

On `tests/fixtures/core/captured.npy`, the Rust device matrix differs from the
checked-in reference `calibration.json` by **0.0795526 max absolute / 0.0463854
RMSE**. The existing device test only requires a 95% reduction in clear-base
squared residual; it never reads that reference matrix. Base fitting compares
one fixture's CMY/exposure within 1e-4, not a range of scan conditions.

There is a separate input-contract difference: GUI `Session::calibrate` fits
the cached preview, whereas CLI `calibrate` reads the original resolution.
Upstream `src/spektrafilm_gui/controller.py` also starts from the full input
(and subsamples at most 40,000 pixels before the device fitter). A
bright clear-base border or sample selection can therefore produce different
fits; equality between the two entry points is not tested.

**Close with:** reference comparisons of fitted parameters, full residuals and
resulting density/RGB on multiple crops, including nonzero exposure, altered
base, difficult/degenerate inputs and GUI/CLI sampling. Preserve the reference
optimizer behavior or explicitly agree a measured compatibility boundary; a
residual-improvement test alone does not close this item.

### D2 — P1: preview resampling and preview mode differ

Upstream `utils/preview.py` uses anti-aliased order-1 skimage resize with truncated
dimensions; `settings.preview_max_size` defaults to 640. Rust
[src/gui/backend.rs](src/gui/backend.rs) uses integer-bin area averaging,
rounded dimensions and a fixed 768-pixel edge. It also has no equivalent of
`settings.preview_mode`, whose upstream digestion disables grain and selected
blur/sharpening operations. Slint renders the selected effects at preview scale.

This is a processing difference, not only a UI presentation choice. It can
change metering and small bright regions before the film simulation. The
framing fixtures test the core crop/rescale stage, not this GUI reducer.

**Close with:** explicit preview semantics and differential input/output fixtures
against `simulate_preview` with identical size/settings, including odd aspect
ratios, bright borders, RAW preview decoding and physical pixel spacing. Keep
Scan as the current way to judge full-resolution grain and spatial detail.

### D3 — P1: CLI output lacks the GUI's file-preservation guarantees

[src/main.rs](src/main.rs) writes `process` results directly to the destination,
then copies metadata. It accepts the input path as output. A controlled probe
with a missing GExiv2 library returned an error after replacing a disposable
existing destination. GUI `Session::export` already checks the source path and
uses a temporary file before replacement. This is a Rust CLI hardening gap,
not a claim that upstream provides atomic saves.

**Close with:** shared destination validation and transactional saves for CLI
image output, tests for source aliases and failed metadata/encoding, and an
explicit overwrite contract. Until then, use a distinct new CLI destination.

### D4 — P2: neutral print-balance solver is missing

Upstream `runtime/print_balance.py::solve_neutral_filter_shifts` solves M/Y
shifts for neutral midgray after film/base changes. Rust loads the static neutral
filter database and exposes manual shifts, but has no corresponding solver.
The original GUI exposes this as **Neutralize print filters** in
`src/spektrafilm_gui/params_manifest.py` and dispatches it from `controller.py`.
Neither the helper nor that action exists in Rust. Base detection does not
automatically rebalance the printer.

**Close with:** a Rust helper and reference fixtures for stock and modified bases,
checking fitted shifts and final neutral RGB. Database lookup is not a substitute
for this solve.

### D5 — P2: runtime entry points, updates and timing are incomplete

Upstream `runtime/pipeline.py` supports `process(..., inject=..., collect=...)`
and stage timing; `runtime/process.py` exposes update/soft-update and timing
accessors. Rust `Simulator::process[_owned]` accepts RGB input and a collection
tap, but cannot resume from external film/print exposure or density arrays.
It is immutable after construction; callers rebuild it when parameters change.
The CLI reports decode/setup/whole-process timing, not per-stage timing.

**Close with:** documented library equivalents or explicit scope decisions for
tap injection and incremental updates; differential injection fixtures and
invalidation tests if implemented. Add stage timings before attributing a
performance regression to a particular stage.

### D6 — P2: file precision and export options are not upstream-equivalent

[src/file_io.rs](src/file_io.rs) reads ordinary images through `to_rgb32f()`
before promoting to float64. Upstream `utils/io.py::load_image_oiio` normalizes
integer RGB into NumPy double, so 8/16-bit input takes a different precision
path. ICC conversion through LittleCMS and orientation handling also need
separate file-input comparisons; NPY stage fixtures bypass these operations.

Upstream saves PNG8, TIFF8/16/32 (ZIP) and EXR16/32. Rust saves PNG16, TIFF32,
EXR32 and JPEG8 (quality 95), with no bit-depth/compression selector; integer
quantization rounds rather than upstream's truncation. PNG16 is useful but does
not replace the missing TIFF/half-EXR export choices. Unsupported ICC variants
are exported without an embedded profile; export currently suppresses all
`icc()` errors, including a missing expected asset. That needs diagnostics.

**Close with:** differential file fixtures for integer normalization, tagged and
untagged input, orientation, wide-gamut ICC and supported output types; restore
needed precision/compression choices and distinguish unsupported profiles from
broken data installations. Use RGB NPY for strict float64 stage interchange;
use NPY for one-channel BW density taps as well.

### D7 — P2: validation gates and reproducibility need strengthening

- No checked-in CI workflow runs the Rust/GUI/differential suites. Lensfun and
  metadata tests return successfully without exercising their optional native
  paths unless explicitly enabled. Preserve that distinction in test reports.
- `tests/core_coverage.rs::compare` checks flat length and absolute error, not
  the full HWC shape or reference finiteness. Its 27-case grain matrix checks
  tolerance, not bit equality; exact equality was observed in this run. The
  stricter bit assertions live in the separate legacy/fast-grain golden cases.
- `tools/benchmark_core.py` records errors but does **not** assert a tolerance
  or finite/shape contract. Its report lacks the environment/binary/lockfile
  provenance that `tools/benchmark.py` records. A completed run is not a parity
  gate. The generated device-calibration matrix is also unused by tests (D1).
- `tools/prepare_reference.py` checks the 235 copied assets, but reuses an
  existing `.reference/<commit>` based on the presence of `pipeline.py`; it does
  not reverify that extracted code. Derived `reference_tables.json` and
  `diffusion.json` are outside the copied-asset manifest. Their generators and
  fixtures provide evidence, but are not an extraction-integrity check.
- The broad Python profile/parameter matrix is a dated manual result. The
  additional core fixtures mostly use small Portra inputs; there is no exhaustive
  profile × illuminant × calibration × diffusion matrix or file-codec matrix.

**Close with:** automated baseline/asset checks and release tests, explicit
optional-dependency jobs, shared shape/finiteness/error assertions, calibration
fixtures actually consumed, and reproducible report provenance. Verify extracted
reference code before regenerating fixtures; never silently bless Rust outputs.

### D8 — P2: end-to-end performance and memory remain unmeasured

Crop benchmarks exclude RAW decode, GUI work and preparation from render timing.
GUI render/validation/preset-resolution paths construct fresh simulators; profile
and table preparation is therefore relevant to edit latency. Full scans retain
float64 output, monitor conversion can clone it, and inverse/FFT paths allocate
additional image-sized arrays. Existing crop RSS cannot establish full-image
memory use or interactive latency.

**Close with:** stage/setup/cache-hit measurements and peak RSS for real GUI
Preview → Scan → Save, inverse scanning and diffusion. Keep identical settings
and physical pixel spacing; use cached sample crops for reference work by
default. Any larger reference workload should be explicit, not a benchmark
script's default.

### D9 — P2: GUI rotation and independent saving colour are missing

Upstream `src/spektrafilm_gui/controller.py` provides clockwise/counterclockwise
quarter-turns that rotate the input before regenerating the preview. Rust has
no equivalent action or source-transform state. Decoder EXIF orientation does
not replace user-controlled rotation.

The original Save action also converts the completed output into separate
`workflow.saving_color_space` / `saving_cctf_encoding` settings. Rust exports
`io.output_color_space` / `output_cctf_encoding` directly. Changing those settings
invalidates the scan and may rerender stochastic effects; it is not equivalent
to converting an already completed scan into a second delivery colour space.

**Close with:** rotation state propagated consistently through crop, preview,
Scan and Save (with cache invalidation tests), and independent export conversion
that preserves the cached simulation pixels. Verify colour conversion against
the pinned save path and keep monitor ICC separate from export settings.

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
| GUI `workflow.saving_color_space` / `saving_cctf_encoding` | No independent export settings; see D9 |
| `taps.inject`, `settings.preview_mode`, `settings.preview_max_size`, `debug.print_timings` | No matching JSON fields; see D2/D5 |

`grain.micro_sublayers` and `film_render.glare` appear in the upstream schema but
have no active consumer in the pinned processing path; their absence is not an
unimplemented working grain/glare stage. Do not infer feature coverage from
schema names alone. Print-base tilt is rejected; upstream has no such control.

Napari layer tooling, decorative Polaroid/paper-back presentation, Qt state-file
compatibility and the legacy Python/ART facade are not reproduced. Rust's seeded
noise is reproducible across its tested thread counts; arbitrary unseeded Numba
noise patterns are outside the parity claim. These explicit boundaries do not
close the processing and verification debts above.
