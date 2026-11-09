from pathlib import Path
import json,subprocess,statistics,hashlib
import argparse
parser=argparse.ArgumentParser(description='Owned artifact retention A/A and AB/BA')
parser.add_argument('baseline',type=Path);parser.add_argument('candidate',type=Path);parser.add_argument('output',type=Path)
args=parser.parse_args();root=args.output;root.mkdir(parents=True,exist_ok=True)
binaries={'baseline':args.baseline.resolve(),'candidate':args.candidate.resolve()}
plan=['baseline','baseline','baseline','candidate','candidate','baseline','baseline','candidate','candidate','baseline'];runs=[]
for i,variant in enumerate(plan):
 binary=binaries[variant]
 result=subprocess.run([str(binary),'--artifact-retention'],capture_output=True,text=True,check=True)
 report=json.loads(result.stdout);(root/f'{i:02}-{variant}.json').write_text(result.stdout);runs.append({'variant':variant,'report':report})
 for row in report['rows']:assert all(s['allocator_delta_after_drop']==0 for s in row['samples'])
 print(i,variant,flush=True)
def value(run,count,metric):return statistics.median(next(x for x in run['report']['rows'] if x['controls']==count)['samples'][j][metric] for j in range(2))
rows=[]
for count in [8,32,96,256]:
 pair_ratios=[]
 for i in [2,4,6,8]:
  pair=runs[i:i+2];a=next(x for x in pair if x['variant']=='baseline');b=next(x for x in pair if x['variant']=='candidate');pair_ratios.append(value(b,count,'render_ms')/value(a,count,'render_ms'))
 rows.append({'controls':count,'baseline':{m:statistics.median(value(x,count,m) for x in runs[2:] if x['variant']=='baseline') for m in ['allocations','requested_bytes','retained_allocator_bytes','render_ms']},'candidate':{m:statistics.median(value(x,count,m) for x in runs[2:] if x['variant']=='candidate') for m in ['allocations','requested_bytes','retained_allocator_bytes','render_ms']},'paired_render_ratios':pair_ratios,'median_ratio':statistics.median(pair_ratios),'aa_render_ratio':value(runs[1],count,'render_ms')/value(runs[0],count,'render_ms')})
(root/'summary.json').write_text(json.dumps({'status':'partial','scope':'A/A plus four AB/BA pairs, instrumented release owned SceneArtifact render; net live allocator bytes exclude Document/allocator overhead/RSS; after-Drop delta zero','rows':rows,'binary_sha256':{v:hashlib.sha256(binaries[v].read_bytes()).hexdigest() for v in ['baseline','candidate']}},indent=2)+'\n')
