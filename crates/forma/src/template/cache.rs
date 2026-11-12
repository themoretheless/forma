//! The parsed-template cache.

use super::*;

// A keystroke rewrites the layout text of one control, so every other control of the
// document is re-derived from identical inputs. Parsing costs about 19x what cloning a
// finished Template costs (see `examples/template_load_perf.rs`), which is what this
// cache trades memory for.
pub const MAX_PARSED_TEMPLATES: usize = 1_024;

pub const MAX_PARSED_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct ParsedTemplate {
    pub(super) id: u64,
    pub(super) source: String,
    pub(super) props: ButtonSpec,
    pub(super) template: Arc<Template>,
}

/// Thread-local and process-wide rather than owned by `Runtime`, because the preview
/// builds a new `Runtime` for every keystroke. The hash only selects candidates: reuse
/// still requires the whole template text and the whole spec to compare equal, so a
/// short hash is never the sole proof of equality (`CACHE_POLICY.md`). Controls of one
/// document share their template text, so one hash can hold many specs.
pub(super) struct TemplateCache {
    pub(super) by_hash: HashMap<u64, Vec<ParsedTemplate>>,
    /// Recency and limits per template id; the value is the hash that holds the template.
    pub(super) lru: WeightedLru<u64, u64>,
    pub(super) next_id: u64,
    pub(super) hits: u64,
    pub(super) misses: u64,
}

impl TemplateCache {
    pub(super) fn new() -> Self {
        Self {
            by_hash: HashMap::new(),
            lru: WeightedLru::with_limits(CacheLimits {
                max_bytes: MAX_PARSED_BYTES as u64,
                max_entries: Some(MAX_PARSED_TEMPLATES),
                ttl: None,
            }),
            next_id: 0,
            hits: 0,
            misses: 0,
        }
    }

    pub(super) fn get(
        &mut self,
        hash: u64,
        source: &str,
        props: &ButtonSpec,
    ) -> Option<Arc<Template>> {
        let entry = self
            .by_hash
            .get(&hash)?
            .iter()
            .find(|entry| entry.source == source && &entry.props == props)?;
        self.lru.get(&entry.id);
        Some(Arc::clone(&entry.template))
    }

    /// Caches a parsed template, evicting least recently used templates first. One that
    /// does not fit the budget even when the cache is empty is used without caching.
    pub(super) fn insert(
        &mut self,
        hash: u64,
        source: &str,
        props: &ButtonSpec,
        template: &Arc<Template>,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        let weight = entry_weight(source, template) as u64;
        let (admitted, evicted) = self.lru.insert_prefetch_with_evictions(id, hash, weight);
        for (victim, victim_hash) in evicted {
            if let Some(entries) = self.by_hash.get_mut(&victim_hash) {
                entries.retain(|entry| entry.id != victim);
                if entries.is_empty() {
                    self.by_hash.remove(&victim_hash);
                }
            }
        }
        if admitted {
            self.by_hash.entry(hash).or_default().push(ParsedTemplate {
                id,
                source: source.to_owned(),
                props: props.clone(),
                template: Arc::clone(template),
            });
        }
    }

    pub(super) fn clear(&mut self) {
        self.lru.clear();
        self.by_hash.clear();
    }
}

thread_local! {
    static TEMPLATE_CACHE: RefCell<TemplateCache> = RefCell::new(TemplateCache::new());
}

pub(super) fn key_hash(source: &str) -> u64 {
    // Full-text coverage folded with adds and rotates: cheap enough to pay on every
    // load, and a collision costs one rejected comparison rather than a wrong template.
    const MIX: u64 = 0x9E37_79B9_7F4A_7C15;
    let bytes = source.as_bytes();
    let mut words = bytes.chunks_exact(16);
    let (mut a, mut b) = (bytes.len() as u64, MIX);
    for word in &mut words {
        a = a.wrapping_add(u64::from_le_bytes(word[..8].try_into().unwrap())) ^ b.rotate_left(11);
        b = b.wrapping_add(u64::from_le_bytes(word[8..].try_into().unwrap())) ^ a.rotate_left(31);
    }
    for byte in words.remainder() {
        a = a.wrapping_add(*byte as u64);
    }
    let mut h = (a ^ b.rotate_left(23)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 31;
    h.wrapping_mul(0x94D0_49BB_1331_11EB)
}

pub(super) fn content_weight(items: &Vec<Content>) -> usize {
    items.capacity() * std::mem::size_of::<Content>()
        + items
            .iter()
            .map(|item| match item {
                Content::Shape { points, .. } => points.capacity() * 8,
                Content::Text { text, .. } => text.text.len(),
                _ => 0,
            })
            .sum::<usize>()
}

/// Declared weight, matching the JS cache convention: predictable and monotonic in the
/// template size, not an exact heap measurement.
pub(super) fn entry_weight(source: &str, template: &Template) -> usize {
    let props = &template.props;
    source.len()
        + std::mem::size_of::<Template>()
        + props.text.len()
        + props.key.len()
        + content_weight(&template.content)
        + template.range.as_ref().map_or(0, |range| {
            content_weight(&range.minimum) + content_weight(&range.maximum)
        })
        + template.inputs.len() * 64
        + template.text.as_ref().map_or(0, |text| text.text.len())
}

/// `parse` with the process-wide cache in front of it. A hit shares one reference-counted
/// template with every other caller, which is the whole point: handing out a deep clone
/// cost almost as much as the parse it replaced. A caller that wants to edit takes its own
/// copy first with `Arc::make_mut`, so the shared entry can never be poisoned.
pub fn parse_cached(source: &str, props: &ButtonSpec) -> Result<Arc<Template>, String> {
    let hash = key_hash(source);
    TEMPLATE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(template) = cache.get(hash, source, props) {
            cache.hits += 1;
            return Ok(template);
        }
        let template = Arc::new(parse(source, props)?);
        cache.misses += 1;
        cache.insert(hash, source, props, &template);
        Ok(template)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseCacheStats {
    pub entries: usize,
    pub bytes: usize,
    pub hits: u64,
    pub misses: u64,
}

pub fn parse_cache_stats() -> ParseCacheStats {
    TEMPLATE_CACHE.with(|cache| {
        let cache = cache.borrow();
        ParseCacheStats {
            entries: cache.lru.len(),
            bytes: cache.lru.resident_weight() as usize,
            hits: cache.hits,
            misses: cache.misses,
        }
    })
}

pub fn flush_parse_cache() {
    TEMPLATE_CACHE.with(|cache| {
        cache.borrow_mut().clear();
    });
}
