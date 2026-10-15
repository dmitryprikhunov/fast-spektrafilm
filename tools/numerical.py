"""Regression policies shared with Rust; invalid arrays never compare equal."""
from pathlib import Path
import json
import numpy as np
CONTRACTS = json.loads((Path(__file__).resolve().parents[1] / 'numerical-contracts.json').read_text())


def metrics(actual, reference, policy='strict'):
    a, b = np.asarray(actual), np.asarray(reference)
    valid = a.shape == b.shape and a.size > 0 and np.isfinite(a).all() and np.isfinite(b).all()
    if not valid:
        return dict(valid=False, finite=bool(np.isfinite(a).all() and np.isfinite(b).all()),
                    actual_shape=list(a.shape), reference_shape=list(b.shape))
    p = CONTRACTS[policy]
    error = abs(a - b)
    tolerance = p['atol'] + p['rtol'] * abs(b)
    with np.errstate(divide='ignore', invalid='ignore'):
        ratio = np.divide(error, tolerance, out=np.zeros_like(error), where=error != 0)
    idx = np.unravel_index(error.argmax(), error.shape)
    strict = error > CONTRACTS['strict']['atol'] + CONTRACTS['strict']['rtol'] * abs(b)
    return dict(valid=True, finite=True, shape=list(a.shape), policy=policy,
                max_abs=float(error.max()), rmse=float(np.linalg.norm(error.ravel()) / np.sqrt(a.size)),
                failures=int((error > tolerance).sum()), strict_failures=int(strict.sum()),
                max_tolerance_ratio=float(ratio.max()), worst_index=[int(i) for i in idx],
                actual=float(a[idx]), reference=float(b[idx]))


def require(actual, reference, label='', policy='strict', bitwise=False):
    report = metrics(actual, reference, policy)
    assert report['valid'] and report['failures'] == 0, (label, report)
    if bitwise:
        report['bitwise_equal'] = np.array_equal(np.asarray(actual, dtype=np.float64).view(np.uint64),
                                               np.asarray(reference, dtype=np.float64).view(np.uint64))
        assert report['bitwise_equal'], (label, report)
    return report
