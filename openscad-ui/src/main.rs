//! Интерактивная модель состояния рабочего пространства OpenSCAD Studio на базе Forma.

#[derive(Clone, Debug)]
pub struct OpenScadVm {
    pub workspace_mode: String,
    pub selection_mode: String,
    pub dock_tab: String,
    pub outer_radius: f32,
    pub height: f32,
    pub teeth: u32,
    pub segments_fn: u32,
    pub surface_area_mm2: f64,
    pub volume_mm3: f64,
    pub status: String,
}

impl Default for OpenScadVm {
    fn default() -> Self {
        Self {
            workspace_mode: String::from("solid"),
            selection_mode: String::from("face"),
            dock_tab: String::from("inspector"),
            outer_radius: 42.0,
            height: 28.0,
            teeth: 18,
            segments_fn: 96,
            surface_area_mm2: 14_912.4,
            volume_mm3: 48_320.0,
            status: String::from("WebGPU · Metal · Manifold CSG · Forma UI"),
        }
    }
}

impl OpenScadVm {
    pub fn open_workspace_mode(&mut self, mode: &str) {
        self.workspace_mode = mode.to_string();
        self.status = format!("Режим рабочего пространства: {mode}");
    }

    pub fn set_selection_mode(&mut self, mode: &str) {
        self.selection_mode = mode.to_string();
        self.status = format!("Режим выделения 3D-сцены: {mode}");
    }

    pub fn select_dock_tab(&mut self, tab: &str) {
        self.dock_tab = tab.to_string();
    }

    pub fn compile_model(&mut self) {
        self.status = format!(
            "Manifold CSG пересобран ($fn = {}, r = {:.1} мм, h = {:.1} мм)",
            self.segments_fn, self.outer_radius, self.height
        );
    }

    pub fn run_geometry_analysis(&mut self) {
        self.status = format!(
            "GPU-редукция завершена: S = {:.1} мм², V = {:.1} мм³ (0.42 мс)",
            self.surface_area_mm2, self.volume_mm3
        );
    }
}

fn main() {
    let mut vm = OpenScadVm::default();
    println!("OpenSCAD Forma Studio запущен: {}", vm.status);
    vm.compile_model();
    vm.run_geometry_analysis();
    println!("Статус: {}", vm.status);
}
