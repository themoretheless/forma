"""Validate synthetic native_form_probe OS journals; summarize observed cadence."""
from pathlib import Path
import argparse,json,statistics,gzip
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('root',type=Path,help='Directory with8/32/96/256/events.jsonl')
parser.add_argument('output',type=Path)
parser.add_argument('--counts',nargs='+',type=int,choices=[8,32,96,256],default=[8,32,96,256])
parser.add_argument('--extra-roundtrips-256',type=int,default=3)
args=parser.parse_args()
assert args.extra_roundtrips_256>=0
def stats(values):
 assert values
 values=sorted(values)
 return {'samples':len(values),'p50':statistics.median(values),'p95':values[min(len(values)-1,len(values)*95//100)],'max':max(values)}
rows=[]
for count in args.counts:
 path=args.root/str(count)/'events.jsonl'
 raw=path.read_text() if path.exists() else gzip.decompress(path.with_suffix('.jsonl.gz').read_bytes()).decode()
 records=[json.loads(line) for line in raw.splitlines()]
 start=records[0];assert start['event']=='start' and start['controls']==count
 models=[x for x in records if x['event']=='model'];presents=[x for x in records if x['event']=='present']
 assert models and presents and not any(x['event']=='gpu_error' for x in records)
 transitions=[];segments=[]
 for frame in presents:
  if not transitions or transitions[-1]!=frame['renderer']:
   transitions.append(frame['renderer']);segments.append({'renderer':frame['renderer'],'first_at_us':frame['at_us'],'first_peak_rss_bytes':frame['peak_rss_bytes'],'max_peak_rss_bytes':0,'frames':0})
  segments[-1]['frames']+=1
  segments[-1]['max_peak_rss_bytes']=max(segments[-1]['max_peak_rss_bytes'],frame['peak_rss_bytes'] or 0)
 assert transitions[:3]==['Gpu','Cpu','Gpu'] and transitions[-1]=='Gpu'
 assert len(transitions)==(3+2*args.extra_roundtrips_256 if count==256 else 3)
 assert all(a['at_us']<=b['at_us'] for a,b in zip(records,records[1:]) if 'at_us' in a and 'at_us' in b)
 initial=models[0]['state']['rows'];final=models[-1]['state']['rows']
 assert len(initial)==len(final)==count
 expected_initial=[{'id':f'row-{i}','title':f'row-{i:03} — 你好 мир🙂'} for i in range(count)]
 assert initial==expected_initial
 expected_final=[dict(x) for x in expected_initial]
 expected_final[0]['title']=f'CPU-{count} — 你好 мир🙂'
 expected_final[-1]['title']=f'LAST-{count} — 你好 мир🙂 — GPU-return'
 assert final==expected_final
 for model in models:
  assert len(model['state']['rows'])==count
  assert model['state']['rows'][1:-1]==expected_initial[1:-1]
 assert any(x['state']['rows'][0]['title']==f'GPU-{count} — 你好 мир🙂' for x in models)
 assert any(x['state']['rows'][-1]['title']==f'LAST-{count} — 你好 мир🙂' for x in models)
 max_scroll=max(x['scroll'][1] for x in models)
 assert max_scroll==max(0,count*38-6-388) and models[-1]['scroll']==[0.,0.]
 assert any(x['renderer']=='Cpu' and x['focused_index']==count-1 for x in presents)
 assert any(x['renderer']=='Gpu' and x['focused_index']==count-1 for x in presents)
 frame_stats={}
 for renderer in ['Cpu','Gpu']:
  intervals=[b['interval_us']/1000 for a,b in zip(presents,presents[1:])
   if a['renderer']==b['renderer']==renderer and a['focused_index']==b['focused_index']
   and a['scroll']==b['scroll'] and a['animating'] and b['animating']]
  frame_stats[renderer]=stats(intervals)
  assert frame_stats[renderer]['samples']>=30
 rows.append({'controls':count,'load_ms':start['load_us']/1000,'first_present_ms':presents[0]['at_us']/1000,
  'presentation_count':len(presents),'renderer_transitions':transitions,'segments':segments,
  'max_scroll_y':max_scroll,'final_scroll':models[-1]['scroll'],'final_model_exact':True,
  'frame_interval_ms':frame_stats,'peak_rss_bytes':max(x['peak_rss_bytes'] or 0 for x in presents),
  'changed_model_update_ms':[x['update_us']/1000 for x in models if x['sync_changed']],
  'gpu_errors':0})
report={'status':'partial','validated':f'Actual OS NativeHost{args.counts} synthetic mixed-font editor forms, CPU/GPU presents, top/bottom scroll and bound model edits; extra{args.extra_roundtrips_256} roundtrips on256',
 'interval_selection':'Adjacent presents with identical renderer/focus/scroll and animating=true in both; no duration cutoff; includes input/journaling overhead. Other intervals remain in raw logs.',
 'units':'Frame intervals/update/load in ms. macOS getrusage ru_maxrss is process peak bytes; includes CPU/GPU host allocations.',
 'rows':rows,'limits':['One release run per form size on one macOS machine; no A/A or baseline comparison','Callback follows CPU buffer.present/GPU frame.present; does not prove display scanout or GPU completion','No OS input-to-display latency claim; update_ms includes host input callback and binding sync only','Process peak RSS is not current live memory, GPU-resident accounting or leak proof','Default Svet scheduler only; other schedulers/platforms and true CJK IME remain open']}
args.output.write_text(json.dumps(report,indent=2)+'\n')
print(f'Validated {len(rows)} OS journals; broader qualification remains partial')
