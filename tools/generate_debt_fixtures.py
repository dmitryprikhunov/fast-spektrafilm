"""Audited features: generate goldens exclusively from the verified reference."""
import os
os.environ.update(NUMBA_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1', OMP_NUM_THREADS='1')
from pathlib import Path
import json
import numpy as np
from validate import ROOT, init_params, digest_params, SimulationPipeline
from spektrafilm.utils.preview import resize_for_preview
from spektrafilm.runtime.print_balance import solve_neutral_filter_shifts
from spektrafilm.utils.io import save_image_oiio, load_image_oiio
import colour

folder = ROOT / 'tests/fixtures/debt'
folder.mkdir(exist_ok=True)

def save(name, image):
    np.save(folder / name, np.asarray(image, dtype=np.float64))

# A real cached camera crop with bright borders, odd axes, and fine spacing.
image = np.load(ROOT / 'artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy')[20:83, 30:115].copy()
image[0] = [4., 3., 2.]
image[:, -1] = [.02, .1, 5.]
save('preview_input', image)
save('preview_pre', resize_for_preview(image, 37))
p = init_params()
p.settings.preview_mode = True
p.settings.preview_max_size = 37
p.camera.film_format_mm = 1.5
p.debug.deactivate_stochastic_effects = True
sim = SimulationPipeline(digest_params(p))
for tap in ['log_e_film', 'cmy_film', 'log_e_print', 'cmy_print', 'rgb_out']:
    save('preview_' + tap, sim.process(resize_for_preview(image, 37), collect=tap))

# Film exposure/density and print exposure/density injection.
p = init_params()
p.camera.auto_exposure = False
p.camera.film_format_mm = 1.5
p.camera.diffusion_filter.active = True
p.enlarger.diffusion_filter.active = True
p.debug.deactivate_stochastic_effects = True
sim = SimulationPipeline(digest_params(p))
save('injection_input', image[:7, :9])
for tap in ['log_e_film', 'cmy_film', 'log_e_print', 'cmy_print']:
    stage = sim.process(image[:7, :9], collect=tap)
    save('injection_' + tap, stage)
    save('injection_' + tap + '_out', sim.process(stage, inject=tap))

balance = []
for stock, cmy in [('kodak_portra_400', (1.,1.,1.)), ('kodak_portra_400', (1.2,.9,.8)), ('fujifilm_provia_100f', (1.,1.,1.))]:
    p = init_params(stock, 'kodak_portra_endura')
    p.camera.auto_exposure = False
    p.debug.deactivate_spatial_effects = True
    p.debug.deactivate_stochastic_effects = True
    p.film_render.base.cyan, p.film_render.base.magenta, p.film_render.base.yellow = cmy
    shifts = solve_neutral_filter_shifts(p)
    p.enlarger.m_filter_shift, p.enlarger.y_filter_shift = shifts
    p.io.output_cctf_encoding = False
    from spektrafilm.utils.gamut_compression import OutputGamutCompressSpec
    p.io.output_gamut_compress = OutputGamutCompressSpec(algorithm='off', lightness_compression=None)
    rgb = SimulationPipeline(digest_params(p)).process(np.full((2,2,3),.184)).mean((0,1))
    balance.append(dict(stock=stock,cmy=cmy,shifts=shifts,rgb=rgb.tolist()))
(folder / 'balance.json').write_text(json.dumps(balance, indent=2)+'\n')

from spektrafilm.model.convert import fit_base_params, sample_base_rgb, format_calibration_matrix
from spektrafilm.model.illuminants import standard_illuminant
import dataclasses
calibrations = []
for idx, (stock, cmy, ev) in enumerate([
    ('kodak_portra_400', (1.,1.,1.), 0.),
    ('kodak_portra_400', (.8,1.15,.9), .7),
    ('fujifilm_provia_100f', (1.,1.,1.), -.5),
    ('kodak_portra_400', (1.,1.,1.), 0.),  # uniform/degenerate gamut
    ('kodak_portra_400', (1.,1.,1.), 0.),  # clear-base-only input
]):
    p = init_params(stock, 'kodak_portra_endura')
    p.workflow.route = 'input > convert-film > print > scan'
    p.camera.auto_exposure = False
    p.debug.deactivate_spatial_effects = True
    p.debug.deactivate_stochastic_effects = True
    p.film_render.base.cyan, p.film_render.base.magenta, p.film_render.base.yellow = cmy
    p.film_render.convert.exposure_compensation_ev = ev
    digested = digest_params(p)
    converter = SimulationPipeline(digested)._converting_stage._converter
    density = np.clip(image[:7,:9],.001,.9)
    if idx == 3: density = np.full_like(density, .184)
    if idx == 4: density = np.zeros_like(density)
    scan = converter._forward_jac(density.reshape(-1,3))[0].reshape(density.shape) / float(converter.gain)
    save(f'calibration_{idx}_input', scan)
    import scipy.optimize
    original_lm = scipy.optimize.least_squares
    final_residual = None
    def traced_lm(fun, *args, **kwargs):
        global final_residual
        result = original_lm(fun,*args,**kwargs)
        final_residual = fun(result.x)
        return result
    scipy.optimize.least_squares = traced_lm
    try:
        fit = converter.fit_blind_calibration(scan)
    finally:
        scipy.optimize.least_squares = original_lm
    save(f'calibration_{idx}_residual',final_residual.reshape(-1,1,1))
    fitted_base, base_ev = fit_base_params(sample_base_rgb(scan),digested.film.data.channel_density,digested.film.data.base_density,digested.film_render.base,standard_illuminant('D55'),'ProPhoto RGB')
    p.film_render.convert.calibration = format_calibration_matrix(fit)
    calibrated = SimulationPipeline(digest_params(p))
    calibrated._converting_stage._converter.calibration = fit.astype(np.float32)
    for tap in ['cmy_film','rgb_out']:
        save(f'calibration_{idx}_{tap}', calibrated.process(scan,collect=tap))
    calibrations.append(dict(stock=stock,cmy=cmy,ev=ev,matrix=fit.tolist(),base=dataclasses.asdict(fitted_base),base_ev=base_ev))
(folder / 'calibration.json').write_text(json.dumps(calibrations,indent=2)+'\n')

# OpenImageIO integer and float encodings, tagged and untagged.
pixels = np.linspace(-.01,1.07,45).reshape(3,5,3)
save('file_input',pixels)
for ext, bits in [('png',8),('tiff',8),('tiff',16),('tiff',32),('exr',16),('exr',32)]:
    name = f'file_{ext}{bits}'
    save_image_oiio(str(folder / f'{name}.{ext}'), pixels, bit_depth=bits, color_space='sRGB')
    save(name, load_image_oiio(str(folder / f'{name}.{ext}')))
encoded = np.load(ROOT / 'tests/fixtures/portra_hdr/rgb_out.npy')
save('saving_input',encoded)
save('saving_p3',colour.RGB_to_RGB(encoded,'sRGB','Display P3',apply_cctf_decoding=True,apply_cctf_encoding=True))

# OIIO does not apply ICC or orientation; preserve its decoded values and
# separately verify the Rust colour-managed/oriented file-input contract.
wide = np.linspace(.25,.65,45).reshape(3,5,3)
save_image_oiio(str(folder/'wide_p3.tiff'),wide,bit_depth=16,color_space='Display P3')
wide = load_image_oiio(str(folder/'wide_p3.tiff'))
save('wide_p3_srgb',colour.RGB_to_RGB(wide,'Display P3','sRGB',apply_cctf_decoding=True,apply_cctf_encoding=True))
import OpenImageIO as oiio
oriented = (np.clip(pixels,0,1)*65535).astype(np.uint16)
spec = oiio.ImageSpec(5,3,3,oiio.TypeDesc('uint16'))
spec.attribute('Orientation',6)
out = oiio.ImageOutput.create(str(folder/'oriented.tiff'))
assert out.open(str(folder/'oriented.tiff'),spec)
assert out.write_image(oriented)
out.close()
save('oriented',np.rot90(oriented.astype(float)/65535,-1))

(folder / 'PROVENANCE.json').write_text(json.dumps(dict(baseline=json.loads((ROOT/'BASELINE.json').read_text())['commit'],generator='tools/generate_debt_fixtures.py',numpy=np.__version__,colour=colour.__version__,scipy=__import__('scipy').__version__,blas={k:v for k,v in np.show_config(mode='dicts')['Build Dependencies']['blas'].items() if k not in ['include directory','lib directory','pc file directory']}),indent=2)+'\n')

# Calibration's two sampling grids use rounded float64 linspace, not integer
# rational division; the 40536-pixel case contains exact-integer boundaries.
sampling = []
for total, count in [(1,1),(2,1),(40536,40000),(80000,40000),(88000,1500),(22000000,40000)]:
    expected = np.linspace(0,total-1,count).astype(int)
    rational = np.arange(count,dtype=np.int64)*(total-1)//max(1,count-1)
    points = sorted(set([0,count-1,count//2] + np.flatnonzero(expected != rational).tolist()))
    sampling.append(dict(total=total,count=count,samples=[[i,int(expected[i])] for i in points]))
(folder/'sampling.json').write_text(json.dumps(sampling,indent=2)+'\n')
