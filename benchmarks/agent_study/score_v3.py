#!/usr/bin/env python3
"""Scorer for expanded guarded-vs-unguarded live-agent tasks (v3).

The artifact is built by openpyxl's structural edit plus zip surgery that splices
only agent-supplied formula bodies. Truth comes from the independent reference
shifter. The guard is the engine-free graph checker, generalized to row/column
insert/delete. Deleted-band hosts are skipped; deleted dependencies make the
guard fail closed and truth skip that cell.
"""
import json, os, re, shutil, sys, zipfile
from collections import Counter
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]; BENCH=ROOT/'benchmarks'
sys.path.insert(0,str(BENCH)); sys.path.insert(0,str(BENCH/'agent_study'))
from foreign_certify import extract
from router import certify_edit
from shift_correctness_real import new_pos, ref_shift, norm, coln, num2col

HERE=Path(__file__).parent; WORK=Path(os.environ.get('AGENT_STUDY_V3_WORK','/tmp/agent_study_v3_work'))
FCELL=re.compile(rb'<c r="([A-Z]+)(\d+)"(?:(?!</c>).)*?<f[^>]*>([^<]*)</f>',re.S)

def unescape(s):
 for ent,ch in (('&lt;','<'),('&gt;','>'),('&quot;','"'),('&apos;',"'"),('&amp;','&')):s=s.replace(ent,ch)
 return s

def sheet_part_by_name(z,sheet):
 wb=z.read('xl/workbook.xml').decode('utf-8','replace'); rid=None
 for m in re.finditer(r'<sheet\b[^>]*?/?>',wb):
  tag=m.group(0); nm=re.search(r'\bname="([^"]*)"',tag); ri=re.search(r'\br:id="([^"]*)"',tag)
  if nm and ri and unescape(nm.group(1))==sheet:rid=ri.group(1);break
 if not rid:return None
 rels=z.read('xl/_rels/workbook.xml.rels').decode('utf-8','replace')
 for m in re.finditer(r'<Relationship\b[^>]*?/?>',rels):
  tag=m.group(0); idm=re.search(r'\bId="([^"]*)"',tag);tm=re.search(r'\bTarget="([^"]*)"',tag)
  if idm and tm and idm.group(1)==rid:
   tgt=tm.group(1).lstrip('/');return tgt if tgt.startswith('xl/') else 'xl/'+tgt

def openpyxl_edit(src,sheet,t,dst):
 import openpyxl,warnings;warnings.simplefilter('ignore')
 wb=openpyxl.load_workbook(src);ws=wb[sheet];op=t['operation'];at=t['at'];count=t['count']
 if op=='insert-rows':ws.insert_rows(at,count)
 elif op=='delete-rows':ws.delete_rows(at,count)
 elif op=='insert-cols':ws.insert_cols(at,count)
 elif op=='delete-cols':ws.delete_cols(at,count)
 else:raise ValueError(op)
 wb.save(dst)

def shifted_a1(a1,axis,op,at,count):
 m=re.fullmatch(r'\$?([A-Za-z]+)\$?(\d+)',str(a1).replace('$',''))
 if not m:return None
 c,r=coln(m.group(1)),int(m.group(2));np_=new_pos(m.group(1),r,axis,op,at,count)
 if np_ is None:return None
 return f'{np_[0]}{np_[1]}'

def splice_formulas(path,sheet,by_a1):
 z=zipfile.ZipFile(path);part=sheet_part_by_name(z,sheet)
 if not part:return ['sheet_part_missing']
 names=z.namelist();data=z.read(part).decode('utf-8','replace');missed=[]
 for a1,f in by_a1.items():
  body=f[1:] if str(f).startswith('=') else str(f)
  pat=re.compile(r'(<c r="'+re.escape(a1)+r'"(?:(?!</c>).)*?<f[^>]*>)(.*?)(</f>)',re.S)
  data,n=pat.subn(lambda mm:mm.group(1)+escape(body)+mm.group(3),data,count=1)
  if n==0:missed.append(a1)
 buf={n:z.read(n) for n in names};buf[part]=data.encode();z.close()
 with zipfile.ZipFile(path,'w',zipfile.ZIP_DEFLATED) as zo:
  for n in names:zo.writestr(n,buf[n])
 return missed

def escape(s):
 return s.replace('&','&amp;').replace('<','&lt;').replace('>','&gt;').replace('"','&quot;').replace("'",'&apos;')

def file_formulas(path,sheet):
 z=zipfile.ZipFile(path);part=sheet_part_by_name(z,sheet);out={}
 if not part:return out
 for m in FCELL.finditer(z.read(part)):
  out[f'{m.group(1).decode()}{int(m.group(2))}']=unescape(m.group(3).decode('utf-8','replace'))
 return out

def sigma_for(t):
 axis=t['axis'];op=t['operation'];at=t['at'];count=t['count']
 def sigma(node):
  kind,a,b=node[:3]
  if kind!='C' or len(node)!=3:raise ValueError('generalized guard supports cell nodes only')
  nc,nr=new_pos(num2col(a),b,axis,op,at,count)
  if nr is None:return ('DELETED',a,b)
  return ('C',coln(nc),nr)
 return sigma

def deleted_dependency(A,op,at,count):
 axis='row' if 'rows' in op else 'col'
 for deps in A.deps.values():
  for dep in deps:
   if dep[0]!='C':continue
   v=dep[1] if axis=='col' else dep[2]
   if at<=v<at+count:return True
 return False

def score_task(t,agent_cells,workdir):
 rel,sheet=t['file'],t['sheet'];src=ROOT/rel;agent_file=workdir/'agent.xlsx'
 try:openpyxl_edit(src,sheet,t,agent_file)
 except Exception as e:return {'file':rel,'operation':t['operation'],'skip':f'artifact_build_failed:{type(e).__name__}'}
 expected_hosts={}
 for c in t['cells']:
  np_=new_pos(num2col(c['col']),c['row'],t['axis'],t['operation'],t['at'],t['count'])
  if np_ is not None:expected_hosts[f'{np_[0]}{np_[1]}']=c
 normalized={};extra=0
 for a1,f in (agent_cells or {}).items():
  key=str(a1).replace('$','').upper()
  if key in expected_hosts:normalized[key]=f
  else:extra+=1
 splice_missed=splice_formulas(agent_file,sheet,normalized)
 built=file_formulas(agent_file,sheet);evaluated=truth_skipped=truth_deleted=wrong=missing=0;wrong_cells=[]
 answered_hosts={str(a1).replace('$','').upper() for a1 in normalized}
 for c in t['cells']:
  np_=new_pos(num2col(c['col']),c['row'],t['axis'],t['operation'],t['at'],t['count'])
  if np_ is None:truth_deleted+=1;continue
  exp=ref_shift(c['formula'],t['axis'],t['operation'],t['at'],t['count'],sheet=sheet)
  if exp is None:truth_skipped+=1;continue
  a1=f'{np_[0]}{np_[1]}'
  if a1 not in answered_hosts:continue
  evaluated+=1;got=built.get(a1)
  if got is None:missing+=1;wrong+=1;wrong_cells.append(c['cell']+':MISSING')
  elif norm(got)!=norm(exp):wrong+=1;wrong_cells.append(c['cell'])
 if evaluated==0:return {'file':rel,'operation':t['operation'],'skip':'truth_undefined_all_answered_cells',
    'truth_skipped_out_of_grammar':truth_skipped,'truth_deleted_band':truth_deleted}
 agent_correct=wrong==0
 try:
  A=extract(src);B=extract(agent_file)
 except Exception as e:A=B=None;guard_err=type(e).__name__
 else:guard_err=None
 if A is None or B is None or deleted_dependency(A,t['operation'],t['at'],t['count']):
  verdict,gnote='REFUSED',guard_err or 'deleted_dependency_or_unparseable_fail_closed'
 else:
  try:
   res=certify_edit(A,B,sigma_for(t),set())
  except Exception as e:verdict,gnote='REFUSED',type(e).__name__
  else:
   verdict=res.status;gnote=res.reason[:120]
 if verdict=='CERTIFIED':guarded='shipped_correct' if agent_correct else 'shipped_CORRUPT_false_cert'
 else:guarded='refused_correct' if agent_correct else 'refused_incorrect'
 unguarded='shipped_correct' if agent_correct else 'shipped_CORRUPT'
 return {'file':rel,'corpus':t['corpus'],'sheet':sheet,'operation':t['operation'],'at':t['at'],'count':t['count'],
   'difficulty':t.get('difficulty'),'answered_cells':len(normalized),'extra_cells_ignored':extra,
   'splice_missed':splice_missed,'truth_evaluated':evaluated,'truth_skipped_out_of_grammar':truth_skipped,
   'truth_deleted_band':truth_deleted,'truth_total':truth_skipped==0 and truth_deleted==0,
   'wrong_cells':wrong_cells,'agent_correct':agent_correct,'guard_verdict':verdict,'guard_note':gnote,
   'guarded':guarded,'unguarded':unguarded}

def summarize(rows,tasks_selected,skips):
 n=len(rows);g=Counter(r['guarded'] for r in rows);u=Counter(r['unguarded'] for r in rows)
 correct=sum(r['agent_correct'] for r in rows);incorrect=n-correct
 false=g['shipped_CORRUPT_false_cert']
 by_op={}
 for op in sorted({r['operation'] for r in rows}):
  rr=[r for r in rows if r['operation']==op]
  gg=Counter(r['guarded'] for r in rr);uu=Counter(r['unguarded'] for r in rr)
  by_op[op]={'tasks':len(rr),'unguarded_corrupt':uu['shipped_CORRUPT'],
             'guarded_false_cert':gg['shipped_CORRUPT_false_cert'],
             'saved_incorrect':gg['refused_incorrect'],'refused_correct_cost':gg['refused_correct']}
 return {'experiment':'expanded guarded-vs-unguarded live-agent study v3',
  'protocol':'multi-operation EUSES+Enron task set; independent reference grammar truth; engine-free graph guard',
  'tasks_selected':tasks_selected,'tasks_scored':n,'tasks_skipped':dict(skips),'per_operation':by_op,
  'agent':{'tasks_correct':correct,'tasks_incorrect':incorrect,'task_error_rate':round(incorrect/n,4) if n else None},
  'UNGUARDED':{'shipped_correct':u['shipped_correct'],'shipped_CORRUPT':u['shipped_CORRUPT'],
               'corruption_incidence':round(u['shipped_CORRUPT']/n,4) if n else None},
  'GUARDED':{'shipped_correct':g['shipped_correct'],'shipped_CORRUPT_false_cert':false,
             'refused_correct_COST':g['refused_correct'],'refused_incorrect_SAVE':g['refused_incorrect'],
             'corruption_incidence':round(false/n,4) if n else None},
  'FALSE_CERT_must_be_0':false,'headline':(f'{n} tasks: unguarded shipped {u["shipped_CORRUPT"]} corrupt; '
    f'guarded shipped {false} corrupt, blocked {g["refused_incorrect"]}, refused {g["refused_correct"]} correct.'),
  'per_task':rows}

if __name__=='__main__':
 if len(sys.argv)<3:raise SystemExit('usage: score_v3.py outputs.json results.json [tasks.json]')
 outputs_path,result_path=sys.argv[1],sys.argv[2]
 tasks_path=sys.argv[3] if len(sys.argv)>3 else HERE/'tasks_v3.json'
 outputs=json.loads(Path(outputs_path).read_text());payload=json.loads(Path(tasks_path).read_text())
 tasks={t['file']+'#'+t['operation']+'@'+str(t['at']):t for t in payload['tasks']}
 WORK.mkdir(parents=True,exist_ok=True);rows=[];skips=Counter()
 # Outputs are keyed by synthetic task IDs when present; fallback to bare file for legacy inputs.
 for i,(key,cells) in enumerate(outputs.items()):
  t=tasks.get(key) or next((x for x in payload['tasks'] if x['file']==key),None)
  if t is None:skips['output_not_in_tasks']+=1;continue
  workdir=WORK/f'{i:04d}';workdir.mkdir(parents=True,exist_ok=True)
  row=score_task(t,cells,workdir);shutil.rmtree(workdir,ignore_errors=True)
  if 'skip' in row:skips[row['skip']]+=1;print('SKIP',row,flush=True)
  else:rows.append(row);print(f"{row['operation']:12} {row['file'][-48:]:48} {'OK' if row['agent_correct'] else 'ERR'} guard={row['guard_verdict']} {row['guarded']}",flush=True)
 summary=summarize(rows,len(payload['tasks']),skips)
 Path(result_path).write_text(json.dumps(summary,indent=2)+'\n')
 print(json.dumps({k:v for k,v in summary.items() if k!='per_task'},indent=2))
