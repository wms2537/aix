#!/usr/bin/env python3
"""Score one agent-A/B trial output: task success + feature survival + validity."""
import sys, json, zipfile, subprocess, hashlib

def parts(path):
    try:
        z=zipfile.ZipFile(path); return {n:z.read(n) for n in z.namelist() if not n.endswith('/')}
    except Exception: return None

def feature_parts(names, key):
    keymap={'charts':'xl/charts/chart','pivot':'xl/pivotTable','vba':'xl/vbaProject.bin'}
    p=keymap[key]; return sorted(n for n in names if p in n)

def cell_value(path, sheet, cell):
    # read via openpyxl data-only? formulas may not eval; read raw value
    import openpyxl
    try:
        wb=openpyxl.load_workbook(path, data_only=False, keep_vba=path.endswith('.xlsm'))
        return wb[sheet][cell].value
    except Exception as e:
        return f'<load-error:{e}>'

def loads_in_engine(path, xlq):
    # use xlq inspect as the "does it open" proxy
    r=subprocess.run([xlq,'inspect',path], capture_output=True)
    return r.returncode==0

def main():
    orig, out, task_json, xlq = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
    task=json.loads(task_json)
    o_parts=parts(orig); n_parts=parts(out)
    res={'task_id':task['id'],'output_exists': n_parts is not None}
    if n_parts is None:
        res['valid']=False; print(json.dumps(res)); return
    # task success: target cell equals expected
    chk=task['check']
    val=cell_value(out, chk['sheet'], chk['cell'])
    res['target_cell_value']=val
    res['task_success']= (str(val)==str(chk['expect']) or val==chk['expect'])
    # validity
    res['loads_in_engine']=loads_in_engine(out, xlq)
    # feature survival + byte-identity
    feats={}
    for key in ('charts','pivot','vba'):
        o_fp=feature_parts(o_parts.keys(), key)
        if not o_fp: continue  # feature not in original
        n_fp=feature_parts(n_parts.keys(), key)
        present= len(n_fp)>0
        # byte-identical: every original feature part present and unchanged
        identical= all(n in n_parts and n_parts[n]==o_parts[n] for n in o_fp)
        feats[key]={'orig_parts':len(o_fp),'out_parts':len(n_fp),'present':present,'byte_identical':identical}
    res['feature_survival']=feats
    # overall part preservation
    common=[n for n in o_parts if n in n_parts]
    identical=sum(1 for n in common if o_parts[n]==n_parts[n])
    res['parts_byte_identical']=identical
    res['parts_total_original']=len(o_parts)
    res['parts_dropped']=sorted(set(o_parts)-set(n_parts))
    print(json.dumps(res))

if __name__=='__main__': main()
