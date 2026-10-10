"""Generate small, checked-in golden arrays directly from the pinned reference."""
from pathlib import Path
import json
import os
os.environ.update(NUMBA_NUM_THREADS='4',OPENBLAS_NUM_THREADS='4',OMP_NUM_THREADS='4')
import numpy as np
from validate import ROOT,BASELINE,init_params,digest_params,SimulationPipeline,TAPS

folder=ROOT/'tests/fixtures'
folder.mkdir(parents=True,exist_ok=True)
random_vectors=[]
for seed in [0,5,12345]:
    for method in ['uniform','normal','poisson','binomial']:
        state=np.random.RandomState(seed)
        values=[]
        args=[]
        for i in range(256):
            if method=='uniform': value=state.random_sample(); arg=[]
            elif method=='normal': value=state.standard_normal(); arg=[]
            elif method=='poisson':
                arg=[[0.1,1.0,10.0,30.0,100.0,1e6][i%6]];value=state.poisson(*arg)
            else:
                arg=[[1,10,100,1000,100000][i%5],[0.001,0.1,0.4,0.7,0.98][i%5]];value=state.binomial(*arg)
            args.append(arg);values.append(float(value))
        random_vectors.append(dict(seed=seed,method=method,args=args,values=values))
import numba
from spektrafilm.utils.fast_stats import fast_poisson,fast_binomial
@numba.njit
def seed_numba(seed):np.random.seed(seed)
numba.set_num_threads(1)
for seed in [0,5,12345]:
    for method in ['fast_poisson','fast_binomial']:
        seed_numba(seed)
        if method=='fast_poisson':
            lam=np.tile(np.array([0.1,1.0,10.0,30.0,100.0,1e6],np.float32),64)
            values=fast_poisson(lam).astype(float).tolist();args=[[float(v)] for v in lam]
        else:
            n=np.tile(np.array([1,10,100,1000,100000,10000,100000],np.int32),64)
            p=np.tile(np.array([0.001,0.1,0.4,0.7,0.98,0.99999,0.999999],np.float32),64)
            values=fast_binomial(n,p).astype(float).tolist();args=[[int(a),float(b)] for a,b in zip(n,p)]
        random_vectors.append(dict(seed=seed,method=method,args=args,values=values))
numba.set_num_threads(4)
(folder/'random.json').write_text(json.dumps(random_vectors,separators=(',',':'))+'\n')
rng=np.random.default_rng(8321)
cases=[
    ('portra_hdr','kodak_portra_400',False,False),
    ('bw','kodak_doublex',False,False),
    ('spatial_reversal','fujifilm_provia_100f',True,True),
    ('matrix_meter','kodak_portra_400',False,True),
    ('grain','kodak_portra_400',True,False),
    ('scanner_print','kodak_portra_400',False,False),
    ('scanner_reversal','fujifilm_provia_100f',False,False),
    ('spectral_surface','kodak_portra_400',False,False),
    ('oklrab','kodak_portra_400',False,False),
    ('jzazbz','kodak_portra_400',False,False),
    ('cam16ucs','kodak_portra_400',False,False),
    ('density_luts','kodak_portra_400',False,False),
    ('bw_density_luts','kodak_doublex',False,False),
    ('fast_grain','kodak_portra_400',True,False),
    ('random_glare','kodak_portra_400',True,False),
    ('fast_grain_single_pixel','kodak_portra_400',True,False),
    ('framing_upscale','kodak_portra_400',False,True),
    ('framing_downscale','kodak_portra_400',True,True),
]
for name,film,spatial,auto in cases:
    image=rng.uniform(-0.015 if not spatial else 0,1.3,(17,19,3))
    p=init_params(film,'kodak_portra_endura')
    p.debug.deactivate_spatial_effects=not spatial
    p.debug.deactivate_stochastic_effects=True
    if name=='grain':
        p.debug.deactivate_stochastic_effects=False
        p.settings.use_fast_stats=False
        p.print_render.glare.active=False
    p.camera.auto_exposure=auto
    p.camera.film_format_mm=1.0 if spatial else 35.0
    p.camera.auto_exposure_method='matrix' if name=='matrix_meter' else 'center_weighted'
    config=dict(film_profile=film,print_profile='kodak_portra_endura',
        debug=dict(deactivate_spatial_effects=not spatial,deactivate_stochastic_effects=True),
        camera=dict(auto_exposure=auto,film_format_mm=p.camera.film_format_mm,auto_exposure_method=p.camera.auto_exposure_method))
    if name=='grain':
        config['debug']['deactivate_stochastic_effects']=False
        config['settings']=dict(use_fast_stats=False)
        config['print_glare']=dict(active=False)
    if name.startswith('scanner_'):
        p.scanner.black_correction=True
        p.scanner.white_correction=True
        p.scanner.black_level=0.025
        p.scanner.white_level=0.94
        config['scanner']=dict(black_correction=True,white_correction=True,black_level=0.025,white_level=0.94)
        if name=='scanner_reversal':
            p.workflow.route='input > film > scan'
            config['workflow']=dict(route=p.workflow.route)
    if name=='spectral_surface':
        p.settings.apply_hanatos2025_adaptation_surface=True
        p.settings.spectral_gaussian_blur=1.3
        config['settings']=dict(apply_hanatos2025_adaptation_surface=True,spectral_gaussian_blur=1.3)
    if name in ['oklrab','jzazbz','cam16ucs']:
        import dataclasses
        p.io.output_gamut_compress=dataclasses.replace(p.io.output_gamut_compress,algorithm=name)
        config['io']=dict(output_gamut_compress=dict(algorithm=name))
    if name in ['density_luts','bw_density_luts']:
        p.settings.use_enlarger_lut=True
        p.settings.use_scanner_lut=True
        p.settings.lut_resolution=9
        config['settings']=dict(use_enlarger_lut=True,use_scanner_lut=True,lut_resolution=9)
    if name in ['fast_grain','fast_grain_single_pixel']:
        p.debug.deactivate_stochastic_effects=False
        p.settings.use_fast_stats=True
        p.print_render.glare.active=False
        config['debug']['deactivate_stochastic_effects']=False
        config['settings']=dict(use_fast_stats=True,random_seed=12345)
        config['print_glare']=dict(active=False)
        if name=='fast_grain_single_pixel':
            image=np.full((1,1,3),0.184)
            p.camera.film_format_mm=35.0
            config['camera']['film_format_mm']=35.0
    if name=='random_glare':
        p.debug.deactivate_stochastic_effects=False
        p.film_render.grain.active=False
        p.print_render.glare.percent=0.3
        p.print_render.glare.blur=0.8
        config['debug']['deactivate_stochastic_effects']=False
        config['settings']=dict(random_seed=12345)
        config['grain']=dict(active=False)
        config['print_glare']=dict(percent=0.3,blur=0.8)
    if name.startswith('framing_'):
        p.io.crop=True
        p.io.crop_size=(0.5,0.4)
        p.io.crop_center=(0.9,0.1)
        p.io.upscale_factor=1.7 if name=='framing_upscale' else 0.63
        config['io']=dict(crop=True,crop_size=list(p.io.crop_size),crop_center=list(p.io.crop_center),upscale_factor=p.io.upscale_factor)
    target=folder/name
    target.mkdir(exist_ok=True)
    (target/'config.json').write_text(json.dumps(config,indent=2)+'\n')
    np.save(target/'input.npy',image)
    sim=SimulationPipeline(digest_params(p))
    taps=[t for t in TAPS if name!='scanner_reversal' or t not in ['log_e_print','cmy_print']]
    if name.startswith('framing_'):taps=['rgb_pre']+taps
    for tap in taps:
        if name in ['fast_grain','fast_grain_single_pixel','random_glare']:
            numba.set_num_threads(1)
            if name in ['fast_grain','fast_grain_single_pixel']:
                import spektrafilm.model.grain as grain_model
                original_layer=grain_model.layer_particle_model
                def seeded_layer(*args,**kwargs):
                    seed_numba((12345+kwargs['seed'])&0xffffffff)
                    return original_layer(*args,**kwargs)
                grain_model.layer_particle_model=seeded_layer
            else: seed_numba((12345+0x6c617265)&0xffffffff)
        try: np.save(target/f'{tap}.npy',sim.process(image,collect=tap))
        finally:
            if name in ['fast_grain','fast_grain_single_pixel']: grain_model.layer_particle_model=original_layer
            numba.set_num_threads(4)
(folder/'PROVENANCE.json').write_text(json.dumps(dict(baseline_commit=BASELINE['commit'],numpy_version=np.__version__,generated_by='tools/generate_fixtures.py',seed=8321,atol=1e-9,rtol=1e-8,controlled_stochastic_seed=12345,controlled_stochastic_threads=1),indent=2)+'\n')
print('Generated',len(cases),'golden cases in',folder)
