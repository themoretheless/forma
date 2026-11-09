from pathlib import Path
import argparse,subprocess,json,statistics,hashlib
parser=argparse.ArgumentParser(description='Instrumented CPU frame A/A and AB/BA release probes')
parser.add_argument('baseline',type=Path);parser.add_argument('candidate',type=Path);parser.add_argument('output',type=Path)
args=parser.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
binaries={'baseline':args.baseline.resolve(),'candidate':args.candidate.resolve()}
plan=['baseline','baseline','baseline','candidate','candidate','baseline','baseline','candidate','candidate','baseline'];runs=[]
for index,variant in enumerate(plan):
 result=subprocess.run([str(binaries[variant]),'--cpu-frame'],capture_output=True,text=True,check=True)
 report=json.loads(result.stdout);assert len(report['rows'])==6
 (out/f'{index:02}-{variant}.json').write_text(result.stdout);runs.append({'variant':variant,'report':report})
 print(index,variant,flush=True)
def metrics(run,key):
 row=next(x for x in run['report']['rows'] if (x['scale'],x['focused'])==key)
 return {field:statistics.median(x[field] for x in row['samples']) for field in ['ms','allocations','requested_bytes']}
rows=[]
for key in [(scale,focused) for scale in [1,1.25,2] for focused in [False,True]]:
 ratios=[]
 for index in [2,4,6,8]:
  pair=runs[index:index+2];a=metrics(next(x for x in pair if x['variant']=='baseline'),key);b=metrics(next(x for x in pair if x['variant']=='candidate'),key);ratios.append(b['ms']/a['ms'])
 baseline=[metrics(x,key) for x in runs[2:] if x['variant']=='baseline'];candidate=[metrics(x,key) for x in runs[2:] if x['variant']=='candidate']
 rows.append({'scale':key[0],'focused':key[1],'baseline':{field:statistics.median(x[field] for x in baseline) for field in baseline[0]},'candidate':{field:statistics.median(x[field] for x in candidate) for field in candidate[0]},'paired_time_ratios':ratios,'median_time_ratio':statistics.median(ratios),'aa_time_ratio':metrics(runs[1],key)['ms']/metrics(runs[0],key)['ms']})
report={'status':'partial','scope':'A/A+four AB/BA pairs of allocator-instrumented release tick/binding sync/paint_native; no OS presentation/input latency or RSS claim','rows':rows,'binary_sha256':{name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in binaries.items()}}
(out/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
