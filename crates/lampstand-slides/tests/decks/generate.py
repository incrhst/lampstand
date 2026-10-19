"""Generates the synthetic decks in this folder.

    python3 -m venv .venv && .venv/bin/pip install python-pptx==1.0.2 pillow==12.0.0
    .venv/bin/python crates/lampstand-slides/tests/decks/generate.py

Needs ffmpeg on PATH for the video in 07. Everything is built with
python-pptx; where it has no API (shadows, auto-numbering, saved autofit
scales, picture backgrounds, line ends) the script adds schema-valid
DrawingML to the element python-pptx made. Output is deterministic apart
from the zip timestamps python-pptx writes.

After regenerating, rebuild the references and snapshots:

    crates/lampstand-slides/tests/decks/reference.sh
    INSTA_UPDATE=always cargo test -p lampstand-slides --test decks
"""

import copy
import io
import os
import subprocess
import sys
import tempfile

from lxml import etree
from PIL import Image, ImageDraw
from pptx import Presentation
from pptx.dml.color import RGBColor
from pptx.enum.dml import MSO_LINE_DASH_STYLE, MSO_PATTERN, MSO_THEME_COLOR
from pptx.enum.shapes import MSO_CONNECTOR, MSO_SHAPE
from pptx.enum.text import MSO_ANCHOR, PP_ALIGN
from pptx.oxml.ns import qn
from pptx.util import Emu, Inches, Pt

HERE = os.path.dirname(os.path.abspath(__file__))
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


# --- media -------------------------------------------------------------------


def png(img):
    out = io.BytesIO()
    img.save(out, "PNG", optimize=False)
    return io.BytesIO(out.getvalue())


def jpeg(img):
    out = io.BytesIO()
    img.save(out, "JPEG", quality=85)
    return io.BytesIO(out.getvalue())


def landscape(w=1600, h=900):
    """A sunset: a vertical gradient sky, a sun and two hills."""
    img = Image.new("RGB", (w, h))
    d = ImageDraw.Draw(img)
    for y in range(h):
        t = y / h
        d.line([(0, y), (w, y)], fill=(int(40 + 200 * t), int(60 + 90 * t), int(140 - 60 * t)))
    d.ellipse([w * 0.6, h * 0.35, w * 0.75, h * 0.35 + w * 0.15], fill=(255, 220, 120))
    d.polygon([(0, h), (0, h * 0.7), (w * 0.4, h * 0.55), (w * 0.8, h * 0.8), (w, h * 0.75), (w, h)], fill=(30, 70, 50))
    d.polygon([(0, h), (w * 0.3, h * 0.8), (w * 0.7, h * 0.9), (w, h)], fill=(20, 45, 35))
    return img


def logo(size=400):
    """A transparent PNG: a ring with a cross."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.ellipse([20, 20, size - 20, size - 20], outline=(200, 140, 30, 255), width=24)
    d.rectangle([size * 0.45, size * 0.2, size * 0.55, size * 0.8], fill=(90, 50, 140, 255))
    d.rectangle([size * 0.3, size * 0.35, size * 0.7, size * 0.45], fill=(90, 50, 140, 255))
    return img


def grid_image(w=800, h=600):
    """Numbered quadrants, so a crop shows which part survived."""
    img = Image.new("RGB", (w, h), (255, 255, 255))
    d = ImageDraw.Draw(img)
    colors = [(220, 60, 60), (60, 160, 70), (50, 90, 200), (230, 180, 40)]
    for i, c in enumerate(colors):
        x, y = (i % 2) * w // 2, (i // 2) * h // 2
        d.rectangle([x, y, x + w // 2, y + h // 2], fill=c)
        d.ellipse([x + w // 8, y + h // 8, x + w * 3 // 8, y + h * 3 // 8], fill=(255, 255, 255))
    return img


def clip_mp4():
    """One second of ffmpeg's test pattern, 160 × 90."""
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "clip.mp4")
        subprocess.run(
            [
                "ffmpeg", "-loglevel", "error", "-f", "lavfi", "-i",
                "testsrc=size=160x90:rate=10:duration=1", "-pix_fmt", "yuv420p",
                "-c:v", "libx264", "-preset", "veryslow", "-bitexact",
                "-map_metadata", "-1", "-fflags", "+bitexact", path,
            ],
            check=True,
        )
        with open(path, "rb") as f:
            return f.read()


# --- helpers -----------------------------------------------------------------


def wide(prs):
    prs.slide_width = Inches(13.333)
    prs.slide_height = Inches(7.5)
    return prs


def layout(prs, name):
    return next(l for l in prs.slide_layouts if l.name == name)


def a(tag, **attrs):
    el = etree.Element(qn("a:" + tag))
    for k, v in attrs.items():
        el.set(k, str(v))
    return el


def sp_pr(shape):
    return shape._element.spPr


def insert_in_order(parent, el, after_tags):
    """Inserts `el` after the last child whose tag is in `after_tags`, or first."""
    idx = 0
    for i, child in enumerate(parent):
        if etree.QName(child).localname in after_tags:
            idx = i + 1
    parent.insert(idx, el)


def outer_shadow(shape, blur=6, dist=4, dir_deg=45, color="000000", alpha=50):
    pr = sp_pr(shape)
    eff = pr.find(qn("a:effectLst"))
    if eff is None:
        eff = a("effectLst")
        insert_in_order(pr, eff, {"xfrm", "custGeom", "prstGeom", "noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill", "ln"})
    sh = a("outerShdw", blurRad=Pt(blur), dist=Pt(dist), dir=int(dir_deg * 60000), algn="ctr", rotWithShape="0")
    clr = etree.SubElement(sh, qn("a:srgbClr"), val=color)
    etree.SubElement(clr, qn("a:alpha"), val=str(alpha * 1000))
    eff.append(sh)


def text_shadow(run, blur=4, dist=3, color="000000", alpha=70):
    rpr = run._r.get_or_add_rPr()
    eff = etree.SubElement(rpr, qn("a:effectLst"))
    sh = etree.SubElement(eff, qn("a:outerShdw"), blurRad=str(Pt(blur)), dist=str(Pt(dist)), dir=str(45 * 60000), algn="tl", rotWithShape="0")
    clr = etree.SubElement(sh, qn("a:srgbClr"), val=color)
    etree.SubElement(clr, qn("a:alpha"), val=str(alpha * 1000))
    # effectLst must come after the fill and before the fonts.
    fill = rpr.find(qn("a:solidFill"))
    if fill is not None:
        fill.addnext(eff)


def alpha_fill(shape, rgb, alpha):
    shape.fill.solid()
    shape.fill.fore_color.rgb = RGBColor.from_string(rgb)
    clr = sp_pr(shape).find(qn("a:solidFill")).find(qn("a:srgbClr"))
    etree.SubElement(clr, qn("a:alpha"), val=str(alpha * 1000))


def radial_fill(shape, inner, outer):
    shape.fill.gradient()
    stops = shape.fill.gradient_stops
    stops[0].color.rgb = RGBColor.from_string(inner)
    stops[0].position = 0
    stops[1].color.rgb = RGBColor.from_string(outer)
    stops[1].position = 1.0
    grad = sp_pr(shape).find(qn("a:gradFill"))
    lin = grad.find(qn("a:lin"))
    if lin is not None:
        grad.remove(lin)
    path = etree.SubElement(grad, qn("a:path"), path="circle")
    etree.SubElement(path, qn("a:fillToRect"), l="50000", t="50000", r="50000", b="50000")


def line_ends(connector, head=None, tail=None, w="med", length="med"):
    ln = connector.line._get_or_add_ln()
    for tag, kind in (("headEnd", head), ("tailEnd", tail)):
        if kind:
            ln.append(a(tag, type=kind, w=w, len=length))


def autonum(paragraph, scheme, start=None):
    ppr = paragraph._p.get_or_add_pPr()
    el = a("buAutoNum", type=scheme)
    if start:
        el.set("startAt", str(start))
    ppr.append(el)


def bullet_char(paragraph, char, indent_pt=None, margin_pt=None):
    ppr = paragraph._p.get_or_add_pPr()
    if margin_pt is not None:
        ppr.set("marL", str(Pt(margin_pt)))
    if indent_pt is not None:
        ppr.set("indent", str(Pt(indent_pt)))
    ppr.append(a("buChar", char=char))


def no_bullet(paragraph):
    paragraph._p.get_or_add_pPr().append(a("buNone"))


def norm_autofit(text_frame, font_scale=None, reduction=None):
    body = text_frame._txBody.bodyPr
    for child in list(body):
        if etree.QName(child).localname in ("noAutofit", "normAutofit", "spAutoFit"):
            body.remove(child)
    el = a("normAutofit")
    if font_scale:
        el.set("fontScale", str(int(font_scale * 1000)))
    if reduction:
        el.set("lnSpcReduction", str(int(reduction * 1000)))
    body.append(el)


def slide_number_field(paragraph):
    fld = etree.SubElement(paragraph._p, qn("a:fld"), id="{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}", type="slidenum")
    etree.SubElement(fld, qn("a:rPr"), lang="en-US")
    t = etree.SubElement(fld, qn("a:t"))
    t.text = "‹#›"


def picture_background(slide, image):
    _, rid = slide.part.get_or_add_image_part(image)
    c_sld = slide._element.cSld
    old = c_sld.find(qn("p:bg"))
    if old is not None:
        c_sld.remove(old)
    bg = etree.Element(qn("p:bg"))
    pr = etree.SubElement(bg, qn("p:bgPr"))
    blip = etree.SubElement(pr, qn("a:blipFill"), dpi="0", rotWithShape="1")
    etree.SubElement(blip, qn("a:blip"), {qn("r:embed"): rid})
    etree.SubElement(blip, qn("a:srcRect"))
    stretch = etree.SubElement(blip, qn("a:stretch"))
    etree.SubElement(stretch, qn("a:fillRect"))
    etree.SubElement(pr, qn("a:effectLst"))
    c_sld.insert(0, bg)


def move_to(slide_shape, target_tree, new_id):
    """Moves a shape made on a slide into a master or layout shape tree."""
    el = slide_shape._element
    el.getparent().remove(el)
    el.find(".//" + qn("p:cNvPr")).set("id", str(new_id))
    target_tree.append(el)


def hide(slide):
    slide._element.set("show", "0")


def notes(slide, text):
    slide.notes_slide.notes_text_frame.text = text


def add_box(slide, x, y, w, h, text="", size=18, shape=MSO_SHAPE.RECTANGLE):
    s = slide.shapes.add_shape(shape, Inches(x), Inches(y), Inches(w), Inches(h))
    if text:
        s.text_frame.text = text
        for p in s.text_frame.paragraphs:
            for r in p.runs:
                r.font.size = Pt(size)
    return s


def blank(prs):
    return prs.slides.add_slide(layout(prs, "Blank"))


def title_only(prs, title):
    s = prs.slides.add_slide(layout(prs, "Title Only"))
    s.shapes.title.text = title
    return s


def list_notes_master(prs):
    """python-pptx adds a notes master without listing it in presentation.xml
    (`p:notesMasterIdLst`, which PowerPoint always writes). Keynote will not
    open a deck with notes until it is listed."""
    if not any(s.has_notes_slide for s in prs.slides):
        return
    pres = prs.part._element
    if pres.find(qn("p:notesMasterIdLst")) is not None:
        return
    rid = prs.part.relate_to(
        prs.notes_master.part,
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster",
    )
    lst = etree.Element(qn("p:notesMasterIdLst"))
    etree.SubElement(lst, qn("p:notesMasterId"), {qn("r:id"): rid})
    pres.find(qn("p:sldMasterIdLst")).addnext(lst)


def save(prs, name):
    list_notes_master(prs)
    path = os.path.join(HERE, name)
    prs.save(path)
    print("wrote", name)


# --- decks -------------------------------------------------------------------


def deck_01():
    """Master → layout → slide inheritance, placeholders, notes, hidden slides."""
    prs = wide(Presentation())
    master = prs.slide_master
    master.background.fill.solid()
    master.background.fill.fore_color.rgb = RGBColor(0xF4, 0xF1, 0xEA)
    # A band across the top of every slide, drawn on the master.
    scratch = prs.slides.add_slide(layout(prs, "Blank"))
    band = scratch.shapes.add_shape(MSO_SHAPE.RECTANGLE, 0, 0, prs.slide_width, Inches(0.35))
    band.fill.solid()
    band.fill.fore_color.theme_color = MSO_THEME_COLOR.ACCENT_1
    band.line.fill.background()
    move_to(band, master.shapes._spTree, 90)
    # The title-and-content layout moves its title down and colours it.
    tc = layout(prs, "Title and Content")
    title_ph = tc.placeholders[0]
    # Setting one side of an inherited box makes python-pptx write an empty
    # extent, so set the whole box (the master's, moved down).
    master_title = master.placeholders[0]
    title_ph.left, title_ph.top = master_title.left, Inches(0.6)
    title_ph.width, title_ph.height = master_title.width, master_title.height
    # Bold accent 2 in the layout's list style, which slide titles inherit
    # (run properties typed into the layout's prompt text would not be).
    lst = title_ph.text_frame._txBody.find(qn("a:lstStyle"))
    lvl = etree.SubElement(lst, qn("a:lvl1pPr"))
    rpr = etree.SubElement(lvl, qn("a:defRPr"), b="1")
    fill = etree.SubElement(rpr, qn("a:solidFill"))
    etree.SubElement(fill, qn("a:schemeClr"), val="accent2")
    title_ph.text_frame.text = "Layout title"
    # Drop the scratch slide.
    sld_id = prs.slides._sldIdLst[0]
    prs.part.drop_rel(sld_id.rId)
    prs.slides._sldIdLst.remove(sld_id)

    s = prs.slides.add_slide(layout(prs, "Title Slide"))
    s.shapes.title.text = "Welcome to Lampstand"
    s.placeholders[1].text = "Masters, layouts and placeholders"
    notes(s, "Greet everyone.\nMention the coffee after the service.")

    s = prs.slides.add_slide(tc)
    s.shapes.title.text = "Inherited title style"
    body = s.placeholders[1].text_frame
    body.text = "First level point"
    for text, level in (("Second level", 1), ("Third level", 2), ("Back to the first level", 0)):
        p = body.add_paragraph()
        p.text = text
        p.level = level
    notes(s, "The title should be bold and in accent 2, from the layout.")

    s = prs.slides.add_slide(layout(prs, "Section Header"))
    s.shapes.title.text = "Section header"
    s.placeholders[1].text = "Its text placeholder sits under the title"

    s = prs.slides.add_slide(layout(prs, "Two Content"))
    s.shapes.title.text = "Hidden slide"
    s.placeholders[1].text = "Not shown when playing"
    s.placeholders[2].text = "But still parsed"
    hide(s)

    s = prs.slides.add_slide(layout(prs, "Title Only"))
    s.shapes.title.text = "Slide number field"
    box = s.shapes.add_textbox(Inches(11.5), Inches(6.7), Inches(1.5), Inches(0.5))
    p = box.text_frame.paragraphs[0]
    p.alignment = PP_ALIGN.RIGHT
    r = p.add_run()
    r.text = "Slide "
    slide_number_field(p)
    save(prs, "01-masters-placeholders.pptx")


def set_theme(prs, colors, major, minor):
    """Rewrites the first master's theme colours and fonts."""
    theme_part = prs.slide_master.part.part_related_by(
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme"
    )
    root = etree.fromstring(theme_part.blob)
    scheme = root.find(".//" + qn("a:clrScheme"))
    scheme.set("name", "Lampstand")
    for slot, rgb in colors.items():
        el = scheme.find(qn("a:" + slot))
        for child in list(el):
            el.remove(child)
        etree.SubElement(el, qn("a:srgbClr"), val=rgb)
    fonts = root.find(".//" + qn("a:fontScheme"))
    fonts.find(qn("a:majorFont")).find(qn("a:latin")).set("typeface", major)
    fonts.find(qn("a:minorFont")).find(qn("a:latin")).set("typeface", minor)
    theme_part._blob = etree.tostring(root, xml_declaration=True, encoding="UTF-8", standalone=True)


def deck_02():
    """Theme colours (with brightness changes) and theme fonts."""
    prs = wide(Presentation())
    set_theme(
        prs,
        {
            "dk2": "1F2A44", "lt2": "E8E4D8",
            "accent1": "2E6F9E", "accent2": "C8553D", "accent3": "F2A541",
            "accent4": "588B5B", "accent5": "8E5572", "accent6": "3BA99C",
        },
        major="Cambria",
        minor="Calibri",
    )
    s = title_only(prs, "Theme colours")
    accents = [MSO_THEME_COLOR.ACCENT_1, MSO_THEME_COLOR.ACCENT_2, MSO_THEME_COLOR.ACCENT_3,
               MSO_THEME_COLOR.ACCENT_4, MSO_THEME_COLOR.ACCENT_5, MSO_THEME_COLOR.ACCENT_6]
    for i, accent in enumerate(accents):
        for j, bright in enumerate((-0.5, -0.25, 0, 0.4, 0.8)):
            b = add_box(s, 0.8 + i * 2.0, 1.8 + j * 1.0, 1.8, 0.85, f"{i + 1}: {bright:+.2f}", 12)
            b.fill.solid()
            b.fill.fore_color.theme_color = accent
            b.fill.fore_color.brightness = bright
            b.line.fill.background()
            run = b.text_frame.paragraphs[0].runs[0]
            run.font.color.theme_color = MSO_THEME_COLOR.LIGHT_1 if bright < 0.4 else MSO_THEME_COLOR.DARK_2

    s = title_only(prs, "Theme fonts: Cambria for headings")
    box = s.shapes.add_textbox(Inches(0.8), Inches(1.8), Inches(11.5), Inches(4.5))
    tf = box.text_frame
    tf.word_wrap = True
    tf.text = "Body text uses the minor font, Calibri."
    p = tf.add_paragraph()
    r = p.add_run()
    r.text = "This run asks for the major font."
    r.font.name = "+mj-lt"
    r.font.size = Pt(28)
    p = tf.add_paragraph()
    r = p.add_run()
    r.text = "This run asks for Montserrat, which is not installed with Lampstand."
    r.font.name = "Montserrat"
    r.font.size = Pt(24)
    r.font.color.theme_color = MSO_THEME_COLOR.TEXT_2
    p = tf.add_paragraph()
    r = p.add_run()
    r.text = "Text 2 and Background 2 come from the colour map."
    r.font.color.theme_color = MSO_THEME_COLOR.TEXT_2
    bg = add_box(s, 0.8, 5.6, 11.5, 1.0, "A Background 2 box", 20)
    bg.fill.solid()
    bg.fill.fore_color.theme_color = MSO_THEME_COLOR.BACKGROUND_2
    bg.text_frame.paragraphs[0].runs[0].font.color.theme_color = MSO_THEME_COLOR.TEXT_2
    save(prs, "02-theme-colors-fonts.pptx")


def deck_03():
    """Paragraphs, runs, bullets, spacing, anchors and normAutofit."""
    prs = wide(Presentation())
    s = title_only(prs, "Runs and alignment")
    box = s.shapes.add_textbox(Inches(0.8), Inches(1.6), Inches(6), Inches(5))
    tf = box.text_frame
    tf.word_wrap = True
    p = tf.paragraphs[0]
    for text, style in (("Bold ", "b"), ("italic ", "i"), ("underline ", "u"), ("strike ", "s"), ("red ", "red"), ("big", "big")):
        r = p.add_run()
        r.text = text
        r.font.size = Pt(24)
        if style == "b":
            r.font.bold = True
        if style == "i":
            r.font.italic = True
        if style == "u":
            r.font.underline = True
        if style == "s":
            r.font._rPr.set("strike", "sngStrike")
        if style == "red":
            r.font.color.rgb = RGBColor(0xC0, 0x20, 0x20)
        if style == "big":
            r.font.size = Pt(40)
    for align, label in ((PP_ALIGN.LEFT, "Left aligned"), (PP_ALIGN.CENTER, "Centred"), (PP_ALIGN.RIGHT, "Right aligned"),
                         (PP_ALIGN.JUSTIFY, "Justified text runs to both edges of the box when it wraps onto a second line like this one does.")):
        p = tf.add_paragraph()
        p.text = label
        p.alignment = align
        p.runs[0].font.size = Pt(20)
    p = tf.add_paragraph()
    # The paragraph setter turns a vertical tab into a:br (a run's would
    # escape it).
    p.text = "A line break\vinside one paragraph"
    for r in p.runs:
        r.font.size = Pt(20)

    box = s.shapes.add_textbox(Inches(7.3), Inches(1.6), Inches(5.3), Inches(5))
    tf = box.text_frame
    tf.word_wrap = True
    tf.text = "Single spacing, 20 pt after"
    tf.paragraphs[0].space_after = Pt(20)
    p = tf.add_paragraph()
    p.text = "One and a half line spacing wraps onto a second line here"
    p.line_spacing = 1.5
    p = tf.add_paragraph()
    p.text = "Exactly 40 pt between lines, which also wraps onto a second line"
    p.line_spacing = Pt(40)
    p.space_before = Pt(12)
    for p in tf.paragraphs:
        for r in p.runs:
            r.font.size = Pt(20)

    s = title_only(prs, "Bullets and numbering")
    box = s.shapes.add_textbox(Inches(0.8), Inches(1.6), Inches(5.6), Inches(5))
    tf = box.text_frame
    tf.word_wrap = True
    items = [("Numbered one", 0, "num"), ("Numbered two", 0, "num"), ("Lettered a", 1, "alpha"), ("Lettered b", 1, "alpha"),
             ("Numbered three", 0, "num"), ("Roman i", 1, "roman"), ("Roman ii", 1, "roman")]
    for i, (text, level, kind) in enumerate(items):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.text = text
        p.level = level
        ppr = p._p.get_or_add_pPr()
        ppr.set("marL", str(Pt(30 + 30 * level)))
        ppr.set("indent", str(Pt(-30)))
        autonum(p, {"num": "arabicPeriod", "alpha": "alphaLcParenR", "roman": "romanLcPeriod"}[kind])
        p.runs[0].font.size = Pt(22)
    box = s.shapes.add_textbox(Inches(6.8), Inches(1.6), Inches(5.8), Inches(5))
    tf = box.text_frame
    tf.word_wrap = True
    for i, (text, level, char) in enumerate((("Round bullet", 0, "•"), ("Dash bullet one level in", 1, "–"),
                                             ("Arrow bullet two levels in", 2, "»"), ("Round bullet again", 0, "•"))):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.text = text
        p.level = level
        bullet_char(p, char, indent_pt=-24, margin_pt=24 + 36 * level)
        p.runs[0].font.size = Pt(22)

    s = title_only(prs, "Anchors and autofit")
    for i, (anchor, label) in enumerate(((MSO_ANCHOR.TOP, "Top"), (MSO_ANCHOR.MIDDLE, "Middle"), (MSO_ANCHOR.BOTTOM, "Bottom"))):
        b = add_box(s, 0.8 + i * 2.2, 1.7, 2.0, 2.4, label, 20)
        b.text_frame.vertical_anchor = anchor
    b = add_box(s, 7.6, 1.7, 5.0, 2.4, "", 20)
    b.fill.solid()
    b.fill.fore_color.rgb = RGBColor(0xEE, 0xEE, 0xEE)
    tf = b.text_frame
    tf.word_wrap = True
    tf.text = ("This box holds more text than fits at 28 pt, so PowerPoint saved a 62.5% "
               "font scale and a 20% line spacing reduction. The parser applies both.")
    for r in tf.paragraphs[0].runs:
        r.font.size = Pt(28)
        r.font.color.rgb = RGBColor(0x20, 0x20, 0x20)
    norm_autofit(tf, font_scale=62.5, reduction=20)
    tb = s.shapes.add_textbox(Inches(0.8), Inches(4.6), Inches(2), Inches(0.6))
    tb.text_frame.word_wrap = False
    tb.text_frame.text = "A text box that does not wrap runs past its edge"
    tb.text_frame.paragraphs[0].runs[0].font.size = Pt(20)
    rot = s.shapes.add_textbox(Inches(9.5), Inches(4.8), Inches(3), Inches(0.8))
    rot.text_frame.text = "Rotated 20 degrees"
    rot.text_frame.paragraphs[0].runs[0].font.size = Pt(24)
    rot.rotation = 20
    vert = add_box(s, 7.6, 4.6, 1.0, 2.5, "Vertical", 20)
    vert.text_frame._txBody.bodyPr.set("vert", "vert270")
    save(prs, "03-text.pptx")


def deck_04():
    """The 40 common preset shapes (see COMMON_PRESETS in geometry.rs)."""
    names = [
        "RECTANGLE", "ROUNDED_RECTANGLE", None, "HEXAGON", "OVAL", None, "LEFT_ARROW", "STRIPED_RIGHT_ARROW",
        "RIGHT_ARROW", "CHEVRON", "FLOWCHART_MERGE", "ROUNDED_RECTANGULAR_CALLOUT", "RIGHT_TRIANGLE", "PIE",
        "FLOWCHART_EXTRACT", "ISOSCELES_TRIANGLE", "PENTAGON", "CORNER", "FLOWCHART_DELAY", None,
        "BENT_UP_ARROW", "RIGHT_BRACE", "ACTION_BUTTON_CUSTOM", None, "STAR_8_POINT", "LEFT_BRACE", "ARC",
        "SUN", "FLOWCHART_MANUAL_INPUT", "SMILEY_FACE", "STAR_5_POINT", "DIAMOND", "REGULAR_PENTAGON",
        "PARALLELOGRAM", "TRAPEZOID", "OCTAGON", "DOWN_ARROW", "UP_ARROW", "RECTANGULAR_CALLOUT", "CLOUD",
    ]
    connectors = {2: MSO_CONNECTOR.STRAIGHT, 5: MSO_CONNECTOR.STRAIGHT, 19: MSO_CONNECTOR.ELBOW, 23: MSO_CONNECTOR.CURVE}
    prs = wide(Presentation())
    fills = ["2E6F9E", "C8553D", "F2A541", "588B5B", "8E5572", "3BA99C", "4F4F4F", "B0413E"]
    for page in range(2):
        s = title_only(prs, f"Preset shapes {page * 20 + 1} to {page * 20 + 20}")
        for k in range(20):
            i = page * 20 + k
            col, row = k % 5, k // 5
            x, y = 0.7 + col * 2.5, 1.7 + row * 1.4
            if i in connectors:
                c = s.shapes.add_connector(connectors[i], Inches(x), Inches(y), Inches(x + 1.9), Inches(y + 1.0))
                c.line.width = Pt(3)
                c.line.color.rgb = RGBColor.from_string(fills[i % 8])
                if i == 5:
                    # python-pptx's straight connector is `line`.
                    c._element.spPr.find(qn("a:prstGeom")).set("prst", "straightConnector1")
                    line_ends(c, tail="triangle")
                continue
            shp = s.shapes.add_shape(getattr(MSO_SHAPE, names[i]), Inches(x), Inches(y), Inches(1.9), Inches(1.1))
            shp.fill.solid()
            shp.fill.fore_color.rgb = RGBColor.from_string(fills[i % 8])
            shp.line.color.rgb = RGBColor(0x22, 0x22, 0x22)
            shp.line.width = Pt(1.5)
    save(prs, "04-preset-shapes.pptx")


def deck_05():
    """Groups (nested, scaled child space, rotated), flips, connectors and line ends."""
    prs = wide(Presentation())
    s = title_only(prs, "Groups")
    outer = s.shapes.add_group_shape()
    a1 = outer.shapes.add_shape(MSO_SHAPE.ROUNDED_RECTANGLE, Inches(1), Inches(2), Inches(3), Inches(1.5))
    a1.text_frame.text = "Outer group"
    inner = outer.shapes.add_group_shape()
    b1 = inner.shapes.add_shape(MSO_SHAPE.OVAL, Inches(1.5), Inches(4), Inches(1.5), Inches(1.5))
    b1.fill.solid()
    b1.fill.fore_color.rgb = RGBColor(0xC8, 0x55, 0x3D)
    b2 = inner.shapes.add_shape(MSO_SHAPE.ISOSCELES_TRIANGLE, Inches(3.2), Inches(4), Inches(1.5), Inches(1.5))
    b2.fill.solid()
    b2.fill.fore_color.rgb = RGBColor(0x58, 0x8B, 0x5B)
    # Shrink the outer group to 60% of its child space: children scale, the
    # outline width and the type stay in points.
    xfrm = outer._element.grpSpPr.find(qn("a:xfrm"))
    ext = xfrm.find(qn("a:ext"))
    ext.set("cx", str(int(int(ext.get("cx")) * 0.6)))
    ext.set("cy", str(int(int(ext.get("cy")) * 0.6)))

    rotated = s.shapes.add_group_shape()
    r1 = rotated.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(6), Inches(2), Inches(2.5), Inches(1))
    r1.text_frame.text = "Rotated group"
    r2 = rotated.shapes.add_shape(MSO_SHAPE.RIGHT_ARROW, Inches(6), Inches(3.2), Inches(2.5), Inches(1))
    r2.fill.solid()
    r2.fill.fore_color.rgb = RGBColor(0xF2, 0xA5, 0x41)
    rotated.rotation = 30

    flip = add_box(s, 9.5, 2, 3, 1.5, "Flipped arrow, text unflipped", 16, MSO_SHAPE.PENTAGON)
    flip._element.spPr.find(qn("a:xfrm")).set("flipH", "1")
    flipv = add_box(s, 9.5, 4.2, 3, 1.5, "Flipped vertically", 16, MSO_SHAPE.PENTAGON)
    flipv._element.spPr.find(qn("a:xfrm")).set("flipV", "1")

    s = title_only(prs, "Connectors and line ends")
    kinds = ["triangle", "stealth", "diamond", "oval", "arrow"]
    for i, kind in enumerate(kinds):
        c = s.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, Inches(0.8), Inches(1.8 + i * 0.9), Inches(5.5), Inches(1.8 + i * 0.9))
        c.line.width = Pt(2.5)
        c.line.color.rgb = RGBColor(0x1F, 0x2A, 0x44)
        line_ends(c, head=kind if i % 2 else None, tail=kind, w=["sm", "med", "lg"][i % 3], length=["sm", "med", "lg"][i % 3])
    elbow = s.shapes.add_connector(MSO_CONNECTOR.ELBOW, Inches(6.5), Inches(1.8), Inches(9.5), Inches(4))
    elbow.line.width = Pt(3)
    elbow.line.color.rgb = RGBColor(0xC8, 0x55, 0x3D)
    line_ends(elbow, tail="triangle")
    curve = s.shapes.add_connector(MSO_CONNECTOR.CURVE, Inches(6.5), Inches(4.5), Inches(12), Inches(6.5))
    curve.line.width = Pt(3)
    curve.line.color.rgb = RGBColor(0x3B, 0xA9, 0x9C)
    line_ends(curve, head="oval", tail="stealth")
    for i, dash in enumerate((MSO_LINE_DASH_STYLE.DASH, MSO_LINE_DASH_STYLE.ROUND_DOT, MSO_LINE_DASH_STYLE.LONG_DASH_DOT)):
        c = s.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, Inches(10), Inches(1.8 + i * 0.7), Inches(12.6), Inches(1.8 + i * 0.7))
        c.line.width = Pt(3)
        c.line.dash_style = dash
        c.line.color.rgb = RGBColor(0x58, 0x8B, 0x5B)
    save(prs, "05-groups-connectors.pptx")


def deck_06():
    """Solid, see-through and gradient fills; outlines; outer shadows."""
    prs = wide(Presentation())
    s = title_only(prs, "Fills")
    b = add_box(s, 0.8, 1.7, 2.6, 2, "Solid", 20)
    b.fill.solid()
    b.fill.fore_color.rgb = RGBColor(0x2E, 0x6F, 0x9E)
    b = add_box(s, 2.2, 2.6, 2.6, 2, "50% see-through", 18)
    alpha_fill(b, "C8553D", 50)
    for i, angle in enumerate((0, 45, 90)):
        g = add_box(s, 5.4 + i * 2.6, 1.7, 2.4, 2, f"Linear {angle}°", 18)
        g.fill.gradient()
        g.fill.gradient_angle = angle
        g.fill.gradient_stops[0].color.rgb = RGBColor(0xF2, 0xA5, 0x41)
        g.fill.gradient_stops[1].color.rgb = RGBColor(0x8E, 0x55, 0x72)
    g = add_box(s, 0.8, 4.6, 2.6, 2.4, "Radial", 18, MSO_SHAPE.OVAL)
    radial_fill(g, "FFFFFF", "2E6F9E")
    g.text_frame.paragraphs[0].runs[0].font.color.rgb = RGBColor(0x10, 0x10, 0x10)
    g = add_box(s, 3.8, 4.6, 3.6, 2.4, "Three stops", 18)
    g.fill.gradient()
    g.fill.gradient_angle = 0
    stops = g.fill.gradient_stops
    stops[0].color.rgb = RGBColor(0x3B, 0xA9, 0x9C)
    stops[1].color.rgb = RGBColor(0x1F, 0x2A, 0x44)
    gs_lst = sp_pr(g).find(qn("a:gradFill")).find(qn("a:gsLst"))
    mid = copy.deepcopy(gs_lst[0])
    mid.set("pos", "50000")
    mid.find(qn("a:srgbClr")).set("val", "FFFFFF")
    gs_lst.insert(1, mid)
    pat = add_box(s, 7.9, 4.6, 2.4, 2.4, "Pattern", 18)
    pat.fill.patterned()
    pat.fill.pattern = MSO_PATTERN.WIDE_UPWARD_DIAGONAL
    pat.fill.fore_color.rgb = RGBColor(0x58, 0x8B, 0x5B)
    pat.fill.back_color.rgb = RGBColor(0xE8, 0xE4, 0xD8)
    none = add_box(s, 10.7, 4.6, 2.0, 2.4, "No fill", 18)
    none.fill.background()
    none.line.color.rgb = RGBColor(0x1F, 0x2A, 0x44)
    none.text_frame.paragraphs[0].runs[0].font.color.rgb = RGBColor(0x1F, 0x2A, 0x44)

    s = title_only(prs, "Outlines and shadows")
    for i, (width, dash) in enumerate(((0.75, None), (3, None), (6, None), (3, MSO_LINE_DASH_STYLE.DASH), (3, MSO_LINE_DASH_STYLE.SQUARE_DOT))):
        b = add_box(s, 0.8 + i * 2.5, 1.7, 2.1, 1.6, f"{width} pt", 18)
        b.fill.solid()
        b.fill.fore_color.rgb = RGBColor(0xE8, 0xE4, 0xD8)
        b.line.color.rgb = RGBColor(0xC8, 0x55, 0x3D)
        b.line.width = Pt(width)
        if dash:
            b.line.dash_style = dash
        b.text_frame.paragraphs[0].runs[0].font.color.rgb = RGBColor(0x1F, 0x2A, 0x44)
    for i, (blur, dist, direction) in enumerate(((4, 4, 45), (12, 8, 90), (0, 6, 135), (20, 0, 0))):
        b = add_box(s, 0.8 + i * 3.1, 4.3, 2.5, 1.8, f"Shadow {blur}/{dist}", 18, MSO_SHAPE.ROUNDED_RECTANGLE)
        b.fill.solid()
        b.fill.fore_color.rgb = RGBColor(0x2E, 0x6F, 0x9E)
        b.line.fill.background()
        outer_shadow(b, blur=blur, dist=dist, dir_deg=direction)
    save(prs, "06-fills-lines-shadows.pptx")


def deck_07():
    """Pictures (PNG with alpha, JPEG, cropped, rotated, outlined) and video."""
    prs = wide(Presentation())
    s = title_only(prs, "Pictures")
    s.shapes.add_picture(jpeg(landscape()), Inches(0.8), Inches(1.6), Inches(4.8))
    s.shapes.add_picture(png(logo()), Inches(6.0), Inches(1.6), Inches(2.2))
    pic = s.shapes.add_picture(png(grid_image()), Inches(8.7), Inches(1.6), Inches(2.0), Inches(1.5))
    pic.crop_left = 0.5
    pic.crop_top = 0.5
    pic = s.shapes.add_picture(png(grid_image()), Inches(6.0), Inches(4.3), Inches(2.4), Inches(1.8))
    pic.rotation = -12
    pic.line.color.rgb = RGBColor(0x1F, 0x2A, 0x44)
    pic.line.width = Pt(4)
    pic = s.shapes.add_picture(jpeg(landscape()), Inches(9.0), Inches(4.3), Inches(3.2), Inches(1.8))
    pic.crop_right = 0.3

    s = title_only(prs, "Video")
    s.shapes.add_movie(
        io.BytesIO(clip_mp4()), Inches(2.5), Inches(1.8), Inches(8), Inches(4.5),
        poster_frame_image=png(grid_image(160, 90)), mime_type="video/mp4",
    )
    save(prs, "07-pictures-media.pptx")


def deck_08():
    """Tables: the default style, merged cells, cell fills and margins."""
    prs = wide(Presentation())
    s = title_only(prs, "Service plan")
    rows = [["Time", "Item", "Leader"], ["10:00", "Welcome", "Pastor"], ["10:05", "Worship (3 songs)", "Band"],
            ["10:25", "Reading: Psalm 23", "Reader"], ["10:30", "Sermon", "Pastor"], ["11:00", "Blessing", "Pastor"]]
    shape = s.shapes.add_table(len(rows), 3, Inches(0.8), Inches(1.7), Inches(7), Inches(4))
    t = shape.table
    t.columns[0].width = Inches(1.5)
    t.columns[1].width = Inches(3.5)
    t.columns[2].width = Inches(2)
    for r, row in enumerate(rows):
        for c, text in enumerate(row):
            t.cell(r, c).text = text
    t.cell(3, 1).fill.solid()
    t.cell(3, 1).fill.fore_color.rgb = RGBColor(0xF2, 0xA5, 0x41)

    shape = s.shapes.add_table(4, 3, Inches(8.3), Inches(1.7), Inches(4.4), Inches(3))
    t = shape.table
    t.first_row = True
    t.horz_banding = False
    t.first_col = True
    t.cell(0, 0).merge(t.cell(0, 2))
    t.cell(0, 0).text = "Merged header"
    t.cell(1, 0).merge(t.cell(3, 0))
    t.cell(1, 0).text = "Rows merged"
    t.cell(1, 0).vertical_anchor = MSO_ANCHOR.MIDDLE
    for r in range(1, 4):
        for c in range(1, 3):
            cell = t.cell(r, c)
            cell.text = f"{r},{c}"
            cell.margin_left = Inches(0.3)
    t.cell(2, 2).fill.background()
    save(prs, "08-tables.pptx")


def deck_09():
    """Backgrounds: slide colours, gradients and pictures, and a layout's own."""
    prs = wide(Presentation())
    s = title_only(prs, "Solid slide background")
    s.background.fill.solid()
    s.background.fill.fore_color.rgb = RGBColor(0x1F, 0x2A, 0x44)
    s.shapes.title.text_frame.paragraphs[0].runs[0].font.color.rgb = RGBColor(0xFF, 0xFF, 0xFF)

    s = title_only(prs, "Gradient slide background")
    s.background.fill.gradient()
    s.background.fill.gradient_angle = 90
    s.background.fill.gradient_stops[0].color.rgb = RGBColor(0xF2, 0xA5, 0x41)
    s.background.fill.gradient_stops[1].color.rgb = RGBColor(0x8E, 0x55, 0x72)

    s = title_only(prs, "Picture slide background")
    picture_background(s, jpeg(landscape()))
    s.shapes.title.text_frame.paragraphs[0].runs[0].font.color.rgb = RGBColor(0xFF, 0xFF, 0xFF)

    lay = layout(prs, "Section Header")
    lay.background.fill.solid()
    lay.background.fill.fore_color.rgb = RGBColor(0x3B, 0xA9, 0x9C)
    s = prs.slides.add_slide(lay)
    s.shapes.title.text = "Background from the layout"
    s.placeholders[1].text = "This slide sets no background of its own"
    save(prs, "09-backgrounds.pptx")


def deck_10():
    """A 4:3 church service deck: lyrics on a photo, scripture, notices."""
    prs = Presentation()  # 10 × 7.5 in, 720 × 540 pt
    s = blank(prs)
    picture_background(s, jpeg(landscape(1200, 900)))
    box = s.shapes.add_textbox(Inches(0.5), Inches(1.5), Inches(9), Inches(4.5))
    tf = box.text_frame
    tf.word_wrap = True
    tf.vertical_anchor = MSO_ANCHOR.MIDDLE
    lines = ["Amazing grace, how sweet the sound", "That saved a wretch like me", "I once was lost, but now am found",
             "Was blind, but now I see"]
    for i, line in enumerate(lines):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.alignment = PP_ALIGN.CENTER
        r = p.add_run()
        r.text = line
        r.font.size = Pt(34)
        r.font.bold = True
        r.font.color.rgb = RGBColor(0xFF, 0xFF, 0xFF)
        text_shadow(r)
    notes(s, "Verse 1. Hold the last line.")

    s = prs.slides.add_slide(layout(prs, "Title and Content"))
    s.shapes.title.text = "Psalm 23"
    tf = s.placeholders[1].text_frame
    verses = ["The Lord is my shepherd; I shall not want.", "He makes me lie down in green pastures. He leads me beside still waters.",
              "He restores my soul. He leads me in paths of righteousness for his name's sake.",
              "Even though I walk through the valley of the shadow of death, I will fear no evil, for you are with me; your rod and your staff, they comfort me.",
              "You prepare a table before me in the presence of my enemies; you anoint my head with oil; my cup overflows."]
    for i, v in enumerate(verses):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.text = v
        no_bullet(p)
    norm_autofit(tf, font_scale=77.5, reduction=20)

    s = title_only(prs, "This week")
    shape = s.shapes.add_table(4, 2, Inches(0.5), Inches(1.6), Inches(5.6), Inches(2.6))
    t = shape.table
    for r, (day, what) in enumerate((("Day", "Event"), ("Tuesday", "Prayer meeting"), ("Thursday", "Youth group"), ("Saturday", "Men's breakfast"))):
        t.cell(r, 0).text = day
        t.cell(r, 1).text = what
    s.shapes.add_picture(png(logo()), Inches(6.6), Inches(1.6), Inches(2.8))
    b = add_box(s, 0.5, 4.8, 9, 1.4, "Coffee and cake in the hall after the service", 24, MSO_SHAPE.ROUNDED_RECTANGLE)
    b.fill.solid()
    b.fill.fore_color.theme_color = MSO_THEME_COLOR.ACCENT_2
    outer_shadow(b, blur=8, dist=5, dir_deg=90)

    s = prs.slides.add_slide(layout(prs, "Title and Content"))
    s.shapes.title.text = "Three things to remember"
    tf = s.placeholders[1].text_frame
    for i, text in enumerate(("God is with us", "We are not alone", "Hope does not disappoint")):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.text = text
        autonum(p, "arabicPeriod")
    notes(s, "Reveal one point at a time.\nPause after the second.")

    s = title_only(prs, "Backup slide")
    hide(s)
    save(prs, "10-church-service.pptx")


DECKS = [deck_01, deck_02, deck_03, deck_04, deck_05, deck_06, deck_07, deck_08, deck_09, deck_10]

if __name__ == "__main__":
    only = sys.argv[1:]
    for deck in DECKS:
        if not only or deck.__name__[-2:] in only:
            deck()
