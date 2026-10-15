"""Save full evidence under artifacts, or summarize an archived Git snapshot."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib
ROOT=Path(__file__).resolve().parents[1]
ART=ROOT/'artifacts/numerical-review'
OUT=ART/'numerical-implementation-2026-10-15-evidence.json'


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
    record = isinstance(value,dict) and ('max_abs' in value or 'seconds' in value or 'url' in value)
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


def validation_summary(rows):
    assert all(row.get('valid', True) and row['finite'] and row['failures']==0 for row in rows)
    worst=max(rows,key=lambda row:row['max_abs'])
    result=dict(comparisons=len(rows),failures=0,
        worst={key:worst[key] for key in ('case','tap','max_abs','rmse','worst_index') if key in worst})
    if any('bitwise_equal' in row for row in rows):
        result['bitwise_equal']=sum(row.get('bitwise_equal',False) for row in rows)
    return result


def summarize_archive(commit):
    commit=subprocess.check_output(['git','rev-parse','--verify',commit+'^{commit}'],cwd=ROOT,text=True).strip()
    reports=[]
    for name in ('numerical-2026-10-14-evidence.json','numerical-implementation-2026-10-15-evidence.json'):
        path='docs/reviews/'+name
        raw=subprocess.check_output(['git','show',f'{commit}:{path}'],cwd=ROOT)
        archive=ART/'archive'/name
        archive.parent.mkdir(parents=True,exist_ok=True)
        archive.write_bytes(raw)
        reports.append(dict(url=f'https://github.com/dmitryprikhunov/fast-spektrafilm/blob/{commit}/{path}',
            sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw)))
    original,full=(read(ART/'archive'/name) for name in
        ('numerical-2026-10-14-evidence.json','numerical-implementation-2026-10-15-evidence.json'))
    validation=full['validation']
    cli=full['cli']
    keep=('cpu','system','rust_version','python_version','dependencies','thread_environment',
          'binary_sha256','cargo_lock_sha256','rust_sources_sha256','assets_manifest_sha256',
          'reference_manifest_sha256','numerical_contracts_sha256','source_sha256')
    payload=dict(reference_commit=cli['baseline_commit'],reviewed_commit=original['environment']['reviewed_commit'],
        archived_evidence_commit=commit,full_reports=reports,
        reproduction='See numerical-implementation-2026-10-15.md; full per-case diagnostics and raw repetitions stay under ignored artifacts/.',
        environment={key:cli[key] for key in keep},
        validation={key:validation[key] for key in ('core','gui','native_metadata','native_lensfun',
            'raw_preview','clippy','baseline_assets_verified')})
    payload['validation'].update(broad=validation_summary(validation['broad']['results']),
        interactions=validation_summary(validation['interactions']),
        pinned_fixture_recheck=validation_summary(validation['pinned_fixture_recheck']['results']),
        inverse_nodes={row['case']:row['max_abs'] for row in validation['interactions'] if row['case'].startswith('inverse_nodes')})
    payload['cli']=dict(threads=cli['threads'],warmups=cli['warmups'],measured_repeats=cli['measured_repeats'],
        release_profile=tomllib.loads(cli['binary_build']['cargo_toml'])['profile']['release'],
        release_build={key:value for key,value in cli['binary_build'].items() if key!='cargo_toml'},
        note='Seconds are median/minimum/maximum; RSS includes setup and retained buffers. Stage allocator results and analytical full-frame estimates are in the implementation report.',results=[])
    for row in cli['results']:
        payload['cli']['results'].append(dict(mode=row['mode'],shape=row['shape'],
            rust_seconds=row['rust_seconds'],reference_seconds=row['reference_seconds'],
            rust_peak_rss_kib=row['rust']['peak_rss_kib'],reference_peak_rss_kib=row['reference']['peak_rss_kib'],
            max_abs=row['port_error']['max_abs'],rmse=row['port_error']['rmse']))
    target=ROOT/'docs/reviews/numerical-2026-10-15-summary.json'
    target.write_text(compact_json(payload)+'\n')
    print(target,target.stat().st_size)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive-commit',help='Summarize the two full reports retained at this Git commit')
    args=parser.parse_args()
    if args.archive_commit:
        summarize_archive(args.archive_commit)
        return
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
