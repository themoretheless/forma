//! What the GPU renderer reads from a control.

use super::*;

#[wasm_bindgen]
impl Button {
    pub fn gpu_commands(&self, scale: f32, background: bool) -> Vec<f32> {
        self.vector_list(scale, background).commands.clone()
    }
    pub fn gpu_edges(&self, scale: f32, background: bool) -> Vec<f32> {
        self.vector_list(scale, background).edges.clone()
    }
    pub fn gpu_tiles(&self, width: u32, height: u32, scale: f32, background: bool) -> Vec<u32> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || !scale.is_finite()
            || scale <= 0.
        {
            return vec![];
        }
        self.vector_list(scale, background).tiles(
            width,
            height,
            scale,
            [self.scroll_x, self.scroll_y],
        )
    }
    // viewport/scale/opaque output, scroll; animated fill and border. Geometry is static.
    pub fn gpu_params(&self, width: u32, height: u32, scale: f32, opaque: bool) -> Vec<f32> {
        use display_list::RenderScene as _;
        let mut params = Vec::with_capacity(display_list::PARAMS_LEN);
        self.gpu_params_into(width, height, scale, opaque, &mut params);
        params
    }
    pub fn release_cpu_cache(&self) {
        self.raster_cache.borrow_mut().take();
    }
    pub fn gpu_paints(&self) -> Vec<f32> {
        use display_list::RenderScene as _;
        let mut paints = Vec::with_capacity(if self.template.reveal.is_some() {
            16
        } else {
            8
        });
        self.gpu_paints_into(&mut paints);
        paints
    }
}

impl display_list::RenderScene for Button {
    fn release_cpu_cache(&self) {
        Button::release_cpu_cache(self);
    }
    fn gpu_scroll(&self) -> [f32; 2] {
        [self.scroll_x, self.scroll_y]
    }
    fn vector_snapshot(&self, s: f32, b: bool) -> std::sync::Arc<display_list::DisplayList> {
        Button::vector_snapshot(self, s, b)
    }
    fn gpu_params(&self, w: u32, h: u32, s: f32, o: bool) -> Vec<f32> {
        Button::gpu_params(self, w, h, s, o)
    }
    fn gpu_paints(&self) -> Vec<f32> {
        Button::gpu_paints(self)
    }
    fn gpu_params_into(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        opaque: bool,
        out: &mut Vec<f32>,
    ) {
        out.clear();
        out.extend([
            width as f32,
            height as f32,
            scale,
            if opaque { 1. } else { 0. },
        ]);
        out.extend(self.fill.color().map(|v| v as f32 / 255.));
        out.extend(self.border.color().map(|v| v as f32 / 255.));
        out.extend([self.scroll_x, self.scroll_y, 0., 0.]);
    }
    fn gpu_paints_into(&self, out: &mut Vec<f32>) {
        out.clear();
        out.extend(
            self.fill
                .color()
                .into_iter()
                .chain(self.border.color())
                .map(|v| v as f32 / 255.),
        );
        if self.template.reveal.is_some() {
            out.extend(self.reveal_paint().gpu());
        }
    }
}
