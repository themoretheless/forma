//! Shared native GPU surface lifecycle for standalone and reusable hosts.
//! Surface retries and occlusion are reported to each host's event-loop policy.
use std::sync::Arc;
use winit::window::Window;
#[cfg(target_os="macos")]
#[path="metal_resize.rs"]
mod metal_resize;

/// Show/hide this window's GPU layer when switching presentation paths.
/// Other platforms use the active surface directly.
pub fn set_visible(window: &Window, visible: bool) -> Result<(),String> {
    #[cfg(target_os="macos")] { metal_resize::set_visible(window,visible) }
    #[cfg(not(target_os="macos"))] { let _=(window,visible);Ok(()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    Presented,
    ZeroSize,
    Occluded,
    Retry,
}


pub struct GpuWindow {
    surface: wgpu::Surface<'static>,
    renderer: crate::gpu::Renderer,
    config: wgpu::SurfaceConfiguration,
}
impl GpuWindow {
    pub async fn new(window: Arc<Window>) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(window.clone()),
        ));
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("No GPU surface configuration")?;
        // Prefer encoded-color target, matching CPU compositing exactly.
        if let Some(format) = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(|f| !f.is_srgb())
        {
            config.format = format;
        }
        config.present_mode = wgpu::PresentMode::Fifo;
        let renderer = crate::gpu::Renderer::new(&adapter, config.format).await?;
        surface.configure(&renderer.device, &config);
        #[cfg(target_os = "macos")]
        metal_resize::configure(&window)?;
        println!(
            "Renderer: GPU vector / {:?} ({}) / {}",
            adapter.get_info().backend,
            renderer.memory_architecture.as_str(),
            adapter.get_info().name
        );
        Ok(Self {
            surface,
            renderer,
            config,
        })
    }
    #[cfg(feature="native-test-hooks")]
    pub(crate) fn inject_device_loss_for_testing(&self) {
        self.renderer.inject_device_loss_for_testing("injected NativeHost device loss (test hook)".into());
    }

    /// Service completion errors without acquiring a drawable or rendering.
    pub fn poll_idle(&self) -> Result<bool,String> {
        self.renderer.check_health()?;
        Ok(crate::gpu::device_queue_idle(&self.renderer.device))
    }

    pub fn draw(
        &mut self,
        button: &crate::Runtime,
        window: &Window,
    ) -> Result<Presentation, String> {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(Presentation::ZeroSize);
        }
        // Device loss can make acquisition return Timeout forever. Detect the
        // driver fault before touching the surface, allowing host CPU fallback.
        self.renderer.check_health()?;
        if self.config.width != size.width || self.config.height != size.height {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.renderer.device, &self.config);
        }
        let mut suboptimal = false;
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                suboptimal = true;
                frame
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(Presentation::Occluded),
            wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(Presentation::Retry);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.renderer.device, &self.config);
                return Ok(Presentation::Retry);
            }
            e => return Err(format!("GPU surface: {e:?}")),
        };
        // AppKit can resize while the next drawable is being acquired. Always
        // address the actual target, not an earlier Window::inner_size snapshot.
        let target_size = frame.texture.size();
        self.renderer.draw(
            button,
            &frame.texture.create_view(&Default::default()),
            target_size.width,
            target_size.height,
            window.scale_factor() as f32,
            true,
            true,
        )?;
        window.pre_present_notify();
        frame.present();
        if suboptimal {
            self.config.width = 0;
            window.request_redraw();
        }
        Ok(Presentation::Presented)
    }
}
