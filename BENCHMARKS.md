# Crop benchmarks

Measured on 2026-10-10 against experimental commit
`28bf883e1672e884307edc75852549376e13644e`, using the four supplied camera
captures as cached **256×256 RGB crops**, four threads and three repetitions.
Processing times below are medians per crop; ranges span the four crops.
Every benchmark output passed the same `atol=1e-9`, `rtol=1e-8` comparison.

Machine: **Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz**.
Compiler: `rustc 1.93.1 (01f6ddf75 2026-02-11) (built from a source tarball)`.
Exact timing samples, peak RSS, environment and binary/lockfile hashes are
saved locally in `validation/benchmark` (excluded from version control).

## Processing time

| Enabled effects | Pixel spacing | Reference median, ms | Rust median, ms | Speedup |
|---|---:|---:|---:|---:|
| Spatial/grain/glare disabled | 136.72 µm | 444.5–459.8 | 60.5–61.1 | 7.34–7.58× |
| Spatial, grain/glare disabled | 136.72 µm | 483.3–492.5 | 140.3–144.0 | 3.36–3.50× |
| Spatial + legacy grain, glare disabled | 136.72 µm | 594.0–621.7 | 346.8–357.6 | 1.66–1.75× |
| Spatial, grain/glare disabled | 5.86 µm | 480.3–497.9 | 195.1–196.9 | 2.44–2.55× |
| Spatial + legacy grain, glare disabled | 5.86 µm | 617.1–641.7 | 389.6–395.6 | 1.58–1.62× |

The default 35 mm format applied to a 256px test input represents coarse
136.72 µm pixels, so many physical blur kernels are below one pixel. The second
spatial/grain runs represent a **1.5 mm crop window**, giving **5.86 µm** spacing
and exercising wider physical filters. Both processors use exactly the same
format value. This finer-spacing run is a filter stress test; its window is not
derived from each camera’s sensor dimensions.

## Peak memory

| Configuration | Reference peak RSS, MiB | Rust peak RSS, MiB | RSS ratio, reference/Rust |
|---|---:|---:|---:|
| deterministic | 508.5–517.9 | 32.4–32.8 | 15.56–15.83× |
| spatial | 511.5–512.4 | 32.6–32.8 | 15.60–15.69× |
| legacy-grain | 511.7–513.4 | 32.4–32.7 | 15.65–15.81× |
| spatial-1.5mm | 509.6–510.6 | 32.2–33.0 | 15.45–15.87× |
| legacy-grain-1.5mm | 511.0–513.5 | 32.2–32.6 | 15.76–15.92× |

## Method and limits

- Stock Kodak Portra 400 → Kodak Portra Endura → sRGB, with autoexposure off.
  Default gamut compression, stock presets and direct spectral integration are
  retained. Legacy grain uses `use_fast_stats=false`; random glare is excluded.
- RAW decoding, pipeline/table preparation and reference JIT warm-up occur
  outside timed processing. Output writing is excluded. Rust consumes the input;
  copies for repeat renders occur before its timer. The reference’s internal
  preprocessing copies remain part of its `process` call.
- RSS is `/usr/bin/time`’s per-process peak, including Python imports, JIT, table
  preparation, Rust startup and processing. It is a practical process-footprint
  comparison, not a measurement of pixel buffers alone.
- These results do not establish full-resolution RAW throughput or memory
  use. They do not benchmark arbitrary unseeded fast-grain/glare patterns.
  Kernel widths and sampler costs change with physical pixel spacing.

## Reproduce

```sh
cargo build --release --locked -j4
.venv/bin/python tools/validate.py --samples --crop-size 256
for mode in deterministic spatial legacy-grain; do
  .venv/bin/python tools/benchmark.py --mode "$mode" --threads 4 --repeat 3
done
for mode in spatial legacy-grain; do
  .venv/bin/python tools/benchmark.py --mode "$mode" --film-format-mm 1.5 --threads 4 --repeat 3
done
```

## Added core paths (2026-10-11)

Same cached Canon 5D Mark III 256×256 crop, four threads, Portra 400 → Portra
Endura, autoexposure off, stochastic effects off, three measured renders.
The conversion case uses the direct inverse solver then print/scan, spatial
effects off. The diffusion case enables the default Black Pro-Mist in both
camera and enlarger, stock halation/scattering, 35 mm format, direct spectral
integration and normal scanner sharpening. Preparation/JIT is outside the
render timer; peak RSS includes startup/preparation for both processes.

| Path | Python median | Rust median | Python RSS | Rust RSS | Max RGB error |
|---|---:|---:|---:|---:|---:|
| Convert → print → scan | 2.147 s | 0.242 s | 539.9 MiB | 34.9 MiB | 6.45e-6 |
| Camera + enlarger diffusion | 0.519 s | 0.329 s | 458.5 MiB | 35.0 MiB | 6.54e-7 |

Reproduce with `.venv/bin/python tools/benchmark_core.py` after preparing the
cached crops and building the release CLI. These tiny-crop figures do not
establish full-resolution throughput; inverse calibration and large FFT kernels
need separate full-image memory/performance studies.

## Audit recheck, 2026-10-13

Rust source `b83e4d1`, release CLI built with `--features gui`; same Xeon CPU
and Rust 1.93.1 as above, Python 3.13.16. Four cached 256×256 sample crops,
Portra 400 → Portra Endura → sRGB, 35 mm format, four threads, three repeats.
Autoexposure, spatial effects and grain/glare were disabled in both processors;
default gamut compression and direct spectral integration were retained.
Preparation/JIT, decoding and output writing are outside the render timer;
peak RSS includes startup and preparation. Compilation/tests were finished
before this benchmark. All four outputs passed `atol=1e-9`, `rtol=1e-8`.

| Camera crop | Python median, ms | Rust median, ms | Python RSS, MiB | Rust RSS, MiB | Max RGB error |
|---|---:|---:|---:|---:|---:|
| Canon 5D Mark III | 450.59 | 60.89 | 465.21 | 35.96 | 2.1202e-14 |
| Hasselblad H3D39II | 451.48 | 61.16 | 456.95 | 36.27 | 1.3993e-14 |
| Mamiya ZD | 445.98 | 63.48 | 458.46 | 35.98 | 1.8735e-15 |
| Nikon D750 | 448.84 | 60.87 | 458.34 | 35.87 | 1.5266e-15 |

These measurements recheck the deterministic forward crop path only. They do
not benchmark calibration, preview parity, inverse/diffusion paths or GUI memory.
Reproduce with `.venv/bin/python tools/benchmark.py --mode deterministic --threads 4 --repeat 3`
after the release build and crop preparation above. Timing samples, environment,
binary and lockfile hashes are in the ignored
`artifacts/tech-debt-audit/benchmark.json` snapshot of the generated report.

## Workflow and core measurements, 2026-10-14

`tools/benchmark_core.py` now asserts full shape/finiteness and inverse/diffusion
error bounds, records environment/binary/lockfile/input hashes, and measures a
separate profiled execution. `tools/benchmark_workflow.py` exercises the actual
Slint `Session` backend: source load → Preview → Scan → TIFF32 ZIP Save → cached
Save. The reference uses the pinned pipeline update path and matching TIFF32
ZIP encoding, with warm-up reported separately. Monitor/display conversion is
included in Rust Session timing; the reference times processing and file saving,
without Qt/Napari display or dialogs. These are backend workflow measurements,
not an OS event-loop or file-dialog benchmark.

Python reference inputs are cached Canon 256×256 crops by default; the script
rejects reference images with a longer edge above 256. Explicit full RAW
measurements use the ignored Rust Session test alone. Process RSS includes
startup, preparation and retained scan/display buffers. Timing and RSS do not
isolate individual allocator buffers. Each Session measurement is one workflow;
core processing measurements use three renders. Compilation and validation
finish before these runs.

Reproduce after the GUI release test executable has been built:

```sh
cargo test --release --locked --features gui --lib --no-run -j4
# Use the executable path printed by Cargo, not a stale binary.
.venv/bin/python tools/benchmark_workflow.py --gui-test <lib-test-executable> --mode forward
.venv/bin/python tools/benchmark_workflow.py --gui-test <lib-test-executable> --mode convert
.venv/bin/python tools/benchmark_workflow.py --gui-test <lib-test-executable> --mode diffusion
.venv/bin/python tools/benchmark_core.py
# Explicit full RAW, Rust only; add GUI_BENCH_MONITOR for monitor ICC memory.
GUI_BENCH_INPUT="$PWD/samples/RAW_CANON_EOS_5DMARK3.CR2" \
GUI_BENCH_DIR="$PWD/artifacts/workflow-full/forward" GUI_BENCH_MODE=forward \
RAYON_NUM_THREADS=4 /usr/bin/time -f '%M KiB peak RSS' \
  <lib-test-executable> gui::tests::benchmark_preview_scan_save --ignored --exact --nocapture
```

### Cached 256×256 crop, four threads

All modes use Portra 400 → Endura → encoded sRGB, autoexposure and stochastic
effects off, 35 mm format (136.72 µm spacing), direct spectral integration.
Spatial effects are off for forward/conversion; diffusion enables both Black
Pro-Mist filters plus stock spatial effects. Each workflow is one measured run.

| Path | Preview Rust / Python, s | Scan Rust / Python, s | TIFF Save Rust / Python, ms | Cached Save Rust / Python, ms | Peak RSS Rust / Python, MiB | Max RGB error |
|---|---:|---:|---:|---:|---:|---:|
| forward | 0.639 / 0.474 | 1.186 / 0.477 | 39.67 / 57.85 | 40.01 / 28.78 | 41.73 / 449.41 | 2.1202e-14 |
| convert | 1.331 / 2.115 | 1.921 / 2.094 | 40.55 / 63.54 | 39.50 / 35.90 | 43.23 / 536.29 | 2.9137e-13 |
| diffusion | 0.774 / 0.525 | 1.455 / 0.545 | 39.88 / 62.86 | 39.88 / 32.43 | 41.81 / 456.29 | 6.5348e-07 |

Rust initial Session setup took 0.20–0.22 s; source loading 0.003–0.004 s.
Its simulator preparation is included in Preview/Scan. Python initial pipeline
setup took 0.83–0.84 s, warm-up 0.32–0.33 s; updates are included in Preview/Scan.
Worker/UI preset resolution and event painting are outside these Session timings.
The GUI backend remains slower on forward/diffusion crops despite faster fused
core processing. Rebuilding tables and scan-cache keys contributes to this
latency; these figures do not establish whole-UI responsiveness.

| Fused core path (three-render median) | Rust / Python, s | Peak RSS Rust / Python, MiB | Max RGB error |
|---|---:|---:|---:|
| convert | 0.747 / 2.123 | 35.72 / 539.86 | 2.9137e-13 |
| diffusion | 0.323 / 0.515 | 36.29 / 460.60 | 6.5348e-07 |

The inverse stage itself took 0.702 s in the separate Rust profiled call versus
1.701 s in Python. Rust diffusion profiling: exposure 0.071 s, film development
0.029 s, printing exposure 0.108 s, print development 0.002 s, scan 0.052 s.
Profiled execution is a separate run and materializes intermediate stages; its
times need not equal the fused median. The corrected mixed-precision inverse
is slower than the earlier inaccurate implementation (dated table above).

### Explicit full Canon RAW, Rust only

One workflow per mode, 5796×3870 pixels, four threads, as-shot white balance,
35 mm format (6.04 µm spacing), stock/profile/settings as above. The monitor
uses the copied encoded-sRGB ICC; its full-image display clones and the retained
float64 Scan are included in process peak RSS. TIFF32 ZIP Save has metadata
copying off. Full-resolution Python processing was not run; the crop
differential checks above establish the measured comparison for those inputs.

| Path | Preview, s | Scan including full decode/display, s | Save / cached Save, s | Peak RSS, GiB |
|---|---:|---:|---:|---:|
| forward | 1.02 | 36.36 | 12.34 / 12.57 | 1.19 |
| convert | 3.91 | 271.27 | 12.31 / 12.72 | 3.89 |
| diffusion | 1.88 | 273.71 | 12.57 / 13.09 | 5.80 |

Initial Session setup took 0.20–0.21 s and RAW preview loading 3.23–3.25 s.
Saving from cache avoids decoding/simulation but still compresses the full TIFF.
Full diffusion needs about 5.8 GiB of process memory in this run; crop RSS
cannot be used to predict its memory use. These measurements include the
selected monitor transform and exclude OS dialogs, UI painting and worker
preset-resolution time. Reports with binary, lockfile and input hashes are in
ignored `artifacts/workflow-benchmark`, `artifacts/core-benchmark` and
`artifacts/workflow-full`.
