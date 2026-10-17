"""Compare added stages on the same cached 256-pixel sample crop, four threads."""
from pathlib import Path
from benchmark import provenance
import os,sys,json,time,subprocess,argparse
ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--worker',action='store_true')
parser.add_argument('--binary',type=Path,default=ROOT/'target/release/fast-spektrafilm')
parser.add_argument('--output-dir',type=Path,default=ROOT/'artifacts/core-benchmark')
parser.add_argument('worker_mode',nargs='?',choices=['convert','diffusion'])
args=parser.parse_args()
if args.worker != (args.worker_mode is not None):
    parser.error('a worker mode is required with --worker and only valid with --worker')
os.environ.update(NUMBA_NUM_THREADS='4',OPENBLAS_NUM_THREADS='4',OMP_NUM_THREADS='4')
import numpy as np
folder=args.output_dir.resolve();folder.mkdir(parents=True,exist_ok=True)
binary=args.binary.resolve()
source=ROOT/'artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy'
if args.worker:
    from validate import init_params,digest_params,SimulationPipeline
    mode=args.worker_mode;p=init_params();p.camera.auto_exposure=False;p.debug.deactivate_stochastic_effects=True
    if mode=='convert':p.workflow.route='input > convert-film > print > scan';p.debug.deactivate_spatial_effects=True
    else:
        p.camera.diffusion_filter.active=True;p.enlarger.diffusion_filter.active=True
    sim=SimulationPipeline(digest_params(p));image=np.load(source);sim.process(image[:8,:8]);times=[]
    for _ in range(3):
        start=time.perf_counter();out=sim.process(image);times.append(time.perf_counter()-start)
    np.save(folder/f'{mode}-reference.npy',out);print(json.dumps(dict(process_seconds=times,stage_seconds=sim.get_timings())));sys.exit()
results=[]
for mode in ['convert','diffusion']:
    config=dict(camera=dict(auto_exposure=False),debug=dict(deactivate_stochastic_effects=True))
    if mode=='convert':config['workflow']=dict(route='input > convert-film > print > scan');config['debug']['deactivate_spatial_effects']=True
    else:config['camera']['diffusion_filter']=dict(active=True);config['enlarger']=dict(diffusion_filter=dict(active=True))
    path=folder/f'{mode}.json';path.write_text(json.dumps(config))
    def run(command,label):
        rss=folder/f'{mode}-{label}.rss'
        result=subprocess.run(['/usr/bin/time','-f','%M','-o',str(rss),*command],capture_output=True,text=True,check=True)
        timing=json.loads((result.stdout or result.stderr).strip().splitlines()[-1]);timing['peak_rss_kib']=int(rss.read_text());return timing
    reference=run([sys.executable,__file__,'--worker','--output-dir',str(folder),mode],'reference')
    rust=run([str(binary),'--threads','4','process',str(source),str(folder/f'{mode}-rust.npy'),'--config',str(path),'--repeat','3'],'rust')
    a=np.load(folder/f'{mode}-rust.npy');b=np.load(folder/f'{mode}-reference.npy')
    assert a.shape == b.shape and np.isfinite(a).all() and np.isfinite(b).all(), 'Invalid comparison shape/values'
    from numerical import require
    comparison=require(a,b,mode,policy='strict' if mode=='convert' else 'optical')
    profiled=run([str(binary),'--threads','4','process',str(source),str(folder/f'{mode}-profiled.npy'),'--config',str(path),'--stage-timings'],'profiled')
    require(np.load(folder/f'{mode}-profiled.npy'),a,mode+' profiled')
    record=dict(comparison=comparison,profiled=profiled,mode=mode,shape=list(a.shape),threads=4,reference=reference,rust=rust,max_abs=float(abs(a-b).max()),rmse=float(np.sqrt(np.mean((a-b)**2))))
    results.append(record);print(json.dumps(record),flush=True)
(folder/'report.json').write_text(json.dumps(dict(**provenance(binary),threads=4,repeats=3,input_sha256=__import__('hashlib').sha256(source.read_bytes()).hexdigest(),results=results),indent=2)+'\n')
