# Numerical review implementation — 2026-10-15

Implements the [13-task review](numerical-2026-10-14.md) on
`codex/numerical-review-implementation`, against experimental
`28bf883e1672e884307edc75852549376e13644e`. The original checkout and the 235
measurement assets are unchanged. Processing remains Rust. Python scripts only
extract reference results, validate and measure.

The camera now executes highlight boost → diffusion → lens blur → halation.
Inverse LUT construction uses the reference float32 gamut grid, batch forward
arithmetic, one logical inverse batch and the appropriate calibration
pretransform. All inverse nodes at 8³/9³/17³/32³ in the committed interaction
fixtures match exactly. A render chooses its seed once: explicit seeds repeat
across executors/threads; absent seeds vary fast grain/glare. Legacy grain keeps
its reference layer seeds.

`numerical-contracts.json` supplies the Rust and Python gates. Direct inverse,
calibrated output and same-resolution inverse LUTs now use
`1e-9 + 1e-8*abs(reference)`. Device coefficients and residuals use 1e-10.
The separate clear-base fit has a 2e-6 coefficient limit: its measured maximum
is 8.22550e-7, versus 3.27283e-12 for device-matrix coefficients. These are
different optimization problems. Controlled grain densities require identical
bits. Comparators reject shape/nonfinite inputs and report max error, RMSE,
failure count, maximum tolerance ratio and worst coordinates.

Diffusion retains an explicit float32 FFT exception: 3e-7 for standalone filters
and camera log exposure, 2e-6 for propagated optical paths. The combined optical
interaction fixtures measured at most 1.112e-7 across their five taps. This is
not universal strict forward parity.

| Task | Implementation and regression protection |
|---|---|
| T1 | Correct camera order; four families, sigma 2.999999/3/3.000001, BW/reversal, edge highlights and odd dimensions; all five taps, both executors and injection |
| T2 | Batch LUT bake and gamut bounds; node grids, calibrated LUT/direct cases, altered base/lamp, negative/dark/HDR and 1/2/odd/even batches |
| T3 | Common seed selection; controlled Python glare plus unseeded variability and explicit-seed execution at 1/2/4 threads |
| T4 | Shared named contracts, calibration bounds, final finiteness on every processing return, numeric-setting checks and deliberate 1e-6 regression rejection |
| T5 | Grain×chemistry, anchors×LUT×base, conversion×calibration×LUT, framing×spacing; consistent executor per CLI timing series with explicit warmups |
| T6 | Owned diffusion, cached FFT plans/PSFs/transforms, explicit reusable FFT scratch, sequential channels and byte budgets; smooth 2/3/5 lengths independently regated against Python |
| T7 | FIR strips replace full temporary planes/reflection tables; vertical IIR uses contiguous column blocks; original axis order, recurrence states and quantization remain |
| T8 | Candidate densities replace full-image RGB/Jacobian arrays; global convergence and odd-final-row arithmetic remain; owned conversion reuses source storage |
| T9 | Float32 layers, accumulated grain and USM planes; probability storage reused as layer storage; completed count/saturation storage released before the last blur; draw order and pairwise reductions retained |
| T10 | Shared immutable tables/profiles/responses and derived LUTs; file-identity and effective-setting keys, byte-bounded retention, atomic updates; inexpensive preset resolution and retained simulator in GUI sessions |
| T11 | Film development reuses log-exposure storage after donor corrections are computed; stack arrays replace tiny per-pixel allocations; owned no-op diffusion and duplicate input scan removed |
| T12 | Explicit spline-axis line iteration and reusable preview-row scratch; channel spline prefilter and 4×4×4 reduction order retained; single-axis/asymmetric framing fixtures |
| T13 | Scheduled/on-demand pinned Python job, hashed compact RAW fixtures, full diagnostics, embedded compiler metadata and separate port/LUT-approximation measurements |

The stage mapping is explicit: preprocessing uses existing framing/preview and
new one-axis/physical-spacing fixtures; filming exposure uses profile/spectral/
optical fixtures; development uses chemistry and exact grain fixtures;
conversion uses inverse node/batch/calibration fixtures; printing uses density
LUT, optical and anchor/base interactions; scanning uses the scanner/gamut/
transfer fixtures. `tests/review.rs` checks materialized/injected/fused execution
at each applicable tap. Profiled stage sums describe the materializing executor;
they are not percentages of the fused runtime.

Caches retain immutable payloads and keys within their byte budgets. Active
renders may hold additional `Arc` owners after eviction. FFT-plan accounting is
conservative; vector capacities and process RSS are reported separately. Large
entries bypass retention. File identity uses canonical path, size, modification
time and Unix device/inode/change time; failed simulator updates retain the
previous working state. Processing, display and delivery keys remain separate.

The full-frame FFT examples are capacity estimates, not full-frame benchmarks.
Smooth complex transforms reduce the power-of-two capacity cliff while preserving
linear convolution and reflected padding. A real-FFT backend remains a possible
further reduction; it was not introduced without its own backend/precision
validation. No full-size Python sample simulation is used.

Reproduction from a clean checkout (original RAW files are optional):

```sh
python3 tools/check_baseline.py
cargo test --release --locked -j4 -- --nocapture
cargo build --release --locked -j4
# Extract the pinned source using tools/prepare_reference.py and install the
# exact development-only tools/requirements-reference.txt in a Python 3.13 venv.
.venv/bin/python tools/check_review_fixtures.py
.venv/bin/python tools/validate.py --all-profiles --spatial --variants --grain \
  --spectral --gamut --lut --framing --cached-crops tests/fixtures/raw_crops --crop-size 64
.venv/bin/python tools/review_numerics.py
```

For the four existing 256px RAW caches, the original broad command in the review
still applies. Benchmark separately after builds/tests/reference validation end:

```sh
.venv/bin/python tools/measure_revisions.py --build
# Only after compilation has finished:
.venv/bin/python tools/measure_revisions.py --measure
.venv/bin/python tools/review_benchmark.py --sizes 64 128 256
```

Full generated reports stay under ignored `artifacts/`. After completing all
validation/benchmark runs, `tools/summarize_numerical_review.py` assembles the full
implementation report there. To recover the archived reports and regenerate
the checked-in summary from the original measurements:

```sh
python3 tools/summarize_numerical_review.py --archive-commit 9b8d78bdcb509ff6d48ded574761f94068208f1b
```

The revision harness uses identical allocator instrumentation and explicit
`lto=false, codegen-units=16` for its historical snapshots. Its stage timings and
Rust allocator counters are distinct from the ordinary CLI release benchmark
(`lto=thin, codegen-units=1`). Input cloning precedes measurement. RSS includes
startup, setup, warmups and retained caches; it is not a live pixel-buffer count.
Observed timing spread is retained before interpreting small changes.

## Validation and measured results

The final GUI-enabled release suite passed **54 tests**. The native metadata,
Lensfun and full-decode-versus-bounded-RAW-preview checks also passed explicitly.
The broad sweep passed **590** stage/decoder comparisons, including **10** bitwise
grain-density comparisons. The 35 new interaction cases contain **129** stage
arrays; the Gaussian sweep adds 20 arrays at 1/2/4 threads. Pinned Haswell Python
rechecked all interaction golden arrays. Formatting, pinned Clippy with warnings
denied, workflow lint and asset/fixture hashes passed.

[The compact summary](numerical-2026-10-15-summary.json) retains validation counts,
worst errors, timing ranges, RSS, environment/build hashes and checksums of the
full reports. [The archived implementation report](https://github.com/dmitryprikhunov/fast-spektrafilm/blob/9b8d78bdcb509ff6d48ded574761f94068208f1b/docs/reviews/numerical-implementation-2026-10-15-evidence.json)
retains per-array diagnostics, input/config hashes, full BLAS/compiler/dependency
provenance, raw repetitions, stage timings, allocator counts and capacity estimates.
Its commit permalink preserves the original measurements without including them
in the current PR diff. Fixture manifests share byte-identical inputs; all 35
interaction cases, 129 stage golden arrays and 20 filter golden arrays remain.
Validation-run timings are diagnostic; only the separately isolated benchmark
sections support performance claims.
The broad sweep's worst error was **8.02678e-11** (chemistry/rgb_out, RMSE 1.0002e-11).

The original 64px sigma-3 camera probe now has RGB max error **8.38518e-9**
(RMSE **1.98955e-9**), versus **0.00429285** before the fixes. Its 32³ inverse
LUT CMY error is now **zero**, versus **1.63900e-4**. Reference/Rust gamut bounds
and the separate 9³ logical inverse grid also agree exactly. Scalar per-node
inversion still differs from a batch by design; it is no longer used to bake LUTs.

Ordinary CLI timings below use the cached Canon **256×256** ProPhoto-linear crop,
four threads, one full-crop warmup and three measured repetitions per executor,
with autoexposure off. Rust uses `lto=thin, codegen-units=1`; Python uses the pinned
Haswell environment. Fine spacing is 5.859375 µm/pixel (1.5 mm window); coarse is
136.71875 µm/pixel (35 mm window). All settings and the 64/128/256 size series
are retained. Timings are medians; RSS is each process's measured peak.

| Mode | Rust ms | Python ms | Rust RSS MiB | Python RSS MiB | Max RGB port error |
|---|---:|---:|---:|---:|---:|
| Forward, coarse | 60.28 | 454.76 | 34.43 | 456.47 | 2.12e-14 |
| Spatial, fine | 97.90 | 479.79 | 34.60 | 458.95 | 6.42e-14 |
| Spatial + legacy grain, fine | 355.23 | 630.79 | 34.71 | 461.92 | 4.39e-15 |
| Diffusion, coarse | 237.59 | 512.09 | 34.48 | 459.64 | 4.28e-07 |
| Diffusion, fine | 346.10 | 602.28 | 37.75 | 458.40 | 1.08e-07 |
| Direct inverse → print → scan | 733.31 | 2062.90 | 34.80 | 537.23 | 2.91e-13 |
| 32³ inverse LUT → print → scan | 55.45 | 423.23 | 34.98 | 491.52 | 2.93e-13 |

Every profiled output agrees with its fused output under the strict gate.
Diffusion remains subject to its named optical exception. The inverse-LUT
approximation is reported separately: for this 256px crop the reference LUT
versus reference direct RGB max error is **0.374571** (RMSE **0.00530178**). That is approximation error, not a port failure.

The allocator-instrumented stage harness builds historical task revisions with
identical `lto=false, codegen-units=16` and runs them separately after all builds
and validation finish. Values below are medians of five warm measurements. Peak
additional live bytes count Rust allocations relative to the prepared input;
they exclude that input and native allocations. They are not process RSS.

| Change / isolated stage | Before → after ms | Allocations before → after | Additional peak MiB before → after |
|---|---:|---:|---:|
| T10 exposure update | 527.84 → 21.40 | 5101 → 4765 | 24.736 → 0.149 |
| T11 materializing executor | 210.25 → 207.92 | 68722 → 3185 | 8.000 → 6.500 |
| T6 preparation reuse, original FFT lengths | 149.06 → 86.07 | 18477 → 16 | 19.020 → 9.516 |
| T8 inverse solver | 699.47 → 684.69 | 11 → 4 | 9.750 → 5.250 |
| T7 FIR, sigma 1 | 4.69 → 2.68 | 781 → 52 | 2.520 → 1.625 |
| T7 IIR, sigma 3 | 5.45 → 3.30 | 4 → 4 | 2.000 → 2.000 |
| T9 legacy grain | 164.61 → 161.27 | 328 → 302 | 4.625 → 2.317 |
| T12 cubic resize | 30.00 → 24.66 | 10 → 10 | 3.422 → 3.422 |
| Combined diffusion including smooth lengths | 146.18 → 47.20 | 18477 → 15 | 19.020 → 6.013 |

For a 97px preview, row scratch batching reduces allocations from
100 to 19. The measured medians were
4.60 → 5.48 ms, with unchanged additional peak storage
(0.262 MiB). This small-crop probe shows no preview-resize latency benefit;
no speedup is claimed for it. Solver/grain time changes are also small relative
to some observed spreads; their memory reductions are the stronger result.

Caching can increase process RSS while reducing per-render workspace. With the
original FFT sizes, diffusion caching measured 53.03 MiB RSS versus 36.91 MiB
before caching, despite lower additional live allocation. Smooth lengths bring
the combined diffusion probe to 35.57 MiB RSS and 6.013 MiB additional live
allocation. Neither figure should be extrapolated to full frames.

The 6000×4000 / 35 mm capacity helper selects 11250×9216 transforms. The two
complex planes and FFT/column scratch account for **1.545 GiB**, versus the
previous two planes alone at **4.00 GiB**. The three-channel PSF remains
**155.56 MiB**. Images, allocator overhead, FFT-plan internals and other live
stages are additional; this is an analytical estimate, not measured full-frame RSS.

Actual GUI Session operations on the same cached 256px crop were measured
separately. These are single workflow observations, not speedup estimates.
The GUI test executable uses `lto=false, codegen-units=16`. Rust Session setup
loads tables and the first Preview constructs the simulator; Python initial
simulator setup is a separate timer. Compare complete lifecycles rather than
assuming the setup and Preview components contain identical work.

| Rust GUI mode | First Preview ms | Scan ms | Save ms | Cached Save ms |
|---|---:|---:|---:|---:|
| forward | 488.26 | 125.39 | 42.35 | 39.63 |
| convert | 1152.70 | 821.08 | 70.24 | 50.43 |
| diffusion | 574.12 | 271.65 | 61.28 | 46.59 |

The final preview scheduling follow-up has its own build/hash and measurements.
All numerical gates, observed ranges and individual repetitions are in the
archived implementation report; small timing differences are not generalized into speed claims.
