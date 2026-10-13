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

## Coverage limits found in the 2026-10-13 audit

The tables above are dated measurements, not evidence that all migrated paths
have a performance/parity gate. `tools/benchmark.py` asserts the strict forward
RGB comparison and records environment/binary/lockfile provenance.
`tools/benchmark_core.py` currently only records max error/RMSE: it does not fail
on a tolerance breach and does not record equivalent provenance. Review its
reported errors against the float32 path bounds in [VALIDATION.md](VALIDATION.md)
before accepting a result; fixing this is migration debt D7.

No measurements here establish Preview → Scan → Save latency, the cost of
rebuilding simulators on edits, full-resolution scan-cache/monitor memory,
calibration performance, or high-resolution diffusion/inverse peak RSS. These
are open items in [MIGRATION.md](MIGRATION.md#d8--p2-end-to-end-performance-and-memory-remain-unmeasured).
Do not infer GUI responsiveness or whole-application speedups from crop render
timings. Keep preparation/JIT outside processing timers and report their costs
separately when measuring interactive use.
