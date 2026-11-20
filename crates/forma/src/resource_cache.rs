//! Shared cache resource identifiers and bounded glyph/SVG resource caches under `memory-budget`.

use memory_budget::{CacheLimits, MemoryBudget, Reservation, WeightedLru};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

/// Default shared memory ceiling for retained glyph outlines and parsed SVGs (16 MiB).
pub const DEFAULT_RESOURCE_BUDGET_BYTES: u64 = 16 * 1024 * 1024;

/// Kind of backend-independent retained resource referenced by a display item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub enum ResourceKind {
    PaintSlot,
    GlyphAtlas,
    SvgShape,
    CanvasSlot,
}

/// Stable identifier linking a display item to a paint slot, glyph atlas entry, SVG shape, or canvas slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct ResourceId(pub u64);

impl ResourceId {
    const KIND_SHIFT: u32 = 60;
    const PAYLOAD_MASK: u64 = (1u64 << Self::KIND_SHIFT) - 1;

    #[inline]
    pub const fn paint_slot(slot: u32) -> Self {
        Self((0u64 << Self::KIND_SHIFT) | (slot as u64))
    }

    #[inline]
    pub const fn glyph_run(hash: u64) -> Self {
        Self((1u64 << Self::KIND_SHIFT) | (hash & Self::PAYLOAD_MASK))
    }

    #[inline]
    pub const fn svg_shape(hash: u64) -> Self {
        Self((2u64 << Self::KIND_SHIFT) | (hash & Self::PAYLOAD_MASK))
    }

    #[inline]
    pub const fn canvas_slot(slot: u32) -> Self {
        Self((3u64 << Self::KIND_SHIFT) | (slot as u64))
    }

    #[inline]
    pub const fn kind(self) -> ResourceKind {
        match self.0 >> Self::KIND_SHIFT {
            0 => ResourceKind::PaintSlot,
            1 => ResourceKind::GlyphAtlas,
            2 => ResourceKind::SvgShape,
            _ => ResourceKind::CanvasSlot,
        }
    }

    #[inline]
    pub const fn payload(self) -> u64 {
        self.0 & Self::PAYLOAD_MASK
    }
}

/// Deterministic FNV-1a 64-bit hash for content resource identifiers and table bucketing.
pub(crate) fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Exact key for a flattened glyph outline in design units quantized by physical font pixel size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GlyphAtlasKey {
    pub(crate) face: u8,
    pub(crate) glyph_id: u16,
    pub(crate) scale_px_q64: u32,
}

/// Flattened glyph contours in font design units `(x, -y)` relative to glyph origin.
#[derive(Clone, Debug)]
pub(crate) struct CachedGlyphOutline {
    pub(crate) unit_edges: Box<[[f32; 4]]>,
    pub(crate) breaks: Box<[u32]>,
}

/// Single parsed SVG polygon/stroke element in image coordinates.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CachedSvgShape {
    pub(crate) points: Vec<[f64; 2]>,
    pub(crate) color: String,
}

/// Shared parsed SVG asset with intrinsic dimensions and shape polygons.
#[derive(Clone, Debug)]
pub(crate) struct CachedSvg {
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) shapes: Arc<[CachedSvgShape]>,
}

/// Aggregate telemetry for the shared glyph outline and SVG caches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct ResourceCacheStats {
    pub budget_limit_bytes: u64,
    pub budget_in_use_bytes: u64,
    pub budget_peak_bytes: u64,
    pub glyph_entries: usize,
    pub glyph_bytes: u64,
    pub glyph_hits: u64,
    pub glyph_misses: u64,
    pub glyph_evictions: u64,
    pub svg_entries: usize,
    pub svg_bytes: u64,
    pub svg_hits: u64,
    pub svg_misses: u64,
    pub svg_evictions: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SvgKey {
    hash: u64,
    seq: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CacheEntryKey {
    Glyph(GlyphAtlasKey),
    Svg(SvgKey),
}

struct SvgValue {
    source: Arc<str>,
    cached: CachedSvg,
}

enum CacheEntryValue {
    Glyph {
        outline: CachedGlyphOutline,
        weight: u64,
        reservation: Option<Reservation>,
    },
    Svg {
        entry: SvgValue,
        weight: u64,
        reservation: Option<Reservation>,
    },
}

impl CacheEntryValue {
    fn weight(&self) -> u64 {
        match self {
            Self::Glyph { weight, .. } | Self::Svg { weight, .. } => *weight,
        }
    }

    fn take_reservation(&mut self) -> Option<Reservation> {
        match self {
            Self::Glyph { reservation, .. } | Self::Svg { reservation, .. } => reservation.take(),
        }
    }

    fn set_reservation(&mut self, next: Reservation) {
        match self {
            Self::Glyph { reservation, .. } | Self::Svg { reservation, .. } => {
                *reservation = Some(next);
            }
        }
    }
}

fn glyph_weight(outline: &CachedGlyphOutline) -> u64 {
    (outline.unit_edges.len() * std::mem::size_of::<[f32; 4]>()
        + outline.breaks.len() * std::mem::size_of::<u32>()
        + 64) as u64
}

fn svg_weight(svg: &str, shapes: &[CachedSvgShape]) -> u64 {
    let shape_bytes: usize = shapes
        .iter()
        .map(|s| s.points.len() * std::mem::size_of::<[f64; 2]>() + s.color.len() + 32)
        .sum();
    (svg.len() + shape_bytes + 96) as u64
}

struct ResourceCacheManager {
    limit_bytes: u64,
    root_budget: MemoryBudget,
    glyph_budget: MemoryBudget,
    svg_budget: MemoryBudget,
    lru: WeightedLru<CacheEntryKey, CacheEntryValue>,
    svg_buckets: HashMap<u64, Vec<SvgKey>>,
    next_svg_seq: u32,
    glyph_entries: usize,
    glyph_bytes: u64,
    glyph_hits: u64,
    glyph_misses: u64,
    glyph_evictions: u64,
    svg_entries: usize,
    svg_bytes: u64,
    svg_hits: u64,
    svg_misses: u64,
    svg_evictions: u64,
}

impl ResourceCacheManager {
    fn new(limit_bytes: u64) -> Self {
        let root_budget = MemoryBudget::new(limit_bytes);
        let glyph_budget = root_budget.child(limit_bytes);
        let svg_budget = root_budget.child(limit_bytes);
        Self {
            limit_bytes,
            root_budget,
            glyph_budget,
            svg_budget,
            lru: WeightedLru::with_limits(CacheLimits::bytes(limit_bytes)),
            svg_buckets: HashMap::new(),
            next_svg_seq: 1,
            glyph_entries: 0,
            glyph_bytes: 0,
            glyph_hits: 0,
            glyph_misses: 0,
            glyph_evictions: 0,
            svg_entries: 0,
            svg_bytes: 0,
            svg_hits: 0,
            svg_misses: 0,
            svg_evictions: 0,
        }
    }

    fn record_eviction(&mut self, key: CacheEntryKey, value: CacheEntryValue) {
        match (key, value) {
            (CacheEntryKey::Glyph(_), CacheEntryValue::Glyph { weight, .. }) => {
                self.glyph_entries = self.glyph_entries.saturating_sub(1);
                self.glyph_bytes = self.glyph_bytes.saturating_sub(weight);
                self.glyph_evictions = self.glyph_evictions.saturating_add(1);
            }
            (CacheEntryKey::Svg(svg_key), CacheEntryValue::Svg { weight, .. }) => {
                self.svg_entries = self.svg_entries.saturating_sub(1);
                self.svg_bytes = self.svg_bytes.saturating_sub(weight);
                self.svg_evictions = self.svg_evictions.saturating_add(1);
                if let Some(bucket) = self.svg_buckets.get_mut(&svg_key.hash) {
                    bucket.retain(|k| *k != svg_key);
                    if bucket.is_empty() {
                        self.svg_buckets.remove(&svg_key.hash);
                    }
                }
            }
            _ => {}
        }
    }

    fn reserve_with_eviction(&mut self, for_svg: bool, weight: u64) -> Option<Reservation> {
        if weight > self.limit_bytes {
            return None;
        }
        loop {
            let target_budget = if for_svg {
                &self.svg_budget
            } else {
                &self.glyph_budget
            };
            if let Ok(reservation) = target_budget.try_reserve(weight) {
                return Some(reservation);
            }
            let (victim_key, victim_val) = self.lru.take_lru()?;
            self.record_eviction(victim_key, victim_val);
        }
    }

    fn set_budget_bytes(&mut self, limit_bytes: u64) {
        self.limit_bytes = limit_bytes;
        if let Some(evicted) = self.lru.set_limits(CacheLimits::bytes(limit_bytes)) {
            for (k, v) in evicted {
                self.record_eviction(k, v);
            }
        }
        while self.lru.resident_weight() > limit_bytes {
            let Some((k, v)) = self.lru.take_lru() else {
                break;
            };
            self.record_eviction(k, v);
        }
        // Drain and re-reserve surviving entries cleanly onto fresh budgets while keeping LRU order.
        let mut drained = Vec::with_capacity(self.lru.len());
        while let Some((k, mut v)) = self.lru.take_lru() {
            drop(v.take_reservation());
            drained.push((k, v));
        }
        self.root_budget = MemoryBudget::new(limit_bytes);
        self.glyph_budget = self.root_budget.child(limit_bytes);
        self.svg_budget = self.root_budget.child(limit_bytes);
        self.lru = WeightedLru::with_limits(CacheLimits::bytes(limit_bytes));
        for (k, mut v) in drained {
            let w = v.weight();
            let target = match k {
                CacheEntryKey::Glyph(_) => &self.glyph_budget,
                CacheEntryKey::Svg(_) => &self.svg_budget,
            };
            if let Ok(res) = target.try_reserve(w) {
                v.set_reservation(res);
                self.lru.insert(k, v, w);
            } else {
                self.record_eviction(k, v);
            }
        }
    }

    fn clear(&mut self) {
        self.lru = WeightedLru::with_limits(CacheLimits::bytes(self.limit_bytes));
        self.svg_buckets.clear();
        self.root_budget = MemoryBudget::new(self.limit_bytes);
        self.glyph_budget = self.root_budget.child(self.limit_bytes);
        self.svg_budget = self.root_budget.child(self.limit_bytes);
        self.glyph_entries = 0;
        self.glyph_bytes = 0;
        self.svg_entries = 0;
        self.svg_bytes = 0;
    }

    fn stats(&self) -> ResourceCacheStats {
        ResourceCacheStats {
            budget_limit_bytes: self.root_budget.limit(),
            budget_in_use_bytes: self.root_budget.used(),
            budget_peak_bytes: self.root_budget.peak(),
            glyph_entries: self.glyph_entries,
            glyph_bytes: self.glyph_bytes,
            glyph_hits: self.glyph_hits,
            glyph_misses: self.glyph_misses,
            glyph_evictions: self.glyph_evictions,
            svg_entries: self.svg_entries,
            svg_bytes: self.svg_bytes,
            svg_hits: self.svg_hits,
            svg_misses: self.svg_misses,
            svg_evictions: self.svg_evictions,
        }
    }
}

thread_local! {
    static RESOURCE_CACHES: RefCell<ResourceCacheManager> =
        RefCell::new(ResourceCacheManager::new(DEFAULT_RESOURCE_BUDGET_BYTES));
}

/// Returns counters and live/peak byte usage for the shared thread-local resource caches.
pub fn resource_cache_stats() -> ResourceCacheStats {
    RESOURCE_CACHES.with(|mgr| mgr.borrow().stats())
}

/// Updates the shared byte budget limit, evicting least-recently-used glyphs/SVGs if needed.
pub fn set_resource_budget_bytes(limit_bytes: u64) {
    RESOURCE_CACHES.with(|mgr| mgr.borrow_mut().set_budget_bytes(limit_bytes));
}

/// Evicts all entries from the shared glyph outline and SVG caches while keeping the budget limit.
pub fn clear_resource_caches() {
    RESOURCE_CACHES.with(|mgr| mgr.borrow_mut().clear());
}

pub(crate) fn with_cached_glyph<R>(
    key: GlyphAtlasKey,
    build: impl FnOnce() -> CachedGlyphOutline,
    use_outline: impl FnOnce(&CachedGlyphOutline) -> R,
) -> R {
    let cache_key = CacheEntryKey::Glyph(key);
    let mut use_outline = Some(use_outline);
    let hit = RESOURCE_CACHES.with(|mgr| {
        let mut mgr = mgr.borrow_mut();
        if mgr.lru.peek(&cache_key).is_some() {
            mgr.glyph_hits = mgr.glyph_hits.saturating_add(1);
            if let Some(CacheEntryValue::Glyph { outline, .. }) = mgr.lru.get(&cache_key) {
                return use_outline.take().map(|f| f(outline));
            }
        }
        mgr.glyph_misses = mgr.glyph_misses.saturating_add(1);
        None
    });
    if let Some(result) = hit {
        return result;
    }

    let outline = build();
    let weight = glyph_weight(&outline);
    let result = use_outline.take().expect("miss consumes callback")(&outline);
    RESOURCE_CACHES.with(|mgr| {
        let mut mgr = mgr.borrow_mut();
        if let Some(reservation) = mgr.reserve_with_eviction(false, weight) {
            if let Some(old) = mgr.lru.get(&cache_key) {
                let _ = old;
                drop(reservation);
                return;
            }
            if mgr.lru.insert(
                cache_key,
                CacheEntryValue::Glyph {
                    outline,
                    weight,
                    reservation: Some(reservation),
                },
                weight,
            ) {
                mgr.glyph_entries += 1;
                mgr.glyph_bytes = mgr.glyph_bytes.saturating_add(weight);
            }
        }
    });
    result
}

pub(crate) fn get_or_insert_svg(
    svg: &str,
    parse: impl FnOnce(&str) -> Result<(f64, f64, Vec<CachedSvgShape>), String>,
) -> Result<CachedSvg, String> {
    let hash = hash_bytes(svg.as_bytes());
    let hit = RESOURCE_CACHES.with(|mgr| {
        let mut mgr = mgr.borrow_mut();
        let matched_key = mgr.svg_buckets.get(&hash).and_then(|bucket| {
            bucket.iter().copied().find(|&k| {
                matches!(
                    mgr.lru.peek(&CacheEntryKey::Svg(k)),
                    Some(CacheEntryValue::Svg { entry, .. }) if entry.source.as_ref() == svg
                )
            })
        });
        if let Some(k) = matched_key {
            mgr.svg_hits = mgr.svg_hits.saturating_add(1);
            if let Some(CacheEntryValue::Svg { entry, .. }) = mgr.lru.get(&CacheEntryKey::Svg(k)) {
                return Some(entry.cached.clone());
            }
        }
        mgr.svg_misses = mgr.svg_misses.saturating_add(1);
        None
    });
    if let Some(cached) = hit {
        return Ok(cached);
    }

    let (width, height, shapes_vec) = parse(svg)?;
    let weight = svg_weight(svg, &shapes_vec);
    let cached = CachedSvg {
        width,
        height,
        shapes: Arc::from(shapes_vec),
    };
    let source: Arc<str> = Arc::from(svg);
    RESOURCE_CACHES.with(|mgr| {
        let mut mgr = mgr.borrow_mut();
        if let Some(reservation) = mgr.reserve_with_eviction(true, weight) {
            let seq = mgr.next_svg_seq;
            mgr.next_svg_seq = mgr.next_svg_seq.wrapping_add(1);
            let svg_key = SvgKey { hash, seq };
            let cache_key = CacheEntryKey::Svg(svg_key);
            if mgr.lru.insert(
                cache_key,
                CacheEntryValue::Svg {
                    entry: SvgValue {
                        source,
                        cached: cached.clone(),
                    },
                    weight,
                    reservation: Some(reservation),
                },
                weight,
            ) {
                mgr.svg_buckets.entry(hash).or_default().push(svg_key);
                mgr.svg_entries += 1;
                mgr.svg_bytes = mgr.svg_bytes.saturating_add(weight);
            }
        }
    });
    Ok(cached)
}
