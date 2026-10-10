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
- Optional native Slint darkroom: measured stock selection, common adjustments,
  full JSON settings, background rendering, cached full-frame previews, original/
  developed/comparison views and full-resolution PNG, float TIFF and NPY export.

## Numerical contract

No float32 conversion is inserted into the deterministic film/print/scan math.
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

## Remaining core coverage

- Scan inverse / convert-film routes and calibration/base detection workflows.
- Camera/enlarger optical diffusion filters. The experimental enlarger
  `lens_blur` field is accepted for compatibility: its printing stage does not
  use it, so it remains a documented no-op here as well.
- Broader image-file/ICC I/O, lens correction and custom RAW temperature/tint
  controls. The GUI currently opens RAW and RGB NPY inputs; ICC monitor
  calibration and embedded output profiles remain unimplemented.
- Full stochastic validation across shapes, thread counts and all stock classes;
  further workspace reuse and spatial-filter performance work.

Unsupported configuration is rejected rather than silently approximated.
The Slint GUI exposes the supported core routes. Remaining core and application
coverage above is not presented as implemented. See [GUI.md](GUI.md).
