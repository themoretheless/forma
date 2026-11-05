"""Validate NativeHost completion-service ordering, not total OS wakeups."""
import argparse
import gzip
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('raw', type=Path)
parser.add_argument('output', type=Path)
args = parser.parse_args()
data = args.raw.read_bytes()
if args.raw.suffix == '.gz':
    data = gzip.decompress(data)
rows = [json.loads(line) for line in data.splitlines()]
assert rows[0]['event'] == 'start' and rows[-1]['event'] == 'stop'
assert not any(r['event'] == 'gpu_error' for r in rows)
health = [r for r in rows if r['event'] == 'gpu_health']
assert health and all(isinstance(r['idle'], bool) for r in health)
# Once idle, another health check requires intervening submitted/presented work.
idle = False
for row in rows:
    if row['event'] == 'present':
        idle = False
    elif row['event'] == 'gpu_health':
        assert not idle, 'completion polling continued after idle without a new frame'
        idle = row['idle']
intervals = []
start = None
for i, row in enumerate(rows):
    if row['event'] != 'occlusion':
        continue
    if row['occluded']:
        start = i
    elif start is not None:
        segment = rows[start+1:i]
        assert not any(r['event'] == 'present' for r in segment)
        intervals.append({'duration_ms': (row['at_us']-rows[start]['at_us'])/1000,
                          'health_checks': sum(r['event'] == 'gpu_health' for r in segment),
                          'idle_observed': any(r['event'] == 'gpu_health' and r['idle'] for r in segment)})
        start = None
assert any(r['idle_observed'] for r in intervals), 'no completion service observed while occluded'
models = [r['state']['rows'] for r in rows if r['event'] == 'model']
expected = [{'id': f'row-{i}', 'title': f'row-{i:03} — 你好 мир🙂'} for i in range(rows[0]['controls'])]
assert models and all(model == expected for model in models)
args.output.write_text(json.dumps({'status': 'partial', 'health_checks': len(health),
    'occluded_intervals': intervals, 'exact_model_unchanged': True,
    'limits': ['No total event-loop wakeup, power, RSS or latency measurement',
               'Physical device-removal notifications remain open',
               'Queue work need not still be executing when OS occlusion occurs; this proves scheduled completion service survives occlusion']}, indent=2)+'\n')
print('Validated completion polling cessation, occlusion ordering and unchanged model')
