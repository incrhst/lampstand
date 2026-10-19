#!/usr/bin/env python3
"""Rebuilds the EasyWorship fixtures in this folder. Run: python3 make_fixtures.py

The songs are public domain hymns written out by hand in the shape EasyWorship
stores them; they are not exports from EasyWorship itself.

ew7/         Songs.db + SongWords.db (SQLite; `word` table in the second file)
ew7-inline/  Songs.db with a `song_words` table in the same database
service.ewsx A zip holding a SQLite main.db, with the zip checksum broken
             on purpose (EasyWorship writes one the zip format rejects).
service.ews  An old schedule file: fixed-size entries, zlib-compressed RTF.
ew6/         Songs.DB + Songs.MB (Paradox 7-style table with a memo file),
             built from the layout OpenLP's importer reads: 0x800 header,
             1024-byte blocks, big-endian-free string fields, memo pointers
             into type 2 and type 3 blocks of the .MB file.
"""
import os
import sqlite3
import struct
import zipfile
import zlib

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


def make_ewsx():
    tmp = os.path.join(HERE, "_main.db")
    fresh(tmp)
    c = sqlite3.connect(tmp)
    c.executescript("""
        CREATE TABLE presentation (title TEXT, author TEXT, copyright TEXT, reference_number TEXT, presentation_type INTEGER);
        CREATE TABLE slide (presentation_id INTEGER, order_index INTEGER);
        CREATE TABLE element (slide_id INTEGER, element_type INTEGER, element_style_type INTEGER, foreground_resource_id INTEGER);
        CREATE TABLE resource_text (resource_id INTEGER, rtf TEXT);
    """)
    c.execute("INSERT INTO presentation VALUES ('Service Song', 'A. Author', '(c) 2010', '777', 6)")
    c.execute("INSERT INTO presentation VALUES ('A Slideshow', '', '', '', 3)")
    # Slides are inserted out of order to check order_index is used.
    for slide_id, order, res, text in [(1, 2, 11, rtf("Chorus", "Second slide")), (2, 1, 10, rtf("Verse 1", "First slide"))]:
        c.execute("INSERT INTO slide(rowid, presentation_id, order_index) VALUES (?,1,?)", (slide_id, order))
        c.execute("INSERT INTO element VALUES (?,6,4,?)", (slide_id, res))
        c.execute("INSERT INTO resource_text VALUES (?,?)", (res, text))
    c.commit(); c.close()
    out = os.path.join(HERE, "service.ewsx")
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.write(tmp, "main.db")
    os.remove(tmp)
    b = bytearray(open(out, "rb").read())
    b[14:18] = b"\xde\xad\xbe\xef"                     # local header CRC
    cd = b.rindex(b"PK\x01\x02")
    b[cd + 16:cd + 20] = b"\xde\xad\xbe\xef"            # central directory CRC
    open(out, "wb").write(bytes(b))


def make_ews():
    entry_len = 0x718
    head = bytearray(62)
    head[:38] = b"EasyWorship Schedule File Version    5"
    struct.pack_into("<ih", head, 56, 3, entry_len)
    songs = [
        (1, "Schedule Song", "S. Writer", "(c) 1990", "Example Admin", "4242",
         rtf("Verse 1", "Line one", "Line two", "", "Chorus", "Sing it")),
        (2, "A Scripture", "", "", "", "", None),
        (1, "Second Song", "", "", "", "", rtf("Only verse caf\\'e9")),
    ]
    entries = bytearray(entry_len * len(songs))
    content = bytearray()
    base = len(head) + len(entries)
    for i, (kind, title, author, cr, admin, ccli, text) in enumerate(songs):
        e = memoryview(entries)[i * entry_len:(i + 1) * entry_len]
        def put(off, v, n): e[off:off + n] = v.encode("cp1252").ljust(n, b"\0")
        put(0, title, 50); put(307, author, 50); put(358, cr, 100); put(459, admin, 50); put(1410, ccli, 10)
        struct.pack_into("<i", e, 820, kind)
        if text is not None:
            z = zlib.compress(text.encode("latin-1"))
            blob = z + b"\0" * 4 + struct.pack("<i", len(text)) + b"\0\0"
            struct.pack_into("<i", e, 800, base + len(content))
            content += struct.pack("<i", len(blob)) + blob
    with open(os.path.join(HERE, "service.ews"), "wb") as f:
        f.write(head + entries + content)


if __name__ == "__main__":
    make_ewsx()
    make_ews()
    make_ew7()
    make_ew6()
