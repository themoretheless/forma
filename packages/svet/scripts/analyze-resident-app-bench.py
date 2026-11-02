"""Independently validate and summarize retained app benchmark JSON (stdlib only)."""
import json
import math
import statistics
import sys
from pathlib import Path


def quantile(values, fraction):
    ordered = sorted(values)
    index = (len(ordered) - 1) * fraction
    lower = math.floor(index)
    return ordered[lower] + (ordered[math.ceil(index)] - ordered[lower]) * (index - lower)


def validate(row):
    frames = row['frames']
    count = row['sampleCount']
    assert len(frames) == count
    assert [f['frame'] for f in frames] == list(range(count))
    for key, fraction in [('p50', .5), ('p95', .95), ('p99', .99)]:
        assert math.isclose(quantile([f['updateMs'] for f in frames], fraction), row['update'][key], abs_tol=1e-8)
    assert all(math.isfinite(f['updateMs']) and f['updateMs'] >= 0 for f in frames)
    resident = row['mode'] == 'resident'
    counters = row['counters']
    assert counters['uploadedBytes'] == count * (16 if resident else 3 * row['bytes'] + 16)
    assert counters['readbackBytes'] == (0 if resident else count * row['bytes'])
    assert counters['mapCalls'] == (0 if resident else count)
    assert counters['submits'] == 2 * count
    assert counters['buffersCreated'] == counters['buffersDestroyed']
    assert counters['bufferBytesCreated'] == counters['bufferBytesDestroyed']
    assert row['quality']['referenceMaxAbsError'] < 2e-5
    assert not row.get('failure') and not row.get('cleanupFailure')
    assert row['measuredStart'] < row['measuredEnd'] <= row['teardownAt']


path = Path(sys.argv[1])
report = json.loads(path.read_text())
assert report['status'] == 'passed'
assert report['protocol'] == 'interleaved-controls-v2'
assert not report['failedRuns'] and not report['uncapturedErrors']
assert len(report['calibration']) == 2
assert len(report['workloads']) == 2 * report['rounds']
assert [p['afterPair'] for p in report['controls']] == list(range(0, report['rounds'], 2))
rows = report['calibration'][:]
for pair in report['workloads'] + report['controls']:
    names = ['mapped', 'resident'] if 'mapped' in pair else ['a1', 'a2']
    a, b = [pair[n] for n in names]
    rows.extend([a, b])
    for key in ['geometrySha256', 'pixelSha256']:
        assert a['quality'][key] == b['quality'][key]
    for key in ['p50', 'p95']:
        change = 100 * (b['update'][key] / a['update'][key] - 1)
        assert math.isclose(change, pair['comparison'][key + 'ChangePercent'], abs_tol=1e-8)
for row in rows:
    validate(row)
ordered = sorted(rows, key=lambda r: r['setupStart'])
assert all(a['teardownAt'] <= b['setupStart'] for a, b in zip(ordered, ordered[1:]))
summary = {'source': str(path), 'validatedRows': len(rows), 'measuredFrames': sum(len(r['frames']) for r in rows), 'workloads': [], 'controls': [p['comparison'] for p in report['controls']], 'longTasks': len(report['longTasks'])}
for vertices in sorted({p['vertices'] for p in report['workloads']}):
    pairs = [p for p in report['workloads'] if p['vertices'] == vertices]
    item = {'vertices': vertices, 'pairChanges': [p['comparison'] for p in pairs]}
    for mode in ['mapped', 'resident']:
        selected = [p[mode] for p in pairs]
        item[mode] = {'medianP50Ms': statistics.median(r['update']['p50'] for r in selected), 'medianP95Ms': statistics.median(r['update']['p95'] for r in selected), 'liveAfterMeasure': selected[0]['liveAfterMeasure'], 'peakBufferBytes': selected[0]['peakBufferBytes']}
    summary['workloads'].append(item)
print(json.dumps(summary, indent=2))
