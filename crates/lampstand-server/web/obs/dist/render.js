// Draws a `Scene` in the DOM the way the wgpu renderer draws it (the rules
// are in docs/ARCHITECTURE.md, "Scenes are the only thing renderers see"):
//
// - every node is a zero-size absolutely positioned wrapper carrying the
//   node's matrix, opacity and shadow, so opacity applies to the node as a
//   whole and shadow, stroke and gradient units are the node's local units;
// - text blocks are flex columns of paragraphs. Line height is
//   size × line_spacing, wrapping is by word then by glyph, a bullet hangs
//   in the indent on the first line's baseline, an outline is drawn under
//   the fill (paint-order: stroke) and overflow is drawn, never clipped;
// - paths are SVG, images are cropped and fitted by hand.
import { cssFamily } from "./fonts.js";
import { css, dropShadow, svgPaint } from "./paint.js";
const SVG = "http://www.w3.org/2000/svg";
function visit(nodes, f) {
    for (const n of nodes) {
        f(n);
        if ("Group" in n.kind)
            visit(n.kind.Group, f);
    }
}
/** Fonts and images the scene needs, loaded before it is shown. */
export async function preload(scene, res) {
    const fonts = new Set();
    const images = new Set();
    visit(scene.nodes, (n) => {
        if ("Text" in n.kind) {
            for (const p of n.kind.Text.paragraphs) {
                for (const r of p.runs)
                    fonts.add(fontShorthand(r));
                const first = p.runs[0];
                if (p.bullet && first)
                    fonts.add(fontShorthand(first));
            }
        }
        else if ("Image" in n.kind) {
            images.add(n.kind.Image.blob);
        }
    });
    const loads = [];
    for (const font of fonts)
        loads.push(document.fonts.load(font, "Ag•").catch(() => []));
    for (const hash of images) {
        if (res.images.has(hash))
            continue;
        loads.push(new Promise((resolve) => {
            const img = new Image();
            img.onload = () => {
                res.images.set(hash, img);
                resolve();
            };
            img.onerror = () => resolve();
            img.src = res.blobUrl(hash);
        }));
    }
    await Promise.all(loads);
}
function fontShorthand(r) {
    return `${r.italic ? "italic" : "normal"} ${r.weight} ${r.size}px ${cssFamily(r.font_family)}`;
}
/** The scene's DOM, ready to be put in the stage. */
export function build(scene, res) {
    const out = document.createDocumentFragment();
    for (const n of scene.nodes) {
        const el = node(n, res);
        if (el)
            out.appendChild(el);
    }
    return out;
}
function node(n, res) {
    if (!n.visible)
        return null;
    const el = document.createElement("div");
    const t = n.transform;
    el.style.cssText = "position:absolute;left:0;top:0;width:0;height:0;transform-origin:0 0;";
    const identity = t.a === 1 && t.b === 0 && t.c === 0 && t.d === 1 && t.e === 0 && t.f === 0;
    if (!identity)
        el.style.transform = `matrix(${t.a},${t.b},${t.c},${t.d},${t.e},${t.f})`;
    if (n.opacity < 1)
        el.style.opacity = String(Math.max(n.opacity, 0));
    el.dataset.node = String(n.id);
    fill(el, n.kind, res);
    return el;
}
function fill(el, kind, res) {
    if ("Group" in kind) {
        for (const c of kind.Group) {
            const child = node(c, res);
            if (child)
                el.appendChild(child);
        }
    }
    else if ("Path" in kind) {
        path(el, kind.Path);
    }
    else if ("Text" in kind) {
        text(el, kind.Text);
    }
    else if ("Image" in kind) {
        image(el, kind.Image, res);
    }
    else {
        video(el, kind.Video, res);
    }
}
function d(commands) {
    return commands
        .map((c) => {
        if (c === "Close")
            return "Z";
        if ("MoveTo" in c)
            return `M${c.MoveTo.x} ${c.MoveTo.y}`;
        if ("LineTo" in c)
            return `L${c.LineTo.x} ${c.LineTo.y}`;
        if ("QuadTo" in c) {
            const [p, q] = c.QuadTo;
            return `Q${p.x} ${p.y} ${q.x} ${q.y}`;
        }
        const [p, q, r] = c.CubicTo;
        return `C${p.x} ${p.y} ${q.x} ${q.y} ${r.x} ${r.y}`;
    })
        .join("");
}
function path(el, p) {
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("width", "1");
    svg.setAttribute("height", "1");
    svg.style.cssText = "position:absolute;left:0;top:0;overflow:visible;";
    const defs = document.createElementNS(SVG, "defs");
    svg.appendChild(defs);
    const shape = document.createElementNS(SVG, "path");
    shape.setAttribute("d", d(p.commands));
    shape.setAttribute("fill", p.fill ? svgPaint(p.fill, defs) : "none");
    shape.setAttribute("fill-rule", "nonzero");
    if (p.stroke) {
        shape.setAttribute("stroke", svgPaint(p.stroke.paint, defs));
        shape.setAttribute("stroke-width", String(p.stroke.width));
        shape.setAttribute("stroke-linejoin", "miter");
        shape.setAttribute("stroke-linecap", "butt");
        if (p.stroke.dash && p.stroke.dash.length > 0) {
            shape.setAttribute("stroke-dasharray", p.stroke.dash.join(" "));
        }
    }
    else {
        shape.setAttribute("stroke", "none");
    }
    svg.appendChild(shape);
    el.appendChild(svg);
    if (p.shadow)
        el.style.filter = dropShadow(p.shadow);
}
const JUSTIFY = { Top: "flex-start", Middle: "center", Bottom: "flex-end" };
const ALIGN = { Left: "left", Center: "center", Right: "right", Justify: "justify" };
function text(el, b) {
    const box = document.createElement("div");
    box.style.cssText =
        `position:absolute;left:${b.rect.x}px;top:${b.rect.y}px;` +
            `width:${b.rect.width}px;height:${b.rect.height}px;` +
            `display:flex;flex-direction:column;justify-content:${JUSTIFY[b.valign]};` +
            "overflow:visible;text-rendering:geometricPrecision;font-kerning:normal;" +
            "-webkit-font-smoothing:antialiased;";
    if (b.outline && b.outline.width > 0) {
        // The outline shows `width` outside the glyph edge: a stroke of twice
        // that, painted first so the fill covers its inner half.
        box.style.cssText +=
            `-webkit-text-stroke:${2 * b.outline.width}px ${css(b.outline.color)};` +
                "paint-order:stroke fill;";
    }
    for (const p of b.paragraphs)
        box.appendChild(paragraph(p));
    el.appendChild(box);
    if (b.shadow)
        el.style.filter = dropShadow(b.shadow);
}
function runStyle(r) {
    return (`font-family:${cssFamily(r.font_family)};font-size:${r.size}px;` +
        `font-weight:${r.weight};font-style:${r.italic ? "italic" : "normal"};color:${css(r.color)};`);
}
function paragraph(p) {
    const el = document.createElement("div");
    // font-size and line-height 0 on the paragraph leave line boxes to the
    // runs' own line height (size × line_spacing) and nothing else.
    el.style.cssText =
        `flex:none;position:relative;margin:${p.space_before}px 0 ${p.space_after}px ${p.indent}px;` +
            `text-align:${ALIGN[p.align]};font-size:0;line-height:0;white-space:pre-wrap;` +
            "overflow-wrap:anywhere;";
    const first = p.runs[0];
    if (!first)
        return el; // No line, only the spacing.
    if (p.bullet) {
        // A zero-width inline box on the first line's baseline, with the
        // bullet hanging out of it to the left, into the indent.
        const anchor = document.createElement("span");
        anchor.style.cssText = "display:inline-block;width:0;overflow:visible;white-space:nowrap;";
        const bullet = document.createElement("span");
        bullet.style.cssText =
            `position:relative;left:${-p.indent}px;text-align:left;${runStyle(first)}` +
                `line-height:${first.size * p.line_spacing}px;`;
        bullet.textContent = p.bullet;
        anchor.appendChild(bullet);
        el.appendChild(anchor);
    }
    for (const r of p.runs) {
        const span = document.createElement("span");
        span.style.cssText = `${runStyle(r)}line-height:${r.size * p.line_spacing}px;`;
        const lines = [r.underline && "underline", r.strike && "line-through"].filter(Boolean);
        if (lines.length > 0) {
            span.style.textDecoration = lines.join(" ");
            span.style.setProperty("text-decoration-skip-ink", "none");
        }
        span.textContent = r.text;
        el.appendChild(span);
    }
    return el;
}
function box(r) {
    return `position:absolute;left:${r.x}px;top:${r.y}px;width:${r.width}px;height:${r.height}px;`;
}
/**
 * The source region (the crop, as fractions of the image) is fitted into
 * the rect; the rect clips, and the part of a crop outside the image
 * stays empty.
 */
function image(el, n, res) {
    const img = res.images.get(n.blob);
    if (!img || img.naturalWidth === 0 || img.naturalHeight === 0)
        return;
    const iw = img.naturalWidth;
    const ih = img.naturalHeight;
    const crop = n.crop ?? { x: 0, y: 0, width: 1, height: 1 };
    const sw = crop.width * iw;
    const sh = crop.height * ih;
    if (!(sw > 0 && sh > 0))
        return;
    const [sx, sy] = fitScale(n.fit, n.rect, sw, sh);
    const dw = sw * sx;
    const dh = sh * sy;
    const outer = document.createElement("div");
    outer.style.cssText = `${box(n.rect)}overflow:hidden;`;
    const region = document.createElement("div");
    region.style.cssText =
        `position:absolute;left:${(n.rect.width - dw) / 2}px;top:${(n.rect.height - dh) / 2}px;` +
            `width:${dw}px;height:${dh}px;overflow:hidden;`;
    const copy = document.createElement("img");
    copy.src = img.src;
    copy.style.cssText =
        `position:absolute;left:${-crop.x * iw * sx}px;top:${-crop.y * ih * sy}px;` +
            `width:${iw * sx}px;height:${ih * sy}px;max-width:none;`;
    region.appendChild(copy);
    outer.appendChild(region);
    el.appendChild(outer);
}
function fitScale(fit, rect, w, h) {
    const fx = rect.width / w;
    const fy = rect.height / h;
    if (fit === "Fill")
        return [fx, fy];
    const s = fit === "Contain" ? Math.min(fx, fy) : Math.max(fx, fy);
    return [s, s];
}
function video(el, n, res) {
    const v = document.createElement("video");
    v.src = res.blobUrl(n.blob);
    v.muted = true; // Browser sources autoplay only when muted.
    v.loop = n.looped;
    v.autoplay = n.autoplay;
    v.playsInline = true;
    v.style.cssText = `${box(n.rect)}object-fit:${n.fit === "Fill" ? "fill" : n.fit.toLowerCase()};`;
    el.appendChild(v);
}
