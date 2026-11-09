"""Pair release dense_raster_bench binaries; retain all samples and hashes."""
from pathlib import Path
import argparse,subprocess,json,statistics,hashlib
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('baseline',type=Path);parser.add_argument('candidate',type=Path);parser.add_argument('output',type=Path)
parser.add_argument("--background",action="store_true")
args=parser.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
binaries={'baseline':args.baseline.resolve(),'candidate':args.candidate.resolve()}
plan=['baseline','baseline','baseline','candidate','candidate','baseline','baseline','candidate','candidate','baseline'];runs=[]
for index,variant in enumerate(plan):
 result=subprocess.run([str(binaries[variant])]+(["--background"] if args.background else []),capture_output=True,text=True,check=True)
 report=json.loads(result.stdout);assert len(report['rows'])==8;assert report['background']==args.background
 (out/f'{index:02}-{variant}.json').write_text(result.stdout);runs.append({'variant':variant,'report':report})
 print(index,variant,flush=True)
def row(run,key):
 return next(x for x in run['report']['rows'] if (x['scale'],x['name'])==key)
rows=[]
for key in [(scale,name) for scale in [1,2] for name in ['full','geometry','mixed','nested']]:
 for phase in ['cold','repaint']:
  ratios=[]
  for index in [2,4,6,8]:
   pair=runs[index:index+2]
   a=row(next(x for x in pair if x['variant']=='baseline'),key)[phase]
   b=row(next(x for x in pair if x['variant']=='candidate'),key)[phase]
   ratios.append(b['p50_ms']/a['p50_ms'])
  med=lambda variant,metric:statistics.median(row(x,key)[phase][metric] for x in runs[2:] if x['variant']==variant)
  rows.append({'scale':key[0],'name':key[1],'phase':phase,'baseline':{m:med('baseline',m) for m in ['p50_ms','p95_ms']},'candidate':{m:med('candidate',m) for m in ['p50_ms','p95_ms']},'paired_p50_ratios':ratios,'median_p50_ratio':statistics.median(ratios),'aa_p50_ratio':row(runs[1],key)[phase]['p50_ms']/row(runs[0],key)[phase]['p50_ms']})
report={'status':'partial','background':args.background,'scope':'A/A+four AB/BA release dense public Button pixel pairs (background selects pixels vs content_pixels); cold build and tick/repaint/RGBA copy, no allocator instrumentation; one host; no OS presentation, input latency or RSS claim','rows':rows,'binary_sha256':{name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in binaries.items()}}
(out/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
