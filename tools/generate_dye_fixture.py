"""The pinned GUI's dye-spectrum permutation, before parameter digestion."""
import os
os.environ.update(NUMBA_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',OMP_NUM_THREADS='1')
import numpy as np
from validate import ROOT, init_params, digest_params, SimulationPipeline
root=ROOT/'tests/fixtures/core'
q=init_params(); q.camera.auto_exposure=False
q.debug.deactivate_spatial_effects=True; q.debug.deactivate_stochastic_effects=True
q.film.data.channel_density=q.film.data.channel_density[:,(0,2,1)]
q.print.data.channel_density=q.print.data.channel_density[:,(2,0,1)]
np.save(root/'dye_swap.npy', SimulationPipeline(digest_params(q)).process(np.load(root/'input.npy'), collect='rgb_out'))
