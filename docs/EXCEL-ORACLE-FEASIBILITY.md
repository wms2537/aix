# Excel-in-the-loop oracle feasibility

Date: 2026-08-25.

## Decision

**Blocked in the current environment.** Desktop Excel cannot be used here as a
final arbitration oracle without either an unavailable licensed installation or
unsupported unattended Office automation. This is an environment and governance
decision, not a claim that Excel-in-the-loop is impossible in every deployment.

LibreOffice is therefore **not** promoted to an Excel substitute. It remains a
differential reference peer.

## Why

1. **No licensed Excel oracle is available locally.** The development and
   verification environment is Linux; desktop Excel is Windows/macOS software.
2. **Unattended COM/UI automation is unsupported by Microsoft.** The recorded
   baseline cites Microsoft's guidance against Office automation from
   unattended, non-interactive clients (`docs/BASELINE.md` §3).
3. **Cloud workbook APIs change the problem.** Microsoft Graph requires
   M365/SharePoint-hosted files and introduces an external service and data
   movement boundary outside xlq's local-only model.
4. **LibreOffice is not ground truth.** The differential report explicitly
   treats it as a reference peer, with Excel documentation used to arbitrate
   disagreements (`docs/AGREEMENT.md`). Known divergences include `POWER(0,0)`,
   boolean coercion behavior, and Treasury-bill day-count semantics.
5. **Headless conversion has a stale-value trap.** LibreOffice does not
   reliably recalculate Excel-produced XLSX on load by default, so treating its
   output values as fresh Excel-equivalent truth would be unsound
   (`docs/BASELINE.md` §4).
6. **Coverage gaps are material.** In the committed oracle corpus, 36 functions
   across 115 cases produced no comparable LibreOffice signal, so those cases
   could not be validated by substituting LibreOffice for Excel.

## Retained validation posture

- Keep Excel documentation arbitration for disputed semantics.
- Keep independent differential evidence from IronCalc, LibreOffice, and the
  pure-Python `formulas` engine where applicable.
- Treat cross-engine agreement as necessary but insufficient evidence of Excel
  compatibility.
- Continue fail-closed behavior when semantics are uncertain or outside the
  certified grammar.
- Disclose absence of a live Excel executable as a limitation rather than
  relabeling LibreOffice output as Excel arbitration.

## Future protocol, only with fresh authorization

If a licensed, supported Excel host later becomes available, the minimal safe
protocol is:

1. Use a dedicated Windows/macOS host under the operator's valid license.
2. Keep execution supervised or use only an automation mode explicitly supported
   for that deployment.
3. Pin Excel version, locale, calculation mode, and security prompts.
4. Start with a small deterministic fixture set and compare exported cell values,
   error codes, reloadability, and package-part preservation.
5. Log exact binary/version, invocation, inputs, outputs, and hashes.

No license circumvention, spoofing, or unsupported unattended Office automation
is authorized by this repository.
