//! Value-semantic maps with shared roots and small copy-on-write overlays.
use super::Value;
use std::{
    borrow::Borrow,
    collections::{BTreeMap, btree_map},
    sync::{Arc, OnceLock},
};

/// Value-semantic map with shared immutable roots and COW local changes.
/// Common lookups and point mutations preserve sharing. A direct BTreeMap
/// view or whole-map mutation can materialize the complete map.
#[derive(Clone)]
pub struct Properties(Storage);
#[derive(Clone)]
enum Storage {
    Flat(Arc<BTreeMap<String, Value>>),
    Overlay(Arc<Overlay>),
}
struct Overlay {
    base: Properties,
    changes: BTreeMap<String, Option<Arc<Value>>>,
    len: usize,
    full: OnceLock<BTreeMap<String, Value>>,
}
impl Clone for Overlay {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            changes: self.changes.clone(),
            len: self.len,
            // Full-map views stay with their snapshot; a local mutation must
            // not clone cached root payloads.
            full: OnceLock::new(),
        }
    }
}
impl Default for Properties {
    fn default() -> Self {
        BTreeMap::new().into()
    }
}
impl Properties {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        match &self.0 {
            Storage::Flat(map) => map.len(),
            Storage::Overlay(layer) => layer.len,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get<Q: ?Sized + Ord>(&self, key: &Q) -> Option<&Value>
    where
        String: Borrow<Q>,
    {
        self.get_key_value(key).map(|(_, value)| value)
    }
    pub fn get_key_value<Q: ?Sized + Ord>(&self, key: &Q) -> Option<(&String, &Value)>
    where
        String: Borrow<Q>,
    {
        match &self.0 {
            Storage::Flat(map) => map.get_key_value(key),
            Storage::Overlay(layer) => match layer.changes.get_key_value(key) {
                Some((name, Some(value))) => Some((name, value.as_ref())),
                Some((_, None)) => None,
                None => layer.base.get_key_value(key),
            },
        }
    }
    pub fn contains_key<Q: ?Sized + Ord>(&self, key: &Q) -> bool
    where
        String: Borrow<Q>,
    {
        self.get(key).is_some()
    }
    pub(super) fn scoped_clone(&self) -> Self {
        match &self.0 {
            Storage::Flat(_) => Self(Storage::Overlay(Arc::new(Overlay {
                base: self.clone(),
                changes: BTreeMap::new(),
                len: self.len(),
                full: OnceLock::new(),
            }))),
            Storage::Overlay(_) => self.clone(),
        }
    }
    // A bounded flat copy is cheaper than layered lookups for short loops.
    // Account for owned lists/text, while object maps already clone by Arc.
    pub(super) fn small_scope_clone(&self, limit: usize) -> Option<Self> {
        fn spend(budget: &mut usize, bytes: usize) -> bool {
            if let Some(left) = budget.checked_sub(bytes) {
                *budget = left;
                true
            } else {
                false
            }
        }
        fn fits(value: &Value, budget: &mut usize, depth: usize) -> bool {
            if depth >= 64 {
                return false;
            }
            match value {
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => true,
                Value::Text(text) | Value::Expr(text) => spend(budget, text.len()),
                Value::List(values) => {
                    values
                        .len()
                        .checked_mul(std::mem::size_of::<Value>())
                        .is_some_and(|bytes| spend(budget, bytes))
                        && values.iter().all(|value| fits(value, budget, depth + 1))
                }
                _ => false,
            }
        }
        if self.len() > limit / std::mem::size_of::<(String, Value)>() {
            return None;
        }
        let mut budget = limit;
        for (key, value) in self {
            if !spend(
                &mut budget,
                key.len() + std::mem::size_of::<(String, Value)>(),
            ) || !fits(value, &mut budget, 0)
            {
                return None;
            }
        }
        Some(
            self.iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
    }
    fn overlay_mut(&mut self) -> &mut Overlay {
        let Storage::Overlay(layer) = &mut self.0 else {
            unreachable!("scoped map")
        };
        let layer = Arc::make_mut(layer);
        layer.full.take();
        layer
    }
    pub fn insert(&mut self, key: String, value: Value) -> Option<Value> {
        if let Storage::Flat(map) = &mut self.0 {
            return Arc::make_mut(map).insert(key, value);
        }
        let layer = self.overlay_mut();
        let old = match layer.changes.entry(key) {
            btree_map::Entry::Occupied(mut entry) => entry
                .get_mut()
                .replace(Arc::new(value))
                .map(Arc::unwrap_or_clone),
            btree_map::Entry::Vacant(entry) => {
                let old = layer.base.get::<str>(entry.key().as_str()).cloned();
                entry.insert(Some(Arc::new(value)));
                old
            }
        };
        if old.is_none() {
            layer.len += 1;
        }
        old
    }
    pub fn remove<Q: ?Sized + Ord>(&mut self, key: &Q) -> Option<Value>
    where
        String: Borrow<Q>,
    {
        if !self.contains_key(key) {
            return None;
        }
        if let Storage::Flat(map) = &mut self.0 {
            return Arc::make_mut(map).remove(key);
        }
        let name = self.get_key_value(key)?.0.clone();
        let layer = self.overlay_mut();
        let old = match layer.changes.entry(name) {
            btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().take().map(Arc::unwrap_or_clone)
            }
            btree_map::Entry::Vacant(entry) => {
                let old = layer.base.get::<str>(entry.key().as_str()).cloned();
                entry.insert(None);
                old
            }
        };
        layer.len -= 1;
        old
    }
    pub fn get_mut<Q: ?Sized + Ord>(&mut self, key: &Q) -> Option<&mut Value>
    where
        String: Borrow<Q>,
    {
        if !self.contains_key(key) {
            return None;
        }
        if matches!(&self.0, Storage::Flat(_)) {
            let Storage::Flat(map) = &mut self.0 else {
                unreachable!()
            };
            return Arc::make_mut(map).get_mut(key);
        }
        let name = self.get_key_value(key)?.0.clone();
        let layer = self.overlay_mut();
        match layer.changes.entry(name) {
            btree_map::Entry::Occupied(entry) => entry.into_mut().as_mut().map(Arc::make_mut),
            btree_map::Entry::Vacant(entry) => {
                let value = layer
                    .base
                    .get::<str>(entry.key().as_str())
                    .cloned()
                    .expect("existing property");
                entry
                    .insert(Some(Arc::new(value)))
                    .as_mut()
                    .map(Arc::make_mut)
            }
        }
    }
    pub fn clear(&mut self) {
        *self = Self::new();
    }
    pub(super) fn get_or_insert_with(
        &mut self,
        key: String,
        value: impl FnOnce() -> Value,
    ) -> &mut Value {
        if !self.contains_key(&key) {
            self.insert(key.clone(), value());
        }
        self.get_mut(&key).expect("inserted property")
    }
    pub fn iter(&self) -> PropertyIter<'_> {
        match &self.0 {
            Storage::Flat(map) => PropertyIter::Flat(map.iter()),
            Storage::Overlay(layer) => {
                let mut entries = Vec::with_capacity(layer.len);
                let mut base = layer.base.iter().peekable();
                let mut changes = layer.changes.iter().peekable();
                loop {
                    match (base.peek(), changes.peek()) {
                        (Some((a, _)), Some((b, _))) if a < b => {
                            entries.push(base.next().unwrap());
                        }
                        (Some((a, _)), Some((b, _))) if a == b => {
                            base.next();
                            let (key, value) = changes.next().unwrap();
                            if let Some(value) = value {
                                entries.push((key, value.as_ref()));
                            }
                        }
                        (_, Some(_)) => {
                            let (key, value) = changes.next().unwrap();
                            if let Some(value) = value {
                                entries.push((key, value.as_ref()));
                            }
                        }
                        (Some(_), None) => {
                            entries.extend(base);
                            break;
                        }
                        (None, None) => break,
                    }
                }
                PropertyIter::Overlay(entries.into_iter())
            }
        }
    }
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &String> + ExactSizeIterator + Clone {
        self.iter().map(|(key, _)| key)
    }
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &Value> + ExactSizeIterator + Clone {
        self.iter().map(|(_, value)| value)
    }
    // Equality must keep IEEE NaN semantics, including for shared snapshots.
    // Visit visible entries without the temporary sorted overlay iterator.
    fn visit_unmasked(
        &self,
        hidden: &dyn Fn(&str) -> bool,
        visit: &mut dyn FnMut(&String, &Value) -> bool,
    ) -> bool {
        match &self.0 {
            Storage::Flat(map) => map
                .iter()
                .all(|(key, value)| hidden(key) || visit(key, value)),
            Storage::Overlay(layer) => {
                if !layer
                    .base
                    .visit_unmasked(&|key| hidden(key) || layer.changes.contains_key(key), visit)
                {
                    return false;
                }
                layer.changes.iter().all(|(key, value)| match value {
                    Some(value) if !hidden(key) => visit(key, value.as_ref()),
                    _ => true,
                })
            }
        }
    }
    pub(super) fn set(&mut self, key: String, value: Value) {
        if let Storage::Flat(map) = &mut self.0 {
            Arc::make_mut(map).insert(key, value);
            return;
        }
        let existed = self.contains_key(&key);
        let layer = self.overlay_mut();
        layer.changes.insert(key, Some(Arc::new(value)));
        if !existed {
            layer.len += 1;
        }
    }
    // Scope masking discards the old value; unlike BTreeMap::remove it must
    // not clone a shared inherited list merely to return and drop it.
    pub(super) fn unset(&mut self, key: &str) {
        if !self.contains_key(key) {
            return;
        }
        if let Storage::Flat(map) = &mut self.0 {
            Arc::make_mut(map).remove(key);
            return;
        }
        let layer = self.overlay_mut();
        layer.changes.insert(key.to_owned(), None);
        layer.len -= 1;
    }
    fn into_map(self) -> BTreeMap<String, Value> {
        match self.0 {
            Storage::Flat(map) => Arc::unwrap_or_clone(map),
            Storage::Overlay(layer) => match Arc::try_unwrap(layer) {
                Ok(layer) => {
                    if let Some(map) = layer.full.into_inner() {
                        return map;
                    }
                    // A shared base may contain a large shadowed value. Own
                    // only its visible entries; copying then deleting the base
                    // value would undo the scope's allocation savings.
                    let unique_base =
                        matches!(&layer.base.0,Storage::Flat(map) if Arc::strong_count(map)==1);
                    let mut map = if unique_base {
                        layer.base.into_map()
                    } else {
                        layer
                            .base
                            .iter()
                            .filter(|(key, _)| !layer.changes.contains_key(*key))
                            .map(|(key, value)| (key.clone(), value.clone()))
                            .collect()
                    };
                    for (key, value) in layer.changes {
                        match value {
                            Some(value) => {
                                map.insert(key, Arc::unwrap_or_clone(value));
                            }
                            None => {
                                map.remove(&key);
                            }
                        }
                    }
                    map
                }
                Err(layer) => Properties(Storage::Overlay(layer))
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            },
        }
    }
    #[cfg(test)]
    pub(super) fn shares_storage(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Storage::Flat(a), Storage::Flat(b)) => Arc::ptr_eq(a, b),
            (Storage::Overlay(a), Storage::Overlay(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
    fn full_mut(&mut self) -> &mut BTreeMap<String, Value> {
        if matches!(&self.0, Storage::Overlay(_)) {
            let map = self
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            self.0 = Storage::Flat(Arc::new(map));
        }
        let Storage::Flat(map) = &mut self.0 else {
            unreachable!()
        };
        Arc::make_mut(map)
    }
}
#[derive(Clone, Debug)]
pub enum PropertyIter<'a> {
    Flat(btree_map::Iter<'a, String, Value>),
    Overlay(std::vec::IntoIter<(&'a String, &'a Value)>),
}
impl<'a> Iterator for PropertyIter<'a> {
    type Item = (&'a String, &'a Value);
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Flat(iter) => iter.next(),
            Self::Overlay(iter) => iter.next(),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Flat(iter) => iter.size_hint(),
            Self::Overlay(iter) => iter.size_hint(),
        }
    }
}
impl DoubleEndedIterator for PropertyIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::Flat(iter) => iter.next_back(),
            Self::Overlay(iter) => iter.next_back(),
        }
    }
}
impl ExactSizeIterator for PropertyIter<'_> {}
impl std::iter::FusedIterator for PropertyIter<'_> {}
impl std::ops::Deref for Properties {
    type Target = BTreeMap<String, Value>;
    fn deref(&self) -> &Self::Target {
        match &self.0 {
            Storage::Flat(map) => map,
            Storage::Overlay(layer) => layer.full.get_or_init(|| {
                self.iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            }),
        }
    }
}
impl std::ops::DerefMut for Properties {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.full_mut()
    }
}
impl<Q: ?Sized + Ord> std::ops::Index<&Q> for Properties
where
    String: Borrow<Q>,
{
    type Output = Value;
    fn index(&self, key: &Q) -> &Value {
        self.get(key).expect("no entry found for key")
    }
}
impl PartialEq for Properties {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            // Preserve the original linear BTreeMap comparison for ordinary
            // models/objects, including its IEEE NaN semantics.
            (Storage::Flat(a), Storage::Flat(b)) => a == b,
            _ => {
                self.len() == other.len()
                    && self
                        .visit_unmasked(&|_| false, &mut |key, value| other.get(key) == Some(value))
            }
        }
    }
}
impl std::fmt::Debug for Properties {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Properties").field(&**self).finish()
    }
}
impl From<BTreeMap<String, Value>> for Properties {
    fn from(map: BTreeMap<String, Value>) -> Self {
        Self(Storage::Flat(Arc::new(map)))
    }
}
impl<const N: usize> From<[(String, Value); N]> for Properties {
    fn from(values: [(String, Value); N]) -> Self {
        BTreeMap::from(values).into()
    }
}
impl FromIterator<(String, Value)> for Properties {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(values: T) -> Self {
        BTreeMap::from_iter(values).into()
    }
}
impl Extend<(String, Value)> for Properties {
    fn extend<T: IntoIterator<Item = (String, Value)>>(&mut self, values: T) {
        for (key, value) in values {
            self.set(key, value);
        }
    }
}
impl IntoIterator for Properties {
    type Item = (String, Value);
    type IntoIter = btree_map::IntoIter<String, Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.into_map().into_iter()
    }
}
impl<'a> IntoIterator for &'a Properties {
    type Item = (&'a String, &'a Value);
    type IntoIter = PropertyIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a> IntoIterator for &'a mut Properties {
    type Item = (&'a String, &'a mut Value);
    type IntoIter = btree_map::IterMut<'a, String, Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.full_mut().iter_mut()
    }
}
impl serde::Serialize for Properties {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (key, value) in self {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}
impl<'de> serde::Deserialize<'de> for Properties {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        BTreeMap::<String, Value>::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value(n: u32) -> Value {
        match n % 5 {
            0 => Value::Null,
            1 => Value::Number(n as f64),
            2 => Value::Text(format!("value-{n} — 你好")),
            3 => Value::List(vec![
                Value::Number(n as f64),
                Value::Text(format!("list-{n}")),
            ]),
            _ => Value::Object(Properties::from([(
                "nested".into(),
                Value::Number(n as f64),
            )])),
        }
    }
    #[test]
    fn overlays_match_btree_map_mutation_and_cached_view_semantics() {
        let mut expected = BTreeMap::from([(
            "payload".into(),
            Value::List(
                (0..64)
                    .map(|i| Value::Text(format!("payload-{i}")))
                    .collect(),
            ),
        )]);
        let mut actual = Properties::from(expected.clone()).scoped_clone();
        let mut snapshots = Vec::new();
        let mut seed = 19u32;
        for step in 0..600 {
            if step % 3 == 0 {
                actual = actual.scoped_clone();
            }
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let key = format!("key-{}", seed % 23);
            let next = value(seed);
            if step % 7 == 0 {
                // Force the compatibility BTreeMap view, then retain it through
                // subsequent COW mutations and cache invalidation.
                assert_eq!(&*actual, &expected);
                snapshots.push((actual.clone(), serde_json::to_string(&actual).unwrap()));
            }
            match seed % 10 {
                0 | 1 => assert_eq!(
                    actual.insert(key.clone(), next.clone()),
                    expected.insert(key.clone(), next)
                ),
                2 => assert_eq!(actual.remove(key.as_str()), expected.remove(key.as_str())),
                3 => {
                    if let Some(value) = actual.get_mut(key.as_str()) {
                        *value = next.clone();
                    }
                    if let Some(value) = expected.get_mut(key.as_str()) {
                        *value = next;
                    }
                }
                4 => {
                    actual.extend([(key.clone(), next.clone())]);
                    expected.extend([(key.clone(), next)]);
                }
                5 => {
                    actual.entry(key.clone()).or_insert(next.clone());
                    expected.entry(key.clone()).or_insert(next);
                }
                6 => {
                    for (_, value) in &mut actual {
                        *value = Value::Bool(true);
                    }
                    for value in expected.values_mut() {
                        *value = Value::Bool(true);
                    }
                }
                7 => {
                    let owned: BTreeMap<_, _> = actual.clone().into_iter().collect();
                    assert_eq!(owned, expected);
                }
                8 if step % 11 == 0 => {
                    actual.clear();
                    expected.clear();
                }
                _ => {
                    assert_eq!(
                        actual.get_key_value(key.as_str()),
                        expected.get_key_value(key.as_str())
                    );
                }
            }
            assert_eq!(actual.len(), expected.len());
            assert_eq!(actual.is_empty(), expected.is_empty());
            assert_eq!(
                actual.iter().collect::<Vec<_>>(),
                expected.iter().collect::<Vec<_>>()
            );
            assert_eq!(
                actual.iter().rev().collect::<Vec<_>>(),
                expected.iter().rev().collect::<Vec<_>>()
            );
            let iter = actual.iter();
            assert_eq!(iter.len(), expected.len());
            assert_eq!(iter.clone().count(), expected.len());
            assert_eq!(
                serde_json::to_string(&actual).unwrap(),
                serde_json::to_string(&expected).unwrap()
            );
            assert_eq!(actual, Properties::from(expected.clone()));
        }
        for (snapshot, json) in snapshots {
            assert_eq!(serde_json::to_string(&snapshot).unwrap(), json);
        }
    }
    #[test]
    fn shared_delta_values_detach_on_nested_list_mutation_and_owned_iteration() {
        let root = Properties::from([(
            "payload".into(),
            Value::List(vec![Value::Text("root".into())]),
        )]);
        let mut local = root.scoped_clone();
        local.insert(
            "alias".into(),
            Value::List(vec![Value::Text("alias".into())]),
        );
        let snapshot = local.clone();
        let before = serde_json::to_string(&snapshot).unwrap();
        let Value::List(alias) = local.get_mut("alias").unwrap() else {
            panic!()
        };
        alias.push(Value::Number(2.));
        let Value::List(payload) = local.get_mut("payload").unwrap() else {
            panic!()
        };
        payload.push(Value::Number(3.));
        assert_eq!(serde_json::to_string(&snapshot).unwrap(), before);
        assert_eq!(
            root["payload"],
            Value::List(vec![Value::Text("root".into())])
        );
        let owned: BTreeMap<_, _> = local.into_iter().collect();
        assert_eq!(
            owned["alias"],
            Value::List(vec![Value::Text("alias".into()), Value::Number(2.)])
        );
        assert_eq!(
            owned["payload"],
            Value::List(vec![Value::Text("root".into()), Value::Number(3.)])
        );
        assert_eq!(
            snapshot["alias"],
            Value::List(vec![Value::Text("alias".into())])
        );
    }
    #[test]
    fn model_map_replacement_releases_the_old_map_after_snapshots_drop() {
        let mut model = Properties::from([(
            "payload".into(),
            Value::List(vec![Value::Text("old payload".repeat(1024))]),
        )]);
        let snapshot = model.clone();
        let Storage::Flat(map) = &snapshot.0 else {
            panic!()
        };
        let old = Arc::downgrade(map);
        model.insert("payload".into(), Value::List(Vec::new()));
        assert!(
            matches!(&model.0, Storage::Flat(_)),
            "live model mutations must not retain a hidden base map"
        );
        assert!(old.upgrade().is_some());
        drop(snapshot);
        assert!(old.upgrade().is_none());
        assert_eq!(model["payload"], Value::List(Vec::new()));
    }
    #[test]
    fn equality_preserves_nan_and_shadowed_nan_btree_semantics() {
        let map = BTreeMap::from([("nan".into(), Value::Number(f64::NAN))]);
        let oracle = Arc::new(map.clone());
        let properties = Properties::from(map);
        assert_eq!(properties == properties.clone(), oracle == oracle.clone());
        let mut shadow = properties.scoped_clone();
        shadow.insert("nan".into(), Value::Number(7.));
        assert_eq!(
            shadow,
            Properties::from([("nan".into(), Value::Number(7.))])
        );
        shadow.remove("nan");
        assert_eq!(shadow, Properties::new());
    }
}
