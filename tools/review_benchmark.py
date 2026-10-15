"""Isolated cached-crop series. Build/test before running; RSS is per process.

Each executor warms on the same full cached crop before measured repetitions.
Port error and the inverse LUT's approximation of direct integration are separate.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
os.environ.update(NUMBA_NUM_THREADS='4',OPENBLAS_NUM_THREADS='4',OMP_NUM_THREADS='4')
import numpy as np
from benchmark import ROOT, provenance
from numerical import require, metrics
SOURCE=ROOT/'artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy'
OUT=ROOT/'artifacts/numerical-review/benchmark'


def config(mode):
    p=dict(camera=dict(auto_exposure=False,film_format_mm=35.),io=dict(copy_metadata=False),
           debug=dict(deactivate_spatial_effects=mode in ('forward','inverse','inverse-lut'),
                      deactivate_stochastic_effects=mode!='legacy-fine'))
    if mode.endswith('fine'): p['camera']['film_format_mm']=1.5
    if mode.startswith('diffusion'):
        p['camera']['diffusion_filter']=dict(active=True)
        p['enlarger']=dict(diffusion_filter=dict(active=True))
    if mode=='legacy-fine':
        p['settings']=dict(use_fast_stats=False)
        p['print_glare']=dict(active=False)
    if mode.startswith('inverse'):
        p['workflow']=dict(route='input > convert-film > print > scan')
    if mode=='inverse-lut': p['settings']=dict(use_convert_lut=True,convert_lut_resolution=32)
    return p


def timed(command,folder,name):
    result=subprocess.run(['/usr/bin/time','-f','%M','-o',str(folder/f'{name}.rss'),*command],
                          capture_output=True,text=True,check=True)
    values=json.loads((result.stdout if result.stdout.strip() else result.stderr).strip().splitlines()[-1])
    values['peak_rss_kib']=int((folder/f'{name}.rss').read_text())
    return values


def spread(samples):
    return dict(median=float(np.median(samples)),minimum=min(samples),maximum=max(samples))


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--worker')
    parser.add_argument('--input',type=Path,default=SOURCE)
    parser.add_argument('--folder',type=Path,default=OUT)
    parser.add_argument('--sizes',type=int,nargs='+',default=[64,128,256])
    parser.add_argument('--warmup',type=int,default=1)
    parser.add_argument('--repeat',type=int,default=3)
    args=parser.parse_args()
    assert args.warmup>=1 and args.repeat>=3
    if args.worker:
        from review_numerics import reference
        image=np.load(args.input)
        assert max(image.shape[:2])<=256,'Reference benchmarks are crop-only'
        start=time.perf_counter(); sim=reference(config(args.worker)); setup=time.perf_counter()-start
        warmups=[]; times=[]
        for i in range(args.warmup+args.repeat):
            start=time.perf_counter(); output=sim.process(image); seconds=time.perf_counter()-start
            (warmups if i<args.warmup else times).append(seconds)
        assert np.isfinite(output).all()
        np.save(args.folder/'reference.npy',output)
        print(json.dumps(dict(setup_seconds=setup,warmup_seconds=warmups,process_seconds=times,
                              executor='reference-staged',stage_seconds=sim.get_timings())))
        return
    records=[]
    source=np.load(args.input)
    for size in args.sizes:
        assert 0<size<=min(source.shape[:2]) and size<=256
        for mode in ('forward','spatial-fine','legacy-fine','diffusion','diffusion-fine','inverse','inverse-lut'):
            folder=args.folder/f'{mode}-{size}'; folder.mkdir(parents=True,exist_ok=True)
            y=(source.shape[0]-size)//2; x=(source.shape[1]-size)//2
            crop=folder/'input.npy'; np.save(crop,source[y:y+size,x:x+size])
            cfg=folder/'config.json'; cfg.write_text(json.dumps(config(mode),indent=2)+'\n')
            ref=timed([sys.executable,str(Path(__file__).resolve()),'--worker',mode,'--input',str(crop),
                       '--folder',str(folder),'--warmup',str(args.warmup),'--repeat',str(args.repeat)],folder,'reference')
            command=[str(ROOT/'target/release/fast-spektrafilm'),'--threads','4','process',str(crop),
                     str(folder/'rust.npy'),'--config',str(cfg),'--warmup',str(args.warmup),'--repeat',str(args.repeat)]
            rust=timed(command,folder,'rust')
            a,b=np.load(folder/'rust.npy'),np.load(folder/'reference.npy')
            comparison=require(a,b,mode,policy='optical' if mode.startswith('diffusion') else 'strict')
            command[5]=str(folder/'profiled.npy')
            profiled=timed(command+['--stage-timings'],folder,'profiled')
            same=require(np.load(folder/'profiled.npy'),a,mode+' profiled')
            record=dict(mode=mode,shape=list(a.shape),config=config(mode),
                        config_sha256=hashlib.sha256(cfg.read_bytes()).hexdigest(),
                        input_sha256=hashlib.sha256(crop.read_bytes()).hexdigest(),reference=ref,rust=rust,
                        profiled=profiled,port_error=comparison,profiled_comparison=same,
                        reference_seconds=spread(ref['process_seconds']),rust_seconds=spread(rust['process_seconds']))
            if mode=='inverse-lut':
                direct=args.folder/f'inverse-{size}'
                record['lut_approximation_vs_direct']=dict(
                    reference=metrics(b,np.load(direct/'reference.npy')),
                    rust=metrics(a,np.load(direct/'rust.npy')))
            records.append(record); print(json.dumps(record),flush=True)
    report=dict(**provenance(),threads=4,warmups=args.warmup,measured_repeats=args.repeat,
                source_sha256=hashlib.sha256(args.input.read_bytes()).hexdigest(),results=records)
    (args.folder/'report.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__': main()
