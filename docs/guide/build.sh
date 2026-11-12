#!/usr/bin/env bash
# Build the branded guide and slides into docs/guide/out/ and make Overleaf zips.
# Needs tectonic (brew install tectonic) or latexmk with XeLaTeX.
# Brand fonts come from ./fonts/ or ../../sentinal-brand-package/fonts/;
# without them the documents fall back to TeX Gyre Heros.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
fonts="$here/fonts"
[ -d "$fonts" ] || fonts="$here/../../sentinal-brand-package/fonts"
out="$here/out"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

build() { # build <name> <files...>
  local name="$1"; shift
  local dir="$work/$name"
  mkdir -p "$dir"
  (cd "$here" && cp "$@" "$dir/")
  [ -d "$fonts" ] && cp -R "$fonts" "$dir/fonts"
  rm -f "$out/$name-overleaf.zip"
  (cd "$dir" && zip -qr "$out/$name-overleaf.zip" .)
  if command -v tectonic >/dev/null; then
    (cd "$dir" && tectonic -X compile --keep-logs "$name.tex" >"$out/$name.log" 2>&1) || { tail -30 "$out/$name.log"; exit 1; }
  else
    (cd "$dir" && latexmk -xelatex -interaction=nonstopmode "$name.tex" >"$out/$name.log" 2>&1) || { tail -30 "$out/$name.log"; exit 1; }
  fi
  cp "$dir/$name.pdf" "$out/"
  echo "built $out/$name.pdf"
}

build sentinal-guide sentinal-guide.tex body.tex architecture-figure.tex sentinal.sty sentinal-brand.sty
build sentinal-slides sentinal-slides.tex beamerthemeSentinal.sty sentinal-brand.sty

# PowerPoint version from the brand template (needs python-pptx).
template="$here/../../sentinal-brand-package/slides/Sentinal Research - Slides.pptx"
if [ -f "$template" ] && python3 -c 'import pptx' 2>/dev/null; then
  python3 "$here/make_pptx.py" "$template" "$out/sentinal-slides.pptx" && echo "built $out/sentinal-slides.pptx"
else
  echo "skipped sentinal-slides.pptx (needs python-pptx and the brand template)"
fi
