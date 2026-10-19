# Per slide: how many clicks PowerPoint itself takes, by playing the deck in PowerPoint for Mac.
# Opens each deck, runs the slide show, and sends "next" (AppleScript
# `go to next slide`, the same as a clicker) until the show ends. A press that leaves the show on
# the same slide is one click of that slide's build. Hidden slides are skipped by the show and do
# not appear. Works on an unlicensed (read-only) PowerPoint: it only plays, never saves.
#
# A press during a running animation finishes it instead of starting the next step, so DELAY
# (seconds between presses) must exceed the longest effect; rerun suspicious slides with a larger
# DELAY. Slides with automatic timings (advTm) can advance on their own during DELAY; check
# those against the XML. Prints: deck, slide (1-based, presentation order), clicks.
#
# PowerPoint is sandboxed: if it asks for file access, copy the decks to ~/Downloads first.
#
# Usage: python3 -I pptclicks.py [--delay S] DECK.pptx...
import os, subprocess, sys

SCRIPT = r'''
on run argv
  set f to item 1 of argv
  set d to (item 2 of argv) as real
  tell application "Microsoft PowerPoint"
    -- Leave nothing from a previous deck open: a lingering show blocks the next open.
    try
      repeat while (count of slide show windows) > 0
        exit slide show (slide show view of slide show window 1)
      end repeat
    end try
    close every presentation saving no
    open POSIX file f
    repeat 60 times
      delay 1
      if (count of presentations) > 0 then exit repeat
    end repeat
    delay 2
    set p to active presentation
    run slide show slide show settings of p
    delay 2
    set v to slide show view of slide show window 1
    set cur to slide index of slide of v
    set n to 0
    set acc to ""
    repeat 3000 times
      try
        go to next slide v
      on error
        exit repeat
      end try
      delay d
      try
        set v to slide show view of slide show window 1
        set nxt to slide index of slide of v
      on error
        exit repeat
      end try
      if nxt is missing value then
        -- The black end-of-show screen.
        exit repeat
      else if nxt = cur then
        set n to n + 1
      else
        set acc to acc & cur & tab & n & linefeed
        set cur to nxt
        set n to 0
      end if
    end repeat
    set acc to acc & cur & tab & n & linefeed
    try
      exit slide show (slide show view of slide show window 1)
    end try
    close p saving no
  end tell
  return acc
end run
'''

args = sys.argv[1:]
delay = "1.5"
if args[:1] == ["--delay"]:
    delay, args = args[1], args[2:]
for path in args:
    name = os.path.splitext(os.path.basename(path))[0]
    r = subprocess.run(["osascript", "-e", SCRIPT, os.path.abspath(path), delay],
                       capture_output=True, text=True, timeout=3600)
    if r.returncode != 0:
        print(f"{name}\t?\t?\t# {r.stderr.strip()[:120]}", flush=True)
        continue
    for line in r.stdout.strip().splitlines():
        slide, clicks = line.split("\t")
        print(f"{name}\t{slide}\t{clicks}", flush=True)
