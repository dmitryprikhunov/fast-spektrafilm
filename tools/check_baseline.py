"""Verify copied/derived assets and, when present, every pinned Python module."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def verify(require_reference=False):
    baseline = json.loads((ROOT / 'BASELINE.json').read_text())
    manifest = json.loads((ROOT / 'REFERENCE_MANIFEST.json').read_text())
    assert baseline['commit'] == manifest['baseline_commit']
    for item in baseline['data_files']:
        path = ROOT / item['path']
        assert path.stat().st_size == item['bytes'] and digest(path) == item['sha256'], f'Copied asset changed: {path}'
    for name, expected in manifest['derived_assets'].items():
        assert digest(ROOT / name) == expected, f'Derived asset changed: {name}'
    reference = ROOT / '.reference' / baseline['commit']
    if require_reference or reference.exists():
        actual = {str(p.relative_to(reference)) for p in (reference / 'src').rglob('*.py')}
        assert actual == set(manifest['reference_python']), 'Reference module set changed'
        for name, expected in manifest['reference_python'].items():
            assert digest(reference / name) == expected, f'Pinned reference changed: {name}'
    review = ROOT/'tests/fixtures/review'
    if (review/'manifest.json').exists():
        fixtures=json.loads((review/'manifest.json').read_text())
        assert fixtures['baseline_commit']==baseline['commit']
        assert set(fixtures['files'])=={str(p.relative_to(review)) for p in review.rglob('*') if p.is_file() and p!=review/'manifest.json'}, 'Review fixture set changed'
        for name, expected in fixtures['files'].items():
            assert digest(review/name)==expected, f'Review fixture changed: {name}'
    crops=ROOT/'tests/fixtures/raw_crops'
    if (crops/'manifest.json').exists():
        for item in json.loads((crops/'manifest.json').read_text())['crops']:
            assert digest(crops/item['path'])==item['sha256'], f'Cached RAW crop changed: {item}'
    return baseline

if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-reference', action='store_true')
    b = verify(require_reference=parser.parse_args().require_reference)
    print(f"Verified {len(b['data_files'])} copied assets, derived tables and any extracted reference code")
