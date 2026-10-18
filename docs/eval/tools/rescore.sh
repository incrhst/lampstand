#!/bin/zsh
# rescore.sh OUT XMLTSV: pair DeckCraft slide renders with reference PDF pages, skipping hidden
# slides (LibreOffice leaves them out of the PDF). Writes OUT/aligned.tsv: deck slide rmse
OUT=$1; XML=$2
print -r -- $'deck\tslide\trmse' > $OUT/aligned.tsv
for dir in $OUT/dc/*(/); do
  d=${dir:t}
  [[ -d $OUT/refpng/$d ]] || continue
  page=0
  for row in ${(f)"$(awk -F'\t' -v d="$d" '$1==d {print $2"\t"$3}' $XML)"}; do
    i=${row%%$'\t'*}; hidden=${row##*$'\t'}
    [[ $hidden == 1 ]] && continue
    s=$(printf "$dir/slide-%02d.png" $((i+1)))
    r=$(printf "$OUT/refpng/$d/p-%02d.png" $page)
    page=$((page+1))
    [[ -f $s && -f $r ]] || continue
    score=$(magick compare -metric RMSE \( $s -background white -alpha remove \) $r null: 2>&1 | sed -E 's/.*\(([0-9.e-]+)\).*/\1/')
    print -r -- "$d"$'\t'"$((i+1))"$'\t'"$score" >> $OUT/aligned.tsv
  done
done
