"""Generate church-service style source decks (public-domain text, generated images/video).

Usage: python make_church_decks.py ASSETS_DIR OUT_DIR
"""
import sys
from pathlib import Path

from lxml import etree
from pptx import Presentation
from pptx.dml.color import RGBColor
from pptx.enum.text import PP_ALIGN
from pptx.oxml.ns import qn
from pptx.util import Emu, Pt

A, OUT = Path(sys.argv[1]), Path(sys.argv[2])
OUT.mkdir(parents=True, exist_ok=True)
P_NS = "http://schemas.openxmlformats.org/presentationml/2006/main"
NSDECL = f'xmlns:p="{P_NS}" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"'


def x(s):
    return etree.fromstring(s)


def set_transition(slide, xml_inner, spd="med", adv=None):
    sld = slide._element
    adv_attr = f' advTm="{adv}"' if adv else ""
    tr = x(f'<p:transition {NSDECL} spd="{spd}"{adv_attr}>{xml_inner}</p:transition>')
    timing = sld.find(qn("p:timing"))
    if timing is not None:
        timing.addprevious(tr)
    else:
        sld.find(qn("p:clrMapOvr")).addnext(tr)


class Timing:
    """Builds a main-sequence p:timing with one click group per effect."""

    def __init__(self):
        self.next_id = 3
        self.clicks = []
        self.builds = []

    def _id(self):
        self.next_id += 1
        return self.next_id - 1

    def tgt(self, spid, para):
        if para is None:
            return f'<p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl>'
        return f'<p:tgtEl><p:spTgt spid="{spid}"><p:txEl><p:pRg st="{para}" end="{para}"/></p:txEl></p:spTgt></p:tgtEl>'

    def effect(self, spid, kind, para=None, node="clickEffect"):
        t = self.tgt(spid, para)
        if kind == "fade-in":
            preset, cls = 10, "entr"
            body = (f'<p:set><p:cBhvr><p:cTn id="{self._id()}" dur="1" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst></p:cTn>{t}'
                    f'<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="visible"/></p:to></p:set>'
                    f'<p:animEffect transition="in" filter="fade"><p:cBhvr><p:cTn id="{self._id()}" dur="500"/>{t}</p:cBhvr></p:animEffect>')
        elif kind == "fly-in":
            preset, cls = 2, "entr"
            body = (f'<p:set><p:cBhvr><p:cTn id="{self._id()}" dur="1" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst></p:cTn>{t}'
                    f'<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="visible"/></p:to></p:set>'
                    f'<p:anim calcmode="lin" valueType="num"><p:cBhvr additive="base"><p:cTn id="{self._id()}" dur="500" fill="hold"/>{t}'
                    f'<p:attrNameLst><p:attrName>ppt_y</p:attrName></p:attrNameLst></p:cBhvr><p:tavLst><p:tav tm="0"><p:val><p:strVal val="1+#ppt_h/2"/></p:val></p:tav>'
                    f'<p:tav tm="100000"><p:val><p:strVal val="#ppt_y"/></p:val></p:tav></p:tavLst></p:anim>')
        elif kind == "grow":
            preset, cls = 6, "emph"
            body = (f'<p:animScale><p:cBhvr><p:cTn id="{self._id()}" dur="500" fill="hold"/>{t}</p:cBhvr>'
                    f'<p:by x="150000" y="150000"/></p:animScale>')
        elif kind == "fade-out":
            preset, cls = 10, "exit"
            body = (f'<p:animEffect transition="out" filter="fade"><p:cBhvr><p:cTn id="{self._id()}" dur="500"/>{t}</p:cBhvr></p:animEffect>'
                    f'<p:set><p:cBhvr><p:cTn id="{self._id()}" dur="1" fill="hold"><p:stCondLst><p:cond delay="499"/></p:stCondLst></p:cTn>{t}'
                    f'<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="hidden"/></p:to></p:set>')
        else:
            raise ValueError(kind)
        outer, mid, eff = self._id(), self._id(), self._id()
        sub = 0 if preset != 2 else 4
        self.clicks.append(
            f'<p:par><p:cTn id="{outer}" fill="hold"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst>'
            f'<p:par><p:cTn id="{mid}" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>'
            f'<p:par><p:cTn id="{eff}" presetID="{preset}" presetClass="{cls}" presetSubtype="{sub}" fill="hold" grpId="{len(self.builds)}" nodeType="{node}">'
            f'<p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>{body}</p:childTnLst></p:cTn></p:par>'
            f'</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>')
        build = "p" if para is not None else "whole"
        if para is not None and (spid, "p") in [(b[0], b[2]) for b in self.builds]:
            # PowerPoint uses one bldP per shape/group for paragraph builds; reuse grpId 0
            pass
        self.builds.append((spid, len(self.builds), build))

    def attach(self, slide):
        sld = slide._element
        old = sld.find(qn("p:timing"))
        if old is not None:
            sld.remove(old)
        seen, bld = set(), []
        for spid, grp, build in self.builds:
            if build == "p":
                if spid in seen:
                    continue
                seen.add(spid)
                bld.append(f'<p:bldP spid="{spid}" grpId="{grp}" build="p"/>')
            else:
                bld.append(f'<p:bldP spid="{spid}" grpId="{grp}" animBg="1"/>' if False else "")
        xml = (f'<p:timing {NSDECL}><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst>'
               f'<p:seq concurrent="1" nextAc="seek"><p:cTn id="2" dur="indefinite" nodeType="mainSeq"><p:childTnLst>{"".join(self.clicks)}</p:childTnLst></p:cTn>'
               f'<p:prevCondLst><p:cond evt="onPrev" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst>'
               f'<p:nextCondLst><p:cond evt="onNext" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq>'
               f'</p:childTnLst></p:cTn></p:par></p:tnLst>{"<p:bldLst>" + "".join(bld) + "</p:bldLst>" if any(bld) else ""}</p:timing>')
        tr = sld.find(qn("p:transition"))
        el = x(xml)
        if tr is not None:
            tr.addnext(el)
        else:
            sld.find(qn("p:clrMapOvr")).addnext(el)


import json, os
NOTES = {}
CUR = []


def note(slide, text):
    ids = [sl.slide_id for sl in CUR[1].slides]
    NOTES.setdefault(CUR[0], {})[ids.index(slide.slide_id) + 1] = text
    if not os.environ.get("NO_NOTES"):
        slide.notes_slide.notes_text_frame.text = text


def widescreen():
    prs = Presentation()
    prs.slide_width, prs.slide_height = Emu(12192000), Emu(6858000)
    return prs


def blank(prs):
    return prs.slides.add_slide(prs.slide_layouts[6])


def lyric_box(slide, prs, text, size=40, shadow=False):
    tb = slide.shapes.add_textbox(Emu(600000), Emu(900000), prs.slide_width - Emu(1200000), prs.slide_height - Emu(1800000))
    tf = tb.text_frame
    tf.word_wrap = True
    for i, line in enumerate(text.split("\n")):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.alignment = PP_ALIGN.CENTER
        r = p.add_run()
        r.text = line
        r.font.size = Pt(size)
        r.font.bold = True
        r.font.name = "Calibri"
        r.font.color.rgb = RGBColor(0xFF, 0xFF, 0xFF)
    bodyPr = tf._txBody.find(qn("a:bodyPr"))
    bodyPr.set("anchor", "ctr")
    if shadow:
        for r in tb._element.iter(qn("a:rPr")):
            r.find(qn('a:solidFill')).addnext(x(f'<a:effectLst {NSDECL}><a:outerShdw blurRad="38100" dist="38100" dir="2700000" algn="tl"><a:srgbClr val="000000"><a:alpha val="60000"/></a:srgbClr></a:outerShdw></a:effectLst>'))
    return tb


def black_bg(slide):
    bg = slide.background.fill
    bg.solid()
    bg.fore_color.rgb = RGBColor(0, 0, 0)


AMAZING = [
    "Amazing grace! How sweet the sound\nThat saved a wretch like me!\nI once was lost, but now am found;\nWas blind, but now I see.",
    "’Twas grace that taught my heart to fear,\nAnd grace my fears relieved;\nHow precious did that grace appear\nThe hour I first believed.",
    "Through many dangers, toils and snares,\nI have already come;\n’Tis grace hath brought me safe thus far,\nAnd grace will lead me home.",
    "The Lord has promised good to me,\nHis Word my hope secures;\nHe will my Shield and Portion be,\nAs long as life endures.",
]
HOLY = [
    "Holy, holy, holy! Lord God Almighty!\nEarly in the morning our song shall rise to Thee;\nHoly, holy, holy, merciful and mighty!\nGod in three Persons, blessed Trinity!",
    "Holy, holy, holy! All the saints adore Thee,\nCasting down their golden crowns around the glassy sea;\nCherubim and seraphim falling down before Thee,\nWhich wert, and art, and evermore shalt be.",
    "Holy, holy, holy! though the darkness hide Thee,\nThough the eye of sinful man Thy glory may not see;\nOnly Thou art holy; there is none beside Thee,\nPerfect in power, in love, and purity.",
]


def deck_lyrics_amazing():
    """Lyric deck: full-bleed picture per slide, white centred text, fade transitions, notes."""
    prs = widescreen()
    CUR[:] = [sys._getframe().f_code.co_name, prs]
    s = blank(prs)
    s.shapes.add_picture(str(A / "worship-bg.jpg"), 0, 0, prs.slide_width, prs.slide_height)
    lyric_box(s, prs, "Amazing Grace\nJohn Newton, 1779 — Public domain", size=54)
    set_transition(s, "<p:fade/>")
    for v, text in enumerate(AMAZING, 1):
        s = blank(prs)
        s.shapes.add_picture(str(A / "worship-bg.jpg"), 0, 0, prs.slide_width, prs.slide_height)
        lyric_box(s, prs, text, shadow=True)
        set_transition(s, "<p:fade/>")
        note(s, f"Verse {v}. Band repeats chorus after verse {v} if the leader signals.")
    prs.save(OUT / "lyrics-amazing-grace.pptx")


def deck_lyrics_holy():
    """Lyric deck: background picture on the slide master, black solid slides, push/cut transitions."""
    prs = widescreen()
    CUR[:] = [sys._getframe().f_code.co_name, prs]
    m = prs.slide_master
    R = 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
    _, rid = m.part.get_or_add_image_part(str(A / "worship-bg.jpg"))
    csld = m._element.find(qn("p:cSld"))
    old_bg = csld.find(qn("p:bg"))
    if old_bg is not None:
        csld.remove(old_bg)
    csld.insert(0, x(f'<p:bg {NSDECL} {R}><p:bgPr><a:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></a:blipFill><a:effectLst/></p:bgPr></p:bg>'))
    _, lrid = m.part.get_or_add_image_part(str(A / "poster.png"))
    m.shapes._spTree.append(x(
        f'<p:pic {NSDECL} {R}><p:nvPicPr><p:cNvPr id="900" name="Church logo"/><p:cNvPicPr/><p:nvPr userDrawn="1"/></p:nvPicPr>'
        f'<p:blipFill><a:blip r:embed="{lrid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>'
        f'<p:spPr><a:xfrm><a:off x="11000000" y="6000000"/><a:ext cx="1000000" cy="562500"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>'))
    for v, text in enumerate(HOLY, 1):
        s = blank(prs)
        lyric_box(s, prs, text, size=36, shadow=True)
        set_transition(s, '<p:push dir="u"/>' if v % 2 else "<p:cut/>", spd="slow")
        note(s, f"Holy, Holy, Holy — verse {v} (Reginald Heber, 1826).")
    s = blank(prs)
    black_bg(s)
    set_transition(s, '<p:fade thruBlk="1"/>')
    note(s, "Blank black slide between songs.")
    prs.save(OUT / "lyrics-holy-holy-holy.pptx")


def deck_sermon():
    """Sermon deck: title layout, scripture, bullet build by paragraph, picture, emphasis + exit, notes."""
    prs = widescreen()
    CUR[:] = [sys._getframe().f_code.co_name, prs]
    s = prs.slides.add_slide(prs.slide_layouts[0])
    s.shapes.title.text = "Living Water"
    s.placeholders[1].text = "John 4:1–26 · Sunday morning"
    note(s, "Welcome. Pray before starting. Sermon runs 30 minutes.")
    set_transition(s, "<p:fade/>")

    s = prs.slides.add_slide(prs.slide_layouts[5])
    s.shapes.title.text = "John 4:13–14 (WEB)"
    tb = s.shapes.add_textbox(Emu(900000), Emu(1700000), Emu(10400000), Emu(4200000))
    tb.text_frame.word_wrap = True
    tb.text_frame.text = ("Jesus answered her, “Everyone who drinks of this water will thirst again, but whoever drinks of the "
                          "water that I will give him will never thirst again; but the water that I will give him will become "
                          "in him a well of water springing up to eternal life.”")
    for r in tb.text_frame.paragraphs[0].runs:
        r.font.size = Pt(30)
        r.font.italic = True
    note(s, "Read slowly. Pause after 'never thirst again'.")
    set_transition(s, '<p:wipe dir="r"/>')

    s = prs.slides.add_slide(prs.slide_layouts[1])
    s.shapes.title.text = "Three things Jesus offers"
    body = s.placeholders[1]
    points = ["He meets us where we are", "He knows our whole story", "He gives what lasts"]
    body.text_frame.text = points[0]
    for p in points[1:]:
        body.text_frame.add_paragraph().text = p
    t = Timing()
    for i in range(3):
        t.effect(body.shape_id, "fade-in", para=i)
    t.attach(s)
    note(s, "Click once per point. Illustration for point 2: the well at noon.")
    set_transition(s, "<p:fade/>")

    s = prs.slides.add_slide(prs.slide_layouts[5])
    s.shapes.title.text = "The well at Sychar"
    pic = s.shapes.add_picture(str(A / "sermon-photo.jpg"), Emu(2100000), Emu(1600000), Emu(8000000), Emu(4500000))
    cap = s.shapes.add_textbox(Emu(2100000), Emu(6150000), Emu(8000000), Emu(500000))
    cap.text_frame.text = "Illustration (generated image)"
    t = Timing()
    t.effect(pic.shape_id, "fly-in")
    t.effect(pic.shape_id, "grow")
    t.effect(cap.shape_id, "fade-in")
    t.effect(cap.shape_id, "fade-out")
    t.attach(s)
    note(s, "Picture flies in, then grows; caption fades in and out.")
    set_transition(s, '<p:push dir="l"/>')

    s = prs.slides.add_slide(prs.slide_layouts[1])
    s.shapes.title.text = "Response"
    s.placeholders[1].text_frame.text = "Where are you thirsty this week?"
    s.placeholders[1].text_frame.add_paragraph().text = "Prayer team at the front after the service"
    note(s, "Invite response. Hand back to worship leader.")
    set_transition(s, '<p:split orient="vert" dir="out"/>')
    prs.save(OUT / "sermon-living-water.pptx")


def deck_announcements():
    """Pre-service loop: auto-advance timings, embedded video, picture, mixed transitions."""
    prs = widescreen()
    CUR[:] = [sys._getframe().f_code.co_name, prs]
    s = blank(prs)
    s.shapes.add_picture(str(A / "worship-bg.jpg"), 0, 0, prs.slide_width, prs.slide_height)
    lyric_box(s, prs, "Welcome!\nService starts at 10:00", size=60)
    set_transition(s, "<p:fade/>", adv=8000)

    s = blank(prs)
    black_bg(s)
    title = s.shapes.add_textbox(Emu(600000), Emu(300000), Emu(11000000), Emu(800000))
    title.text_frame.text = "Youth camp video"
    for r in title.text_frame.paragraphs[0].runs:
        r.font.color.rgb = RGBColor(255, 255, 255)
        r.font.size = Pt(36)
    s.shapes.add_movie(str(A / "announce.mp4"), Emu(1800000), Emu(1300000), Emu(8600000), Emu(4837500),
                       poster_frame_image=str(A / "poster.png"), mime_type="video/mp4")
    set_transition(s, '<p:wipe dir="d"/>', adv=10000)
    note(s, "Video plays on click (6 s).")

    s = blank(prs)
    s.shapes.add_picture(str(A / "sermon-photo.jpg"), 0, 0, prs.slide_width, prs.slide_height)
    box = lyric_box(s, prs, "Small groups start next week\nSign up in the foyer", size=44, shadow=True)
    set_transition(s, '<p:cover dir="l"/>', adv=8000)
    t = Timing()
    t.effect(box.shape_id, "fade-in", para=0)
    t.effect(box.shape_id, "fade-in", para=1)
    t.attach(s)
    prs.save(OUT / "announcements-loop.pptx")


deck_lyrics_amazing()
deck_lyrics_holy()
deck_sermon()
deck_announcements()
json.dump(NOTES, open(OUT / "notes.json", "w"), indent=1)
print("ok")
