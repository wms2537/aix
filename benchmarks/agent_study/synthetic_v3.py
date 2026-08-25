#!/usr/bin/env python3
"""Deterministic smoke agents for the expanded live-agent harness.

perfect returns the independent reference shift for every truth-modeled cell.
sloppy leaves 10% of truth-visible shifts unchanged (seed 42), creating known
unguarded corruption that the guard must refuse.
"""
import json, random, sys
from pathlib import Path
HERE=Path(__file__).parent;ROOT=HERE.parents[1]
sys.path.insert(0,str(ROOT/'benchmarks'))
from shift_correctness_real import new_pos,num2col,ref_shift,norm

mode=sys.argv[1] if len(sys.argv)>1 else 'perfect'
tasks_path=Path(sys.argv[2]) if len(sys.argv)>2 else HERE/'tasks_v3.json'
out_path=Path(sys.argv[3]) if len(sys.argv)>3 else HERE/f'outputs_v3_{mode}.json'
payload=json.loads(tasks_path.read_text());out={};shiftable=[]
for t in payload['tasks']:
 key=f"{t['file']}#{t['operation']}@{t['at']}";cells={}
 for c in t['cells']:
  np_=new_pos(num2col(c['col']),c['row'],t['axis'],t['operation'],t['at'],t['count'])
  if np_ is None:cells[c['cell']]=c['formula'];continue
  exp=ref_shift(c['formula'],t['axis'],t['operation'],t['at'],t['count'],sheet=t['sheet'])
  cells[c['cell']]=exp if exp is not None else c['formula']
  if exp is not None and norm(exp)!=norm(c['formula']):shiftable.append((key,c['cell'],c['formula']))
 out[key]=cells
n_slop=0
if mode=='sloppy':
 rng=random.Random(42);n_slop=max(1,round(.10*len(shiftable)))
 for key,orig_a1,formula in rng.sample(shiftable,n_slop):out[key][orig_a1]=formula
out_path.write_text(json.dumps(out,indent=2)+'\n')
print(f'{mode}: {len(out)} tasks, {sum(map(len,out.values()))} cells, {len(shiftable)} truth-visible shifts, {n_slop} left unshifted')
