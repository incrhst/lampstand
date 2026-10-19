//! One output screen: a window, its wgpu surface, and the renderer that
//! draws into it.

use std::sync::Arc;

use lampstand_core::safe_area::SafeArea;
use winit::window::Window;

use crate::renderer::Renderer;

pub struct Output {
    pub renderer: Renderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    // Dropped after the surface.
    window: Arc<Window>,
}

impl Output {
    pub async fn new(window: Arc<Window>, safe_area: SafeArea) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("no GPU adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);
        let renderer = Renderer::new(
            device,
            queue,
            format,
            config.width,
            config.height,
            safe_area,
        );
        Output {
            renderer,
            surface,
            config,
            window,
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(self.renderer.device(), &self.config);
        self.renderer.resize(width, height);
    }

    /// Draws a frame. Returns whether it reached the screen: a hidden
    /// window draws nothing until winit reports it visible again.
    pub fn render(&mut self, live: &crate::live::Live) -> bool {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.renderer.device(), &self.config);
                self.window.request_redraw();
                return false;
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.window.request_redraw();
                return false;
            }
            wgpu::CurrentSurfaceTexture::Occluded => return false,
            wgpu::CurrentSurfaceTexture::Validation => panic!("surface validation error"),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer.draw(&view, live);
        self.window.pre_present_notify();
        self.renderer.queue().present(frame);
        true
    }
}
