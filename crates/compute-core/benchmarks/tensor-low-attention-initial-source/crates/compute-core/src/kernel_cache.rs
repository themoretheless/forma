use crate::{Binding, Kernel, KernelError, gpu_compute::GpuContext};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    source: String,
    entry: String,
    bindings: Vec<Binding>,
}
struct Cached {
    kernel: Arc<Kernel>,
    used_at: u64,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelCacheStats {
    pub hits: u64,
    /// Compilation attempts, including rejected shaders.
    pub misses: u64,
    pub evictions: u64,
}

/// Bounded LRU cache of compiled pipelines on one trusted context. Keys include
/// the exact compiled source, entry point and layout; labels are diagnostic only.
/// Returned handles keep evicted kernels alive, so capacity bounds cache entries,
/// not total GPU memory. Failed compilations neither occupy nor evict entries.
pub struct KernelCache {
    context: GpuContext,
    entries: HashMap<Key, Cached>,
    capacity: usize,
    clock: u64,
    stats: KernelCacheStats,
}
impl KernelCache {
    /// Capacity zero disables retention while preserving checked compilation.
    pub fn new(context: &GpuContext, capacity: usize) -> Self {
        Self {
            context: context.clone(),
            entries: HashMap::new(),
            capacity,
            clock: 0,
            stats: KernelCacheStats::default(),
        }
    }
    pub fn context(&self) -> &GpuContext {
        &self.context
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn stats(&self) -> KernelCacheStats {
        self.stats
    }
    /// Clears retained pipelines; outstanding handles remain usable.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn get(
        &mut self,
        label: &str,
        source: &str,
        entry: &str,
        bindings: &[Binding],
    ) -> Result<Arc<Kernel>, KernelError> {
        self.compile(label, source, entry, bindings, None)
    }
    pub fn get_with_workgroup_size(
        &mut self,
        label: &str,
        source: &str,
        entry: &str,
        bindings: &[Binding],
        size: u32,
    ) -> Result<Arc<Kernel>, KernelError> {
        let (source, selected) = Kernel::tuned_source(&self.context.device, source, size)?;
        self.compile(label, &source, entry, bindings, Some(selected))
    }
    fn compile(
        &mut self,
        label: &str,
        source: &str,
        entry: &str,
        bindings: &[Binding],
        selected: Option<u32>,
    ) -> Result<Arc<Kernel>, KernelError> {
        let key = Key {
            source: source.to_owned(),
            entry: entry.to_owned(),
            bindings: bindings.to_vec(),
        };
        self.clock = self.clock.saturating_add(1);
        if let Some(cached) = self.entries.get_mut(&key) {
            if selected.is_some_and(|s| s != cached.kernel.workgroup_size()) {
                return Err(KernelError {
                    message: "tuning anchor does not control the entry point".into(),
                });
            }
            cached.used_at = self.clock;
            self.stats.hits += 1;
            return Ok(cached.kernel.clone());
        }
        self.stats.misses += 1;
        let kernel = Kernel::from_context(&self.context, label, source, entry, bindings)?;
        if selected.is_some_and(|s| s != kernel.workgroup_size()) {
            return Err(KernelError {
                message: "tuning anchor does not control the entry point".into(),
            });
        }
        let kernel = Arc::new(kernel);
        if self.capacity != 0 {
            if self.entries.len() == self.capacity {
                let oldest = self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.used_at)
                    .map(|(key, _)| key.clone())
                    .expect("nonempty cache");
                self.entries.remove(&oldest);
                self.stats.evictions += 1;
            }
            self.entries.insert(
                key,
                Cached {
                    kernel: kernel.clone(),
                    used_at: self.clock,
                },
            );
        }
        Ok(kernel)
    }
}
