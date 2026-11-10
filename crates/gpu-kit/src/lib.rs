//! Reusable GPU building blocks.
//!
//! Everything here is written against [`wgpu`]: Forma's `gpu-runtime` (Metal) on macOS and
//! upstream `wgpu` elsewhere, so an application gets the same types on every platform. Memory is
//! accounted with [`memory_budget`]: GPU allocations reserve bytes from a [`MemoryBudget`] and a
//! [`GpuResourceLease`] keeps those reservations alive until the GPU finishes the work using them.
//!
//! - [`GpuBackend`], [`GpuVendor`]: explicit backend and vendor selection, never falling back.
//! - [`ModelRenderer`]: depth-tested triangle meshes with a neutral material.
//! - [`MaterialRenderer`], [`MaterialTextureUploader`]: glTF metallic-roughness materials with
//!   mipmapped, budgeted textures.
//!
//! [`MemoryBudget`]: memory_budget::MemoryBudget

/// The GPU API: Forma's Metal runtime on macOS, upstream `wgpu` elsewhere. [`wgpu::naga`] is the
/// WGSL compiler the target runs.
pub mod wgpu {
    #[cfg(target_os = "macos")]
    pub use gpu_runtime::*;
    #[cfg(target_os = "macos")]
    pub use gpu_shader as naga;

    #[cfg(not(target_os = "macos"))]
    pub use naga;
    #[cfg(not(target_os = "macos"))]
    pub use wgpu::*;
}

pub use memory_budget;

/// Whether a float that reaches a shader is finite and within the range the shaders accept.
pub(crate) fn finite_bounded(value: f32) -> bool {
    value.is_finite() && value.abs() <= 1e15
}

/// Whether a 2D texture extent is non-empty and within the device's 2D texture limit.
pub(crate) fn extent_fits(device: &wgpu::Device, size: [u32; 2]) -> bool {
    let max = device.limits().max_texture_dimension_2d;
    size.iter().all(|&n| n > 0 && n <= max)
}

mod backend;
mod material_render;
mod material_texture;
mod model;

pub use backend::{BackendError, GpuBackend, GpuVendor};
pub use material_render::{
    MaterialAlpha, MaterialDraw, MaterialParameters, MaterialRenderError, MaterialRenderer,
    MaterialVertex,
};
pub use material_texture::{
    MaterialSampler, MaterialTexture, MaterialTextureError, MaterialTextureUploader,
};
pub use model::{ModelRenderer, ModelUploadError, ModelUploadPreparer, PreparedModelUpload};

use memory_budget::Reservation;
use std::sync::Arc;

/// Largest single texture or buffer allocated eagerly (512 MiB); larger content must be tiled.
pub const MAX_EAGER_ATLAS_BYTES: u64 = 512 * 1024 * 1024;

/// Snapshot of logical GPU resource credits used by an encoded submission.
/// Capture before replacing resources, then retain after submitting that work.
#[derive(Debug, Default)]
pub struct GpuResourceLease(Vec<Arc<Reservation>>);

impl GpuResourceLease {
    /// Retain caller-owned GPU resource credit through a submitted command.
    pub fn from_reservation(value: Arc<Reservation>) -> Self {
        Self::new([value])
    }
    pub fn new(values: impl IntoIterator<Item = Arc<Reservation>>) -> Self {
        Self(values.into_iter().collect())
    }
    pub fn extend(&mut self, other: Self) {
        self.0.extend(other.0);
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Call only after the submission using this snapshot has been queued.
    /// Device polling must progress completion callbacks.
    pub fn retain_until_complete(self, queue: &wgpu::Queue) {
        if !self.is_empty() {
            queue.on_submitted_work_done(move || drop(self));
        }
    }
}

/// Retains logical upload occupancy through GPU completion, including early returns.
#[derive(Debug)]
pub struct QueuedUploadReservation {
    pub queue: wgpu::Queue,
    pub reservation: Option<Reservation>,
    pub resources: GpuResourceLease,
}

impl Drop for QueuedUploadReservation {
    fn drop(&mut self) {
        let reservation = self.reservation.take();
        let resources = std::mem::take(&mut self.resources);
        if reservation.is_some() || !resources.is_empty() {
            self.queue.submit([]);
            self.queue
                .on_submitted_work_done(move || drop((reservation, resources)));
        }
    }
}
