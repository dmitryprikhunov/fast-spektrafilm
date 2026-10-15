"""Recheck committed RAW interaction fixtures directly against pinned Python."""
import json
import os
os.environ.update(NUMBA_NUM_THREADS='4',OPENBLAS_NUM_THREADS='4',OMP_NUM_THREADS='4')
import numpy as np
from numerical import require
from review_numerics import reference
from validate import ROOT
folder=ROOT/'tests/fixtures/review'
manifest=json.loads((folder/'manifest.json').read_text())
reports=[]
for case in manifest['cases']:
    directory=folder/case['name']
    sim=reference(json.loads((directory/'config.json').read_text()))
    image=np.load(folder/case['input'])
    for tap in case['taps']:
        if case['name']=='convert_glare':
            import numba
            @numba.njit
            def seed(seed): np.random.seed(seed)
            numba.set_num_threads(1)
            seed((12345+0x6c617265)&0xffffffff)
        try:
            actual=sim.process(image,collect=tap)
        finally:
            if case['name']=='convert_glare': numba.set_num_threads(4)
        expected=np.load(directory/f'{tap}.npy')
        report=dict(case=case['name'],tap=tap,**require(actual,expected,case['name'],
            bitwise=case['name']=='grain_chemistry' and tap=='cmy_film'))
        reports.append(report)
        print(json.dumps(report),flush=True)
output=ROOT/'artifacts/numerical-review/pinned-fixtures.json'
output.parent.mkdir(parents=True,exist_ok=True)
output.write_text(json.dumps(dict(baseline_commit=manifest['baseline_commit'],results=reports),indent=2)+'\n')
