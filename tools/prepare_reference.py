"""Materialize the pinned reference locally, without changing its checkout."""
import argparse
import io
import json
from pathlib import Path
import subprocess
import tarfile
from check_baseline import verify

root=Path(__file__).resolve().parents[1]
baseline=json.loads((root/'BASELINE.json').read_text())
parser=argparse.ArgumentParser()
parser.add_argument('--source',type=Path,default=root.parent/'spektrafilm')
args=parser.parse_args()
target=root/'.reference'/baseline['commit']
if not (target/'src/spektrafilm/runtime/pipeline.py').exists():
    archive=subprocess.check_output(['git','-C',str(args.source),'archive',baseline['commit']])
    target.mkdir(parents=True,exist_ok=True)
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:tar.extractall(target,filter='data')
verify(require_reference=True)
print('Pinned reference:',target)
print('Verified',len(baseline['data_files']),'unmodified copied assets')
