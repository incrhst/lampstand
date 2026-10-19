//! winit glue for output windows, shared by the `outputs` demo and the
//! `windows` test. The app (T46) does the same with Tauri windows: create
//! the window hidden and borderless on its display, attach it, show it and
//! go fullscreen on `OutputEvent::FirstFrame`, hide the cursor.

#![allow(dead_code)]

use std::sync::Arc;

use lampstand_core::output::OutputId;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{Background, Outputs, RenderError};
use winit::dpi::PhysicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowLevel};

/// The monitors as `DisplayInfo`s, with their winit handles.
pub fn displays(event_loop: &ActiveEventLoop) -> Vec<(DisplayInfo, MonitorHandle)> {
    event_loop
        .available_monitors()
        .map(|m| {
            let size = m.size();
            let position = m.position();
            let info = DisplayInfo {
                name: m.name().unwrap_or_default(),
                width: size.width,
                height: size.height,
                x: position.x,
                y: position.y,
            };
            (info, m)
        })
        .collect()
}

pub struct OutputWindow {
    pub id: OutputId,
    pub window: Arc<Window>,
    /// The display it is fullscreen on; `None` for a normal window.
    pub monitor: Option<MonitorHandle>,
    pub shown: bool,
}

#[derive(Debug)]
pub enum OpenError {
    Window(winit::error::OsError),
    Render(RenderError),
}

/// Opens a hidden output window, fullscreen-sized on `monitor` or a normal
/// `size` window, and attaches it to `outputs`.
pub fn open(
    event_loop: &ActiveEventLoop,
    outputs: &Outputs,
    id: OutputId,
    title: &str,
    monitor: Option<MonitorHandle>,
    size: (u32, u32),
    position: Option<(i32, i32)>,
) -> Result<OutputWindow, OpenError> {
    let mut attributes = Window::default_attributes()
        .with_title(title)
        .with_visible(false);
    attributes = match &monitor {
        Some(m) => attributes
            .with_decorations(false)
            .with_position(m.position())
            .with_inner_size(m.size())
            .with_window_level(WindowLevel::AlwaysOnTop),
        None => {
            let a = attributes.with_inner_size(PhysicalSize::new(size.0, size.1));
            match position {
                Some((x, y)) => a.with_position(winit::dpi::PhysicalPosition::new(x, y)),
                None => a,
            }
        }
    };
    let window = Arc::new(
        event_loop
            .create_window(attributes)
            .map_err(OpenError::Window)?,
    );
    black_background(&window);
    let inner = window.inner_size();
    outputs
        .attach(
            id,
            window.clone(),
            inner.width,
            inner.height,
            Background::Opaque,
        )
        .map_err(OpenError::Render)?;
    Ok(OutputWindow {
        id,
        window,
        monitor,
        shown: false,
    })
}

impl OutputWindow {
    /// The first frame is on the surface: show the window.
    pub fn on_first_frame(&mut self) {
        if self.shown {
            return;
        }
        self.shown = true;
        self.window.set_visible(true);
        if let Some(monitor) = &self.monitor {
            fullscreen(&self.window, monitor);
        }
        self.window.set_cursor_visible(false);
    }

    /// Its display went away: hide it, so the OS does not move it onto
    /// another screen (the operator's).
    pub fn hide(&mut self) {
        self.window.set_visible(false);
        self.shown = false;
    }

    /// Its display came back: move there and show again.
    pub fn place(&mut self, monitor: MonitorHandle) {
        let _ = self.window.request_inner_size(monitor.size());
        self.window.set_outer_position(monitor.position());
        self.monitor = Some(monitor);
        self.on_first_frame();
    }
}

/// Makes the window black until the first frame covers it. Only macOS
/// needs it: it will not present to a hidden window, so the window appears
/// one refresh before its first frame.
fn black_background(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSColor, NSView};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return;
        };
        // SAFETY: winit's handle is a live NSView, and this runs on the main
        // thread, which owns it.
        let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        if let Some(ns_window) = view.window() {
            let black = unsafe { NSColor::blackColor() };
            ns_window.setBackgroundColor(Some(&black));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Borderless fullscreen on `monitor`. On macOS that is "simple"
/// fullscreen: the window covers the screen in place, without the Spaces
/// animation or a separate Space that `Fullscreen::Borderless` brings.
fn fullscreen(window: &Window, monitor: &MonitorHandle) {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowExtMacOS;
        let _ = monitor;
        window.set_simple_fullscreen(true);
    }
    #[cfg(not(target_os = "macos"))]
    window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(Some(
        monitor.clone(),
    ))));
}

/// Click-to-photon statistics from `FrameTiming`s.
#[derive(Debug, Default)]
pub struct Latency {
    /// send → present, in ms.
    pub samples: Vec<f64>,
}

impl Latency {
    pub fn push(&mut self, timing: &lampstand_render::FrameTiming) {
        self.samples.push(timing.latency().as_secs_f64() * 1000.0);
    }

    fn percentile(&self, p: f64) -> f64 {
        let mut s = self.samples.clone();
        s.sort_by(f64::total_cmp);
        if s.is_empty() {
            return f64::NAN;
        }
        s[((s.len() - 1) as f64 * p).round() as usize]
    }

    pub fn p95(&self) -> f64 {
        self.percentile(0.95)
    }

    /// Prints the numbers and returns the p95 click-to-photon bound: send
    /// to present plus one refresh (FIFO shows a presented frame at the
    /// next vblank).
    pub fn report(&self, label: &str, refresh_ms: f64) -> f64 {
        let bound = self.p95() + refresh_ms;
        println!(
            "{label}: {n} frames, send→present median {med:.1} ms, p95 {p95:.1} ms, max {max:.1} ms; \
             refresh {refresh_ms:.1} ms; click-to-photon ≤ {bound:.1} ms (p95 + one refresh)",
            n = self.samples.len(),
            med = self.percentile(0.5),
            p95 = self.p95(),
            max = self.percentile(1.0),
        );
        bound
    }
}

/// The refresh interval of the window's monitor, 60 Hz if unknown.
pub fn refresh_ms(window: &Window) -> f64 {
    window
        .current_monitor()
        .and_then(|m| m.refresh_rate_millihertz())
        .map_or(1000.0 / 60.0, |mhz| 1_000_000.0 / mhz as f64)
}
