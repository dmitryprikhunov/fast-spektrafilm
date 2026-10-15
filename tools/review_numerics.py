"""Reproduce the 2026-10 numerical review; uses cached RAW crops, never full RAW.

This is diagnostic reference tooling, not a processing implementation. Known
failures are recorded rather than asserted so every probe completes.
"""
import dataclasses
import hashlib
import json
import os
import subprocess
import sys

os.environ.update(NUMBA_NUM_THREADS="4", OPENBLAS_NUM_THREADS="4", OMP_NUM_THREADS="4")
import numpy as np
from validate import ROOT, init_params, digest_params, SimulationPipeline
from benchmark import provenance

OUT = ROOT / "artifacts/numerical-review/probes"
OUT.mkdir(parents=True, exist_ok=True)
SOURCE = ROOT / "artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy"
CLI = ROOT / "target/release/fast-spektrafilm"


def merge(a, b):
    for key, value in b.items():
        if isinstance(value, dict):
            merge(a.setdefault(key, {}), value)
        else:
            a[key] = value
    return a


def update(obj, values):
    changes = {k: update(getattr(obj, k), v) if isinstance(v, dict) else v
               for k, v in values.items()}
    if dataclasses.is_dataclass(obj) and obj.__dataclass_params__.frozen:
        return dataclasses.replace(obj, **changes)
    for k, v in changes.items():
        setattr(obj, k, v)
    return obj


def reference(config):
    p = init_params(config.get("film_profile", "kodak_portra_400"),
                    config.get("print_profile", "kodak_portra_endura"))
    for group, values in config.items():
        if group.endswith("_profile"):
            continue
        parent = p
        name = group
        if group in ("halation", "grain", "dir_couplers", "convert"):
            parent = p.film_render
        elif group in ("film_chemistry", "film_base"):
            parent, name = p.film_render, group.split('_', 1)[1]
        elif group in ("print_chemistry", "print_base"):
            parent, name = p.print_render, group.split('_', 1)[1]
        elif group == "print_glare":
            parent, name = p.print_render, "glare"
        if group == "convert":
            values = dict(values)
            if "calibration" in values:
                values["calibration"] = " ".join(str(x) for row in values["calibration"] for x in row)
        setattr(parent, name, update(getattr(parent, name), values))
    return SimulationPipeline(digest_params(p))


from numerical import metrics


def rust(folder, tap, suffix="rust", profiled=False):
    output = folder / f"{suffix}_{tap}.npy"
    command = [str(CLI), "--threads", "4", "process", str(folder / "input.npy"), str(output),
               "--config", str(folder / "config.json"), "--collect", tap.replace("_", "-")]
    if profiled:
        command.append("--stage-timings")
    result = subprocess.run(command, text=True, capture_output=True)
    if result.returncode:
        return None, dict(exit_code=result.returncode, error=result.stderr[-1500:])
    return np.load(output), json.loads(result.stderr.strip().splitlines()[-1])


def main():
    crop = np.load(SOURCE)
    image = crop[96:160, 96:160].copy()
    base = dict(camera=dict(auto_exposure=False, film_format_mm=0.375),
                debug=dict(deactivate_stochastic_effects=True), io=dict(copy_metadata=False),
                halation=dict(active=False))
    cases = []
    for sigma in (1.0, 3.0):
        for mode in ("blur", "diffusion", "both"):
            cfg = merge(json.loads(json.dumps(base)), dict(camera=dict(
                lens_blur_um=sigma * 5.859375 if mode != "diffusion" else 0.,
                diffusion_filter=dict(active=mode != "blur", strength=1.0))))
            cases.append((f"camera_{mode}_sigma{sigma:g}", image, cfg, ["log_e_film", "rgb_out"]))
    inverse = dict(camera=dict(auto_exposure=False), io=dict(copy_metadata=False),
                   debug=dict(deactivate_spatial_effects=True, deactivate_stochastic_effects=True),
                   workflow=dict(route="input > convert-film > print > scan"))
    matrix = [[1.07, -0.03, 0.012], [0.017, 0.94, -0.021], [-0.011, 0.025, 1.04]]
    for lut in (False, True):
        cfg = merge(json.loads(json.dumps(inverse)), dict(convert=dict(calibration=matrix),
                    settings=dict(use_convert_lut=lut, convert_lut_resolution=9)))
        cases.append((f"inverse_calibrated_lut{int(lut)}", image, cfg, ["cmy_film", "rgb_out"]))
    for resolution in (9, 17, 32):
        cfg = merge(json.loads(json.dumps(inverse)), dict(settings=dict(
            use_convert_lut=True, convert_lut_resolution=resolution)))
        cases.append((f"inverse_identity_lut{resolution}", image, cfg, ["cmy_film", "rgb_out"]))
    records = []
    for name, data, cfg, taps in cases:
        folder = OUT / name
        folder.mkdir(exist_ok=True)
        np.save(folder / "input.npy", data)
        (folder / "config.json").write_text(json.dumps(cfg, indent=2) + "\n")
        sim = reference(cfg)
        for tap in taps:
            expected = sim.process(data, collect=tap)
            np.save(folder / f"reference_{tap}.npy", expected)
            actual, timing = rust(folder, tap)
            record = dict(case=name, tap=tap, timing=timing,
                          comparison=metrics(actual, expected) if actual is not None else None)
            if name.startswith("camera_both") and tap == "log_e_film":
                # Causal diagnostic only: swap calls in memory, never edit reference
                # files and never use this altered order as an expected fixture.
                import spektrafilm.runtime.stages.filming as filming
                blur, diffusion = filming.apply_gaussian_blur_um, filming.apply_diffusion_filter_um
                try:
                    filming.apply_gaussian_blur_um = lambda raw, *_: raw
                    filming.apply_diffusion_filter_um = lambda raw, filt, pixel_size_um: diffusion(
                        blur(raw, cfg["camera"]["lens_blur_um"], pixel_size_um), filt, pixel_size_um)
                    swapped = sim.process(data, collect=tap)
                    record["diagnostic_python_with_rust_order"] = metrics(actual, swapped)
                finally:
                    filming.apply_gaussian_blur_um, filming.apply_diffusion_filter_um = blur, diffusion
            records.append(record)
            print(json.dumps(record), flush=True)
    # Unseeded conversion should vary glare between calls, as forward processing does.
    cfg = merge(json.loads(json.dumps(inverse)), dict(debug=dict(deactivate_stochastic_effects=False),
                grain=dict(active=False), print_glare=dict(active=True, percent=10., roughness=1.)))
    folder = OUT / "unseeded_convert_glare"
    folder.mkdir(exist_ok=True)
    np.save(folder / "input.npy", image)
    (folder / "config.json").write_text(json.dumps(cfg, indent=2) + "\n")
    a, _ = rust(folder, "rgb_out", "first")
    b, _ = rust(folder, "rgb_out", "second")
    c, _ = rust(folder, "rgb_out", "profiled", True)
    sim = reference(cfg)
    r1, r2 = sim.process(image), sim.process(image)
    records.append(dict(case="unseeded_convert_glare", rust_repeated=metrics(a, b),
                        rust_profiled_difference=metrics(a, c), reference_repeated=metrics(r1, r2)))
    report = dict(**provenance(), source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
                  crop_slice="[96:160,96:160] from cached 256x256 Canon crop", results=records)
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(records[-1]), flush=True)


def inverse_grid():
    folder = OUT / "inverse_grid"
    folder.mkdir(exist_ok=True)
    sim = reference(dict(camera=dict(auto_exposure=False),
                         debug=dict(deactivate_spatial_effects=True, deactivate_stochastic_effects=True),
                         workflow=dict(route="input > convert-film > scan")))
    converter = sim._converting_stage._converter
    lo, hi = converter.gamut_bounds()
    grid = np.stack(np.meshgrid(*(np.linspace(a, b, 9) for a, b in zip(lo, hi)), indexing="ij"), axis=-1)
    grid = grid.reshape(81, 9, 3)
    expected = converter.invert(grid)
    np.save(folder / "grid.npy", grid)
    np.save(folder / "reference.npy", expected)
    env = dict(os.environ, RAYON_NUM_THREADS="4")
    result = subprocess.run([str(ROOT / "target/release/examples/review_inverse"),
                             str(folder / "grid.npy"), str(folder)], env=env,
                            text=True, capture_output=True, check=True)
    bounds = json.loads(result.stdout)
    record = dict(reference_lo=lo.tolist(), reference_hi=hi.tolist(), rust_bounds=bounds,
                  bounds_max_abs=float(max(abs(lo-bounds["lo"]).max(), abs(hi-bounds["hi"]).max())),
                  batch=metrics(np.load(folder / "batch.npy"), expected),
                  scalar=metrics(np.load(folder / "scalar.npy"), expected))
    (folder / "report.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record))


if __name__ == "__main__":
    if sys.argv[1:] == ["--inverse-grid"]:
        inverse_grid()
    else:
        main()
