//! Per-display margins, in the same `calibration.json` format as
//! lampstand-render's `calibrate` example:
//!
//! ```json
//! { "DELL U2720Q": { "width": 3840, "height": 2160,
//!                    "safe_area": { "top": 0.03, "right": 0.02, "bottom": 0.05, "left": 0.02 } } }
//! ```
//!
//! and the keys that adjust them while the grid is up.

use std::path::Path;

use lampstand_render::calibration::Edge;
use lampstand_render::winit::keyboard::NamedKey;
use lampstand_render::SafeArea;
use serde_json::{json, Value};

/// The saved margins for display `name`, if any.
pub fn load(path: &Path, name: &str) -> Option<SafeArea> {
    let all: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let saved = all.get(name)?.get("safe_area")?;
    let mut safe = SafeArea::default();
    for edge in Edge::ALL {
        if let Some(v) = saved.get(edge.name().to_lowercase()).and_then(Value::as_f64) {
            *edge.margin_mut(&mut safe) = v as f32;
        }
    }
    Some(safe)
}

/// Save margins for display `name`, keeping every other display's entry.
pub fn save(path: &Path, name: &str, width: u32, height: u32, safe: SafeArea) -> std::io::Result<()> {
    let mut all = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let round = |m: f32| (m as f64 * 1000.0).round() / 1000.0;
    all[name] = json!({
        "width": width,
        "height": height,
        "safe_area": { "top": round(safe.top), "right": round(safe.right), "bottom": round(safe.bottom), "left": round(safe.left) },
    });
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&all).expect("JSON") + "\n")
}

/// Move the active edge with an arrow key, as in the calibrate example: an
/// arrow pointing along the edge's axis moves it that way; one along the
/// other axis makes the edge it points at active. `big` is Shift.
pub fn arrow(safe: &mut SafeArea, edge: &mut Edge, key: NamedKey, big: bool) {
    let step = if big { 0.02 } else { 0.005 };
    let (towards, inward) = match key {
        NamedKey::ArrowUp => (Edge::Top, Edge::Bottom),
        NamedKey::ArrowDown => (Edge::Bottom, Edge::Top),
        NamedKey::ArrowLeft => (Edge::Left, Edge::Right),
        NamedKey::ArrowRight => (Edge::Right, Edge::Left),
        _ => return,
    };
    let delta = if *edge == towards {
        -step
    } else if *edge == inward {
        step
    } else {
        *edge = towards;
        return;
    };
    let m = edge.margin_mut(safe);
    *m = (*m + delta).clamp(0.0, 0.45);
}

/// The edge after `edge` (Tab), or before it (Shift+Tab).
pub fn next_edge(edge: Edge, back: bool) -> Edge {
    let i = Edge::ALL.iter().position(|e| *e == edge).unwrap_or(0);
    Edge::ALL[(i + if back { 3 } else { 1 }) % 4]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_keeps_other_displays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/calibration.json");
        let a = SafeArea { top: 0.03, right: 0.02, bottom: 0.05, left: 0.0125 };
        save(&path, "Projector", 1920, 1080, a).unwrap();
        save(&path, "TV", 3840, 2160, SafeArea { top: 0.1, ..Default::default() }).unwrap();
        let got = load(&path, "Projector").unwrap();
        assert!((got.left - 0.013).abs() < 1e-6, "rounded to 0.1%");
        assert_eq!(got.top, 0.03);
        assert_eq!(load(&path, "TV").unwrap().top, 0.1);
        assert_eq!(load(&path, "Other"), None);
    }

    #[test]
    fn arrows_move_or_switch_edges() {
        let mut safe = SafeArea::default();
        let mut edge = Edge::Top;
        arrow(&mut safe, &mut edge, NamedKey::ArrowDown, false);
        assert_eq!(safe.top, 0.005);
        arrow(&mut safe, &mut edge, NamedKey::ArrowUp, true);
        assert_eq!(safe.top, 0.0, "clamped");
        arrow(&mut safe, &mut edge, NamedKey::ArrowLeft, false);
        assert_eq!(edge, Edge::Left);
        assert_eq!(next_edge(Edge::Left, false), Edge::Top);
        assert_eq!(next_edge(Edge::Top, true), Edge::Left);
    }
}
