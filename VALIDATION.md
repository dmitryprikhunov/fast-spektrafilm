# Numerical validation

The reference is experimental commit
`28bf883e1672e884307edc75852549376e13644e`. Its archive is run under Python 3.13
with the exact dependencies in `tools/requirements-reference.txt`. The original
checkout is read-only. `tools/prepare_reference.py` verifies all 235 copied data
assets against their recorded SHA-256 hashes.

The forward-stage comparison suite uses float64 HWC arrays. Diffusion and
inverse scanning have separate bounds below; calibration and image I/O are not
covered by this strict contract. Shapes must match and all results must be
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

This suite verifies the tested settings, not complete migration or the open debts in
[MIGRATION.md](MIGRATION.md). Decoder parity is established for the four supplied
RAW formats with bundled LibRaw 0.22.0. Linking a different system LibRaw or adding
other RAW formats requires additional verification.

## Additional core coverage

`tools/generate_core_fixtures.py` extracts fixtures from the same pinned source.
`cargo test --release --locked --test core_coverage -- --nocapture` prints errors.
The added deterministic float32 stages are tested separately: direct diffusion
PSFs ≤ 3e-7, complete optical paths ≤ 2e-6, inverse routes/LUT ≤ 2e-5, arbitrary
inverse inputs ≤ 2e-4. These are explicit allowances for the source's float32
FFT/solver stores, not a relaxation of the existing float64 tests. RAW
white-balance fixtures match exactly; base-fit parameters agree within 1e-4.
Blind calibration is checked only by reduction of the observed clear-base
residual. The reference matrix in `core/calibration.json` is not consumed by
that test. This is an actual numerical-compatibility gap, not merely a lack of
bitwise SciPy MINPACK equivalence; see the audit result below and migration D1.

The Lensfun differential test is enabled with `SPEKTRAFILM_LENSFUN_LIBRARY`
pointing to a Lensfun 0.3 library and `SPEKTRAFILM_LENSFUN_DATABASE` pointing
to the same XML database used to generate the fixture. It exercises all native
corrections plus Rust float32 bilinear resampling. The test used the bundled
reference lensfunpy database. Native packages used for local validation live
under `.tools/`; they are not installed into the base system or committed.

The grain matrix adds 27 comparisons: three stock classes × three image shapes
× 1/2/4 Rayon threads. Its assertion is max absolute error < 1e-9; zero error was
observed for all cases. Only the designated cases in `tests/reference.rs` enforce
identical float64 bit patterns. The core comparison helper checks flat length,
not HWC shape, and does not explicitly check reference finiteness (migration D7).

## GUI integration review

Film/print dye permutations are tested against `core/dye_swap.npy`, generated by
`tools/generate_dye_fixture.py` from the pinned GUI mapper's exact spectrum-column
permutation, with a maximum-error bound of 1e-8. The GUI suite verifies native
parameter coverage, source cache invalidation, full-resolution scanning and
colour-preserving export. Opt-in GExiv2 tests copy synthetic EXIF/IPTC/XMP into
PNG/JPEG/TIFF, re-read the tags, and require unchanged output pixels and ICC.
The synthetic metadata source is generated by `tools/generate_metadata_fixture.py`.

## Migration audit, 2026-10-13

Source inspected: `b83e4d1`, compared with the pinned archive. The 235 copied
asset hashes passed again; all 120 archived `src/**/*.py` files were also
compared byte-for-byte with a fresh `git archive` of the pinned commit. The
historical 590-comparison Python sweep above was not rerun for this documentation
audit; do not interpret its date as a new
result. Current checked-in fixtures and focused probes were used instead.

The release suite with GUI enabled passed **32 tests**. Lensfun and GExiv2
were then explicitly enabled and passed their focused tests. Strict release
Clippy (`--all-targets --features gui -- -D warnings`), formatting and documentation
link checks passed. The real-RAW preview check was not enabled in this audit. Current measured fixture errors:

| Comparison | Maximum absolute error |
|---|---:|
| Four direct diffusion families | 1.1921e-7 |
| Camera/enlarger diffusion stage arrays | 1.1592e-6 |
| Direct inverse, captured / arbitrary input | 5.1103e-7 / 4.0454e-6 |
| Three inverse routes / inverse LUT | 1.0254e-6 / 3.0898e-6 |
| RAW temperature/tint / Lensfun / 27 grain cases | 0 / 0 / 0 |
| Blackbody spectrum | 4.4409e-16 |
| PNG16 / float TIFF round trip (not upstream differential) | 7.5731e-6 / 2.9629e-8 |

Device calibration reduced the clear-base squared residual from 3.1295e-2 to
1.1155e-7, satisfying its current test while still differing from the reference
fit. Test/probe logs are local under `artifacts/tech-debt-audit/` (ignored).

The device-calibration probe used the existing 7×9 RGB `captured.npy` fixture,
four threads and default conversion settings. Relative to the reference matrix
in `core/calibration.json`, max absolute coefficient error was **0.0795526** and
RMSE **0.0463854**. These are matrix-coefficient errors, not output RGB errors.
The passing residual-reduction test does not justify a calibration-parity claim.
Reproduce independently of the test's acceptance criterion:

```sh
mkdir -p artifacts/tech-debt-audit
target/release/fast-spektrafilm --threads 4 calibrate \
  tests/fixtures/core/captured.npy artifacts/tech-debt-audit/device.json --device
.venv/bin/python - <<'PY'
import json
import numpy as np
from pathlib import Path
actual = json.loads(Path("artifacts/tech-debt-audit/device.json").read_text())
expected = json.loads(Path("tests/fixtures/core/calibration.json").read_text())
delta = np.asarray(actual["convert"]["calibration"]) - np.asarray(expected)
print("max_abs", np.abs(delta).max(), "rmse", np.sqrt(np.mean(delta**2)))
PY
```

File-I/O round trips in `tests/core_coverage.rs` use sRGB PNG/TIFF tolerance
2e-4; JPEG checks length only. They are not comparisons against the pinned
OpenImageIO decoder/encoder and do not establish ICC/EXIF/EXR/WebP equivalence.
GUI resize tests check the Rust area-average behavior, not the upstream skimage
preview. Neither set of tests closes migration D2 or D6.

### Optional native paths

A normal green suite can include early-returning native tests. To exercise them,
use a Lensfun 0.3 library and the **same database** as the fixture generator, and
explicitly enable GExiv2 tests. For the existing user-local layout:

```sh
SPEKTRAFILM_LENSFUN_LIBRARY="$PWD/.tools/lensfun/root/usr/lib/x86_64-linux-gnu/liblensfun.so.1" \
SPEKTRAFILM_LENSFUN_DATABASE="$PWD/.venv/lib/python3.13/site-packages/lensfunpy/db_files" \
  cargo test --release --locked --features gui --test core_coverage \
  lensfun_matches_reference_when_available -- --nocapture
SPEKTRAFILM_TEST_METADATA=1 \
  cargo test --release --locked --features gui --lib metadata -- --nocapture
```

These paths describe optional local tools, not repository dependencies. Set
`SPEKTRAFILM_GEXIV2_LIBRARY` when GExiv2 is outside the loader's search path.
`GUI_QA_RAW` and `GUI_QA_DIR` enable the separate real-RAW preview and screenshot
checks described in [GUI.md](GUI.md); native file-dialog interaction still needs
manual desktop testing. No packages are installed by these commands.
