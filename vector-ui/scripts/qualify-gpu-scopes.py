"""Collect baseline A/A and four alternating GPU scope benchmark pairs."""
import argparse
import json
import math
import hashlib
import statistics
import subprocess
from pathlib import Path

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('baseline',type=Path)
parser.add_argument('candidate',type=Path)
parser.add_argument('output',type=Path)
parser.add_argument('--summarize-only',action='store_true')
args=parser.parse_args()
args.output.mkdir(parents=True,exist_ok=True)
if not args.summarize_only:
    assert hashlib.sha256(args.baseline.read_bytes()).digest()!=hashlib.sha256(args.candidate.read_bytes()).digest(), 'baseline and candidate binaries are identical; reject comparison'
def run(binary,name):
    if args.summarize_only:
        return json.loads((args.output/f'{name}.json').read_text())
    result=subprocess.run([str(binary)],capture_output=True,text=True,check=True)
    (args.output/f'{name}.stderr.txt').write_text(result.stderr)
    raw=json.loads(result.stdout)
    raw['benchmark_binary_sha256']=hashlib.sha256(binary.read_bytes()).hexdigest()
    assert raw['status']=='partial' and len(raw['samples'])==8*raw.get('measured_samples_per_group',12)
    (args.output/f'{name}.json').write_text(json.dumps(raw,indent=2)+'\n')
    return raw

aa=[run(args.baseline,f'aa-{i}') for i in range(2)]
pairs=[]
for i in range(4):
    order=['baseline','candidate'] if i%2==0 else ['candidate','baseline']
    pair={}
    for label in order:
        pair[label]=run(getattr(args,label),f'pair-{i}-{label}')
    pairs.append(pair)
def median(raw,n,mode,field,parity=None):
    raw_field=field.removeprefix('p95_')
    values=[r[raw_field] for r in raw['samples'] if r['scopes']==n and r['explicit_wait']==mode and (parity is None or (r['round']+raw.get('warmups',4))%2==parity)]
    expected=raw.get('measured_samples_per_group',12)
    assert len(values)==expected if parity is None else len(values) in [expected//2,(expected+1)//2]
    return sorted(values)[math.ceil(.95*len(values))-1] if field.startswith('p95_') else statistics.median(values)
rows=[]
for n in [1,8,32,64]:
    for mode in [False,True]:
        fields={}
        for field in ['wait_ms','total_ms','p95_wait_ms','p95_total_ms']:
            ratios=[median(p['candidate'],n,mode,field)/median(p['baseline'],n,mode,field) for p in pairs]
            fields[field]={'baseline_ms':statistics.median(median(p['baseline'],n,mode,field) for p in pairs),
                'candidate_ms':statistics.median(median(p['candidate'],n,mode,field) for p in pairs),
                'paired_ratio':statistics.median(ratios),'raw_pair_ratios':ratios,
                'baseline_aa_ratio':median(aa[1],n,mode,field)/median(aa[0],n,mode,field)}
        rows.append({'scopes':n,'explicit_wait':mode,'metrics':fields})
# Fixed acceptance thresholds for this protocol; retained even if a control fails.
trials=aa+[p[label] for p in pairs for label in ['baseline','candidate']]
checks={
    'distinct_recorded_binaries':bool(aa[0].get('benchmark_binary_sha256')) and aa[0].get('benchmark_binary_sha256')==aa[1].get('benchmark_binary_sha256') and all(p['baseline'].get('benchmark_binary_sha256')==aa[0]['benchmark_binary_sha256'] and p['candidate'].get('benchmark_binary_sha256')==pairs[0]['candidate'].get('benchmark_binary_sha256') and p['candidate'].get('benchmark_binary_sha256')!=aa[0]['benchmark_binary_sha256'] for p in pairs),
    'same_protocol':all((r.get('measured_samples_per_group',12),r.get('warmups',4),r['buffer_bytes'],r['adapter'],r['backend'])==(aa[0].get('measured_samples_per_group',12),aa[0].get('warmups',4),aa[0]['buffer_bytes'],aa[0]['adapter'],aa[0]['backend']) for r in trials),
    'independent_mode_backends':all(r.get('independent_mode_backends',False) for r in aa+[p[label] for p in pairs for label in ['baseline','candidate']]),
    'control_aa_within15pct':all(.85<=r['metrics']['total_ms']['baseline_aa_ratio']<=1.15 for r in rows if r['explicit_wait']),
    'async_aa_within20pct':all(.8<=r['metrics']['total_ms']['baseline_aa_ratio']<=1.2 for r in rows if not r['explicit_wait']),
    'unchanged_control_within15pct':all(.85<=r['metrics']['total_ms']['paired_ratio']<=1.15 for r in rows if r['explicit_wait']),
    'async_no_p50_total_regression_over5pct':all(r['metrics']['total_ms']['paired_ratio']<=1.05 for r in rows if not r['explicit_wait']),
}
strata=[]
for parity in [0,1]:
    for n in [1,8,32,64]:
        for mode in [False,True]:
            ratios=[median(p['candidate'],n,mode,'total_ms',parity)/median(p['baseline'],n,mode,'total_ms',parity) for p in pairs]
            strata.append({'source_round_parity':parity,'scopes':n,'explicit_wait':mode,
                'baseline_p50_total_ms':statistics.median(median(p['baseline'],n,mode,'total_ms',parity) for p in pairs),
                'candidate_p50_total_ms':statistics.median(median(p['candidate'],n,mode,'total_ms',parity) for p in pairs),
                'paired_ratio':statistics.median(ratios),
                'baseline_aa_ratio':median(aa[1],n,mode,'total_ms',parity)/median(aa[0],n,mode,'total_ms',parity)})
strata_checks={
    'all_samples_partitioned':True,
    'control_aa_within15pct':all(.85<=r['baseline_aa_ratio']<=1.15 for r in strata if r['explicit_wait']),
    'async_aa_within20pct':all(.8<=r['baseline_aa_ratio']<=1.2 for r in strata if not r['explicit_wait']),
    'unchanged_control_within15pct':all(.85<=r['paired_ratio']<=1.15 for r in strata if r['explicit_wait']),
    'async_no_p50_total_regression_over5pct':all(r['paired_ratio']<=1.05 for r in strata if not r['explicit_wait']),
}
summary={'status':'partial','adapter':aa[0]['adapter'],'backend':aa[0]['backend'],
    'protocol':f"Baseline A/A +4 pairs, AB/BA/AB/BA;{aa[0].get('measured_samples_per_group',12)} measured samples/group/process after{aa[0].get('warmups',4)} warmups;1MiB exact clear output each operation",
    'checks':checks,
    'performance_gate':'passed_for_this_workload' if all(checks.values()) else 'inconclusive_control_or_regression_gate',
    'p95_note':'Nearest-rank p95 per process/group, then median across pairs; descriptive tail statistic only',
    'order_strata':strata,'order_strata_checks':strata_checks,
    'order_strata_gate':'passed_for_both_orders' if all(strata_checks.values()) and checks['same_protocol'] and checks['distinct_recorded_binaries'] and checks['independent_mode_backends'] else 'inconclusive',
    'rows':rows,'limits':['Single machine/driver; descriptive paired CPU wall times, not universal optimization qualification',
    'Explicit wait is an unchanged blocking control; async wait includes completion scheduling',
    'No OS input/presentation, GPU timestamps, thread sampling, RSS or energy measurement']}
(args.output/'qualification.json').write_text(json.dumps(summary,indent=2)+'\n')
print(('Summarized10 existing runs' if args.summarize_only else 'Collected10 real-GPU runs with exact clear output assertions')+'; inspect controls and A/A before claiming speedups')
