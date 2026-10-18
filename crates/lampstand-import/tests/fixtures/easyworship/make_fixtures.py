#!/usr/bin/env python3
"""Rebuilds the EasyWorship fixtures in this folder. Run: python3 make_fixtures.py

The songs are public domain hymns written out by hand in the shape EasyWorship
stores them; they are not exports from EasyWorship itself.

ew7/         Songs.db + SongWords.db (SQLite; `word` table in the second file)
ew7-inline/  Songs.db with a `song_words` table in the same database
ew6/         Songs.DB + Songs.MB (Paradox 7-style table with a memo file),
             built from the layout OpenLP's importer reads: 0x800 header,
             1024-byte blocks, big-endian-free string fields, memo pointers
             into type 2 and type 3 blocks of the .MB file.
"""
import os
import sqlite3
import struct

HERE = os.path.dirname(os.path.abspath(__file__))


def rtf(*paras, cp="ansi\\ansicpg1252"):
    body = "".join(p + "\\par\r\n" for p in paras)
    return ("{\\rtf1\\%s\\deff0{\\fonttbl{\\f0\\fnil Arial;}}{\\colortbl;\\red0\\green0\\blue0;}"
            "\\pard\\plain\\f0\\fs80 %s}" % (cp, body))


def fresh(path):
    if os.path.exists(path):
        os.remove(path)


# --- EW7 -------------------------------------------------------------------

AMAZING = rtf(
    "Verse 1", "Amazing grace, how sweet the sound", "That saved a wretch like me",
    "I once was lost, but now am found", "Was blind but now I see", "",
    "Verse 2", "\\u8216?Twas grace that taught my heart to fear", "And grace my fears relieved", "",
    "Chorus", "Praise God, praise God", "Praise God from whom all blessings flow", "",
    "Verse 3", "Through many dangers, toils and snares", "I have already come", "",
    "Chorus", "",
)
ROCK = rtf("Verse 1", "Be Thou my Vision, O Lord of my heart", "Naught be all else to me, save that Thou art", "",
           "Endless love", "Thou my best Thought, by day or by night")  # second block is plain verse text


def make_ew7():
    d = os.path.join(HERE, "ew7")
    os.makedirs(d, exist_ok=True)
    fresh(f"{d}/Songs.db"); fresh(f"{d}/SongWords.db")
    s = sqlite3.connect(f"{d}/Songs.db")
    s.execute("CREATE TABLE song (title TEXT, author TEXT, copyright TEXT, administrator TEXT, vendor_id TEXT)")
    s.executemany("INSERT INTO song VALUES (?,?,?,?,?)", [
        ("Amazing Grace", "John Newton / Edwin Excell", "Public Domain", "", "22025"),
        ("Be Thou My Vision", "Ancient Irish; tr. Mary Byrne", "Public Domain", "Example Admin", "CCLI 30939"),
        ("Empty Song", "", "", "", ""),
    ])
    s.commit(); s.close()
    w = sqlite3.connect(f"{d}/SongWords.db")
    w.execute("CREATE TABLE word (song_id INTEGER, words TEXT)")
    w.executemany("INSERT INTO word VALUES (?,?)", [(1, AMAZING), (2, ROCK)])
    w.commit(); w.close()

    d = os.path.join(HERE, "ew7-inline")
    os.makedirs(d, exist_ok=True)
    fresh(f"{d}/Songs.db")
    c = sqlite3.connect(f"{d}/Songs.db")
    c.execute("CREATE TABLE song (title TEXT, author TEXT, copyright TEXT, vendor_id INTEGER)")
    c.execute("CREATE TABLE song_words (song_id INTEGER, words TEXT)")
    c.execute("INSERT INTO song VALUES ('Caf\u00e9 Song', 'A. Writer', '(c) 2001', 1234567)")
    c.execute("INSERT INTO song_words VALUES (1, ?)", (rtf("Verse 1", "Line caf\\'e9 one", "Line \\u1081?two"),))
    c.commit(); c.close()


# --- EW6 -------------------------------------------------------------------

FIELDS = [  # name, type, size
    ("Title", 1, 60), ("Author", 1, 40), ("Copyright", 1, 60),
    ("Administrator", 1, 40), ("Words", 0x0C, 20), ("Song Number", 1, 20),
]


def paradox_header(record_size):
    h = bytearray(0x800)
    struct.pack_into("<hh", h, 0, record_size, 0x800)
    h[5] = 1                                   # block size, in 1024-byte units
    struct.pack_into("<h", h, 14, 1)           # first data block
    struct.pack_into("<h", h, 33, len(FIELDS))
    struct.pack_into("<h", h, 106, 1252)
    for i, (_, t, size) in enumerate(FIELDS):
        h[120 + 2 * i] = t
        h[121 + 2 * i] = size
    at = 120 + 2 * len(FIELDS) + 4 + 4 * len(FIELDS) + 261
    for name, _, _ in FIELDS:
        raw = name.encode() + b"\0"
        h[at:at + len(raw)] = raw
        at += len(raw)
    return bytes(h)


def memo_ptr(offset, size, sub=0):
    return b"\0" * 10 + struct.pack("<II", offset | sub, size) + b"\0\0"


def make_ew6():
    d = os.path.join(HERE, "ew6")
    os.makedirs(d, exist_ok=True)
    mb = bytearray(0x4000)
    # Type 3 shared block at 0x2000: index of 5-byte entries from +12, data in
    # 16-byte units after the 332 byte index.
    shared = 0x2000
    mb[shared] = 3
    texts = [
        rtf("Verse 1", "Holy, holy, holy! Lord God Almighty!", "Early in the morning our song shall rise to Thee", "",
            "Chorus", "Which wert, and art, and evermore shalt be", "", "Chorus"),
        rtf("Verse 1", "Caf\\'e9 and na\\'efve words", "On two lines"),
    ]
    ptrs = []
    unit = 22
    for sub, t in enumerate(texts):
        data = t.encode("latin-1")
        mb[shared + 12 + 5 * sub] = unit
        mb[shared + unit * 16: shared + unit * 16 + len(data)] = data
        ptrs.append(memo_ptr(shared, len(data), sub))
        unit += (len(data) + 15) // 16
    # Type 2 block at 0x1000: 9-byte header then the data.
    big = rtf("Verse 1", "Come, thou fount of every blessing", "Tune my heart to sing thy grace").encode("latin-1")
    mb[0x1000] = 2
    mb[0x1009:0x1009 + len(big)] = big
    ptrs.append(memo_ptr(0x1000, len(big)))
    ptrs.append(b"\0" * 20)  # a song with no memo at all

    def s(v, n): return v.encode("cp1252").ljust(n, b"\0")
    rows = [
        ("Holy Holy Holy", "Reginald Heber", "Public Domain", "", ptrs[0], "12345"),
        ("Caf\u00e9 Song", "A. Writer", "(c) 1999", "Example Admin", ptrs[1], ""),
        ("Come Thou Fount", "Robert Robinson", "", "", ptrs[2], "CCLI 20000"),
        ("No Words", "", "", "", ptrs[3], ""),
    ]
    record_size = sum(f[2] for f in FIELDS)
    block = bytearray(1024)
    struct.pack_into("<hxxh", block, 0, 0, (len(rows) - 1) * record_size)
    at = 6
    for title, author, cr, admin, ptr, num in rows:
        rec = s(title, 60) + s(author, 40) + s(cr, 60) + s(admin, 40) + ptr + s(num, 20)
        assert len(rec) == record_size
        block[at:at + record_size] = rec
        at += record_size
    with open(f"{d}/Songs.DB", "wb") as f:
        f.write(paradox_header(record_size) + bytes(block))
    with open(f"{d}/Songs.MB", "wb") as f:
        f.write(mb)


if __name__ == "__main__":
    make_ew7()
    make_ew6()
