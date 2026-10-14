"""Compare the actual Slint Session workflow on a cached crop (never full RAW).

GUI_BENCH_INPUT can be supplied to the Rust ignored test separately for explicit
full-image measurements. This script always bounds the Python reference input.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from benchmark import ROOT, provenance

def worker(args):
    import copy
    import numpy as np
    from validate import init_params, digest_params, SimulationPipeline
    from spektrafilm.utils.preview import resize_for_preview
    from spektrafilm.utils.io import save_image_oiio
    p=init_params()
    p.camera.auto_exposure=False
    p.debug.deactivate_stochastic_effects=True
    p.debug.deactivate_spatial_effects=args.mode != 'diffusion'
    if args.mode == 'convert': p.workflow.route='input > convert-film > print > scan'
    if args.mode == 'diffusion': p.camera.diffusion_filter.active=p.enlarger.diffusion_filter.active=True
    start=time.perf_counter(); image=np.load(args.input); load=time.perf_counter()-start
    assert max(image.shape[:2]) <= 256, 'Reference workflow requires a cached <=256px crop'
    start=time.perf_counter(); sim=SimulationPipeline(digest_params(p)); setup=time.perf_counter()-start
    start=time.perf_counter(); sim.process(image[:8,:8]); warmup=time.perf_counter()-start
    pp=copy.deepcopy(p); pp.settings.preview_mode=True
    start=time.perf_counter(); sim.update(digest_params(pp)); sim.process(resize_for_preview(image,640)); preview=time.perf_counter()-start
    start=time.perf_counter(); sim.update(digest_params(p)); result=sim.process(image); scan=time.perf_counter()-start
    stages=sim.get_timings().copy()
    np.save(args.folder/'scan-reference.npy',result)
    start=time.perf_counter(); save_image_oiio(str(args.folder/'scan-reference.tiff'),result,bit_depth=32,color_space='sRGB'); save=time.perf_counter()-start
    start=time.perf_counter(); save_image_oiio(str(args.folder/'scan-reference-again.tiff'),result,bit_depth=32,color_space='sRGB'); cache_save=time.perf_counter()-start
    print(json.dumps(dict(setup_seconds=setup,source_load_seconds=load,warmup_seconds=warmup,preview_seconds=preview,scan_seconds=scan,save_seconds=save,cache_hit_save_seconds=cache_save,save_format="TIFF32 ZIP",stage_seconds=stages)))

def main(args):
    if args.worker: worker(args); return
    import numpy as np
    folder=args.folder/args.mode; folder.mkdir(parents=True,exist_ok=True)
    env=os.environ.copy()
    env.update(RAYON_NUM_THREADS=str(args.threads),NUMBA_NUM_THREADS=str(args.threads),OPENBLAS_NUM_THREADS=str(args.threads),OMP_NUM_THREADS=str(args.threads))
    env.update(GUI_BENCH_INPUT=str(args.input),GUI_BENCH_DIR=str(folder),GUI_BENCH_MODE=args.mode)
    binary=args.gui_test.resolve()
    subprocess.run(['/usr/bin/time','-f','%M','-o',str(folder/'rust.rss'),str(binary),'gui::tests::benchmark_preview_scan_save','--ignored','--exact','--nocapture'],env=env,check=True)
    ref=subprocess.run(['/usr/bin/time','-f','%M','-o',str(folder/'reference.rss'),sys.executable,__file__,'--worker','--mode',args.mode,'--folder',str(folder),'--input',str(args.input)],env=env,check=True,text=True,capture_output=True)
    reference=json.loads(ref.stdout.strip().splitlines()[-1]); reference['peak_rss_kib']=int((folder/'reference.rss').read_text())
    rust=json.loads((folder/'workflow.json').read_text()); rust['peak_rss_kib']=int((folder/'rust.rss').read_text())
    a=np.load(folder/'scan.npy'); b=np.load(folder/'scan-reference.npy')
    assert a.shape == b.shape and np.isfinite(a).all() and np.isfinite(b).all()
    np.testing.assert_allclose(a,b,atol=2e-6 if args.mode=='diffusion' else 1e-9,rtol=1e-8)
    report=dict(**provenance(),gui_test_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),input_sha256=hashlib.sha256(args.input.read_bytes()).hexdigest(),mode=args.mode,threads=args.threads,rust=rust,reference=reference,max_abs=float(np.abs(a-b).max()))
    (folder/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))

if __name__ == '__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--worker',action='store_true')
    parser.add_argument('--gui-test',type=Path,help='Release lib-test executable from cargo test --no-run')
    parser.add_argument('--mode',choices=['forward','convert','diffusion'],default='forward')
    parser.add_argument('--threads',type=int,default=4)
    parser.add_argument('--input',type=Path,default=ROOT/'artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy')
    parser.add_argument('--folder',type=Path,default=ROOT/'artifacts/workflow-benchmark')
    args=parser.parse_args()
    if not args.worker and args.gui_test is None: parser.error('--gui-test is required')
    main(args)
