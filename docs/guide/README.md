# Sentinal guide and slides

Built with the Sentinal Research brand templates (`sentinal-brand-package/`):
the report layout (`sentinal.sty`), the shared brand file (`sentinal-brand.sty`),
the Beamer theme (`beamerthemeSentinal.sty`) and the PowerPoint template.

| File | What it is |
|---|---|
| `sentinal-guide.tex`, `body.tex`, `architecture-figure.tex` | System design and code guide (report, dark cover) |
| `sentinal-slides.tex` | System overview deck (Beamer, 16:9, dark) |
| `make_pptx.py` | The same deck as `.pptx`, built from the brand PowerPoint template |
| `build.sh` | Builds everything into `out/`: PDFs, Overleaf zips, and the `.pptx` |

Build: `./build.sh` (needs `tectonic` or `latexmk` with XeLaTeX; the `.pptx`
needs `python-pptx`). For Overleaf, upload `out/<name>-overleaf.zip` and choose
Menu > Compiler > XeLaTeX. Install the fonts in `sentinal-brand-package/fonts`
before opening the `.pptx`.

`sentinal-brand.sty` here carries one fix over the brand package: the marks
(`\srmark`, `\sentinalmark`) used `baseline={(0,24)}`, which TikZ reads in
centimetres, so a mark in a header or a line of text was pushed about 24 cm
down. It is now `baseline={(0pt,-#1)}`.

The guide describes the `dev` branch at commit `9512ded` (8 November 2026).
Update `body.tex` when the system changes.
