"""Benchmark identical cached crops, including per-process peak RSS.

Reference JIT compilation and pipeline preparation are outside timed runs.
RSS includes process startup and preparation for both implementations.
"""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sys
import time
import platform
import hashlib

ROOT=Path(__file__).resolve().parents[1]

def provenance():
    import importlib.metadata
    import contextlib
    import io
    import numpy as np
    from check_baseline import verify
    verify()
    cpu=next((line.split(':',1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')),platform.processor())
    digest=lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    runtime=io.StringIO()
    with contextlib.redirect_stdout(runtime): np.show_runtime()
    binary=ROOT/'target/release/fast-spektrafilm'
    build=json.loads(subprocess.check_output([str(binary),'build-info'],text=True))
    versions={name: importlib.metadata.version(name) for name in ['numpy','scipy','numba','colour-science','rawpy']}
    sources=sorted((ROOT/'src').rglob('*.rs'))+[ROOT/'build.rs',ROOT/'Cargo.toml',ROOT/'Cargo.lock']
    return dict(baseline_commit=json.loads((ROOT/'BASELINE.json').read_text())['commit'],
        measured_at=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),cpu=cpu,
        system=platform.platform(),rust_version=subprocess.check_output(['rustc','--version'],text=True).strip(),
        python_version=platform.python_version(), dependencies=versions, numpy_runtime=runtime.getvalue(),
        numpy_build=np.show_config(mode='dicts'),
        thread_environment={k:os.environ.get(k) for k in ['OPENBLAS_CORETYPE','OPENBLAS_NUM_THREADS','OMP_NUM_THREADS','NUMBA_NUM_THREADS','NUMBA_CPU_NAME','NUMBA_CPU_FEATURES','RAYON_NUM_THREADS']},
        binary_sha256=digest(binary), binary_build=build, cargo_lock_sha256=digest(ROOT/'Cargo.lock'),
        rust_sources_sha256=hashlib.sha256(''.join(str(p.relative_to(ROOT))+digest(p) for p in sources).encode()).hexdigest(),
        assets_manifest_sha256=digest(ROOT/'BASELINE.json'), reference_manifest_sha256=digest(ROOT/'REFERENCE_MANIFEST.json'),
        numerical_contracts_sha256=digest(ROOT/'numerical-contracts.json'))

def config_for_mode(mode,film_format_mm=35.0):
    config=dict(camera=dict(auto_exposure=False,film_format_mm=film_format_mm),debug=dict(deactivate_spatial_effects=mode=='deterministic',deactivate_stochastic_effects=mode!='legacy-grain'))
    if mode=='legacy-grain':
        config.update(settings=dict(use_fast_stats=False),print_glare=dict(active=False))
    return config

def reference_worker(input_path,output_path,repeats,mode,film_format_mm):
    import numpy as np
    from validate import reference_params
    from spektrafilm.runtime.pipeline import SimulationPipeline
    if mode=='deterministic':
        params=reference_params('kodak_portra_400','kodak_portra_endura')
        params.camera.film_format_mm=film_format_mm
        sim=SimulationPipeline(params)
    else:
        from validate import init_params,digest_params
        params=init_params('kodak_portra_400','kodak_portra_endura')
        params.camera.auto_exposure=False
        params.camera.film_format_mm=film_format_mm
        params.debug.deactivate_stochastic_effects=mode!='legacy-grain'
        if mode=='legacy-grain':
            params.settings.use_fast_stats=False
            params.print_render.glare.active=False
        sim=SimulationPipeline(digest_params(params))
    image=np.load(input_path)
    # Same full cached-crop warmup as the Rust executor.
    start=time.perf_counter()
    sim.process(image)
    warmup=time.perf_counter()-start
    times=[]
    for _ in range(repeats):
        result=None
        start=time.perf_counter()
        result=sim.process(image)
        times.append(time.perf_counter()-start)
    np.save(output_path,result)
    print(json.dumps(dict(process_seconds=times,warmup_seconds=[warmup],executor='reference-staged')))

def timed(command,rss_file,env):
    result=subprocess.run(['/usr/bin/time','-f','%M','-o',str(rss_file),*command],env=env,capture_output=True,text=True,check=True)
    timing=json.loads((result.stdout if result.stdout.strip() else result.stderr).strip().splitlines()[-1])
    timing['peak_rss_kib']=int(rss_file.read_text().strip())
    return timing

def main(args):
    if args.worker:
        reference_worker(args.input,args.output,args.repeat,args.mode,args.film_format_mm)
        return
    import numpy as np
    folder=ROOT/'artifacts/benchmark'/(args.mode if args.film_format_mm==35.0 else f'{args.mode}-{args.film_format_mm:g}mm')
    folder.mkdir(parents=True,exist_ok=True)
    config=folder/'config.json'
    config.write_text(json.dumps(config_for_mode(args.mode,args.film_format_mm)))
    env=os.environ.copy()
    env.update(NUMBA_NUM_THREADS=str(args.threads),OPENBLAS_NUM_THREADS=str(args.threads),OMP_NUM_THREADS=str(args.threads))
    results=[]
    for crop in sorted((ROOT/'artifacts/crops').glob('RAW*_256.npy')):
        reference=timed([sys.executable,str(Path(__file__).resolve()),'--worker','--mode',args.mode,'--film-format-mm',str(args.film_format_mm),'--input',str(crop),'--output',str(folder/'reference.npy'),'--repeat',str(args.repeat)],folder/'reference.rss',env)
        rust=timed([str(ROOT/'target/release/fast-spektrafilm'),'--threads',str(args.threads),'process',str(crop),str(folder/'rust.npy'),'--config',str(config),'--repeat',str(args.repeat),'--warmup','1'],folder/'rust.rss',env)
        actual=np.load(folder/'rust.npy')
        expected=np.load(folder/'reference.npy')
        from numerical import require
        comparison=require(actual,expected,crop.name)
        ref_time=float(np.median(reference['process_seconds']))
        rust_time=float(np.median(rust['process_seconds']))
        record=dict(crop=crop.name,input_sha256=hashlib.sha256(crop.read_bytes()).hexdigest(),config_sha256=hashlib.sha256(config.read_bytes()).hexdigest(),comparison=comparison,shape=list(actual.shape),reference=reference,rust=rust,speedup=ref_time/rust_time,rss_reduction=reference['peak_rss_kib']/rust['peak_rss_kib'],max_abs=float(np.max(np.abs(actual-expected))))
        results.append(record)
        print(json.dumps(record),flush=True)
    assert results,'Run validate.py --samples first to cache sample crops'
    (folder/'report.json').write_text(json.dumps(dict(**provenance(),threads=args.threads,repeats=args.repeat,spatial=args.mode!='deterministic',stochastic=args.mode=='legacy-grain',auto_exposure=False,mode=args.mode,film_format_mm=args.film_format_mm,results=results),indent=2)+'\n')

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--worker',action='store_true')
    parser.add_argument('--mode',choices=['deterministic','spatial','legacy-grain'],default='deterministic')
    parser.add_argument('--input',type=Path)
    parser.add_argument('--output',type=Path)
    parser.add_argument('--repeat',type=int,default=3)
    parser.add_argument('--threads',type=int,default=4)
    parser.add_argument('--film-format-mm',type=float,default=35.0,help='Physical long side represented by the cached crop; reduce this to retain fine physical pixel spacing')
    main(parser.parse_args())
