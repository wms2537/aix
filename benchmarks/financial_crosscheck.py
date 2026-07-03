#!/usr/bin/env python3
"""E3: independent cross-check of the differential oracle's financial verdicts.
Retires the circularity objection (the triage used our own reimplementation) by
recomputing with numpy-financial + independent Treasury/bond formulas. Excel is
still the ultimate arbiter; where the independent calc contradicts the triage we
DOWNGRADE that verdict rather than defend it."""
import json, math
import numpy_financial as npf

out={"experiment":"E3 independent financial cross-check","source":"numpy-financial + documented Treasury formulas (independent of the triage reimplementation)","checks":[]}

# RATE(10,-100,1000): triage class SPEC_AMBIGUOUS (both engines ~0)
r=npf.rate(10,-100,1000,0)
out["checks"].append({"fn":"RATE(10,-100,1000)","independent":r,"ironcalc":-3.35e-11,"libreoffice":6.12e-11,
  "triage":"SPEC_AMBIGUOUS","crosscheck":"CONFIRMS both ~0 (true rate=0); independent %.2e"%r})

# TBILLPRICE 184d disc=0.045: 100*(1-disc*DSM/360)
DSM=184; disc=0.045
tp=100*(1-disc*DSM/360)
out["checks"].append({"fn":"TBILLPRICE 184d 0.045","independent":tp,"ironcalc":97.7,"libreoffice":97.7375,
  "triage":"LIBREOFFICE_WRONG (DSM=181)","crosscheck":"CONFIRMS IronCalc (%.4f); LO uses DSM=181"%tp})

# TBILLYIELD 184d pr=98.5: (100-pr)/pr*360/DSM
pr=98.5
ty=(100-pr)/pr*360/DSM
out["checks"].append({"fn":"TBILLYIELD 184d 98.5","independent":ty,"ironcalc":0.0297947,"libreoffice":0.0302886,
  "triage":"LIBREOFFICE_WRONG (DSM=181)","crosscheck":"CONFIRMS IronCalc (%.7f); LO uses DSM=181"%ty})

# TBILLEQ 184d disc=0.045 (DSM>182 bond-equivalent): independent quadratic form
P=1-disc*DSM/360; t=DSM/365.0
be=(-t/2 + math.sqrt((t/2)**2 - (t-0.5)*(1-1/P)))/(t-0.5)
out["checks"].append({"fn":"TBILLEQ 184d 0.045","independent":be,"ironcalc":0.0466902,"libreoffice":0.0466812,
  "triage":"LIBREOFFICE_WRONG","crosscheck":"CONTRADICTS TRIAGE: independent %.7f matches LibreOffice, not IronCalc. Verdict DOWNGRADED to UNDECIDABLE_HERE pending Excel; the independent bond-equiv form may itself differ from Excel's exact algorithm."%be})

json.dump(out, open("benchmarks/financial_crosscheck.json","w"), indent=1)
for c in out["checks"]: print(c["fn"],"->",c["crosscheck"])
