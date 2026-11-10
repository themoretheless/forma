use crate::{Binding, Kernel, KernelError, gpu_compute::GpuContext};
use memory_budget::{CacheLimits, WeightedLru};
use std::{
    collections::HashMap,
    hash::{BuildHasher, Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    source: String,
    entry: String,
    bindings: Vec<Binding>,
}

impl Key {
    fn matches(&self, source: &str, entry: &str, bindings: &[Binding]) -> bool {
        self.entry == entry && self.bindings.as_slice() == bindings && self.source == source
    }
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
    // Keys are shared so recency updates clone a pointer, not the shader source.
    entries: WeightedLru<Arc<Key>, Arc<Kernel>>,
    // Borrowed lookup index so cache hits never allocate Strings, Vec<Binding> or Arc<Key>.
    index: HashMap<u64, Vec<Arc<Key>>>,
    hasher: std::collections::hash_map::RandomState,
    stats: KernelCacheStats,
}
impl KernelCache {
    /// Capacity zero disables retention while preserving checked compilation.
    pub fn new(context: &GpuContext, capacity: usize) -> Self {
        Self {
            context: context.clone(),
            entries: WeightedLru::with_limits(CacheLimits {
                max_bytes: u64::MAX,
                max_entries: Some(capacity),
                ttl: None,
            }),
            index: HashMap::with_capacity(capacity.min(64)),
            hasher: std::collections::hash_map::RandomState::new(),
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
        self.index.clear();
    }

    fn key_hash(&self, source: &str, entry: &str, bindings: &[Binding]) -> u64 {
        let mut state = self.hasher.build_hasher();
        source.hash(&mut state);
        entry.hash(&mut state);
        bindings.hash(&mut state);
        state.finish()
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
        let hash = self.key_hash(source, entry, bindings);
        let existing_key = self
            .index
            .get(&hash)
            .and_then(|bucket| bucket.iter().find(|k| k.matches(source, entry, bindings)))
            .cloned();
        if let Some(kernel) = existing_key
            .as_ref()
            .and_then(|key| self.entries.get(key))
            .cloned()
        {
            if selected.is_some_and(|s| s != kernel.workgroup_size()) {
                return Err(KernelError {
                    message: "tuning anchor does not control the entry point".into(),
                });
            }
            self.stats.hits += 1;
            return Ok(kernel);
        }
        self.stats.misses += 1;
        let kernel = Kernel::from_context(&self.context, label, source, entry, bindings)?;
        if selected.is_some_and(|s| s != kernel.workgroup_size()) {
            return Err(KernelError {
                message: "tuning anchor does not control the entry point".into(),
            });
        }
        let kernel = Arc::new(kernel);
        let key = Arc::new(Key {
            source: source.to_owned(),
            entry: entry.to_owned(),
            bindings: bindings.to_vec(),
        });
        // Capacity zero rejects the insert, which keeps compilation checked but uncached.
        let (inserted, evicted) =
            self.entries
                .insert_prefetch_with_evictions(key.clone(), kernel.clone(), 1);
        for (evicted_key, _) in &evicted {
            let evicted_hash = self.key_hash(
                &evicted_key.source,
                &evicted_key.entry,
                &evicted_key.bindings,
            );
            if let Some(bucket) = self.index.get_mut(&evicted_hash) {
                bucket.retain(|k| !Arc::ptr_eq(k, evicted_key));
                if bucket.is_empty() {
                    self.index.remove(&evicted_hash);
                }
            }
        }
        if inserted {
            self.index.entry(hash).or_default().push(key);
        }
        self.stats.evictions += evicted.len() as u64;
        Ok(kernel)
    }
}
