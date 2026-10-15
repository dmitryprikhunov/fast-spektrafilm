"""Build first, then measure individual review changes on the identical cached crop.

All compilation precedes all measurements. Scratch checkouts/binaries are ignored;
this never changes the working checkout or the sibling reference repository.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import numpy as np
from numerical import require
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'artifacts/numerical-review/revisions'
SCRATCH=ROOT/'.tools/review-revision'
# Each row isolates its stage(s); the final cumulative revision is also measured.
REVISIONS=[('correctness','fd3c00b'),('T10','51e570f'),('T11','fe422c3'),('T6','e6988ea'),
           ('T8','de05c40'),('T7','d6921f7'),('T9','7c841dc'),('T12','1b2b5a2'),('final','HEAD')]
MODES=['fir','iir','diffusion','inverse','grain','preview','resize','staged',
       'update-exposure','update-film','update-base','update-chemistry','update-filter',
       'update-gamut','update-lut','update-dimensions','update-seed','update-delivery']


def build():
    OUT.mkdir(parents=True,exist_ok=True); SCRATCH.mkdir(parents=True,exist_ok=True)
    example=(ROOT/'examples/review_performance.rs').read_bytes()
    for label,ref in REVISIONS:
        sha=subprocess.check_output(['git','rev-parse',ref],cwd=ROOT,text=True).strip()
        paths=[] if label=='correctness' else ['src','build.rs','Cargo.toml','Cargo.lock','numerical-contracts.json']
        archive=subprocess.check_output(['git','archive',sha,*paths],cwd=ROOT)
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
            for member in tar.getmembers():
                if not member.isfile(): continue
                target=SCRATCH/member.name
                content=tar.extractfile(member).read()
                if target.exists() and target.read_bytes()==content: continue
                target.parent.mkdir(parents=True,exist_ok=True); target.write_bytes(content)
        (SCRATCH/'examples').mkdir(exist_ok=True)
        (SCRATCH/'examples/review_performance.rs').write_bytes(example)
        env=dict(os.environ,CARGO_PROFILE_RELEASE_LTO='false',CARGO_PROFILE_RELEASE_CODEGEN_UNITS='16',
                 CARGO_TARGET_DIR=str(ROOT/'target'))
        folder=OUT/label;folder.mkdir(exist_ok=True)
        with (folder/'build.log').open('w') as log:
            subprocess.run(['cargo','build','--release','--locked','--example','review_performance','-j4'],
                           cwd=SCRATCH,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        binary=folder/'review_performance';shutil.copy2(ROOT/'target/release/examples/review_performance',binary)
        info=dict(commit=sha,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                  compiler=subprocess.check_output(['rustc','--version'],text=True).strip(),
                  command='cargo build --release --locked --example review_performance -j4',
                  overrides=dict(lto=False,codegen_units=16),
                  cargo_lock_sha256=hashlib.sha256((SCRATCH/'Cargo.lock').read_bytes()).hexdigest())
        (folder/'build.json').write_text(json.dumps(info,indent=2)+'\n')
        print('Built',label,sha,flush=True)


def measure():
    crop=ROOT/'artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy'
    assert crop.exists(),'Decode/cache sample crops with tools/validate.py --samples first'
    records=[]
    for label,_ in REVISIONS:
        folder=OUT/label
        for mode in MODES:
            rss=folder/f'{mode}.rss'
            result=subprocess.run(['/usr/bin/time','-f','%M','-o',str(rss),str(folder/'review_performance'),
                                   str(crop),mode,str(folder)],capture_output=True,text=True,check=True)
            report=json.loads(result.stdout.strip().splitlines()[-1])
            report['revision']=label;report['peak_rss_kib']=int(rss.read_text())
            output=np.load(folder/f'{mode}.npy')
            assert np.isfinite(output).all()
            if label!='correctness':
                policy='optical' if mode in ('diffusion','update-filter') else 'strict'
                report['comparison']=require(output,np.load(OUT/'correctness'/f'{mode}.npy'),
                                             label+'/'+mode,policy=policy,bitwise=mode=='grain')
            (folder/f'{mode}.json').write_text(json.dumps(report,indent=2)+'\n')
            records.append(report);print(label,mode,flush=True)
    from benchmark import provenance
    (OUT/'report.json').write_text(json.dumps(dict(**provenance(),
        input_sha256=hashlib.sha256(crop.read_bytes()).hexdigest(),
        builds={label:json.loads((OUT/label/'build.json').read_text()) for label,_ in REVISIONS},
        note='Instrumented allocator counts and stage timings; not CLI throughput. RSS includes cold setup, caches and warmups.',
        results=records),indent=2)+'\n')


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',action='store_true');p.add_argument('--measure',action='store_true');a=p.parse_args()
    if a.build:build()
    if a.measure:measure()
