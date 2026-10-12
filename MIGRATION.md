# Migration status

Baseline: experimental commit `28bf883e1672e884307edc75852549376e13644e`.
The original checkout remains untouched. This file records coverage rather than
claiming that the entire original application has been rewritten.

## Implemented

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
  Test crops avoid full-frame floating-point conversion and simulation.

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

## Coverage boundaries

The remaining named processing features above are implemented. The new
reference fixtures in `tests/fixtures/core` cover both diffusion placements,
all inverse routes and inverse LUTs, base fitting, RAW white balance, blackbody
lamps, Lensfun and image I/O. Controlled fast grain now also covers negative,
reversal and BW stocks, single-row/single-column/rectangular images and 1/2/4
threads, with exact density matches.

Inverse scanning preserves the original float32, eight-step bounded solver;
FFT roundoff and linear-algebra reduction order differ between libraries.
These paths have explicit measured tolerances rather than claiming the
float64 contract. See `VALIDATION.md` and `BENCHMARKS.md`.

Lens correction requires Lensfun 0.3 and its database. No matching lens leaves
the image unchanged, as in the original; an unavailable requested library is
an error. The original inverse converter requires three dye channels; BW
conversion is rejected. The experimental enlarger `lens_blur` field remains a
no-op because the pinned printing stage does not use it.

The Slint review and integration are recorded in `GUI_REVIEW.md`. Native controls,
full-resolution Scan, preview scheduling/caching, monitor ICC, metadata carryover
and startup settings are implemented. Napari layer tools, decorative Polaroid/
paper-back presentation and the Qt GUI-state file schema are not reproduced;
see `GUI.md`. These are application differences, not processing-model omissions.
Arbitrary Numba thread-local random patterns and exhaustive performance
optimization are not feature-parity claims. Unsupported processing configuration is rejected; the upstream enlarger blur compatibility field remains the documented exception.
