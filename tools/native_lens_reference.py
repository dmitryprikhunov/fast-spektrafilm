"""Extract the verified pinned Lensfun reference on the current native backend.

Lensfun SIMD reciprocal/sqrt approximations vary across CPUs. The checked-in
fixture remains the local golden; CI also needs a same-runner reference output.
Only pinned Python function nodes are executed; no Rust output generates goldens.
"""
import argparse
import ast
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
manifest = json.loads((ROOT/'REFERENCE_MANIFEST.json').read_text())
module = 'src/spektrafilm/utils/raw_file_processor.py'
url = f"https://raw.githubusercontent.com/andreavolpato/spektrafilm/{manifest['baseline_commit']}/{module}"
local = ROOT/'.reference'/manifest['baseline_commit']/module
source = local.read_bytes() if local.exists() else urlopen(url, timeout=60).read()
assert hashlib.sha256(source).hexdigest() == manifest['reference_python'][module], 'Pinned lens source changed'

import lensfunpy
import numpy as np
from scipy.ndimage import map_coordinates
names = {'ExifData', '_normalize_lens_text', '_compact_lens_text', '_lens_model_score',
         '_lens_matches_focal', '_lens_matches_aperture', '_find_lens_candidates',
         '_select_lens_candidate', '_apply_lens_correction'}
tree = ast.parse(source)
body = [ast.ImportFrom(module='__future__',names=[ast.alias(name='annotations')],level=0)]
body += [node for node in tree.body if isinstance(node,(ast.ClassDef,ast.FunctionDef)) and node.name in names]
assert {node.name for node in body[1:]} == names
namespace = dict(dataclass=dataclass,lensfunpy=lensfunpy,np=np,map_coordinates=map_coordinates)
exec(compile(ast.fix_missing_locations(ast.Module(body=body,type_ignores=[])),url,'exec'),namespace)
image = np.load(ROOT/'tests/fixtures/core/lens_input.npy').astype(np.float32)
metadata = namespace['ExifData']('Canon','Canon EOS 5D Mark III','Canon','Canon EF 24-105mm f/4L IS USM',24.,4.)
result, matched = namespace['_apply_lens_correction'](image,metadata)
assert matched and result.shape == image.shape and np.isfinite(result).all()
args.output.parent.mkdir(parents=True,exist_ok=True)
np.save(args.output,result.astype(np.float64))
static = np.load(ROOT/'tests/fixtures/core/lens_output.npy')
print(json.dumps(dict(baseline=manifest['baseline_commit'],source_sha256=hashlib.sha256(source).hexdigest(),matched=matched,shape=list(result.shape),static_fixture_max_abs=float(np.abs(result-static).max()),output_sha256=hashlib.sha256(args.output.read_bytes()).hexdigest())))
