# arXiv draft source bundle

This package is a draft for upload review. It contains the verified
`paper-v3` manuscript as `main.tex`. The XeLaTeX-specific font declarations
were added so the source rebuilds without missing characters.

## Before submission

1. Replace the empty `\author{}` declaration with the author names and
   affiliations.
2. Review the arXiv metadata in `main.tex`.
3. Choose the license in the arXiv form.
4. Set the primary category to `cs.SE`; add `cs.AI` and `cs.HC` as
   cross-lists.
5. Put `https://github.com/wms2537/aix` in the Comments field after confirming
   that repository visibility is intended.

## Local build

Run:

```sh
xelatex -interaction=nonstopmode main.tex
```

The draft compiled with TeX Live 2025/dev on 2026-08-25 and produced 20 pages
without missing-character warnings. No figures or bibliography files are
required.
