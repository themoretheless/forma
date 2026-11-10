#![allow(unsafe_code, dead_code)]

#[derive(Debug, Clone)]
pub struct Dx12Device {
    pub name: String,
    pub unified_memory: bool,
}

impl Dx12Device {
    /// Finds a real Direct3D 12 adapter. The backend is not implemented yet, so this always
    /// returns `None`; it used to report a fabricated adapter on every Windows machine.
    pub fn probe_available() -> Option<Self> {
        None
    }
}
