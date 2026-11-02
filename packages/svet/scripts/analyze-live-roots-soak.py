"""Validate retained timed-soak accounting and summarize memory windows."""
import hashlib
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
p = json.loads(path.read_text())
assert p['protocol'] == 'timed-retention-soak-v1'
assert p['status'] == 'passed'
assert p['createdRoots'] == 80 + p['batches'] * 16
assert p['cleanups'] == p['aborts'] == p['createdRoots']
assert p['collectedObjects'] == p.get('objectsPerRoot',4) * p['createdRoots']
assert p['pendingObjects'] == 0
if p.get('scenario') == 'actions-scenes':
    assert p['actionCompleted'] == p['actionCancelled'] == p['actionAborts'] == p['createdRoots']
assert p['loadEndedMs'] >= p['durationMs']
assert len(p['samples']) <= 64
assert p['samples'][-1]['final']
for sample in p['samples']:
    assert sample['backlog'] <= 10000
    for i, core in enumerate(sample['cores']):
        assert core['capacity'] == p['baseline']['cores'][i]['capacity']
        assert core['wasmMemoryBytes'] == p['baseline']['cores'][i]['wasmMemoryBytes']
        assert core['pendingEffectCount'] == 0
for core in p['samples'][-1]['cores']:
    assert core['nodes'] == core['edges'] == 0
for window in p['windows']:
    for stat in window['memory'].values():
        assert stat['min'] <= stat['mean'] <= stat['max']
print(json.dumps({'status': 'passed', 'rawSha256': hashlib.sha256(path.read_bytes()).hexdigest(),
    'createdRoots': p['createdRoots'], 'collectedObjects': p['collectedObjects'],
    'maxBacklog': p['maxBacklog'], 'maxAgeMs': p['maxAgeMs'],
    'windowMeans': [{key: stat['mean'] for key, stat in w['memory'].items()} for w in p['windows']],
    'scope': 'Independent retained-report accounting validation, not a general plateau assertion.'}, indent=2))
