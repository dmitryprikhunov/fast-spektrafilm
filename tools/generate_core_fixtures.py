"""Additional fixtures from the pinned release; no processing implementation lives here."""
from pathlib import Path
import os
os.environ.update(NUMBA_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',OMP_NUM_THREADS='1')
import json, dataclasses
import numpy as np
from validate import ROOT, init_params, digest_params, SimulationPipeline
from spektrafilm.model import diffusion
from spektrafilm.model.convert import FilmConverter, fit_base_params, sample_base_rgb
from spektrafilm.model.illuminants import standard_illuminant
root=ROOT/'tests/fixtures/core'; root.mkdir(exist_ok=True)
rng=np.random.default_rng(491)
image=rng.uniform(.001,.9,(7,9,3))
np.save(root/'input.npy',image)
# Source family constants are copied, not re-created by hand.
(ROOT/'data/diffusion.json').write_text(json.dumps(dict(shapes=diffusion._DIFFUSION_FILTER_SHAPES,gains=diffusion._DIFFUSION_FAMILY_TOTAL_GAIN,breakpoints=diffusion._DIFFUSION_STRENGTH_BREAKPOINTS.tolist(),fractions=diffusion._DIFFUSION_STRENGTH_TOTAL_FRACTION.tolist()),indent=2)+'\n')
for family in diffusion.DIFFUSION_FILTER_FAMILIES:
    p=init_params(); filt=p.camera.diffusion_filter
    filt.active=True;filt.filter_family=family;filt.strength=.7;filt.halo_warmth=.2;filt.core_size=.8;filt.bloom_intensity=1.2
    np.save(root/f'diffusion_{family}.npy',diffusion.apply_diffusion_filter_um(image,filt,55.))
    (root/f'diffusion_{family}.json').write_text(json.dumps(dataclasses.asdict(filt)))
p=init_params(); p.camera.auto_exposure=False;p.debug.deactivate_spatial_effects=True;p.debug.deactivate_stochastic_effects=True
p.workflow.route='input > convert-film > scan'
p=digest_params(p)
pipe=SimulationPipeline(p)
converter=pipe._converting_stage._converter
np.save(root/'captured.npy',converter._forward_jac(image.reshape(-1,3))[0].reshape(image.shape).astype(float))
scan=np.load(root/'captured.npy')
np.save(root/'convert.npy',converter.convert(scan))
np.save(root/'convert_arbitrary.npy',converter.convert(image))
for route in ['input > convert-film > scan','input > convert-film > scan-minus-base','input > convert-film > print > scan']:
    q=init_params();q.camera.auto_exposure=False;q.debug.deactivate_spatial_effects=True;q.debug.deactivate_stochastic_effects=True;q.workflow.route=route
    out=SimulationPipeline(digest_params(q)).process(scan,collect='rgb_out')
    np.save(root/(route.split(' > ')[-1]+('_print' if 'print' in route else '')+'.npy'),out)
# Base fitting, including the source's scale/tilt behaviour.
b,ev=fit_base_params(sample_base_rgb(scan),p.film.data.channel_density,p.film.data.base_density,p.film_render.base,standard_illuminant('D55'),'ProPhoto RGB')
(root/'base.json').write_text(json.dumps(dict(base=dataclasses.asdict(b),exposure=ev)))

# Camera and enlarger placement, including log/power exposure round trips.
for stage in ['camera','enlarger']:
    q=init_params();q.camera.auto_exposure=False;q.debug.deactivate_stochastic_effects=True;q.camera.film_format_mm=.5
    filt=getattr(q,stage).diffusion_filter;filt.active=True;filt.strength=.7
    sim=SimulationPipeline(digest_params(q))
    for tap in ['log_e_film','cmy_film','log_e_print','cmy_print','rgb_out']:
        np.save(root/f'{stage}_{tap}.npy',sim.process(image,collect=tap))
# RAW white balance is a distinct float32 stage.
from spektrafilm.utils.raw_file_processor import _postprocess_params, _apply_white_balance_adaptation, _apply_tint_adjustment
for temp in [1900.,2850.,5500.,9000.]:
    _,adapt,tint=_postprocess_params('custom',temp,1.13)
    out=image.astype(np.float32)
    if adapt is not None:out=_apply_white_balance_adaptation(out,*adapt)
    out=_apply_tint_adjustment(out,tint)
    np.save(root/f'raw_{int(temp)}.npy',out.astype(float))
# Lensfun with explicit metadata is reproducible without shipping a RAW.
from spektrafilm.utils.raw_file_processor import ExifData,_apply_lens_correction
lens=ExifData('Canon','Canon EOS 5D Mark III','Canon','Canon EF 24-105mm f/4L IS USM',24.,4.)
lens_image=np.tile(image,(7,7,1)).astype(np.float32)
np.save(root/'lens_input.npy',lens_image.astype(float))
lens_out,info=_apply_lens_correction(lens_image,lens)
assert info, 'Reference lens database did not match fixture lens'
np.save(root/'lens_output.npy',lens_out.astype(float))
(root/'PROVENANCE.json').write_text(json.dumps(dict(baseline='28bf883e1672e884307edc75852549376e13644e',generator='tools/generate_core_fixtures.py',seed=491,numpy=np.__version__,lens_info=info),indent=2)+'\n')
# Target-free device calibration: fitted values and residual quality.
calibration=converter.fit_blind_calibration(scan)
(root/'calibration.json').write_text(json.dumps(calibration.tolist())+'\n')
# Grain classes / shapes, controlled Numba stream matches Rust's seeded fast sampler.
import numba
import spektrafilm.model.grain as grain_model
@numba.njit
def seed_numba(seed): np.random.seed(seed)
original_layer=grain_model.layer_particle_model
def seeded_layer(*args,**kwargs):
    seed_numba((12345+kwargs['seed'])&0xffffffff)
    return original_layer(*args,**kwargs)
grain_model.layer_particle_model=seeded_layer
try:
    for stock in ['kodak_portra_400','fujifilm_provia_100f','kodak_doublex']:
        for h,w in [(1,9),(9,1),(8,11)]:
            im=rng.uniform(.02,.8,(h,w,3))
            q=init_params(stock,'kodak_portra_endura');q.camera.auto_exposure=False;q.print_render.glare.active=False
            sim=SimulationPipeline(digest_params(q))
            name=f'grain_{stock}_{h}_{w}'
            np.save(root/f'{name}_input.npy',im)
            np.save(root/f'{name}.npy',sim.process(im,collect='cmy_film'))
finally: grain_model.layer_particle_model=original_layer
q=init_params();q.camera.auto_exposure=False;q.debug.deactivate_spatial_effects=True;q.debug.deactivate_stochastic_effects=True;q.workflow.route='input > convert-film > print > scan';q.settings.use_convert_lut=True;q.settings.convert_lut_resolution=9
np.save(root/'convert_lut.npy',SimulationPipeline(digest_params(q)).process(scan,collect='rgb_out'))
np.save(root/'blackbody.npy',standard_illuminant('BB4725').reshape(1,81,1))
