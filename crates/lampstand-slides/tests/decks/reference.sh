#!/bin/zsh
# Rebuilds ref/<deck>/slide-NN.png, the reference renders of each deck here,
# with LibreOffice (PDF export, rasterised at 1 px per point).
#
# These stand in for PowerPoint's own PNG exports: the PowerPoint on the
# machine that made them is unlicensed and cannot export. LibreOffice is
# wrong in places (docs/deckcraft-evaluation.md): it sets text in a
# narrower substitute and wraps differently, and it ignores a saved
# `normAutofit fontScale` and fits the text itself. tests/render.rs says
# which differences are LibreOffice's.
#
# Needs LibreOffice, pdftoppm (poppler) and ImageMagick. With deck names
# (`reference.sh 01-masters-placeholders`), rebuilds only those.
set -eu
HERE=${0:a:h}
SOFFICE=${SOFFICE:-/Applications/LibreOffice.app/Contents/MacOS/soffice}
TMP=$(mktemp -d)
trap 'rm -rf $TMP' EXIT
if (( $# )); then decks=(${@/#/$HERE/}); decks=(${^decks}.pptx); else decks=($HERE/*.pptx); fi
$SOFFICE -env:UserInstallation=file://$TMP/profile --headless --convert-to pdf --outdir $TMP $decks >/dev/null 2>&1
for deck in $decks; do
  name=${deck:t:r}
  out=$HERE/ref/$name
  rm -rf $out
  mkdir -p $out
  # LibreOffice leaves hidden slides out of the PDF: number pages by the
  # visible slides they show.
  visible=($(python3 -I - $deck <<'EOF'
import re, sys, zipfile
z = zipfile.ZipFile(sys.argv[1])
pres = z.read("ppt/presentation.xml").decode()
rels = z.read("ppt/_rels/presentation.xml.rels").decode()
targets = dict(re.findall(r'Id="(rId\d+)"[^>]*Target="([^"]+)"', rels))
targets.update({k: v for v, k in re.findall(r'Target="([^"]+)"[^>]*Id="(rId\d+)"', rels)})
order = re.findall(r'<p:sldId [^>]*r:id="(rId\d+)"', pres)
for i, rid in enumerate(order, 1):
    xml = z.read("ppt/" + targets[rid]).decode()
    if not re.search(r'<p:sld [^>]*show="0"', xml):
        print(i)
EOF
))
  width=$(python3 -I -c "import re,sys,zipfile; x=zipfile.ZipFile(sys.argv[1]).read('ppt/presentation.xml').decode(); print(round(int(re.search(r'<p:sldSz cx=\"(\d+)\"', x).group(1))/12700))" $deck)
  pdftoppm -png -r 144 $TMP/$name.pdf $TMP/$name-page
  i=1
  for n in $visible; do
    page=$(ls $TMP/$name-page-*.png | sed -n "${i}p")
    magick $page -resize "${width}x" -background white -alpha remove -strip PNG24:$out/slide-$(printf %02d $n).png
    i=$((i + 1))
  done
  print "$name: ${#visible} visible slides"
done
