# v0.1.2 measurements

Fresh measurements on 2026-10-17 of the **published Linux x86_64 binary**, release
commit `331f487a46cf87814e0f389c8dce248a8d68288d`, against original spektrafilm
experimental commit `28bf883e1672e884307edc75852549376e13644e`.
[Release identity and hashes](release.json) identify the archive and executable;
the archive checksum and all 235 original measurement-asset hashes were verified.

Machine: Intel Xeon E5-2680 v4 @ 2.40 GHz, Linux x86_64, host glibc 2.43.
The published binary was built with Rust 1.93.1 on Ubuntu 24.04 / glibc 2.39,
normal release profile, thin LTO and one codegen unit. Reference: Python 3.13.16
and [pinned dependencies](../../../tools/requirements-reference.txt).
Full build/runtime provenance is retained in the JSON reports below.

## Speed and memory

Four cached 256×256 center crops from the Canon 5D Mark III, Hasselblad H3D39II,
Mamiya ZD and Nikon D750 captures in `samples/`. Both implementations receive
identical float64 RGB arrays and settings: Portra 400 → Portra Endura → sRGB,
35 mm film format, autoexposure off, direct spectral integration and default
gamut compression. Each report contains the exact config and input hashes.

Four threads in Rust and in the reference's Numba/OpenBLAS/OpenMP environment.
One full-crop warm-up, then three measured renders per camera and mode.
No benchmark jobs ran concurrently with compilation or validation.

- **Speed:** median of the four camera medians; whiskers span those camera
  medians. Decoding, pipeline preparation, JIT warm-up and export are excluded.
- **RAM:** median of four process peak RSS measurements from `/usr/bin/time`;
  whiskers span the cameras. Includes startup, Python imports, JIT, preparation
  and processing. This measures complete process footprint, including the
  different runtime overheads.
- **Modes:** no spatial effects; spatial effects; spatial effects plus legacy
  grain (`use_fast_stats=false`, glare off). Grain and glare are off in the
  first two modes. Every benchmark output also passed the strict float64 gate.

These are crop/core results, not full-resolution throughput, GUI latency or
GUI memory measurements. The 35 mm format on a 256px crop gives 136.72 µm pixel
spacing; fine-scale and full-image kernels have different costs.

Raw timing samples, peak RSS and errors:
[No spatial effects](deterministic.json) · [Spatial](spatial.json) ·
[Spatial + legacy grain](legacy-grain.json).

## Accuracy

[Forward-stage and decoder report](accuracy.json): the broad differential sweep
covers five processing stages, film profiles, chemistry, spatial effects,
controlled grain/glare, spectral methods, gamut mapping, LUTs, framing and the
four RAW decoders. Test images include small synthetic probes and the cached
camera crops. All comparisons require matching shapes and finite float64 arrays.
Per-value tolerance is `1e-9 + 1e-8 × abs(reference)`; controlled grain-density
cases additionally require identical float64 bit patterns.

[Inverse and combined-optics report](core.json): the same cached Canon crop,
four threads, stochastic effects off. Direct inverse → print → scan uses the
strict gate. Camera + enlarger Black Pro-Mist diffusion uses the existing
`2e-6` absolute gate, reflecting the reference's float32 FFT working buffers.
The separate profiled Rust executor is also checked against fused output.

The accuracy chart groups the worst pointwise `abs(error) / tolerance` across
the tested cases; 100% is the acceptance boundary. The right column shows the
largest absolute error in each group, which may occur in a different case.
The JSON reports also retain RMSE and individual results. Validation timings
are not used in the speed chart. These probes do not replace the separate
calibration, codec, native-library or GUI suites in [VALIDATION.md](../../../VALIDATION.md).

## Reproduce

Use the published archive verified against `SHA256SUMS`, a user-local reference
environment from the pinned requirements, and cached sample crops. The reference
is extracted and hash-checked by the existing tools; the sibling checkout is
read-only. No full-resolution reference simulation is run.

```sh
# Set this to the executable in the unpacked v0.1.2 release.
release_binary="$PWD/artifacts/readme-v0.1.2/release/fast-spektrafilm-0.1.2-linux-x86_64/fast-spektrafilm"
report_dir="$PWD/artifacts/readme-v0.1.2"

for mode in deterministic spatial legacy-grain; do
  .venv/bin/python tools/benchmark.py --binary "$release_binary" \
    --output-dir "$report_dir/benchmark" --mode "$mode" --threads 4 --repeat 3
done
.venv/bin/python tools/validate.py --binary "$release_binary" \
  --output-dir "$report_dir/parity" --all-profiles --samples --decode \
  --spatial --variants --grain --spectral --gamut --lut --framing --crop-size 256
.venv/bin/python tools/benchmark_core.py --binary "$release_binary" \
  --output-dir "$report_dir/core"

# Regenerate the figures from the checked-in measurements.
.venv/bin/python tools/plot_release_report.py \
  --data-dir docs/benchmarks/v0.1.2 --output-dir docs/images/benchmarks/v0.1.2
```

The checked-in benchmark JSON files are snapshots of each mode's `report.json`
with its `config.json` included as `config`; `accuracy.json` and `core.json` are
snapshots of their respective reports, with core configs retained as `configs`.
Arrays and temporary logs stay in ignored
`artifacts/`. Existing dated results in [BENCHMARKS.md](../../../BENCHMARKS.md)
remain historical measurements of their recorded builds.
