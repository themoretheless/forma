import hashlib,json,statistics,sys
from pathlib import Path
source=Path(sys.argv[1]);raw=json.loads(source.read_text())
assert raw['status']=='passed' and len(raw['rounds'])==12
result={'status':'passed','rawSha256':hashlib.sha256(source.read_bytes()).hexdigest(),'cases':{}}
for kind in ['small','repeated','varied']:
 rows=[r for r in raw['rounds'] if r['kind']==kind];assert len(rows)==4
 assert len({t['fixtureSha256'] for r in rows for t in r['trials']})==1
 out={'bytes':{},'medianMs':{},'sampledHeapMaxMiB':{},'sampledRssMaxMiB':{},'pairedPercent':{}}
 for row in rows:
  assert len(row['trials'])==6 and row['order']==[t['mode'] for t in row['trials']]
  assert row['order'][-2:]==['plain','plain']
 for mode in ['plain','codec','gzip','codec-gzip']:
  trials=[t for r in rows for t in r['trials'] if t['mode']==mode]
  assert len(trials)==(12 if mode=='plain' else 4) and all(len(t['times'])==12 and all(x>0 for x in t['times']) for t in trials)
  sizes={t['bytes'] for t in trials};assert len(sizes)==1
  out['bytes'][mode]=sizes.pop();out['medianMs'][mode]=statistics.median(statistics.median(t['times']) for t in trials)
  out['sampledHeapMaxMiB'][mode]=max(t['sampledHeapMax'] for t in trials)/1048576
  out['sampledRssMaxMiB'][mode]=max(t['sampledRssMax'] for t in trials)/1048576
 for label,x,y in [('codec/plain','plain','codec'),('codec-gzip/gzip','gzip','codec-gzip'),('A/A','plain','plain')]:
  ratios=[]
  for row in rows:
   if label=='A/A':left,right=row['trials'][-2:]
   else:left=next(t for t in row['trials'] if t['mode']==x);right=next(t for t in row['trials'] if t['mode']==y)
   ratios.append((statistics.median(right['times'])/statistics.median(left['times'])-1)*100)
  out['pairedPercent'][label]=ratios
 result['cases'][kind]=out
text=json.dumps(result,indent=2)+'\n'
if len(sys.argv)>2:Path(sys.argv[2]).write_text(text)
else:print(text)
