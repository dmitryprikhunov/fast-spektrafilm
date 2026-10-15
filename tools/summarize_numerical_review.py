"""Commit compact evidence from completed validation/isolated benchmark runs."""
import json
from pathlib import Path
import re
ROOT=Path(__file__).resolve().parents[1]
ART=ROOT/'artifacts/numerical-review'
OUT=ROOT/'docs/reviews/numerical-implementation-2026-10-15-evidence.json'


def read(path): return json.loads(path.read_text())

def test_results(path):
    text=path.read_text()
    assert 'FAILED' not in text, path
    totals=re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored',text)
    assert totals,path
    return dict(passed=sum(int(a) for a,b in totals),ignored=sum(int(b) for a,b in totals))


def measured(rows):
    result=[]
    for row in rows:
        samples=row['measurements']['samples']
        keep={key:row[key] for key in ('revision','mode','cold_setup_seconds','peak_rss_kib')}
        keep['warmup']=samples[0]
        keep['measured']=samples[1:]
        keep['comparison']=row.get('comparison')
        result.append(keep)
    return result


def compact_json(value, level=0):
    """Keep metrics/samples on one line while preserving navigable containers."""
    scalar = not isinstance(value,(dict,list))
    vector = isinstance(value,list) and all(not isinstance(v,(dict,list)) for v in value)
    record = isinstance(value,dict) and ('max_abs' in value or 'seconds' in value)
    if scalar or vector or record:
        return json.dumps(value,separators=(',',':'),allow_nan=False)
    indent='  '*(level+1)
    if isinstance(value,dict):
        parts=[json.dumps(k)+': '+compact_json(v,level+1) for k,v in value.items()]
        opening,closing='{','}'
    else:
        parts=[compact_json(v,level+1) for v in value]
        opening,closing='[',']'
    if not parts:return opening+closing
    return opening+'\n'+indent+(',\n'+indent).join(parts)+'\n'+'  '*level+closing


def main():
    broad=read(ROOT/'artifacts/parity/report.json')
    assert len(broad['results'])==590
    assert all(r['valid'] and r['failures']==0 for r in broad['results'])
    revision=read(ART/'revisions/report.json')
    revision['results']=measured(revision['results'])
    cli=read(ART/'benchmark/report.json')
    assert len(cli['results'])==21
    interactions=[]
    text=(ART/'delivery-tests.log').read_text()
    for line in text.splitlines():
        m=re.search(r'((?:optical_|inverse_|blur_|convert_|grain_|scanner_|framing_)[a-zA-Z0-9_]+/[a-z_]+): (\{.*\})',line)
        if m:
            metrics=json.loads(m[2]);assert metrics['finite'] and metrics['failures']==0
            interactions.append(dict(case=m[1],**metrics))
    assert len(interactions)==129,len(interactions)
    validation=dict(
        core=test_results(ART/'core-final-stable.log'),
        gui=test_results(ART/'delivery-tests.log'),
        native_metadata=test_results(ART/'native-metadata.log'),
        native_lensfun=test_results(ART/'native-lensfun.log'),
        raw_preview=test_results(ART/'raw-preview.log'),
        clippy='Rust 1.93 Clippy, all targets + gui, -D warnings',
        baseline_assets_verified=235,
        timings_note='Validation timings are diagnostic and may overlap builds. Only the separate benchmark sections support timing claims.',
        broad=broad,interactions=interactions,
        pinned_fixture_recheck=read(ART/'pinned-fixtures.json'),
        probes=read(ART/'probes/report.json'),
        same_grid=read(ART/'probes/inverse_grid/report.json'))
    workflow={mode:read(ROOT/f'artifacts/workflow-benchmark/{mode}/report.json')
              for mode in ['forward','convert','diffusion']}
    preview=read(ART/'preview-batching/report.json')
    payload=dict(validation=validation,cli=cli,instrumented_revisions=revision,
                 revised_preview=preview,workspace_capacity=read(ART/'workspaces.json'),
                 gui_workflow=workflow)
    encoded=compact_json(payload)+'\n'
    assert json.loads(encoded)==payload
    OUT.write_text(encoded)
    print(OUT,OUT.stat().st_size)
    print('core/gui:',validation['core'],validation['gui'])
    for row in cli['results']:
        if row['shape'][0]==256:
            print(row['mode'],row['rust_seconds'],row['reference_seconds'],row['rust']['peak_rss_kib'])


if __name__=='__main__': main()
