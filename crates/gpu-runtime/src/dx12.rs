#![allow(unsafe_code, dead_code)]

#[derive(Debug, Clone)]
pub struct Dx12Device {
    pub name: String,
    pub unified_memory: bool,
}

impl Dx12Device {
    pub fn probe_available() -> Option<Self> {
        #[cfg(target_os = "windows")]
        {
            Some(Self {
                name: "Direct3D 12 Native Adapter".to_string(),
                unified_memory: false,
            })
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }
}
