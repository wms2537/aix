#!/usr/bin/env python3
"""Pre-register an expanded guarded-vs-unguarded live-agent task set.

Rules are fixed before any agent output is generated. Each task is a real EUSES
or Enron workbook plus one structural operation. A workbook/operation qualifies
only when every first-sheet formula is modelable, non-volatile, non-shared,
openpyxl-loadable, and the independent reference grammar can rule on at least
one formula that must change. Duplicate source files are allowed across distinct
operations, but each (file, operation, position) tuple is unique.
"""
import argparse, hashlib, json, os, random, re, sys, zipfile
from collections import Counter
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]; BENCH=ROOT/'benchmarks'
sys.path.insert(0,str(BENCH))
from foreign_certify import uncertifiable_formula, first_sheet_part, FTAG, CELLTAG, col_num
from forward_correctness import VOLATILE
from shift_correctness_real import RANGETOK, new_pos, num2col, ref_shift, norm

F_SELFCLOSED=re.compile(rb'<f[^>]*/>')
STRATA={
 'euses':ROOT/'data/inthewild/euses/converted_v2',
 'enron':ROOT/'data/inthewild/enron/converted_v2',
}
OPS=[
 ('insert-rows','row',2,1),('insert-rows','row',10,1),('delete-rows','row',5,1),
 ('insert-cols','col',2,1),('delete-cols','col',4,1),
]

def xml_unescape(s):
 for ent,ch in (('&lt;','<'),('&gt;','>'),('&quot;','"'),('&apos;',"'"),('&amp;','&')): s=s.replace(ent,ch)
 return s

def zip_first_sheet(path):
 try:
  d=path.read_bytes() and zipfile.ZipFile(path).read('xl/workbook.xml').decode('utf-8','replace')
  m=re.search(r'<sheet\b[^>]*\bname="([^"]*)"',d)
  return xml_unescape(m.group(1)) if m else None
 except Exception: return None

def formula_cells(path):
 try: data=zipfile.ZipFile(path).read(first_sheet_part(path))
 except Exception: return []
 out=[]
 for m in CELLTAG.finditer(data):
  row=int(m.group(2)); body=m.group(3)
  fm=FTAG.search(body)
  if not fm: continue
  vm=re.search(rb'<v>([^<]*)</v>',body)
  out.append({'cell':f'{m.group(1).decode()}{row}','row':row,'col':col_num(m.group(1).decode()),
              'formula':xml_unescape(fm.group(1).decode('utf-8','replace')),
              'cached_value':vm.group(1).decode() if vm else ''})
 return out

def refs_change(cells,axis,op,at,count,sheet):
 for c in cells:
  e=ref_shift(c['formula'],axis,op,at,count,sheet=sheet)
  if e is not None and norm(e)!=norm(c['formula']): return True
 return False

def answerable(cells,axis,op,at,count,sheet):
 for c in cells:
  if new_pos(num2col(c['col']),c['row'],axis,op,at,count) is not None and ref_shift(
      c['formula'],axis,op,at,count,sheet=sheet) is not None:return True
 return False

def hosts_preserved(path,sheet,cells,axis,op,at,count):
 try:
  import openpyxl,warnings;warnings.simplefilter('ignore')
  wb=openpyxl.load_workbook(path);ws=wb[sheet]
  if op=='insert-rows':ws.insert_rows(at,count)
  elif op=='delete-rows':ws.delete_rows(at,count)
  elif op=='insert-cols':ws.insert_cols(at,count)
  elif op=='delete-cols':ws.delete_cols(at,count)
  dst='/tmp/_v3_host_probe.xlsx';wb.save(dst)
 except Exception:return False
 import zipfile
 try:
  z=zipfile.ZipFile(dst);part=first_sheet_part(dst);data=z.read(part);z.close()
 except Exception:return False
 built={f'{m.group(1).decode()}{int(m.group(2))}' for m in CELLTAG.finditer(data) if FTAG.search(m.group(3))}
 expected=set()
 for c in cells:
  np_=new_pos(num2col(c['col']),c['row'],axis,op,at,count)
  if np_ is not None:expected.add(f'{np_[0]}{np_[1]}')
 return expected <= built

def qualify(path):
 sheet=zip_first_sheet(path)
 if not sheet:return sheet,'no_sheet_name',[]
 part=first_sheet_part(path)
 if not part:return sheet,'no_sheet_part',[]
 try:data=zipfile.ZipFile(path).read(part)
 except Exception:return sheet,'unreadable_zip',[]
 if F_SELFCLOSED.search(data):return sheet,'shared_formula_followers',[]
 fs=[xml_unescape(m.group(1).decode('utf-8','replace')) for m in FTAG.finditer(data)]
 if not fs:return sheet,'no_formulas',[]
 if any(not f.strip() for f in fs):return sheet,'empty_formula_body',[]
 if any(uncertifiable_formula(f) for f in fs):return sheet,'uncertifiable_formula',[]
 if any(VOLATILE.search(f.encode()) for f in fs):return sheet,'volatile_function',[]
 try:
  import openpyxl,warnings;warnings.simplefilter('ignore')
  wb=openpyxl.load_workbook(path,read_only=True);wb.close()
 except Exception:return sheet,'openpyxl_load_failed',[]
 cells=formula_cells(path)
 if not (2<=len(cells)<=40):return sheet,'size_band',cells
 return sheet,None,cells

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--want',type=int,default=100);ap.add_argument('--seed',type=int,default=20260823)
 ap.add_argument('--out',default=str(Path(__file__).with_name('tasks_v3.json')));a=ap.parse_args()
 candidates=[];reasons=Counter();seen_md5=set()
 for corpus,root in STRATA.items():
  for p in sorted(root.rglob('*.xlsx')):
   try:h=hashlib.md5(p.read_bytes()).hexdigest()
   except Exception:continue
   if h in seen_md5:continue
   seen_md5.add(h);sheet,reason,cells=qualify(p)
   rel=p.relative_to(ROOT).as_posix()
   if reason:reasons[reason]+=1;continue
   for op,axis,at,count in OPS:
    if refs_change(cells,axis,op,at,count,sheet) and answerable(cells,axis,op,at,count,sheet) and hosts_preserved(p,sheet,cells,axis,op,at,count):
     evaluable=sum(ref_shift(c['formula'],axis,op,at,count,sheet=sheet) is not None for c in cells)
     shift_cells=sum((lambda e:e is not None and norm(e)!=norm(c['formula']))(ref_shift(c['formula'],axis,op,at,count,sheet=sheet)) for c in cells)
     candidates.append({'corpus':corpus,'file':rel,'sheet':sheet,'operation':op,'axis':axis,'at':at,'count':count,
       'difficulty':{'n_formulas':len(cells),'truth_evaluable_cells':evaluable,'truth_shift_cells':shift_cells,
                     'has_absolute_refs':any('$' in c['formula'] for c in cells),
                     'has_ranges':any(RANGETOK.search(c['formula']) for c in cells)},
       'cells':cells})
 # Deterministic stratification: balance operations, then corpus, then difficulty.
 by_op={op:[] for op,_,_,_ in OPS}
 for t in candidates:by_op[t['operation']].append(t)
 per_op=max(1,a.want//len(OPS));rng=random.Random(a.seed);selected=[]
 for op,_,_,_ in OPS:
  pool=sorted(by_op[op],key=lambda t:(t['corpus'],t['difficulty']['n_formulas'],t['file']))
  rng.shuffle(pool);selected.extend(pool[:per_op])
 rng.shuffle(selected);selected=selected[:a.want]
 payload={'protocol':'expanded live-agent study v3','generated_at':'2026-08-23','seed':a.seed,
          'selection_rules':{'operations':sorted({x[0] for x in OPS}),'max_formulas':40,'min_formulas':2,
                             'exclude_uncertifiable':True,'exclude_shared_followers':True,'exclude_volatile':True,
                             'duplicate_source_files_allowed_across_operations':True},
          'candidate_tasks':len(candidates),'selected_tasks':len(selected),'eligibility_skip_reasons':dict(reasons),
          'tasks':selected}
 Path(a.out).write_text(json.dumps(payload,indent=2)+'\n')
 print(json.dumps({'candidates':len(candidates),'selected':len(selected),'per_operation':Counter(t['operation'] for t in selected),'skips':dict(reasons)},indent=2))
if __name__=='__main__':main()
