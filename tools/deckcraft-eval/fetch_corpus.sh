#!/bin/sh
# Download the public part of the DeckCraft evaluation corpus into DIR (default ./corpus).
# The decks are not committed: their licences vary and several are scraped from the web.
# See docs/research/deckcraft-evaluation.md, "Corpus".
set -eu
DIR=${1:-corpus}
POI=https://raw.githubusercontent.com/apache/poi/trunk/test-data/slideshow
LO=https://raw.githubusercontent.com/LibreOffice/core/master/sd/qa/unit/data/pptx
mkdir -p "$DIR/poi" "$DIR/lo"
for f in Divino_Revelado KEY02 EmbeddedVideo EmbeddedAudio SampleShow keyframes 2411-Performance_Up \
  aascu.org_hbcu_leadershipsummit_cooper_ ae.ac.uaeu.faculty_nafaachbili_GeomLec1 \
  'ca.ubc.cs.people_~emhill_presentations_HowWeRefactor' \
  au.asn.aes.www_conferences_2011_presentations_Fri_20Room4Level4_20930_20Maloney \
  ececapstonespring2012 tika-2605 alterman_security rain backgrounds themes WithMaster present1 \
  LIBRE_OFFICE-100610-0; do
  curl -sfL -o "$DIR/poi/$f.pptx" "$POI/$f.pptx"
done
for f in airplane-transition animation-sound-embedded narration tdf148478_transitionMusic trigger \
  loopNoPause pres-with-notes master-slides master-and-eleven-layouts media-placeholder \
  subtitle-animation-save tdf98477grow tdf113818-swivel connector-shape-animations effectOrder \
  tdf104792-smart-art-animation slide-sections; do
  curl -sfL -o "$DIR/lo/$f.pptx" "$LO/$f.pptx"
done
echo "public decks in $DIR; generate the church decks with make_church_decks.py"
