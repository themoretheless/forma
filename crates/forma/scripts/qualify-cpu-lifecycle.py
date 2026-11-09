"""Summarize sustained CPU allocator journals without making an RSS claim."""
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('raw', type=Path)
parser.add_argument('output', type=Path)
args = parser.parse_args()
raw = json.loads(args.raw.read_text())
assert len(raw['rows']) == 8
seen = set()
rows = []
for row in raw['rows']:
    key = (row['svet'], row['controls'])
    assert key not in seen
    seen.add(key)
    assert row['controls'] in [8, 32, 96, 256]
    assert row['cycles_per_lifecycle'] == 32 and len(row['samples']) == 3
    trajectories = []
    for sample in row['samples']:
        assert sample['after_drop_delta'] == 0
        live = sample['live_bytes_by_edit']
        assert len(live) == 32 and all(x > 0 for x in live)
        assert sample['peak_bytes'] >= max(live)
        assert sample['requested_bytes'] >= sample['peak_bytes']
        late = live[16:]
        assert len(set(late)) == 1, "fixed-width warmed workload retained growing allocator state"
        trajectories.append({
            'late_min_bytes': min(late), 'late_max_bytes': max(late),
            'late_spread_bytes': max(late)-min(late),
            'last_minus_midpoint_bytes': live[-1]-live[15],
            'retained_bytes': sample['retained_bytes'],
            'peak_bytes': sample['peak_bytes'],
            'after_drop_delta': sample['after_drop_delta'],
        })
    assert len({s['retained_bytes'] for s in trajectories}) == 1
    rows.append({'svet': key[0], 'controls': key[1], 'samples': trajectories})
assert seen == {(s, n) for s in [False, True] for n in [8, 32, 96, 256]}
args.output.write_text(json.dumps({
    'status': 'partial', 'scope': raw['scope'], 'accounting': raw['accounting'],
    'rows': rows,
    'limits': ['A bounded synthetic workload; no universal leak-freedom claim',
               'No process RSS, GPU memory or native presentation measurement',
               'Exact late-series plateau is gated only for this fixed-width 32-edit workload'],
}, indent=2)+'\n')
print('Validated 24 lifecycles; live trajectories summarized separately from after-Drop checks')
