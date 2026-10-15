"""Extract compact interaction fixtures from the pinned reference and cached RAW.

No simulation is run on a full RAW. Re-run validate.py --samples first to cache.
"""
import hashlib
import io
import json
from pathlib import Path
import os
os.environ.update(NUMBA_NUM_THREADS='4', OPENBLAS_NUM_THREADS='4', OMP_NUM_THREADS='4')
import numpy as np
from review_numerics import reference, merge, SOURCE
from validate import ROOT, BASELINE, TAPS

OUT = ROOT / 'tests/fixtures/review'
OUT.mkdir(parents=True, exist_ok=True)
crop = np.load(SOURCE)
records = []
inputs = {}
for item in json.loads((ROOT/'tests/fixtures/raw_crops/manifest.json').read_text())['crops']:
    inputs[item['sha256']] = '../raw_crops/' + item['path']
base = dict(camera=dict(auto_exposure=False, film_format_mm=0.111328125),
            debug=dict(deactivate_stochastic_effects=True), io=dict(copy_metadata=False),
            settings=dict(random_seed=12345))


def fixture_input(name, image):
    buffer = io.BytesIO()
    np.save(buffer, image)
    payload = buffer.getvalue()
    digest = hashlib.sha256(payload).hexdigest()
    if digest not in inputs:
        path = OUT/'inputs'/f'{name}.npy'
        path.parent.mkdir(exist_ok=True)
        path.write_bytes(payload)
        inputs[digest] = str(path.relative_to(OUT))
    return inputs[digest]


def case(name, config, image, policy='strict', taps=TAPS, glare=False):
    folder = OUT / name
    folder.mkdir(exist_ok=True)
    cfg = merge(json.loads(json.dumps(base)), config)
    (folder / 'config.json').write_text(json.dumps(cfg, indent=2) + '\n')
    input_path = fixture_input(name, image)
    sim = reference(cfg)
    for tap in taps:
        if glare:
            import numba
            @numba.njit
            def seed_numba(seed):
                np.random.seed(seed)
            numba.set_num_threads(1)
            seed_numba((12345 + 0x6c617265) & 0xffffffff)
        try:
            np.save(folder / f'{tap}.npy', sim.process(image, collect=tap))
        finally:
            if glare:
                numba.set_num_threads(4)
    records.append(dict(name=name, input=input_path, policy=policy, taps=taps))
    print(name, flush=True)


stress = crop[96:160,96:160].copy() if crop.shape[0] > 64 else crop.copy()
small = stress[23:40,22:41].copy()
fixture_input('canon_17x19', small)
bright_corner = small.copy()
bright_corner[0, 0] *= 8
fixture_input('canon_17x19_bright_corner', bright_corner)
for i, family in enumerate(('black_pro_mist', 'pro_mist', 'glimmerglass', 'cinebloom')):
    for j, sigma in enumerate((2.999999, 3., 3.000001)):
        stock = ('kodak_portra_400', 'kodak_doublex', 'fujifilm_provia_100f')[(i+j) % 3]
        image = small.copy()
        image[0, 0] *= 8
        case(f'optical_{i}_{j}', dict(film_profile=stock, halation=dict(active=False),
             camera=dict(lens_blur_um=sigma*5.859375, diffusion_filter=dict(active=True,
                 filter_family=family, strength=1., halo_warmth=0.2 if j == 1 else 0.))), image, 'optical')
for sigma in (0., 1., 3.):
    case(f'blur_{sigma:g}', dict(camera=dict(lens_blur_um=sigma*5.859375),
         halation=dict(active=False)), small)
case('optical_stress64', dict(camera=dict(film_format_mm=.375, lens_blur_um=17.578125,
     diffusion_filter=dict(active=True, strength=1.)), halation=dict(active=False)),
     stress, 'optical')
inverse = dict(workflow=dict(route='input > convert-film > print > scan'),
               debug=dict(deactivate_spatial_effects=True))
matrix = [[1.07,-.03,.012],[.017,.94,-.021],[-.011,.025,1.04]]
for resolution in (8, 9, 17, 32):
    cfg = merge(json.loads(json.dumps(inverse)), dict(settings=dict(use_convert_lut=True,
        convert_lut_resolution=resolution), convert=dict(calibration=matrix)))
    case(f'inverse_lut{resolution}', cfg, small, taps=['cmy_film','rgb_out'])
    sim = reference(merge(json.loads(json.dumps(base)), inverse))
    converter = sim._converting_stage._converter
    lo, hi = converter.gamut_bounds()
    grid = np.stack(np.meshgrid(*(np.linspace(a,b,resolution) for a,b in zip(lo,hi)),
                    indexing='ij'), axis=-1).reshape(resolution**2,resolution,3)
    case(f'inverse_nodes{resolution}', merge(json.loads(json.dumps(inverse)),
         dict(settings=dict(use_convert_lut=True, convert_lut_resolution=resolution))),
         grid, taps=['cmy_film'])
for n in (1,2,3,4):
    edge = np.array([[0.,1e-12,-1e-5],[.08125,.018,.0031308],[1.5,.2,.005],[.1,.5,2.]])[:n].reshape(1,n,3)
    case(f'inverse_batch{n}', merge(json.loads(json.dumps(inverse)),
        dict(convert=dict(calibration=matrix))), edge, taps=['cmy_film','rgb_out'])
case('inverse_base_lamp', merge(json.loads(json.dumps(inverse)), dict(
    convert=dict(scan_illuminant='D65',calibration=matrix), film_base=dict(scale=.85,tilt=.1))),
    small, taps=['cmy_film','rgb_out'])
case('convert_glare', merge(json.loads(json.dumps(inverse)), dict(
     debug=dict(deactivate_stochastic_effects=False), grain=dict(active=False),
     print_glare=dict(active=True, percent=10.,roughness=1.))), small, taps=['cmy_film','rgb_out'],glare=True)
case('grain_chemistry', dict(debug=dict(deactivate_stochastic_effects=False),
     settings=dict(use_fast_stats=False), print_glare=dict(active=False),
     film_chemistry=dict(gamma_factor=1.08,developer_exhaustion=.1)), small)
case('scanner_base_lut', dict(debug=dict(deactivate_spatial_effects=True),
     film_base=dict(scale=.9), print_base=dict(scale=.8),
     scanner=dict(black_correction=True,white_correction=True,black_level=.02,white_level=.95),
     settings=dict(use_enlarger_lut=True,use_scanner_lut=True,lut_resolution=9)), small)
case('framing_spacing', dict(io=dict(crop=True,crop_size=[.6,.5],upscale_factor=1.5)),small)
for axis in (0,1):
    image=small[:1,:9] if axis==0 else small[:9,:1]
    case(f'framing_axis{axis}', dict(io=dict(upscale_factor=1.7)), image)
manifest = dict(baseline_commit=BASELINE['commit'], source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
                source='cached Canon 256x256 center crop', cases=records)
# Filter-only sweeps make scheduling and quantization regressions localizable.
from spektrafilm.utils.fast_gaussian_filter import fast_gaussian_filter
filters = OUT / 'filters'
filters.mkdir(exist_ok=True)
filter_input = fixture_input('filters', small)
filter_cases=[]
for sigma in (0., .1666666, .1666667, .4999999, .5, 1., 2.999999, 3., 3.000001, 25.):
    for dtype in (np.float64, np.float32):
        image=small.astype(dtype)
        output=fast_gaussian_filter(image, sigma)
        name=f'{sigma:g}_{np.dtype(dtype).name}_{len(filter_cases)}'
        np.save(filters/f'{name}.npy', output.astype(np.float64))
        filter_cases.append(dict(name=name,sigma=sigma,float32=dtype==np.float32))
(filters/'manifest.json').write_text(json.dumps(dict(input=filter_input, cases=filter_cases),indent=2)+'\n')

# Include every compact fixture in the final durable manifest.
manifest['files'] = {str(p.relative_to(OUT)): hashlib.sha256(p.read_bytes()).hexdigest()
                     for p in sorted(OUT.rglob('*')) if p.is_file() and p != OUT/'manifest.json'}
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
