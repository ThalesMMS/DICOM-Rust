use super::client::Response;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
const DEFAULT_CACHE_BYTES: i64 = 128 << 20;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: i64,
    pub max_bytes: i64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}
struct Inner {
    max_bytes: i64,
    bytes: i64,
    entries: HashMap<String, (Response, i64)>,
    lru: VecDeque<String>,
    hits: u64,
    misses: u64,
    evictions: u64,
}
pub(crate) struct ResponseCache {
    inner: Mutex<Inner>,
}
impl ResponseCache {
    pub fn new(max_bytes: i64) -> ResponseCache {
        ResponseCache {
            inner: Mutex::new(Inner {
                max_bytes: if max_bytes <= 0 {
                    DEFAULT_CACHE_BYTES
                } else {
                    max_bytes
                },
                bytes: 0,
                entries: HashMap::new(),
                lru: VecDeque::new(),
                hits: 0,
                misses: 0,
                evictions: 0,
            }),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn get(&self, key: &str) -> Option<Response> {
        let mut c = self.lock();
        let Some((response, _)) = c.entries.get(key) else {
            c.misses += 1;
            return None;
        };
        let mut response = response.clone();
        c.hits += 1;
        c.move_to_front(key);
        response.cache_hit = true;
        Some(response)
    }
    pub fn put(&self, key: &str, response: &Response) {
        if key.is_empty() || response.data.is_empty() {
            return;
        }
        let size = response.data.len() as i64;
        let mut c = self.lock();
        if size > c.max_bytes {
            return;
        }
        let mut stored = response.clone();
        stored.cache_hit = false;
        if let Some((_, old)) = c.entries.get(key).map(|(r, s)| (r.clone(), *s)) {
            c.bytes -= old;
            c.entries.insert(key.to_string(), (stored, size));
            c.bytes += size;
            c.move_to_front(key);
        } else {
            c.entries.insert(key.to_string(), (stored, size));
            c.lru.push_front(key.to_string());
            c.bytes += size;
        }
        while c.bytes > c.max_bytes {
            let Some(back) = c.lru.pop_back() else {
                break;
            };
            if let Some((_, s)) = c.entries.remove(&back) {
                c.bytes -= s;
            }
            c.evictions += 1;
        }
    }
    pub fn stats(&self) -> CacheStats {
        let c = self.lock();
        CacheStats {
            entries: c.entries.len(),
            bytes: c.bytes,
            max_bytes: c.max_bytes,
            hits: c.hits,
            misses: c.misses,
            evictions: c.evictions,
        }
    }
}
impl Inner {
    fn move_to_front(&mut self, key: &str) {
        if let Some(i) = self.lru.iter().position(|k| k == key)
            && let Some(k) = self.lru.remove(i)
        {
            self.lru.push_front(k);
        }
    }
}
