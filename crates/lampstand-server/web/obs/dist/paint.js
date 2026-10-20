// Paints, shadows and colours the way the wgpu renderer reads them:
// straight-alpha sRGB, blended in sRGB, gradient interpolation on
// premultiplied sRGB (as CSS does), shadow blur as a CSS blur radius.
const SVG = "http://www.w3.org/2000/svg";
export function css(c) {
    return `rgba(${c.r},${c.g},${c.b},${+(c.a / 255).toFixed(4)})`;
}
export function dropShadow(s) {
    return `drop-shadow(${s.offset.x}px ${s.offset.y}px ${Math.max(s.blur, 0)}px ${css(s.color)})`;
}
/**
 * SVG gradients interpolate straight colours; the renderer (and CSS)
 * interpolate premultiplied ones. The two agree when every stop has the
 * same alpha. When they differ, add stops in between that hold the
 * premultiplied blend.
 */
export function premultipliedStops(stops) {
    const sorted = [...stops].sort((p, q) => p.offset - q.offset);
    if (sorted.every((s) => s.color.a === sorted[0].color.a))
        return sorted;
    const out = [];
    const STEPS = 16;
    sorted.forEach((to, i) => {
        if (i === 0) {
            out.push(to);
            return;
        }
        const from = sorted[i - 1];
        for (let k = 1; k <= STEPS; k++) {
            const t = k / STEPS;
            const a = from.color.a + (to.color.a - from.color.a) * t;
            const mix = (x, y) => {
                const pm = x * from.color.a * (1 - t) + y * to.color.a * t;
                return a === 0 ? 0 : Math.round(pm / a);
            };
            out.push({
                offset: from.offset + (to.offset - from.offset) * t,
                color: {
                    r: mix(from.color.r, to.color.r),
                    g: mix(from.color.g, to.color.g),
                    b: mix(from.color.b, to.color.b),
                    a: Math.round(a),
                },
            });
        }
    });
    return out;
}
let gradientCount = 0;
/**
 * An SVG paint value for `paint`; gradients are added to `defs` in the
 * node's local units.
 */
export function svgPaint(paint, defs) {
    if ("Solid" in paint)
        return css(paint.Solid);
    const id = `g${gradientCount++}`;
    let g;
    let stops;
    if ("Linear" in paint) {
        const { start, end } = paint.Linear;
        g = document.createElementNS(SVG, "linearGradient");
        g.setAttribute("x1", String(start.x));
        g.setAttribute("y1", String(start.y));
        g.setAttribute("x2", String(end.x));
        g.setAttribute("y2", String(end.y));
        stops = paint.Linear.stops;
    }
    else {
        const { center, radius } = paint.Radial;
        g = document.createElementNS(SVG, "radialGradient");
        g.setAttribute("cx", String(center.x));
        g.setAttribute("cy", String(center.y));
        g.setAttribute("fx", String(center.x));
        g.setAttribute("fy", String(center.y));
        g.setAttribute("r", String(radius));
        stops = paint.Radial.stops;
    }
    g.setAttribute("id", id);
    g.setAttribute("gradientUnits", "userSpaceOnUse");
    for (const s of premultipliedStops(stops)) {
        const el = document.createElementNS(SVG, "stop");
        el.setAttribute("offset", String(s.offset));
        el.setAttribute("stop-color", `rgb(${s.color.r},${s.color.g},${s.color.b})`);
        el.setAttribute("stop-opacity", String(+(s.color.a / 255).toFixed(4)));
        g.appendChild(el);
    }
    defs.appendChild(g);
    return `url(#${id})`;
}
