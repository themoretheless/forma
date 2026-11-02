"""Inspect strong inbound paths to owner targets in measure-live-roots WeakRef probes.

Usage: python3 inspect-live-root-retainers.py <V8 heapsnapshot> [probe-kind=owner]
Snapshot creation itself changes GC/VM state; paths are observations, not proof
of the preceding GC failure's cause. WeakRef keep-during-job roots are reported.
"""
import json,sys,collections
p=json.load(open(sys.argv[1]));meta=p['snapshot']['meta'];ns=p['nodes'];es=p['edges'];strings=p['strings'];nf=len(meta['node_fields']);ef=len(meta['edge_fields']);nt=meta['node_types'][0];et=meta['edge_types'][0]
assert meta['node_fields'][:5]==['type','name','id','self_size','edge_count']
assert meta['edge_fields']==['type','name_or_index','to_node']
count=len(ns)//nf;out=[[] for _ in range(count)];rev=[[] for _ in range(count)];offset=0
for source in range(count):
 for i in range(ns[source*nf+4]):
  typ,name,target=es[offset:offset+ef];offset+=ef;target//=nf
  label=str(name) if et[typ] in ['element','hidden'] else strings[name]
  edge=(target,et[typ],label);out[source].append(edge)
  if et[typ]!='weak':rev[target].append((source,et[typ],label))
def desc(i):return {'index':i,'type':nt[ns[i*nf]],'name':strings[ns[i*nf+1]],'heapId':ns[i*nf+2]}
targets=[];target_kind=sys.argv[2] if len(sys.argv)>2 else 'owner'
for i in range(count):
 props={label:target for target,typ,label in out[i] if typ=='property'}
 if 'kind' in props and strings[ns[props['kind']*nf+1]]==target_kind and 'ref' in props:
  for target,typ,label in out[props['ref']]:
   if typ=='weak':targets.append(target)
print(target_kind,'targets',len(targets))
for target in targets:
 print('TARGET',desc(target));print('INBOUND',[(desc(s),t,n) for s,t,n in rev[target]])
 queue=collections.deque([(target,[])]);visited={target};found=0
 while queue and found<3:
  node,path=queue.popleft()
  if node==0 or (nt[ns[node*nf]]=='synthetic' and node!=target):
   print('ROOTPATH',json.dumps([desc(node)]+path));found+=1;continue
  if len(path)>14:continue
  for source,typ,label in rev[node]:
   if source not in visited:
    visited.add(source);queue.append((source,[{'edge':label,'edgeType':typ,**desc(node)}]+path))
