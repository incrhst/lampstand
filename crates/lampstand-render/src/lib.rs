//! Render engine: one GPU-accelerated (wgpu) renderer per output, applying
//! that output's layout, theme and safe area.
//!
//! PRD: LV-3, LV-4, TS-1 to TS-10, OB-1 to OB-3.

pub mod fit;
pub mod live;
pub mod output;
pub mod renderer;
