use super::{IccError, MAX_PROFILE_BYTES, Profile, Transform, lossy_text};
use crate::textfmt::decode_rune;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheOptions {
    pub max_entries: i64,
    pub max_bytes: i64,
    pub max_profile_bytes: i64,
}
#[derive(Debug, Clone, Default)]
pub struct CachedTransform {
    pub transform: Option<Arc<Transform>>,
    pub description: Vec<u8>,
    pub err: Option<IccError>,
}
impl std::fmt::Display for CachedTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&lossy_text(&self.text()))
    }
}
impl CachedTransform {
    pub fn text(&self) -> Vec<u8> {
        if let Some(err) = &self.err {
            return format!("ICC unavailable: {err}").into_bytes();
        }
        let Some(t) = &self.transform else {
            return b"ICC unavailable".to_vec();
        };
        if t.identity() {
            return b"ICC profile sRGB".to_vec();
        }
        let name: &[u8] = if self.description.is_empty() {
            b"embedded profile"
        } else {
            &self.description
        };
        [b"ICC ".as_slice(), name, b" converted to sRGB"].concat()
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: i64,
    pub builds: u64,
}
struct Entry {
    result: CachedTransform,
    weight: i64,
}
struct State {
    lru: VecDeque<Vec<u8>>,
    entries: HashMap<Vec<u8>, Entry>,
    bytes: i64,
    builds: u64,
}
pub struct TransformCache {
    options: CacheOptions,
    state: Mutex<State>,
}
impl TransformCache {
    pub fn new(mut options: CacheOptions) -> Self {
        if options.max_entries <= 0 {
            options.max_entries = 32;
        }
        if options.max_bytes <= 0 {
            options.max_bytes = 128 << 20;
        }
        if options.max_profile_bytes <= 0 {
            options.max_profile_bytes = 1 << 20;
        }
        options.max_profile_bytes = options.max_profile_bytes.min(MAX_PROFILE_BYTES);
        TransformCache {
            options,
            state: Mutex::new(State {
                lru: VecDeque::new(),
                entries: HashMap::new(),
                bytes: 0,
                builds: 0,
            }),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn resolve(&self, data: &[u8]) -> CachedTransform {
        let mut s = self.lock();
        let weight = data.len() as i64 * 64 + (64 << 10);
        if data.len() as i64 > self.options.max_profile_bytes || weight > self.options.max_bytes {
            return CachedTransform {
                err: Some(IccError::CacheResourceLimit),
                ..CachedTransform::default()
            };
        }
        if let Some(entry) = s.entries.get(data) {
            let result = entry.result.clone();
            if let Some(at) = s.lru.iter().position(|k| k.as_slice() == data) {
                let key = s.lru.remove(at).unwrap_or_default();
                s.lru.push_front(key);
            }
            return result;
        }
        let mut result = CachedTransform::default();
        match Profile::parse(data) {
            Ok(profile) => {
                let mut description = trim_space(&profile.description_bytes()).to_vec();
                description.truncate(48);
                result.description = description;
                match Transform::new_srgb(&profile) {
                    Ok(t) => result.transform = Some(Arc::new(t)),
                    Err(e) => result.err = Some(e),
                }
            }
            Err(e) => result.err = Some(e),
        }
        s.builds += 1;
        while s.lru.len() as i64 >= self.options.max_entries
            || s.bytes + weight > self.options.max_bytes
        {
            let Some(old) = s.lru.pop_back() else {
                break;
            };
            if let Some(entry) = s.entries.remove(&old) {
                s.bytes -= entry.weight;
            }
        }
        s.lru.push_front(data.to_vec());
        s.entries.insert(
            data.to_vec(),
            Entry {
                result: result.clone(),
                weight,
            },
        );
        s.bytes += weight;
        result
    }
    pub fn stats(&self) -> CacheStats {
        let s = self.lock();
        CacheStats {
            entries: s.lru.len(),
            bytes: s.bytes,
            builds: s.builds,
        }
    }
    pub fn clear(&self) {
        let mut s = self.lock();
        s.entries.clear();
        s.lru.clear();
        s.bytes = 0;
    }
}
fn trim_space(mut s: &[u8]) -> &[u8] {
    loop {
        let (r, n) = decode_rune(s);
        match r {
            Some(c) if !s.is_empty() && c.is_whitespace() => s = &s[n..],
            _ => break,
        }
    }
    while !s.is_empty() {
        let mut start = s.len() - 1;
        let lim = s.len().saturating_sub(4);
        while start > lim && s[start] & 0xC0 == 0x80 {
            start -= 1;
        }
        let (r, n) = decode_rune(&s[start..]);
        let (r, width) = if start + n != s.len() {
            (None, 1)
        } else {
            (r, n)
        };
        match r {
            Some(c) if c.is_whitespace() => s = &s[..s.len() - width],
            _ => break,
        }
    }
    s
}
