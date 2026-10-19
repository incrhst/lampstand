# Per slide (presentation order): click steps in the main sequence, read straight from the XML.
# A main-sequence click step is a top-level <p:par> under mainSeq that starts on delay="indefinite" (waits for a click).
import sys, zipfile, re, posixpath
import xml.etree.ElementTree as ET
P = '{http://schemas.openxmlformats.org/presentationml/2006/main}'
R = '{http://schemas.openxmlformats.org/officeDocument/2006/relationships}'
for path in sys.argv[1:]:
    name = posixpath.splitext(posixpath.basename(path))[0]
    try:
        z = zipfile.ZipFile(path)
        pres = ET.fromstring(z.read('ppt/presentation.xml'))
        rels = ET.fromstring(z.read('ppt/_rels/presentation.xml.rels'))
    except Exception:
        continue
    tgt = {r.get('Id'): r.get('Target') for r in rels}
    for i, sid in enumerate(pres.iter(P + 'sldId')):
        t = tgt.get(sid.get(R + 'id'), '')
        part = posixpath.normpath(posixpath.join('ppt', t)) if not t.startswith('/') else t[1:]
        try:
            s = ET.fromstring(z.read(part))
        except Exception:
            print(f"{name}\t{i}\t?\t?"); continue
        hidden = s.get('show') == '0'
        clicks = 0; interactive = 0
        for ctn in s.iter(P + 'cTn'):
            if ctn.get('nodeType') == 'interactiveSeq': interactive += 1
            if ctn.get('nodeType') == 'mainSeq':
                lst = ctn.find(P + 'childTnLst')
                for par in (lst if lst is not None else []):
                    own = par.find(P + 'cTn')
                    cond = own.find(P + 'stCondLst/' + P + 'cond') if own is not None else None
                    auto = own is not None and any(c.get('evt') == 'onBegin' for c in own.iter(P + 'cond'))
                    if cond is not None and cond.get('delay') == 'indefinite' and not auto:
                        clicks += 1
        print(f"{name}\t{i}\t{int(hidden)}\t{clicks}\t{interactive}")
