"""Extract immutable colour-science tables; never used by the Rust runtime.

All model algorithms are implemented in Rust. This development utility records
the exact reference's tabulated colour data instead of manually transcribing it.
"""
from pathlib import Path
import json
import sys
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
BASELINE = json.loads((ROOT / 'BASELINE.json').read_text())
from check_baseline import verify
verify(require_reference=True)
sys.path.insert(0, str(ROOT / '.reference' / BASELINE['commit'] / 'src'))
import colour
from spektrafilm.config import STANDARD_OBSERVER_CMFS
from spektrafilm.model.illuminants import standard_illuminant, available_illuminants
from spektrafilm.model.color_filters import custom_dichroic_filters, color_filter_transmittance, CameraColorFilters
from spektrafilm.utils.gamut_compression import inscribed_locus_hull, spectral_locus_xy
from spektrafilm.utils.spectral_upsampling import _illuminant_to_xy
from colour.models.oklab import MATRIX_1_XYZ_TO_LMS, MATRIX_2_LMS_TO_LAB
from colour.models.jzazbz import MATRIX_JZAZBZ_XYZ_TO_LMS, MATRIX_JZAZBZ_LMS_P_TO_IZAZBZ_SAFDAR2017

def convert(value):
    if isinstance(value, np.ndarray): return value.tolist()
    if isinstance(value, np.generic): return value.item()
    raise TypeError(type(value))

names = ['sRGB', 'ProPhoto RGB', 'ITU-R BT.2020', 'ACES2065-1', 'ACEScg', 'Display P3', 'Adobe RGB (1998)']
spaces = {}
for name in names:
    cs = colour.RGB_COLOURSPACES[name]
    spaces[name] = dict(to_xyz=cs.matrix_RGB_to_XYZ, from_xyz=cs.matrix_XYZ_to_RGB, white=cs.whitepoint)
illuminants = {}
for name in available_illuminants():
    if name.startswith('BB<'): continue
    illuminants[name] = dict(spectrum=standard_illuminant(name), xy=_illuminant_to_xy(name))
tables = dict(
    baseline_commit=BASELINE['commit'], colour_version=colour.__version__,
    cmfs=STANDARD_OBSERVER_CMFS.values, spaces=spaces, illuminants=illuminants,
    cat02=colour.adaptation.CAT_CAT02, cat16=colour.adaptation.CAT_CAT16,
    oklab_m1=MATRIX_1_XYZ_TO_LMS, oklab_m2=MATRIX_2_LMS_TO_LAB,
    jzazbz_m1=MATRIX_JZAZBZ_XYZ_TO_LMS, jzazbz_m2=MATRIX_JZAZBZ_LMS_P_TO_IZAZBZ_SAFDAR2017,
    dichroics=custom_dichroic_filters.filters,
    camera_filters={key:color_filter_transmittance(key) for key in [v.value for v in CameraColorFilters]},
    input_hull=inscribed_locus_hull(_illuminant_to_xy('D65'),5.0),
    locus=spectral_locus_xy(),
)
(ROOT / 'data' / 'reference_tables.json').write_text(json.dumps(tables,default=convert,separators=(',',':'))+'\n')
print('Extracted colour-science tables from', BASELINE['commit'])
