from pathlib import Path
import subprocess,json,hashlib,statistics,time
import argparse
parser=argparse.ArgumentParser(description='Instrumented binding-sync AB/BA with baseline A/A controls')
parser.add_argument('baseline',type=Path);parser.add_argument('candidate',type=Path);parser.add_argument('output',type=Path)
parser.add_argument("--payload-records",type=int,default=0)
args=parser.parse_args();root=args.output.resolve().parent;out=args.output.resolve();out.mkdir(parents=True,exist_ok=True);runs=[]
binaries={'baseline':args.baseline.resolve(),'candidate':args.candidate.resolve()}
plan=['baseline','baseline','baseline','candidate','candidate','baseline','baseline','candidate','candidate','baseline']
for i,variant in enumerate(plan):
 binary=binaries[variant];start=time.time()
 result=subprocess.run([str(binary),'--retained','--lexical','--nested-layout',f'--payload-records={args.payload_records}'],capture_output=True,text=True,check=True)
 report=json.loads(result.stdout);assert report['nested_depth']==12 and len(report['results'])==8;assert report['payload_records']==args.payload_records
 path=out/f'layout-edit-paired-run-{i:02}-{variant}-2026-11-03.json';path.write_text(result.stdout)
 runs.append({'variant':variant,'path':str(path.relative_to(root)),'elapsed_s':time.time()-start,'report':report})
 print(f'{i}: {variant} finished',flush=True)
median=statistics.median
def metrics(run,key):
 row=next(r for r in run['report']['results'] if (r['svet'],r['controls'])==key)
 return {name:median(s[name] for s in row['samples']) for name in ['edit_ms','edit_allocations','edit_requested_bytes','idle_allocations']}
rows=[]
for key in [(s,c) for s in [False,True] for c in [8,32,96,256]]:
 baseline=[metrics(r,key) for r in runs[2:] if r['variant']=='baseline'];candidate=[metrics(r,key) for r in runs[2:] if r['variant']=='candidate']
 ratios=[]
 for i in [2,4,6,8]:
  pair=runs[i:i+2];a=metrics(next(r for r in pair if r['variant']=='baseline'),key);b=metrics(next(r for r in pair if r['variant']=='candidate'),key);ratios.append(b['edit_ms']/a['edit_ms'])
 rows.append({'svet':key[0],'controls':key[1],'baseline':{k:median(x[k] for x in baseline) for k in baseline[0]},'candidate':{k:median(x[k] for x in candidate) for k in candidate[0]},'paired_edit_ms_ratios':ratios,'median_ratio':median(ratios),'aa_edit_ms_ratio':metrics(runs[1],key)['edit_ms']/metrics(runs[0],key)['edit_ms']})
report={'status':'partial','scope':'Four AB/BA pairs plus baseline A/A; instrumented release CPU binding edit/rebuild, excludes painting/presentation','nested_depth':12,'payload_records':args.payload_records,'rows':rows,'runs':[{k:v for k,v in r.items() if k!='report'} for r in runs],'binary_sha256':{v:hashlib.sha256(binaries[v].read_bytes()).hexdigest() for v in ['baseline','candidate']},'limits':['Allocator instrumentation affects timings','Not application latency/frame qualification','No RSS/live/peak accounting','Only nested lexical list fixture']}
(out/'layout-edit-paired-qualification-2026-11-03.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(rows,indent=2),flush=True)
