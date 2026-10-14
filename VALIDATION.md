# Numerical validation

The reference is experimental commit
`28bf883e1672e884307edc75852549376e13644e`. Its archive is run under Python 3.13
with the exact dependencies in `tools/requirements-reference.txt`. The original
checkout is read-only. `tools/check_baseline.py` verifies all 235 copied assets,
two derived tables and the 120 extracted Python source files against recorded
SHA-256 hashes. `prepare_reference.py` rejects modified extracted code.

The forward-stage comparison suite uses float64 HWC arrays. Diffusion and
inverse scanning and calibration have separate bounds below. Image codec
fixtures exercise the supported precisions separately. Shapes must match and all results must be
finite. The acceptance condition at every element is
`abs(actual-reference) <= 1e-9 + 1e-8 * abs(reference)`. Reports record maximum
absolute error, mean absolute error and RMSE separately; tolerances are not
substitutes for the measured errors.

The Python comparison suite covers film exposure/density, print exposure/density
and output RGB; all copied filming and printing profiles; chemistry/base controls;
all six upsampling methods; seven RGB spaces; all six output gamut methods;
input hull/locus compression; scanner anchors; density LUTs; metering; spatial
effects; grain and glare; and crop/up/downscaling. The four camera samples use
cached 256×256 center crops. LibRaw must still unpack and demosaic the capture,
but floating-point conversion and the simulation are limited to those crops.

The Rust-only regression suite contains 18 small reference cases and 90 stage
arrays. Additional tests cover unsupported configuration, malformed images,
NumPy/Numba sampler streams, and seeded renders across one and four threads.
Tested legacy and fast grain density fixtures also require identical float64 bit
patterns. Most fixtures are 17×19 pixels; one exercises the reference's signed
32-bit fast-grain particle counts on a one-pixel input.

Three additional regressions exercise CLI cube baking with crop/resize settings,
CLI baking with each internal acceleration LUT enabled, and BT.2020 decoding
around the pinned transfer-function breakpoint. Bake tests require 125 samples
for a 5³ cube and identical sample values to the exact, unframed transform.
The decoding test uses golden values from colour-science 0.4.7.

## Results, 2026-10-10

The complete Python comparison run passed **590 stage/decoder comparisons**.
The largest absolute error was **8.0268e-11**, in the chemistry variant's output
RGB (RMSE **1.0002e-11**). All ten tested seeded grain-density cases had identical
float64 bit patterns. The four RAW decoder crop errors were at most **3.3307e-16**.
The crop/rescale inputs differed by at most **1.3323e-15**.

The release build, all five Rust test functions (including the 90-array golden
loop) and Clippy with `-D warnings` passed. The detailed stage results are saved
locally in `validation/parity.json` (excluded from version control). Their timing
fields came from a validation run; use [BENCHMARKS.md](BENCHMARKS.md) for isolated timings.

## Reproduce

```sh
cargo build --release --locked -j4
cargo test --release --locked -j4
python3 tools/prepare_reference.py --source ../spektrafilm
.venv/bin/python tools/validate.py --all-profiles --samples --decode \
  --spatial --variants --grain --spectral --gamut --lut --framing --crop-size 256
```

The development virtual environment is separate from the processor. Install its
frozen requirements in a user-local Python 3.13 environment. The runtime uses no
Python libraries. Detailed generated reports and arrays are in `artifacts/parity`.

## Limits

Bit-for-bit floating-point portability across compilers, CPUs and math libraries
is not promised. The deterministic path is judged by the strict array comparison
above. The reference's grain float32 stores and reduction order are preserved.
Legacy grain uses the reference's fixed layer seeds. Fast grain/glare comparison
uses controlled reference seeds and one Numba sampling thread, because arbitrary
unseeded thread-local noise cannot be compared pixel by pixel. Default unseeded
Rust noise varies between renders; an explicit seed makes Rust renders repeatable.

This suite verifies the tested settings; audit fixes and remaining scope are in
[MIGRATION.md](MIGRATION.md). Decoder parity is established for the four supplied
RAW formats with bundled LibRaw 0.22.0. Linking a different system LibRaw or adding
other RAW formats requires additional verification.

## Audit fixes, 2026-10-14

The release suite passed **42 tests** with GUI enabled; strict release Clippy
passed. Lensfun, GExiv2, real RAW preview and offscreen screenshots were
explicitly exercised and passed. Calibration sampling additionally matches
pinned NumPy linspace at integer boundaries in its 40,000-pixel cap.

The broad Python sweep was rerun: **590 comparisons passed**, including all
profiles, four cached RAW crops/decoders and parameter/spatial/grain/gamut/LUT/
framing variants. Maximum error was **8.0268e-11** (chemistry output RGB),
RMSE **1.0002e-11**. Ten controlled grain-density cases were bitwise equal.

The source arithmetic in inverse conversion promotes float32 inputs/tables to
float64 at `LN10`, then promotes CMY after the first Newton step. Rust preserves
that mixed precision, the float32 damping coefficient, Haswell OpenBLAS reduction
order and left-looking LU. Calibration uses a Rust translation of pinned SciPy
1.18.1 LMDER with its two-point Jacobian and stopping rules. The binary32 seed logarithm uses a Rust CORE-MATH implementation matching
pinned glibc 2.43/NumPy rounding, avoiding the older glibc 2.39 log10f error.
The full source fit
residual was compared at each of the 195 evaluations in the Provia probe.

| Differential check | Measured maximum absolute error |
|---|---:|
| Direct inverse, captured/arbitrary inputs / original core fitted matrix | 0 / 0 |
| Five fitted device matrices (two stocks, altered base/EV, uniform/clear-base inputs) | 3.2729e-12 |
| Full fitted residual vectors | 1.8387e-12 |
| Calibrated film densities / output RGB | 0 / 4.5519e-15 |
| Anti-aliased preview input / five processing stages | 6.6614e-16 / 3.5749e-14 |
| Film/print injection with diffusion | ≤2.3e-7 |
| Profiled versus fused processing / validated update versus fresh simulator | 0 / 0 |
| OpenImageIO PNG8, TIFF8/16/32, EXR16/32 input and export | 0 |
| Independent saving-colour conversion | 3.3307e-16 |
| Wide-gamut ICC conversion (LittleCMS versus colour-science) | 2.5795e-4 |
| TIFF orientation / RAW white balance / Lensfun / 27 controlled grain cases | 0 / 0 / 0 / 0 |

`tests/debt.rs` consumes the five calibration fixtures. Coefficients must agree
within 1e-4, full residual within 1e-5, calibrated density/RGB within 1e-4;
these limits were preserved while fixing the fitted-parameter disagreement.
Base-fit CMY/exposure comparisons require 1e-5. Neutral M/Y shifts require 1e-4
and their final RGB requires 1e-6. Source-generated fixture provenance includes
the NumPy/SciPy/BLAS versions. Cross-BLAS bitwise fit portability is not claimed.

The core float32 allowances remain explicit: direct diffusion PSFs ≤3e-7,
complete optical paths ≤2e-6, inverse routes/LUT ≤2e-5 and arbitrary inverse
inputs ≤2e-4. The shared comparator rejects mismatched HWC shapes or nonfinite
values in either array and prints measured max error/RMSE. All 27 grain matrix
cases now additionally require exact float64 equality.

The GUI suite exercises shared full-input calibration, rotation/source/scan
invalidation, delivery-colour changes preserving completed pixels, metadata
failures, monitor separation and offscreen layout. The actual RAW reducer test
compares its bounded buffer with the full decode followed by the same reference
reducer. File dialogs and desktop interaction still need manual UI review.

Cargo CI runs baseline checks, formatting, release builds/tests and strict Clippy
for both library/CLI and GUI, then explicitly runs the native Lensfun and GExiv2
paths. Ordinary runs report these dependency-dependent tests as ignored.

```sh
python3 tools/check_baseline.py --require-reference
cargo test --release --locked --features gui -j4 -- --nocapture
cargo clippy --release --locked --all-targets --features gui -j4 -- -D warnings
.venv/bin/python tools/generate_debt_fixtures.py
SPEKTRAFILM_LENSFUN_LIBRARY="$PWD/.tools/lensfun/root/usr/lib/x86_64-linux-gnu/liblensfun.so.1" \
SPEKTRAFILM_LENSFUN_DATABASE="$PWD/.venv/lib/python3.13/site-packages/lensfunpy/db_files" \
  cargo test --release --locked --features gui --test core_coverage \
  lensfun_matches_reference_when_available -- --ignored --exact --nocapture
SPEKTRAFILM_TEST_METADATA=1 \
  cargo test --release --locked --features gui --lib metadata -- --ignored --nocapture
GUI_QA_RAW="$PWD/samples/RAW_CANON_EOS_5DMARK3.CR2" \
  cargo test --release --locked --features gui --lib raw_preview -- --ignored --nocapture
GUI_QA_DIR="$PWD/artifacts/gui" \
  cargo test --release --locked --features gui --lib offscreen_darkroom -- --nocapture
```

These paths describe optional user-local tools, not base-system installations.
`SPEKTRAFILM_GEXIV2_LIBRARY` selects GExiv2 outside the loader search path.
Detailed logs and measured arrays live in ignored `artifacts/tech-debt-audit/`.
