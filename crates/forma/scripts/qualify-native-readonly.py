"""Validate fixture model journals; IME event gates are explicit, never inferred."""
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('journal', type=Path)
parser.add_argument('--require-ime', action='store_true')
parser.add_argument('--require-cpu-gpu', action='store_true')
args = parser.parse_args()
rows = [json.loads(line) for line in args.journal.read_text().splitlines() if line.strip()]
starts = [i for i, row in enumerate(rows) if row.get('event') == 'start']
if starts:
    rows = rows[starts[-1]:]
errors = []
updates = [row for row in rows if row.get('event') == 'update']
if not updates:
    errors.append('No model updates')
else:
    reference = updates[0]['values'][0]
    if any(row['values'][0] != reference or row['readonly'] != [True, False] for row in updates):
        errors.append('Readonly reference or eligibility changed')
    if not any(row['selected'] == reference for row in updates):
        errors.append('Readonly full selection not observed')
    if not any(row['focus'] == 1 and row['values'][1] != 'Editable' for row in updates):
        errors.append('Editable sibling mutation not observed')
if any(row.get('event') == 'gpu_error' for row in rows):
    errors.append('GPU error observed')
renderers = sorted({row['renderer'] for row in rows if row.get('event') == 'present'})
if args.require_cpu_gpu and renderers != ['Cpu', 'Gpu']:
    errors.append('Both CPU and GPU presentations required')
ime = [row.get('value', '') for row in rows if row.get('event') == 'ime']
if args.require_ime and (not any(v.startswith('Preedit(') and '""' not in v for v in ime)
                         or not any(v.startswith('Commit(') for v in ime)):
    errors.append('Actual nonempty IME Preedit and Commit events required; ordinary text is insufficient')
print(json.dumps({'status': 'failed' if errors else 'passed-scoped', 'errors': errors,
                  'updates': len(updates), 'renderers': renderers, 'imeEvents': ime,
                  'scope': 'Model/event journal only; UI actions, OS source and visual output need separate evidence'}, indent=2))
raise SystemExit(bool(errors))
