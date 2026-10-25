import React from "react";

/** Largest font size (<= maxSize) at which `text` wraps into a w x h box. Estimated from average glyph width. */
export function fitSize(text: string, w: number, h: number, maxSize: number, minSize: number, glyph: number, lh: number): number {
  const ws = text.split(/\s+/).filter(Boolean);
  const fits = (size: number) => {
    const perLine = Math.max(1, Math.floor(w / (size * glyph)));
    let lines = 1;
    let cur = 0;
    for (const word of ws) {
      const n = word.length;
      if (n > perLine) return false; // a single word would overflow
      if (cur === 0) cur = n;
      else if (cur + 1 + n <= perLine) cur += 1 + n;
      else { lines++; cur = n; }
    }
    return lines * size * lh <= h;
  };
  let lo = minSize;
  let hi = maxSize;
  if (fits(hi)) return hi;
  while (hi - lo > 1) {
    const mid = Math.floor((lo + hi) / 2);
    if (fits(mid)) lo = mid; else hi = mid;
  }
  return lo;
}

export const FitText: React.FC<{
  text: string; w: number; h: number; maxSize: number; minSize?: number;
  font: string; color: string; weight?: number; italic?: boolean; align?: "left" | "center" | "right";
  lineHeight?: number; letterSpacing?: string; mono?: boolean; vAlign?: "top" | "center" | "bottom";
  style?: React.CSSProperties;
}> = ({ text, w, h, maxSize, minSize = 16, font, color, weight = 400, italic, align = "left", lineHeight = 1.15, letterSpacing, mono, vAlign = "center", style }) => {
  const glyph = (mono ? 0.6 : 0.5) + (weight >= 700 ? 0.04 : 0) + (letterSpacing ? 0.05 : 0);
  const size = fitSize(text, w, h, maxSize, minSize, glyph, lineHeight);
  return (
    <div style={{ width: w, height: h, display: "flex", flexDirection: "column", justifyContent: vAlign === "top" ? "flex-start" : vAlign === "bottom" ? "flex-end" : "center", ...style }}>
      <div style={{ fontFamily: font, fontSize: size, fontWeight: weight, fontStyle: italic ? "italic" : "normal", color, textAlign: align, lineHeight, letterSpacing, textWrap: "balance" as any }}>{text}</div>
    </div>
  );
};
