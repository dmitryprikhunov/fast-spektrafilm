"""Differential tests against the pinned experimental reference (development only)."""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sys
import time
import dataclasses

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--threads',type=int,default=4)
    parser.add_argument('--spatial',action='store_true')
    parser.add_argument('--variants',action='store_true')
    parser.add_argument('--decode',action='store_true')
    parser.add_argument('--grain',action='store_true')
    parser.add_argument('--spectral',action='store_true')
    parser.add_argument('--gamut',action='store_true')
    parser.add_argument('--lut',action='store_true')
    parser.add_argument('--framing',action='store_true')
    parser.add_argument('--all-profiles',action='store_true')
    parser.add_argument('--samples',action='store_true')
    parser.add_argument('--cached-crops',type=Path,help='Already decoded NPY crops (no RAW files required)')
    parser.add_argument('--crop-size',type=int,default=256,dest='sample_size')
    ARGS=parser.parse_args()
    os.environ['NUMBA_NUM_THREADS']=str(ARGS.threads)
    os.environ['OPENBLAS_NUM_THREADS']=str(ARGS.threads)
    os.environ['OMP_NUM_THREADS']=str(ARGS.threads)
import numpy as np

ROOT=Path(__file__).resolve().parents[1]
BASELINE=json.loads((ROOT/'BASELINE.json').read_text())
REFERENCE=ROOT/'.reference'/BASELINE['commit']
from check_baseline import verify
verify(require_reference=True)
sys.path.insert(0,str(REFERENCE/'src'))
from spektrafilm.runtime.params_builder import init_params,digest_params
from spektrafilm.runtime.pipeline import SimulationPipeline

TAPS=['log_e_film','cmy_film','log_e_print','cmy_print','rgb_out']

def rust(input_path,output_path,config,tap='rgb_out',repeat=1):
    args=[str(ROOT/'target/release/fast-spektrafilm'),'--threads',str(ARGS.threads),'process',str(input_path),str(output_path),'--config',str(config),'--collect',tap.replace('_','-'),'--repeat',str(repeat)]
    result=subprocess.run(args,capture_output=True,text=True)
    if result.returncode: raise RuntimeError(result.stderr)
    return json.loads(result.stderr.strip().splitlines()[-1])

def reference_params(film,paper):
    p=init_params(film,paper)
    p.debug.deactivate_spatial_effects=True
    p.debug.deactivate_stochastic_effects=True
    p.camera.auto_exposure=False
    return digest_params(p)

def compare(actual,expected,label):
    from numerical import require
    return dict(case=label, **require(actual,expected,label,
        bitwise=label.startswith('grain_') and label.endswith('/cmy_film')))

def run_case(image,name,film,paper,options=None,taps=None,controlled_glare=False,controlled_grain=False):
    folder=ROOT/'artifacts/parity'/name
    folder.mkdir(parents=True,exist_ok=True)
    np.save(folder/'input.npy',image)
    config=dict(film_profile=film,print_profile=paper,debug=dict(deactivate_spatial_effects=True,deactivate_stochastic_effects=True),camera=dict(auto_exposure=False))
    options=options or {}
    for group,values in options.items():
        if isinstance(values,dict): config.setdefault(group,{}).update(values)
        else: config[group]=values
    (folder/'config.json').write_text(json.dumps(config))
    ref=init_params(film,paper)
    ref.debug.deactivate_spatial_effects=True
    ref.debug.deactivate_stochastic_effects=True
    ref.camera.auto_exposure=False
    for group,values in options.items():
        if group in ['dir_couplers','halation','grain']: target=getattr(ref.film_render,group)
        elif group=='print_glare': target=ref.print_render.glare
        elif group=='film_chemistry': target=ref.film_render.chemistry
        elif group=='print_chemistry': target=ref.print_render.chemistry
        elif group=='film_base':target=ref.film_render.base
        elif group=='print_base':target=ref.print_render.base
        else: target=getattr(ref,group)
        def apply(target,values):
            updates={}
            for key,value in values.items():
                updates[key]=apply(getattr(target,key),value) if isinstance(value,dict) else value
            if dataclasses.is_dataclass(target) and target.__dataclass_params__.frozen:
                return dataclasses.replace(target,**updates)
            for key,value in updates.items():setattr(target,key,value)
            return target
        updated=apply(target,values)
        if group=='film_chemistry':ref.film_render.chemistry=updated
        elif group=='print_chemistry':ref.print_render.chemistry=updated
    sim=SimulationPipeline(digest_params(ref))
    records=[]
    route=config.get('workflow',{}).get('route','input > film > print > scan')
    taps=taps or (['rgb_out'] if route=='input' else [t for t in TAPS if route!='input > film > scan' or t not in ['log_e_print','cmy_print']])
    for tap in taps:
        if controlled_glare or controlled_grain:
            import numba
            @numba.njit
            def seed_numba(seed):np.random.seed(seed)
            numba.set_num_threads(1)
            if controlled_glare:seed_numba((config.get('settings',{}).get('random_seed',0)+0x6c617265)&0xffffffff)
            if controlled_grain:
                import spektrafilm.model.grain as grain_model
                original_layer=grain_model.layer_particle_model
                def seeded_layer(*args,**kwargs):
                    seed_numba((config['settings']['random_seed']+kwargs['seed'])&0xffffffff)
                    return original_layer(*args,**kwargs)
                grain_model.layer_particle_model=seeded_layer
        try:expected=sim.process(image,collect=tap)
        finally:
            if controlled_grain:grain_model.layer_particle_model=original_layer
            if controlled_glare or controlled_grain:numba.set_num_threads(ARGS.threads)
        np.save(folder/f'reference_{tap}.npy',expected)
        metrics=rust(folder/'input.npy',folder/f'rust_{tap}.npy',folder/'config.json',tap)
        record=compare(np.load(folder/f'rust_{tap}.npy'),expected,f'{name}/{tap}')
        record.update(rust_seconds=metrics['process_seconds'][0],reference_seconds=sim.get_total_elapsed_time())
        records.append(record)
        print(json.dumps(record),flush=True)
    return records

def main():
    results=[]
    ramp=np.broadcast_to(np.linspace(0.01,1.0,16)[None,:,None],(16,16,3)).copy()
    results+=run_case(ramp,'portra_ramp','kodak_portra_400','kodak_portra_endura')
    results+=run_case(ramp,'fuji_ramp','fujifilm_pro_400h','fujifilm_crystal_archive_typeii')
    rng=np.random.default_rng(4321)
    image=rng.uniform(-0.03,1.5,(32,37,3))
    results+=run_case(image,'random_hdr','kodak_portra_400','kodak_portra_endura')
    if ARGS.grain:
        options=dict(debug=dict(deactivate_stochastic_effects=False),settings=dict(use_fast_stats=False),print_glare=dict(active=False))
        results+=run_case(rng.uniform(0,1,(31,37,3)),'grain_legacy_portra','kodak_portra_400','kodak_portra_endura',options)
        options['debug']['deactivate_spatial_effects']=False
        options['camera']=dict(film_format_mm=1.0)
        results+=run_case(rng.uniform(0,1,(31,37,3)),'grain_spatial_portra','kodak_portra_400','kodak_portra_endura',options)
        results+=run_case(rng.uniform(0,1,(31,37,3)),'grain_spatial_reversal','fujifilm_provia_100f','kodak_portra_endura',options)
        results+=run_case(rng.uniform(0,1,(31,37,3)),'grain_spatial_bw','kodak_doublex','kodak_portra_endura',options)
        options['camera']['film_format_mm']=0.01
        results+=run_case(rng.uniform(0,1,(31,37,3)),'grain_microstructure','kodak_portra_400','kodak_portra_endura',options)
        glare_options=dict(debug=dict(deactivate_spatial_effects=False,deactivate_stochastic_effects=False),grain=dict(active=False),print_glare=dict(roughness=0.0,percent=0.3,blur=0.8))
        results+=run_case(rng.uniform(0,1,(31,37,3)),'glare_constant','kodak_portra_400','kodak_portra_endura',glare_options)
        glare_options['print_glare']['roughness']=0.7
        glare_options['settings']=dict(random_seed=12345)
        results+=run_case(rng.uniform(0,1,(63,67,3)),'glare_seeded','kodak_portra_400','kodak_portra_endura',glare_options,controlled_glare=True)
        fast_options=dict(debug=dict(deactivate_spatial_effects=False,deactivate_stochastic_effects=False),settings=dict(use_fast_stats=True,random_seed=12345),print_glare=dict(active=False),camera=dict(film_format_mm=1.0))
        for stock in ['kodak_portra_400','fujifilm_provia_100f','kodak_doublex']:
            results+=run_case(rng.uniform(0,1,(63,67,3)),f'grain_fast_{stock}',stock,'kodak_portra_endura',fast_options,controlled_grain=True)
        fast_options['camera']['film_format_mm']=0.01
        results+=run_case(rng.uniform(0,1,(31,37,3)),'grain_fast_microstructure','kodak_portra_400','kodak_portra_endura',fast_options,controlled_grain=True)
        fast_options['camera']['film_format_mm']=35.0
        results+=run_case(np.full((1,1,3),0.184),'grain_fast_single_pixel','kodak_portra_400','kodak_portra_endura',fast_options,controlled_grain=True)
    if ARGS.spectral:
        for name,options in [
            ('surface',dict(settings=dict(apply_hanatos2025_adaptation_surface=True))),
            ('smoothing',dict(settings=dict(spectral_gaussian_blur=1.3))),
            ('surface_smoothing',dict(settings=dict(apply_hanatos2025_adaptation_surface=True,spectral_gaussian_blur=2.7))),
            ('hull_round',dict(io=dict(input_gamut_compress=dict(hull_detail=2.0)))),
            ('hull_sharp',dict(io=dict(input_gamut_compress=dict(hull_detail=12.0)))),
            ('locus',dict(io=dict(input_gamut_compress=dict(boundary='locus')))),
        ]:
            results+=run_case(image,f'spectral_{name}','kodak_portra_400','kodak_portra_endura',options)
    if ARGS.gamut:
        for algorithm in ['aces_rgc','oklrab','jzazbz','cam16ucs']:
            for space in ['sRGB','Adobe RGB (1998)','ITU-R BT.2020','ACES2065-1','ACEScg','ProPhoto RGB','Display P3']:
                results+=run_case(image,f'gamut_{algorithm}_{space}','kodak_portra_400','kodak_portra_endura',dict(io=dict(output_color_space=space,output_gamut_compress=dict(algorithm=algorithm))),taps=['rgb_out'])
    if ARGS.lut:
        for film in ['kodak_portra_400','fujifilm_provia_100f','kodak_doublex']:
            for resolution in [9,17]:
                results+=run_case(image,f'lut_{film}_{resolution}',film,'kodak_portra_endura',dict(settings=dict(use_enlarger_lut=True,use_scanner_lut=True,lut_resolution=resolution)))
            results+=run_case(image,f'lut_scan_{film}',film,'kodak_portra_endura',dict(settings=dict(use_scanner_lut=True),workflow=dict(route='input > film > scan')))
        results+=run_case(image,'lut_chemistry_blackwhite','kodak_portra_400','kodak_portra_endura',dict(settings=dict(use_enlarger_lut=True,use_scanner_lut=True),scanner=dict(black_correction=True,white_correction=True),print_chemistry=dict(gamma_factor=0.9),enlarger=dict(preflash_exposure=0.001,m_filter_shift=-5)))
    if ARGS.variants:
        for correction in [dict(black_correction=True),dict(white_correction=True),dict(black_correction=True,white_correction=True,black_level=0.025,white_level=0.94)]:
            label=''.join('b' if key=='black_correction' else 'w' for key in ['black_correction','white_correction'] if correction.get(key))
            results+=run_case(image,f'scanner_print_{label}','kodak_portra_400','kodak_portra_endura',dict(scanner=correction))
            results+=run_case(image,f'scanner_reversal_{label}','fujifilm_provia_100f','kodak_portra_endura',dict(scanner=correction,workflow=dict(route='input > film > scan')))
        results+=run_case(image,'scanner_negative_bypass','kodak_portra_400','kodak_portra_endura',dict(scanner=dict(black_correction=True,white_correction=True),workflow=dict(route='input > film > scan')))
        results+=run_case(image,'scanner_chemistry','kodak_portra_400','kodak_portra_endura',dict(scanner=dict(black_correction=True,white_correction=True),film_chemistry=dict(gamma_factor=0.85),print_chemistry=dict(gamma_factor=1.15),film_base=dict(cyan=1.04,scale=0.9),print_base=dict(scale=1.1)))
        for algorithm in ['aces_rgc','oklrab']:
            results+=run_case(image,f'gamut_{algorithm}','kodak_portra_400','kodak_portra_endura',dict(io=dict(output_gamut_compress=dict(algorithm=algorithm))))
        results+=run_case(image,'highlight_boost','kodak_portra_400','kodak_portra_endura',dict(halation=dict(boost_ev=2.0,protect_ev=1.0,boost_range=0.35)))
        results+=run_case(image,'base_tuning','kodak_portra_400','kodak_portra_endura',dict(film_base=dict(cyan=1.1,magenta=0.98,scale=0.8,tilt=0.04),print_base=dict(yellow=1.2,cyan=0.95,scale=1.1),enlarger=dict(preflash_exposure=0.002)))
        results+=run_case(image,'chemistry','kodak_portra_400','kodak_portra_endura',dict(film_chemistry=dict(gamma_factor=1.15,gamma_factor_fast=0.8,gamma_factor_blue=1.1,developer_exhaustion=0.4),print_chemistry=dict(gamma_factor=0.9,gamma_factor_slow=1.15,developer_exhaustion=0.7)))
        results+=run_case(image,'bw_development','kodak_doublex','kodak_portra_endura',dict(film_chemistry=dict(development_time=12,gamma_factor=0.85)))
        for method in ['arctic2026beta04','jakob2019','mallett2019','otsu2018','gauss-lasers']:
            results+=run_case(image,f'upsampling_{method}','kodak_portra_400','kodak_portra_endura',dict(settings=dict(rgb_to_raw_method=method)))
        for space in ['sRGB','Adobe RGB (1998)','ITU-R BT.2020','ACES2065-1','ACEScg','ProPhoto RGB','Display P3']:
            results+=run_case(image,f'colorspace_{space.replace("/","_")}','kodak_portra_400','kodak_portra_endura',dict(io=dict(input_color_space=space,input_cctf_decoding=True,output_color_space=space)))
        results+=run_case(image,'exposure_filters_preflash','kodak_portra_400','kodak_portra_endura',dict(camera=dict(exposure_compensation_ev=1.3),enlarger=dict(print_exposure=0.8,y_filter_shift=-8,m_filter_shift=5,preflash_exposure=0.002)))
        results+=run_case(image,'no_compression_no_couplers','kodak_portra_400','kodak_portra_endura',dict(dir_couplers=dict(active=False),io=dict(input_gamut_compress=dict(active=False),output_gamut_compress=dict(algorithm='off'))))
        results+=run_case(image,'film_scan','fujifilm_provia_100f','kodak_portra_endura',dict(workflow=dict(route='input > film > scan')))
        results+=run_case(image,'passthrough','kodak_portra_400','kodak_portra_endura',dict(workflow=dict(route='input'),io=dict(input_cctf_decoding=True,output_color_space='ProPhoto RGB')))
        for path in sorted((ROOT/'data/profiles').glob('*.json')):
            if json.loads(path.read_text())['info']['stage']=='printing':
                results+=run_case(image,f'paper_{path.stem}','kodak_portra_400',path.stem)
    if ARGS.spatial:
        for method in ['average','center_weighted','median','partial','matrix','multi_zone','highlight_weighted']:
            results+=run_case(rng.uniform(0.0,1.0,(19,23,3)),f'meter_{method}','kodak_portra_400','kodak_portra_endura',dict(camera=dict(auto_exposure=True,auto_exposure_method=method)))
        results+=run_case(rng.uniform(0.0,1.0,(263,389,3)),'meter_resampled','kodak_portra_400','kodak_portra_endura',dict(camera=dict(auto_exposure=True)))
        options=dict(debug=dict(deactivate_spatial_effects=False),camera=dict(film_format_mm=2.5,auto_exposure=True))
        results+=run_case(rng.uniform(0.0,1.0,(128,160,3)),'spatial_portra','kodak_portra_400','kodak_portra_endura',options)
        results+=run_case(rng.uniform(0.0,1.0,(128,160,3)),'spatial_reversal','fujifilm_provia_100f','kodak_portra_endura',options)
    if ARGS.framing:
        framing_input=rng.uniform(-0.01,1.3,(31,43,3))
        for name,io in [
            ('center',dict(crop=True,crop_size=[0.4,0.3])),
            ('edge',dict(crop=True,crop_center=[0.98,0.02],crop_size=[0.4,0.3])),
            ('oversize',dict(crop=True,crop_center=[0.5,0.5],crop_size=[0.8,0.9])),
            ('upscale',dict(upscale_factor=1.7)),
            ('downscale',dict(upscale_factor=0.63)),
            ('crop_upscale',dict(crop=True,crop_center=[0.8,0.2],crop_size=[0.4,0.3],upscale_factor=1.5)),
        ]:
            results+=run_case(framing_input,f'framing_{name}','kodak_portra_400','kodak_portra_endura',dict(io=io,camera=dict(auto_exposure=True)),taps=['rgb_pre']+TAPS)
        results+=run_case(rng.uniform(0.0,1.0,(97,131,3)),'framing_spatial','kodak_portra_400','kodak_portra_endura',dict(io=dict(crop=True,crop_size=[0.4,0.3],upscale_factor=0.7),camera=dict(auto_exposure=True,film_format_mm=2.5),debug=dict(deactivate_spatial_effects=False)),taps=['rgb_pre']+TAPS)
        results+=run_case(rng.uniform(0.0,1.0,(263,389,3)),'framing_full_meter','kodak_portra_400','kodak_portra_endura',dict(io=dict(crop=True,crop_center=[0.8,0.1],crop_size=[0.1,0.1]),camera=dict(auto_exposure=True)),taps=['rgb_pre']+TAPS)
    if ARGS.all_profiles:
        for profile in sorted((ROOT/'data/profiles').glob('*.json')):
            info=json.loads(profile.read_text())['info']
            if info['stage']=='filming':
                results+=run_case(image,profile.stem,profile.stem,'kodak_portra_endura')
    if ARGS.cached_crops:
        for path in sorted(ARGS.cached_crops.glob('RAW*.npy')):
            image=np.load(path)
            assert max(image.shape[:2]) <= ARGS.sample_size, (path,image.shape)
            results+=run_case(image,path.stem,'kodak_portra_400','kodak_portra_endura')
    if ARGS.samples or ARGS.decode:
        import rawpy
        from spektrafilm.utils.raw_file_processor import load_and_process_raw_file
        
        for path in sorted((ROOT/'samples').iterdir()):
            print('Sample:',path.name,'rawpy LibRaw:',rawpy.libraw_version,flush=True)
            cache=ROOT/'artifacts/crops'/f'{path.stem}_{ARGS.sample_size}.npy'
            cache.parent.mkdir(parents=True,exist_ok=True)
            if cache.exists():
                image=np.load(cache)
            else:
                import colour
                with rawpy.imread(str(path)) as raw:
                    rgb16=raw.postprocess(output_color=rawpy.ColorSpace.ACES,output_bps=16,no_auto_bright=True,gamma=(1,1),use_camera_wb=True)
                h,w=rgb16.shape[:2]
                size=ARGS.sample_size
                y=max(0,(h-size)//2);x=max(0,(w-size)//2)
                crop=rgb16[y:y+size,x:x+size].astype(np.float32)/np.float32(65535.0)
                del rgb16
                image=colour.RGB_to_RGB(crop,'ACES2065-1','ProPhoto RGB',apply_cctf_decoding=False,apply_cctf_encoding=False)
                np.save(cache,image)
            if ARGS.decode:
                output=cache.parent/f'decoded_{path.stem}_{ARGS.sample_size}.npy'
                decoded=subprocess.run([str(ROOT/'target/release/fast-spektrafilm'),'decode',str(path),str(output),'--crop-size',str(ARGS.sample_size)],capture_output=True,text=True,check=True)
                record=compare(np.load(output),image,f'{path.stem}/decode')
                record.update(json.loads(decoded.stderr.strip().splitlines()[-1]),reference_libraw=list(rawpy.libraw_version))
                results.append(record)
                print(json.dumps(record),flush=True)
            if ARGS.samples: results+=run_case(image,path.stem,'kodak_portra_400','kodak_portra_endura')
    report=ROOT/'artifacts/parity/report.json'
    report.write_text(json.dumps(dict(baseline_commit=BASELINE['commit'],rtol=1e-8,atol=1e-9,results=results),indent=2)+'\n')
    print('PASS:',len(results),'stage comparisons. Report:',report)

if __name__=='__main__':
    main()
