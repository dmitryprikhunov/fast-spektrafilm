"""Isolated, identical-setting crop benchmarks for the numerical review.

Run after compilation and validation finish. Peak RSS includes setup and warmup.
Each processor warms on the same full *cached crop*, then renders three times.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess
import sys
import time

os.environ.update(NUMBA_NUM_THREADS="4", OPENBLAS_NUM_THREADS="4", OMP_NUM_THREADS="4")
import numpy as np
from benchmark import ROOT, provenance

SOURCE = ROOT / "artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy"
OUT = ROOT / "artifacts/numerical-review/benchmark"


def config(mode):
    p = dict(camera=dict(auto_exposure=False, film_format_mm=35.), io=dict(copy_metadata=False),
             debug=dict(deactivate_spatial_effects=mode in ("forward", "inverse"),
                        deactivate_stochastic_effects=mode != "legacy-fine"))
    if mode.endswith("fine"):
        p["camera"]["film_format_mm"] = 1.5
    if mode.startswith("diffusion"):
        p["camera"]["diffusion_filter"] = dict(active=True)
        p["enlarger"] = dict(diffusion_filter=dict(active=True))
    if mode == "legacy-fine":
        p["settings"] = dict(use_fast_stats=False)
        p["print_glare"] = dict(active=False)
    if mode == "inverse":
        p["workflow"] = dict(route="input > convert-film > print > scan")
    return p


def timed(command, folder, name):
    result = subprocess.run(["/usr/bin/time", "-f", "%M", "-o", str(folder / f"{name}.rss"),
                             *command], capture_output=True, text=True, check=True)
    values = json.loads((result.stdout if result.stdout.strip() else result.stderr).strip().splitlines()[-1])
    values["peak_rss_kib"] = int((folder / f"{name}.rss").read_text())
    return values


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--worker")
    args = parser.parse_args()
    if args.worker:
        from review_numerics import reference
        folder = OUT / args.worker
        image = np.load(SOURCE)
        start = time.perf_counter()
        sim = reference(config(args.worker))
        setup = time.perf_counter() - start
        times = []
        for _ in range(4):
            start = time.perf_counter()
            output = sim.process(image)
            times.append(time.perf_counter() - start)
        np.save(folder / "reference.npy", output)
        print(json.dumps(dict(setup_seconds=setup, process_seconds=times, stage_seconds=sim.get_timings())))
        return
    from review_numerics import metrics
    records = []
    for mode in ("forward", "spatial-fine", "legacy-fine", "diffusion", "diffusion-fine", "inverse"):
        folder = OUT / mode
        folder.mkdir(parents=True, exist_ok=True)
        cfg = folder / "config.json"
        cfg.write_text(json.dumps(config(mode), indent=2) + "\n")
        ref = timed([sys.executable, str(Path(__file__).resolve()), "--worker", mode], folder, "reference")
        command = [str(ROOT / "target/release/fast-spektrafilm"), "--threads", "4", "process",
                   str(SOURCE), str(folder / "rust.npy"), "--config", str(cfg)]
        rust = timed(command + ["--repeat", "4"], folder, "rust")
        a, b = np.load(folder / "rust.npy"), np.load(folder / "reference.npy")
        comparison = metrics(a, b)
        assert comparison["valid"]
        tolerance = 2e-6 if mode.startswith("diffusion") else 2e-5 if mode == "inverse" else None
        if tolerance is None:
            assert comparison["strict_failures"] == 0, comparison
        else:
            assert comparison["max_abs"] <= tolerance, comparison
        command[5] = str(folder / "profiled.npy")
        profiled = timed(command + ["--stage-timings"], folder, "profiled")
        same = metrics(np.load(folder / "profiled.npy"), a)
        assert same["valid"] and same["strict_failures"] == 0, same
        record = dict(mode=mode, shape=list(a.shape), config=config(mode), reference=ref,
                      rust=rust, profiled=profiled, comparison=comparison, profiled_comparison=same,
                      reference_median_s=float(np.median(ref["process_seconds"][1:])),
                      rust_median_s=float(np.median(rust["process_seconds"][1:])))
        records.append(record)
        print(json.dumps(record), flush=True)
    report = dict(**provenance(), threads=4, warmups=1, measured_repeats=3,
                  input_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(), results=records)
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
