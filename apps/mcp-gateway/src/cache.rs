use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_ENTRIES: usize = 10_000;

#[derive(Debug)]
pub struct TtlCache<K, V> {
    ttl: Duration,
    entries: Mutex<HashMap<K, (V, Instant)>>,
}

impl<K: Eq + Hash, V: Clone> TtlCache<K, V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: &K, now: Instant) -> Option<V> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries
            .get(key)
            .filter(|(_, until)| now < *until)
            .map(|(value, _)| value.clone())
    }

    pub fn put(&self, key: K, value: V, now: Instant) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|_, (_, until)| now < *until);
        if entries.len() >= MAX_ENTRIES {
            entries.clear();
        }
        entries.insert(key, (value, now + self.ttl));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_lives_for_its_ttl_only() {
        let cache = TtlCache::new(Duration::from_secs(30));
        let now = Instant::now();
        cache.put("a", 1, now);
        assert_eq!(cache.get(&"a", now), Some(1));
        assert_eq!(cache.get(&"a", now + Duration::from_secs(29)), Some(1));
        assert_eq!(cache.get(&"a", now + Duration::from_secs(30)), None);
        assert_eq!(cache.get(&"b", now), None);
    }

    #[test]
    fn putting_drops_the_expired_entries() {
        let cache = TtlCache::new(Duration::from_secs(1));
        let now = Instant::now();
        cache.put("old", 1, now);
        cache.put("new", 2, now + Duration::from_secs(5));
        assert_eq!(cache.entries.lock().unwrap().len(), 1);
    }
}
