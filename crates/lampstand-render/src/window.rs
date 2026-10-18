//! Native output windows: one borderless fullscreen winit window per display,
//! cursor hidden, drawn with wgpu (LV-3). Never a webview.
//!
//! The window is created hidden and shown only after its first frame is
//! presented, so the room never sees a desktop or an empty window.

use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window, WindowId, WindowLevel};

use crate::gpu::{Gpu, RenderError, Renderer};
use crate::layout::SlideLayout;
use crate::scene::{Output, Slide};

/// A connected display, as the operating system reports it.
#[derive(Debug, Clone)]
pub struct DisplayInfo {
    pub index: usize,
    pub name: String,
    /// Top-left corner on the virtual desktop, in physical pixels.
    pub position: (i32, i32),
    /// Native mode size in physical pixels.
    pub size: (u32, u32),
    pub scale_factor: f64,
    pub refresh_hz: Option<f32>,
    pub monitor: MonitorHandle,
    /// A stand-in shown in an ordinary window instead of fullscreen, for
    /// previewing an output whose screen is not connected.
    pub preview: bool,
}

impl DisplayInfo {
    /// A preview of display `index` (connected or not) as a `width` x
    /// `height` window on the operator's screen. The window can be resized
    /// to try other aspect ratios.
    pub fn preview(event_loop: &ActiveEventLoop, index: usize, width: u32, height: u32) -> Option<DisplayInfo> {
        let monitor = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next())?;
        Some(DisplayInfo {
            index,
            name: format!("Preview of display {index}"),
            position: (0, 0),
            size: (width, height),
            scale_factor: monitor.scale_factor(),
            refresh_hz: None,
            monitor,
            preview: true,
        })
    }
}

/// The displays connected now, in the order the OS lists them.
pub fn displays(event_loop: &ActiveEventLoop) -> Vec<DisplayInfo> {
    event_loop
        .available_monitors()
        .enumerate()
        .map(|(index, monitor)| {
            let position = monitor.position();
            let size = monitor.size();
            DisplayInfo {
                index,
                name: monitor.name().unwrap_or_else(|| format!("Display {}", index + 1)),
                position: (position.x, position.y),
                size: (size.width, size.height),
                scale_factor: monitor.scale_factor(),
                refresh_hz: monitor.refresh_rate_millihertz().map(|m| m as f32 / 1000.0),
                monitor,
                preview: false,
            }
        })
        .collect()
}

/// A wgpu instance that can create surfaces for this event loop's windows.
pub fn instance(event_loop: &ActiveEventLoop) -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(event_loop.owned_display_handle())))
}

fn create_window(event_loop: &ActiveEventLoop, display: &DisplayInfo) -> Result<Arc<Window>, RenderError> {
    if display.preview {
        let (w, h) = display.size;
        let attributes = Window::default_attributes()
            .with_title(format!("Lampstand: {} ({w}x{h})", display.name))
            .with_inner_size(winit::dpi::LogicalSize::new(w, h))
            .with_visible(false);
        let window = event_loop.create_window(attributes).map_err(|e| RenderError::Surface(e.to_string()))?;
        return Ok(Arc::new(window));
    }
    let attributes = Window::default_attributes()
        .with_title(format!("Lampstand output: {}", display.name))
        .with_decorations(false)
        .with_visible(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_fullscreen(Some(Fullscreen::Borderless(Some(display.monitor.clone()))));
    let window = event_loop.create_window(attributes).map_err(|e| RenderError::Surface(e.to_string()))?;
    window.set_cursor_visible(false);
    Ok(Arc::new(window))
}

/// Open one output window per entry, all sharing one GPU device. `output`
/// gives each window's settings; its width and height are replaced by the
/// window's real size.
pub fn open_outputs(
    event_loop: &ActiveEventLoop,
    targets: impl IntoIterator<Item = (DisplayInfo, Output, Slide)>,
) -> Result<(Arc<Gpu>, Vec<OutputWindow>), RenderError> {
    let instance = instance(event_loop);
    let mut pending = Vec::new();
    for (display, output, slide) in targets {
        let window = create_window(event_loop, &display)?;
        let surface = instance.create_surface(window.clone()).map_err(|e| RenderError::Surface(e.to_string()))?;
        pending.push((display, window, surface, output, slide));
    }
    let gpu = Gpu::from_instance(instance, pending.first().map(|p| &p.2))?;
    let windows = pending
        .into_iter()
        .map(|(display, window, surface, output, slide)| OutputWindow::new(gpu.clone(), display, window, surface, output, slide))
        .collect::<Result<_, _>>()?;
    Ok((gpu, windows))
}

pub struct OutputWindow {
    display: DisplayInfo,
    renderer: Renderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    output: Output,
    slide: Slide,
    changed_at: Option<Instant>,
    // Dropped after the surface.
    window: Arc<Window>,
}

impl OutputWindow {
    fn new(
        gpu: Arc<Gpu>,
        display: DisplayInfo,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        mut output: Output,
        slide: Slide,
    ) -> Result<Self, RenderError> {
        let size = window.inner_size();
        let (width, height) = if size.width > 0 && size.height > 0 { (size.width, size.height) } else { display.size };
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps.formats.iter().copied().find(|f| f.is_srgb()).or(caps.formats.first().copied());
        let mut config = surface
            .get_default_config(&gpu.adapter, width, height)
            .ok_or_else(|| RenderError::Surface("surface not supported by adapter".into()))?;
        if let Some(format) = format {
            config.format = format;
        }
        // Vsync without tearing, but at most one frame queued so a click
        // reaches the screen within a frame or two (50 ms budget).
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.desired_maximum_frame_latency = 1;
        if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        }
        surface.configure(&gpu.device, &config);

        output.width = width;
        output.height = height;
        let renderer = Renderer::new(gpu, config.format);
        let mut this = OutputWindow { display, renderer, surface, config, output, slide, changed_at: None, window };
        this.renderer.prepare(&this.output, &this.slide)?;
        // First frame while hidden, then show.
        this.draw()?;
        this.window.set_visible(true);
        this.window.request_redraw();
        Ok(this)
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn display(&self) -> &DisplayInfo {
        &self.display
    }

    pub fn output(&self) -> &Output {
        &self.output
    }

    pub fn layout(&self) -> Option<&SlideLayout> {
        self.renderer.layout()
    }

    /// Show `slide`. Lays it out now and draws on the next redraw.
    pub fn set_slide(&mut self, slide: Slide) -> Result<&SlideLayout, RenderError> {
        self.changed_at = Some(Instant::now());
        self.slide = slide;
        self.window.request_redraw();
        self.renderer.prepare(&self.output, &self.slide)
    }

    /// Change this output's settings (safe area, bleed, style). Size is kept.
    pub fn set_output(&mut self, output: Output) -> Result<&SlideLayout, RenderError> {
        self.output = Output { width: self.output.width, height: self.output.height, ..output };
        self.window.request_redraw();
        self.renderer.prepare(&self.output, &self.slide)
    }

    /// Handle an event for this window. Returns how long the last
    /// [`set_slide`](Self::set_slide) took to reach the screen, once it has.
    pub fn handle_event(&mut self, event: &WindowEvent) -> Result<Option<Duration>, RenderError> {
        match event {
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                self.config.width = size.width;
                self.config.height = size.height;
                self.surface.configure(&self.renderer.gpu().device, &self.config);
                self.output.width = size.width;
                self.output.height = size.height;
                self.renderer.prepare(&self.output, &self.slide)?;
                self.window.request_redraw();
                Ok(None)
            }
            WindowEvent::RedrawRequested => {
                if self.draw()? {
                    Ok(self.changed_at.take().map(|t| t.elapsed()))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    /// Draw and present. Returns false if the surface was not ready and a
    /// redraw has been requested instead.
    fn draw(&mut self) -> Result<bool, RenderError> {
        let device = &self.renderer.gpu().device;
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                self.window.request_redraw();
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
                self.surface.configure(device, &self.config);
                self.window.request_redraw();
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                let gpu = self.renderer.gpu().clone();
                self.surface = gpu.instance.create_surface(self.window.clone()).map_err(|e| RenderError::Surface(e.to_string()))?;
                self.surface.configure(&gpu.device, &self.config);
                self.window.request_redraw();
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Validation => return Err(RenderError::Surface("validation error".into())),
        };
        let view = frame.texture.create_view(&Default::default());
        self.renderer.render(&view)?;
        self.window.pre_present_notify();
        self.renderer.gpu().queue.present(frame);
        Ok(true)
    }
}
