//! Case-insensitive maps for PHP symbol names.
//!
//! PHP resolves class, interface, trait, enum, function, and namespace
//! names case-insensitively; only constants, variables, and properties
//! are case-sensitive.  [`CiMap`] and [`CiSet`] key their entries by the
//! ASCII-lowercased name (PHP identifier folding is ASCII-only) while
//! [`CiMap`] preserves the original spelling for iteration, so lookups
//! follow PHP semantics without losing the canonical casing that
//! completions and symbol listings display.
//!
//! Do **not** use these for constant indexes (`global_defines`,
//! `stub_constant_index`) or property lookups — those are
//! case-sensitive in PHP.

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;

/// ASCII-lowercase a key, borrowing when it is already lowercase.
#[inline]
fn fold(key: &str) -> Cow<'_, str> {
    if key.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(key.to_ascii_lowercase())
    } else {
        Cow::Borrowed(key)
    }
}

/// A map keyed by ASCII-lowercased symbol name.
///
/// The original (as-inserted) key spelling is kept alongside the value
/// and is what [`iter`](CiMap::iter) and [`keys`](CiMap::keys) yield.
/// When the same name is inserted twice with different casings, the
/// spelling of the most recent insert wins (like `HashMap::insert`),
/// except for [`or_insert_with`](CiMap::or_insert_with) which keeps
/// the existing entry untouched.
#[derive(Debug, Clone, Default)]
pub struct CiMap<V> {
    inner: HashMap<String, (String, V)>,
}

impl<V> CiMap<V> {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    #[inline]
    pub fn get(&self, key: &str) -> Option<&V> {
        self.inner.get(fold(key).as_ref()).map(|(_, v)| v)
    }

    /// Like [`get`](Self::get), but also returns the original key
    /// spelling, letting callers canonicalize a differently-cased
    /// lookup name.
    #[inline]
    pub fn get_key_value(&self, key: &str) -> Option<(&str, &V)> {
        self.inner
            .get(fold(key).as_ref())
            .map(|(k, v)| (k.as_str(), v))
    }

    #[inline]
    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.contains_key(fold(key).as_ref())
    }

    pub fn insert(&mut self, key: impl Into<String>, value: V) -> Option<V> {
        let key = key.into();
        let folded = fold(&key).into_owned();
        self.inner.insert(folded, (key, value)).map(|(_, v)| v)
    }

    /// Insert `value` only when no entry exists for `key` (compared
    /// case-insensitively).  Replacement for `entry(k).or_insert_with(f)`.
    pub fn or_insert_with(&mut self, key: impl Into<String>, f: impl FnOnce() -> V) -> &mut V {
        let key = key.into();
        let folded = fold(&key).into_owned();
        &mut self.inner.entry(folded).or_insert_with(|| (key, f())).1
    }

    pub fn remove(&mut self, key: &str) -> Option<V> {
        self.inner.remove(fold(key).as_ref()).map(|(_, v)| v)
    }

    /// Iterate over `(original_key, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.inner.values().map(|(k, v)| (k.as_str(), v))
    }

    /// Iterate over the original (as-inserted) key spellings.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.inner.values().map(|(k, _)| k.as_str())
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.values().map(|(_, v)| v)
    }

    /// Retain entries for which `f(original_key, &mut value)` is true.
    pub fn retain(&mut self, mut f: impl FnMut(&str, &mut V) -> bool) {
        self.inner.retain(|_, (k, v)| f(k.as_str(), v));
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }
}

impl<K: Into<String>, V> FromIterator<(K, V)> for CiMap<V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let iter = iter.into_iter();
        let mut map = Self::new();
        map.reserve(iter.size_hint().0);
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

impl<K: Into<String>, V, S> From<HashMap<K, V, S>> for CiMap<V> {
    fn from(map: HashMap<K, V, S>) -> Self {
        map.into_iter().collect()
    }
}

/// A set of ASCII-lowercased symbol names.
///
/// Unlike [`CiMap`], the original spelling is not preserved — this is
/// intended for membership tests only (e.g. the negative class-lookup
/// cache).
#[derive(Debug, Clone, Default)]
pub struct CiSet {
    inner: HashSet<String>,
}

impl CiSet {
    pub fn new() -> Self {
        Self {
            inner: HashSet::new(),
        }
    }

    #[inline]
    pub fn contains(&self, key: &str) -> bool {
        self.inner.contains(fold(key).as_ref())
    }

    pub fn insert(&mut self, key: &str) -> bool {
        self.inner.insert(fold(key).into_owned())
    }

    pub fn remove(&mut self, key: &str) -> bool {
        self.inner.remove(fold(key).as_ref())
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_lookup_is_case_insensitive() {
        let mut map = CiMap::new();
        map.insert("App\\Models\\User", 1);
        assert_eq!(map.get("app\\models\\user"), Some(&1));
        assert_eq!(map.get("APP\\MODELS\\USER"), Some(&1));
        assert_eq!(map.get("App\\Models\\User"), Some(&1));
        assert_eq!(map.get("App\\Models\\Users"), None);
        assert!(map.contains_key("aPp\\mOdEls\\usEr"));
    }

    #[test]
    fn map_preserves_original_key_spelling() {
        let mut map = CiMap::new();
        map.insert("PDO", "stub");
        let keys: Vec<&str> = map.keys().collect();
        assert_eq!(keys, vec!["PDO"]);
        assert_eq!(map.get_key_value("pdo"), Some(("PDO", &"stub")));
    }

    #[test]
    fn map_insert_same_name_different_case_overwrites() {
        let mut map = CiMap::new();
        map.insert("StdClass", 1);
        map.insert("stdClass", 2);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("stdclass"), Some(&2));
    }

    #[test]
    fn map_or_insert_with_keeps_existing() {
        let mut map = CiMap::new();
        map.insert("strlen", 1);
        map.or_insert_with("STRLEN", || 2);
        assert_eq!(map.get("strlen"), Some(&1));
        map.or_insert_with("str_contains", || 3);
        assert_eq!(map.get("STR_CONTAINS"), Some(&3));
    }

    #[test]
    fn map_remove_and_retain_are_case_insensitive() {
        let mut map = CiMap::new();
        map.insert("Foo\\Bar", 1);
        map.insert("Foo\\Baz", 2);
        assert_eq!(map.remove("foo\\bar"), Some(1));
        map.retain(|k, _| k != "Foo\\Baz");
        assert!(map.is_empty());
    }

    #[test]
    fn set_is_case_insensitive() {
        let mut set = CiSet::new();
        assert!(set.insert("App\\Missing"));
        assert!(!set.insert("app\\missing"));
        assert!(set.contains("APP\\MISSING"));
        assert!(set.remove("aPp\\MiSsInG"));
        assert!(set.is_empty());
    }
}
