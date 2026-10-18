//! Paints: solid colours and gradient ramps.
//!
//! Each gradient becomes one 256-texel row of a ramp texture, built on the
//! CPU, so any number of stops costs one texture lookup. Stops interpolate
//! in premultiplied sRGB, as CSS gradients do.

use lampstand_core::geom::Color;
use lampstand_core::scene::{GradientStop, Paint};

use crate::gpu::PaintUniform;

pub(crate) const RAMP_WIDTH: usize = 256;

pub(crate) fn premultiplied(color: Color) -> [f32; 4] {
    let a = color.a as f32 / 255.0;
    [
        color.r as f32 / 255.0 * a,
        color.g as f32 / 255.0 * a,
        color.b as f32 / 255.0 * a,
        a,
    ]
}

/// Ramps for one frame; `rows` is filled in `RAMP_WIDTH` chunks.
#[derive(Default)]
pub(crate) struct Ramps {
    pub texels: Vec<[u8; 4]>,
}

impl Ramps {
    pub fn rows(&self) -> usize {
        self.texels.len() / RAMP_WIDTH
    }

    fn push(&mut self, stops: &[GradientStop]) -> usize {
        let row = self.rows();
        let mut sorted: Vec<GradientStop> = stops.to_vec();
        sorted.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        for i in 0..RAMP_WIDTH {
            let t = i as f32 / (RAMP_WIDTH - 1) as f32;
            let c = sample(&sorted, t);
            self.texels
                .push(c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8));
        }
        row
    }

    /// The uniform for `paint`, with `opacity` folded in.
    pub fn uniform(&mut self, paint: &Paint, opacity: f32) -> PaintUniform {
        let mut u = PaintUniform {
            opacity,
            ..Default::default()
        };
        match paint {
            Paint::Solid(color) => u.color = premultiplied(*color),
            Paint::Linear { start, end, stops } => {
                u.kind = 1;
                u.geom = [start.x, start.y, end.x, end.y];
                u.ramp_v = self.ramp(stops);
            }
            Paint::Radial {
                center,
                radius,
                stops,
            } => {
                u.kind = 2;
                u.geom = [center.x, center.y, *radius, 0.0];
                u.ramp_v = self.ramp(stops);
            }
        }
        u
    }

    /// Row index for now; converted to a texture coordinate once the
    /// number of rows is known (see `finish_v`).
    fn ramp(&mut self, stops: &[GradientStop]) -> f32 {
        self.push(stops) as f32
    }
}

/// Turns a row index stored in `ramp_v` into the row's texel-centre `v`.
pub(crate) fn finish_v(row: f32, rows: usize) -> f32 {
    (row + 0.5) / rows.max(1) as f32
}

fn sample(stops: &[GradientStop], t: f32) -> [f32; 4] {
    let Some(first) = stops.first() else {
        return [0.0; 4];
    };
    if t <= first.offset {
        return premultiplied(first.color);
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.offset {
            let span = b.offset - a.offset;
            let f = if span > 0.0 {
                (t - a.offset) / span
            } else {
                1.0
            };
            let (ca, cb) = (premultiplied(a.color), premultiplied(b.color));
            return std::array::from_fn(|i| ca[i] + (cb[i] - ca[i]) * f);
        }
    }
    premultiplied(stops[stops.len() - 1].color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(offset: f32, color: Color) -> GradientStop {
        GradientStop { offset, color }
    }

    #[test]
    fn ramp_hits_end_stops_and_midpoint() {
        let mut ramps = Ramps::default();
        ramps.push(&[stop(1.0, Color::WHITE), stop(0.0, Color::BLACK)]);
        assert_eq!(ramps.texels[0], [0, 0, 0, 255]);
        assert_eq!(ramps.texels[RAMP_WIDTH - 1], [255, 255, 255, 255]);
        assert!((ramps.texels[128][0] as i32 - 128).abs() <= 1);
    }

    #[test]
    fn interpolates_premultiplied() {
        let red_clear = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 0,
        };
        let mid = sample(
            &[stop(0.0, red_clear), stop(1.0, Color::rgb(0, 0, 255))],
            0.5,
        );
        // No red bleeds in from the transparent stop.
        assert_eq!(mid, [0.0, 0.0, 0.5, 0.5]);
    }

    #[test]
    fn clamps_outside_the_stops() {
        let stops = [stop(0.25, Color::BLACK), stop(0.75, Color::WHITE)];
        assert_eq!(sample(&stops, 0.0), premultiplied(Color::BLACK));
        assert_eq!(sample(&stops, 1.0), premultiplied(Color::WHITE));
    }
}
