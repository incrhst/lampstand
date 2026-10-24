import sys, html
OUT = sys.argv[1]
ROLE = {"Projection": "#1f5fbf", "Operator": "#1f7a45", "Stream": "#6a3fc0", "Audio": "#b85a00"}
GRAY = "#5f5e5a"
FONT = "font-family=\"-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif\""

class D:
    def __init__(s, w, h, name):
        s.w, s.h, s.name, s.b, s.colors = w, h, name, [], set()
    def e(s, x): s.b.append(x)
    def text(s, x, y, t, size=14, weight=400, fill="#2b2a27", anchor="start", halo=False):
        st = ' stroke="#ffffff" stroke-width="5" paint-order="stroke" stroke-linejoin="round"' if halo else ''
        s.e(f'<text x="{x}" y="{y}" font-size="{size}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}" {FONT}{st}>{html.escape(t)}</text>')
    def machine(s, x, y, w, h, title, sub, roles):
        s.e(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="14" fill="#fbfaf7" stroke="#8b8982" stroke-width="2"/>')
        s.text(x + 20, y + 28, title, 18, 700)
        s.text(x + 20, y + 48, sub, 13, 400, "#55544f")
        for i, r in enumerate(roles):
            cx = x + 20 + (i % 2) * 180
            cy = y + 66 + (i // 2) * 48
            s.e(f'<rect x="{cx}" y="{cy}" width="160" height="36" rx="18" fill="{ROLE[r]}"/>')
            s.text(cx + 80, cy + 24, r, 15, 700, "#ffffff", "middle")
    def thing(s, x, y, w, h, label, sub=None, dashed=False):
        d = ' stroke-dasharray="6 5"' if dashed else ''
        s.e(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="12" fill="#efede6" stroke="#8b8982" stroke-width="2"{d}/>')
        if sub:
            s.text(x + w/2, y + h/2 - 3, label, 16, 600, anchor="middle"); s.text(x + w/2, y + h/2 + 17, sub, 12.5, 400, "#55544f", "middle")
        else:
            s.text(x + w/2, y + h/2 + 6, label, 16, 600, anchor="middle")
    def arrow(s, pts, color, dashed=False, both=False):
        s.colors.add(color)
        def short(p, q, d=9):
            dx, dy = q[0]-p[0], q[1]-p[1]; n = (dx*dx+dy*dy) ** .5
            return (q[0]-dx/n*d, q[1]-dy/n*d)
        P = list(pts); P[-1] = short(P[-2], P[-1])
        if both:
            P[0] = short(P[1], P[0])
        d = "M " + " L ".join(f"{a:g} {b:g}" for a, b in P)
        da = ' stroke-dasharray="7 6"' if dashed else ''
        cid = color[1:]
        ms = f' marker-end="url(#h{cid})"' + (f' marker-start="url(#s{cid})"' if both else '')
        s.e(f'<path d="{d}" fill="none" stroke="{color}" stroke-width="3" stroke-linejoin="round"{da}{ms}/>')
    def legend(s):
        x = 40
        for r in ROLE:
            s.e(f'<rect x="{x}" y="16" width="120" height="26" rx="13" fill="{ROLE[r]}"/>')
            s.text(x + 60, 34, r, 13.5, 700, "#fff", "middle"); x += 134
        s.e(f'<line x1="{x+10}" y1="29" x2="{x+50}" y2="29" stroke="{GRAY}" stroke-width="3" stroke-dasharray="7 6"/>')
        s.text(x + 60, 34, "grey: equipment, room sound, network", 13, 400, "#55544f")
    def save(s):
        defs = ""
        for c in s.colors:
            i = c[1:]
            defs += f'<marker id="h{i}" markerUnits="userSpaceOnUse" markerWidth="12" markerHeight="12" refX="0" refY="6" orient="auto"><path d="M0 1 L11 6 L0 11 Z" fill="{c}"/></marker>'
            defs += f'<marker id="s{i}" markerUnits="userSpaceOnUse" markerWidth="12" markerHeight="12" refX="0" refY="6" orient="auto-start-reverse"><path d="M0 1 L11 6 L0 11 Z" fill="{c}"/></marker>'
        svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {s.w} {s.h}" width="{s.w}" height="{s.h}" role="img" aria-label="{html.escape(s.name)}">'
               f'<rect width="{s.w}" height="{s.h}" fill="#ffffff"/><defs>{defs}</defs>' + "".join(s.b) + '</svg>')
        open(f"{OUT}/{s.name}.svg", "w").write(svg)

B, O, S, A, G = ROLE["Projection"], ROLE["Operator"], ROLE["Stream"], ROLE["Audio"], GRAY

# 1. one machine
d = D(1100, 640, "roles-one-machine"); d.legend()
d.machine(40, 70, 400, 430, "One computer", "does everything", ["Projection", "Operator", "Stream", "Audio"])
d.thing(60, 330, 360, 100, "OBS + Zoom", "optional, on the same computer")
d.thing(700, 120, 300, 80, "Projector and screens"); d.thing(700, 260, 300, 80, "Mixer"); d.thing(700, 400, 300, 70, "House speakers"); d.thing(700, 520, 300, 70, "Cameras")
d.thing(120, 540, 240, 60, "Zoom call")
d.arrow([(440, 160), (699, 160)], B); d.text(560, 150, "picture", 13, 600, B, halo=True)
d.arrow([(440, 210), (570, 210), (570, 300), (699, 300)], A); d.text(585, 292, "sound", 13, 600, A, halo=True)
d.arrow([(850, 340), (850, 399)], G)
d.arrow([(700, 325), (600, 325), (600, 380), (421, 380)], G, True); d.text(592, 358, "microphones and band", 13, 400, G, "end", True)
d.arrow([(700, 555), (620, 555), (620, 410), (421, 410)], G); 
d.arrow([(140, 230), (140, 329)], S); d.text(152, 290, "graphics and scene control", 13, 600, S, halo=True)
d.arrow([(240, 430), (240, 539)], S); d.text(252, 520, "Virtual Camera", 13, 600, S, halo=True)
d.save()

# 2. two machines
d = D(1100, 800, "roles-two-machines"); d.legend()
d.machine(40, 70, 400, 220, "Machine A: at the projector", "can run unattended", ["Projection", "Operator", "Audio"])
d.machine(40, 380, 400, 310, "Machine B: the stream machine", "OBS, Zoom and the Virtual Camera", ["Stream"])
d.thing(60, 520, 360, 100, "OBS + Zoom", "on machine B")
d.thing(700, 120, 300, 80, "Projector and screens"); d.thing(700, 240, 300, 80, "Mixer"); d.thing(700, 370, 300, 70, "House speakers"); d.thing(700, 590, 300, 70, "Cameras")
d.thing(120, 730, 260, 60, "Zoom call")
d.arrow([(440, 160), (699, 160)], B); d.text(560, 150, "picture", 13, 600, B, halo=True)
d.arrow([(440, 210), (570, 210), (570, 280), (699, 280)], A); d.text(585, 272, "sound", 13, 600, A, halo=True)
d.arrow([(850, 320), (850, 369)], G)
d.arrow([(700, 305), (620, 305), (620, 565), (421, 565)], G, True); d.text(612, 490, "microphones and band", 13, 400, G, "end", True)
d.arrow([(700, 625), (660, 625), (660, 595), (421, 595)], G)
d.arrow([(240, 290), (240, 379)], G, True, both=True); d.text(252, 340, "sync", 13, 400, G, halo=True)
d.arrow([(360, 290), (360, 519)], B); d.text(372, 420, "graphics (browser source)", 13, 600, B, halo=True)
d.arrow([(140, 490), (140, 519)], S); d.text(152, 510, "scene control", 13, 600, S, halo=True)
d.arrow([(240, 620), (240, 729)], S); d.text(252, 700, "Virtual Camera", 13, 600, S, halo=True)
d.save()

def left_column(d, a_roles, a_sub):
    d.machine(40, 70, 380, 170, "Machine A: at the projector", a_sub, a_roles)
    d.machine(40, 310, 380, 124, "Machine B: back of the room", "runs the service", ["Operator"])
    d.machine(40, 500, 380, 260, "Machine C: the stream machine", "OBS, Zoom and the Virtual Camera", ["Stream"])
    d.thing(60, 640, 340, 90, "OBS + Zoom", "on machine C")
    d.thing(120, 800, 260, 56, "Zoom call")
    d.arrow([(140, 310), (140, 241)], O); d.text(152, 285, "next, jump, take live", 13, 600, O, halo=True)
    d.arrow([(320, 241), (320, 309)], O); d.text(332, 285, "what is on air", 13, 600, O, halo=True)
    d.arrow([(420, 205), (470, 205), (470, 660), (401, 660)], B); d.text(482, 440, "graphics and scene cues", 13, 600, B, halo=True); d.text(482, 458, "(browser source)", 13, 600, B, halo=True)
    d.arrow([(140, 604), (140, 639)], S)
    d.arrow([(250, 730), (250, 799)], S); d.text(262, 775, "Virtual Camera", 13, 600, S, halo=True)
    d.arrow([(700, 712), (401, 712)], G)

# 3. three machines
d = D(1100, 900, "roles-three-machines"); d.legend()
left_column(d, ["Projection", "Audio"], "can run unattended")
d.thing(700, 100, 300, 80, "Projector and screens"); d.thing(700, 200, 300, 80, "Mixer"); d.thing(700, 330, 300, 70, "House speakers"); d.thing(700, 677, 300, 70, "Cameras")
d.arrow([(420, 140), (699, 140)], B); d.text(540, 130, "picture", 13, 600, B, halo=True)
d.arrow([(420, 165), (560, 165), (560, 240), (699, 240)], A); d.text(575, 232, "sound", 13, 600, A, halo=True)
d.arrow([(850, 280), (850, 329)], G)
d.arrow([(700, 265), (620, 265), (620, 685), (401, 685)], G, True); d.text(612, 560, "microphones and band", 13, 400, G, "end", True)
d.save()

# 4. four machines
d = D(1100, 900, "roles-four-machines"); d.legend()
left_column(d, ["Projection"], "can run unattended")
d.machine(700, 230, 300, 110, "Machine D: at the sound desk", "plays what Lampstand plays", ["Audio"])
d.thing(700, 100, 300, 80, "Projector and screens"); d.thing(700, 420, 300, 80, "Mixer"); d.thing(700, 560, 300, 70, "House speakers"); d.thing(700, 677, 300, 70, "Cameras")
d.arrow([(420, 140), (699, 140)], B); d.text(540, 130, "picture", 13, 600, B, halo=True)
d.arrow([(420, 175), (520, 175), (520, 290), (699, 290)], A); d.text(532, 282, "sound cues", 13, 600, A, halo=True)
d.arrow([(850, 340), (850, 419)], A); d.text(862, 385, "sound out", 13, 600, A, halo=True)
d.arrow([(850, 500), (850, 559)], G)
d.arrow([(700, 470), (620, 470), (620, 685), (401, 685)], G, True); d.text(612, 590, "microphones and band", 13, 400, G, "end", True)
d.save()

# 5. confidence screen
d = D(1100, 470, "roles-confidence-screen"); d.legend()
d.machine(40, 70, 380, 170, "Machine A: at the projector", "can run unattended", ["Projection"])
d.thing(700, 100, 300, 80, "Projector and screens"); d.thing(700, 220, 300, 80, "Confidence screen", "second display on a cable"); d.thing(700, 350, 300, 80, "Tablet or phone", "browser page (a helper)", True)
d.arrow([(420, 140), (699, 140)], B); d.text(540, 130, "picture", 13, 600, B, halo=True)
d.arrow([(420, 175), (560, 175), (560, 260), (699, 260)], B); d.text(572, 252, "stage output", 13, 600, B, halo=True)
d.arrow([(230, 240), (230, 390), (699, 390)], G, True); d.text(250, 380, "stage page over the church network", 13, 400, G, halo=True)
d.save()
