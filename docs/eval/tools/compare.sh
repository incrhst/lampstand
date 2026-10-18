#!/bin/zsh
# compare.sh DECKDIR OUTDIR [REF=lo|pp]
# For each deck: DeckCraft render at 1 px/pt, reference PDF -> PNG at the same size,
# per-slide normalised RMSE. Writes OUTDIR/scores.tsv: deck slide rmse
set -u
DECKS=$1; OUT=$2; REF=${3:-lo}
EVAL=${EVAL:-${0:a:h}/../../../../deckcraft-eval}
CLI=$EVAL/deckcraft/target/release/deckcraft-cli
SOFFICE=/Applications/LibreOffice.app/Contents/MacOS/soffice
mkdir -p $OUT/dc $OUT/ref $OUT/refpng
print -r -- "deck\tslide\trmse\tnote" > $OUT/scores.tsv
if [[ $REF == lo ]]; then
  $SOFFICE -env:UserInstallation=file://$OUT/lo-profile --headless --convert-to pdf --outdir $OUT/ref $DECKS/*.pptx >/dev/null 2>&1
fi
for f in $DECKS/*.pptx; do
  d=${f:t:r}
  rm -rf $OUT/dc/$d; mkdir -p $OUT/dc/$d $OUT/refpng/$d
  if ! $CLI render $f --all --scale 1 $OUT/dc/$d >/dev/null 2>&1; then
    print -r -- "$d\t-\t-\tdc-fail" >> $OUT/scores.tsv; continue
  fi
  pdf=$OUT/ref/$d.pdf
  if [[ ! -s $pdf ]]; then print -r -- "$d\t-\t-\tref-fail" >> $OUT/scores.tsv; continue; fi
  first=$OUT/dc/$d/slide-01.png
  size=$(magick identify -format '%wx%h' $first)
  magick -density 144 $pdf -resize "${size}!" -background white -alpha remove $OUT/refpng/$d/p-%02d.png 2>/dev/null
  n=1
  for s in $OUT/dc/$d/slide-*.png; do
    r=$(printf "$OUT/refpng/$d/p-%02d.png" $((n-1)))
    if [[ -f $r ]]; then
      score=$(magick compare -metric RMSE \
        \( $s -background white -alpha remove \) $r null: 2>&1 | sed -E 's/.*\(([0-9.e-]+)\).*/\1/')
      print -r -- "$d\t$n\t$score\t" >> $OUT/scores.tsv
    else
      print -r -- "$d\t$n\t-\tno-ref-page" >> $OUT/scores.tsv
    fi
    n=$((n+1))
  done
done
