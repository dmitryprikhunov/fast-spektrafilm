"""Compare added stages on the same cached 256-pixel sample crop, four threads."""
from pathlib import Path
import os,sys,json,time,subprocess
os.environ.update(NUMBA_NUM_THREADS='4',OPENBLAS_NUM_THREADS='4',OMP_NUM_THREADS='4')
ROOT=Path(__file__).resolve().parents[1]
import numpy as np
folder=ROOT/'artifacts/core-benchmark';folder.mkdir(exist_ok=True)
source=ROOT/'artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy'
if '--worker' in sys.argv:
    from validate import init_params,digest_params,SimulationPipeline
    mode=sys.argv[-1];p=init_params();p.camera.auto_exposure=False;p.debug.deactivate_stochastic_effects=True
    if mode=='convert':p.workflow.route='input > convert-film > print > scan';p.debug.deactivate_spatial_effects=True
    else:
        p.camera.diffusion_filter.active=True;p.enlarger.diffusion_filter.active=True
    sim=SimulationPipeline(digest_params(p));image=np.load(source);sim.process(image[:8,:8]);times=[]
    for _ in range(3):
        start=time.perf_counter();out=sim.process(image);times.append(time.perf_counter()-start)
    np.save(folder/f'{mode}-reference.npy',out);print(json.dumps(dict(process_seconds=times)));sys.exit()
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
    reference=run([sys.executable,__file__,'--worker',mode],'reference')
    rust=run([str(ROOT/'target/release/fast-spektrafilm'),'--threads','4','process',str(source),str(folder/f'{mode}-rust.npy'),'--config',str(path),'--repeat','3'],'rust')
    a=np.load(folder/f'{mode}-rust.npy');b=np.load(folder/f'{mode}-reference.npy')
    record=dict(mode=mode,shape=list(a.shape),threads=4,reference=reference,rust=rust,max_abs=float(abs(a-b).max()),rmse=float(np.sqrt(np.mean((a-b)**2))))
    results.append(record);print(json.dumps(record),flush=True)
(folder/'report.json').write_text(json.dumps(results,indent=2)+'\n')
