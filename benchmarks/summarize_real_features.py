#!/usr/bin/env python3
import collections, json, re
from pathlib import Path
HERE=Path(__file__).parent; ROOT=HERE.parent
D=json.loads((HERE/'real_feature_fidelity.json').read_text())
rows=D['results']

def classify_error(text):
    s=str(text)
    if 'refusing to write error-valued cell' in s: return 'refused_error_valued_target'
    if '"write_reliable": false' in s and ('user_defined_functions' in s or 'nondeterministic_in_affected' in s): return 'refused_unreliable_write'
    if 'Unexpected feature' in s or 'unsupported rich styling' in s: return 'refused_unsupported_rich_styling'
    return 'tool_failed'

summary={
  'experiment':'Stratified real-corpus feature preservation (EUSES + Enron converted_v2)',
  'source_manifest':'benchmarks/real_feature_manifest.summary.json',
  'sample_size':len(rows),
  'seed':D.get('seed'),
  'selection_rule':'Deterministic seed 20260823 sample: up to 10 files each from chart, pivot, external-link-only, comment/drawing-only, and plain-control strata. Edit target is the first non-formula numeric cell in workbook order.',
  'methodology':{
    'part_metric':'Byte-identical zip members between output and untouched original.',
    'xlq_edit':'Typed set-cell apply with proof-carrying commit.',
    'openpyxl_edit':'load_workbook + assign numeric target + save using default keep_vba=False.',
    'libreoffice_edit':'Headless same-format convert-to re-save proxy; upper bound on churn, not a targeted edit.',
    'failure_policy':'Failures are retained per file. xlq refusals are classified as fail-closed outcomes, not silent corruption.',
  },
}
by_stratum={}
for strat in sorted({r['stratum'] for r in rows}):
    rr=[r for r in rows if r['stratum']==strat]; entry={'files':len(rr),'tools':{}}
    for tool in ('xlq','openpyxl','libreoffice'):
        completed=[]; errors=[]
        for r in rr:
            m=r.get('tools',{}).get(tool)
            if m is None or 'error' in m:
                reason='no_attempt' if m is None else classify_error(m['error'])
                if tool=='xlq' and reason.startswith('refused_'): reason='fail_closed_'+reason[len('refused_'):]
                elif tool!='xlq': reason='error_'+reason
                errors.append({'path':r['path'],'reason':reason})
            else: completed.append(m)
        ident=sum(m['parts_byte_identical'] for m in completed); total=sum(m['parts_total'] for m in completed)
        loads_ic=sum(1 for m in completed if m.get('output_loads_in_ironcalc') is True)
        loads_lo=sum(1 for m in completed if m.get('output_loads_in_soffice') is True)
        entry['tools'][tool]={
          'files_completed':len(completed), 'parts_identical':ident, 'parts_total':total,
          'fraction_byte_identical':round(ident/total,3) if total else None,
          'median_parts_identical_fraction':round(sorted(m['parts_byte_identical']/m['parts_total'] for m in completed)[len(completed)//2],3) if completed else None,
          'outputs_loaded_in_ironcalc':loads_ic, 'outputs_loaded_in_soffice':loads_lo,
          'noncompletion_reasons':dict(collections.Counter(e['reason'] for e in errors)),
        }
    by_stratum[strat]=entry
summary['by_stratum']=by_stratum

alltools={t:{'files_completed':0,'parts_identical':0,'parts_total':0} for t in ('xlq','openpyxl','libreoffice')}
for e in by_stratum.values():
    for t,v in e['tools'].items():
        alltools[t]['files_completed']+=v['files_completed']; alltools[t]['parts_identical']+=v['parts_identical']; alltools[t]['parts_total']+=v['parts_total']
for v in alltools.values(): v['fraction_byte_identical']=round(v['parts_identical']/v['parts_total'],3) if v['parts_total'] else None
summary['aggregate_completed_files']=alltools
summary['headline']=('Across completed comparisons on real EUSES/Enron workbooks, byte preservation was '
  f"{alltools['xlq']['fraction_byte_identical']} for xlq versus "
  f"{alltools['openpyxl']['fraction_byte_identical']} for openpyxl and "
  f"{alltools['libreoffice']['fraction_byte_identical']} for the LibreOffice re-save proxy.")
Path(__file__).with_name('real_feature_fidelity.summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
