use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use uuid::Uuid;

const PER_ORG: usize = 256;
const MAX_ORGS: usize = 1024;

type Entries<V> = HashMap<String, (V, Instant)>;

#[derive(Debug)]
pub struct OrgCache<V> {
    ttl: Duration,
    per_org: usize,
    orgs: Mutex<HashMap<Uuid, Entries<V>>>,
}

impl<V: Clone> OrgCache<V> {
    pub fn new(ttl: Duration) -> Self {
        Self::capped(ttl, PER_ORG)
    }

    pub fn capped(ttl: Duration, per_org: usize) -> Self {
        Self {
            ttl,
            per_org,
            orgs: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, org: Uuid, key: &str, now: Instant) -> Option<V> {
        let orgs = self
            .orgs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        orgs.get(&org)?
            .get(key)
            .filter(|(_, until)| now < *until)
            .map(|(value, _)| value.clone())
    }

    pub fn put(&self, org: Uuid, key: String, value: V, now: Instant) {
        let mut orgs = self
            .orgs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !orgs.contains_key(&org) && orgs.len() >= MAX_ORGS {
            orgs.retain(|_, entries| entries.values().any(|(_, until)| now < *until));
            if orgs.len() >= MAX_ORGS {
                return;
            }
        }
        let entries = orgs.entry(org).or_default();
        if entries.len() >= self.per_org && !entries.contains_key(&key) {
            entries.retain(|_, (_, until)| now < *until);
            if entries.len() >= self.per_org {
                let oldest = entries
                    .iter()
                    .min_by_key(|(_, (_, until))| *until)
                    .map(|(held, _)| held.clone());
                if let Some(oldest) = oldest {
                    entries.remove(&oldest);
                }
            }
        }
        entries.insert(key, (value, now + self.ttl));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Uuid = Uuid::from_u128(1);
    const B: Uuid = Uuid::from_u128(2);

    #[test]
    fn an_entry_lives_for_its_ttl_only() {
        let cache = OrgCache::new(Duration::from_secs(30));
        let now = Instant::now();
        cache.put(A, "a".to_owned(), 1, now);
        assert_eq!(cache.get(A, "a", now), Some(1));
        assert_eq!(cache.get(A, "a", now + Duration::from_secs(29)), Some(1));
        assert_eq!(cache.get(A, "a", now + Duration::from_secs(30)), None);
        assert_eq!(cache.get(A, "b", now), None);
        assert_eq!(cache.get(B, "a", now), None);
    }

    #[test]
    fn one_organisation_filling_its_share_evicts_only_its_own_entries() {
        let cache = OrgCache::capped(Duration::from_secs(60), 3);
        let now = Instant::now();
        cache.put(B, "kept".to_owned(), 99, now);
        for index in 0..10 {
            cache.put(
                A,
                format!("key-{index}"),
                index,
                now + Duration::from_millis(index),
            );
        }
        assert_eq!(cache.get(B, "kept", now), Some(99));
        let held = (0..10)
            .filter(|index| cache.get(A, &format!("key-{index}"), now).is_some())
            .count();
        assert_eq!(held, 3);
        assert_eq!(cache.get(A, "key-9", now), Some(9));
        assert_eq!(cache.get(A, "key-0", now), None);
    }

    #[test]
    fn replacing_a_key_at_the_cap_evicts_nothing() {
        let cache = OrgCache::capped(Duration::from_secs(60), 2);
        let now = Instant::now();
        cache.put(A, "x".to_owned(), 1, now);
        cache.put(A, "y".to_owned(), 2, now);
        cache.put(A, "x".to_owned(), 3, now);
        assert_eq!(cache.get(A, "x", now), Some(3));
        assert_eq!(cache.get(A, "y", now), Some(2));
    }
}
