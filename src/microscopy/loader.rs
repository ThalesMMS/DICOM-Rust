use super::{Cause, Error, Kind, Tile, TileFormat, TileImage, TileKey, TileSource};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileResult {
    pub tile: Tile,
    pub image: Option<TileImage>,
    pub err: Option<Error>,
    pub generation: u64,
    pub from_cache: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: i64,
    pub max_bytes: i64,
    pub hits: u64,
    pub misses: u64,
}
pub type RequestOutcome = Option<Error>;
struct Entry {
    image: TileImage,
    bytes: i64,
}
#[derive(Default)]
struct TileCache {
    max_bytes: i64,
    used: i64,
    hits: u64,
    misses: u64,
    entries: HashMap<TileKey, Entry>,
    lru: VecDeque<TileKey>,
}
impl TileCache {
    fn touch(&mut self, key: &TileKey) {
        if let Some(at) = self.lru.iter().position(|k| k == key)
            && let Some(k) = self.lru.remove(at)
        {
            self.lru.push_front(k);
        }
    }
    fn get(&mut self, key: &TileKey) -> Option<TileImage> {
        if let Some(entry) = self.entries.get(key) {
            let image = entry.image.clone();
            self.hits += 1;
            self.touch(key);
            return Some(image);
        }
        self.misses += 1;
        None
    }
    fn put(&mut self, key: TileKey, value: &TileImage) {
        if self.max_bytes <= 0 {
            return;
        }
        let (width, height) = (value.width as i64, value.height as i64);
        let wide = matches!(
            value.format,
            TileFormat::Rgba64 | TileFormat::Nrgba64 | TileFormat::Gray16
        );
        let bytes_per_pixel = if wide { 8 } else { 4 };
        if width <= 0 || height <= 0 || width > self.max_bytes / bytes_per_pixel / height {
            return;
        }
        let owned = if wide {
            value.to_rgba64()
        } else {
            value.to_nrgba()
        };
        let bytes = owned.pix.len() as i64;
        if let Some(entry) = self.entries.get_mut(&key) {
            self.used += bytes - entry.bytes;
            entry.image = owned;
            entry.bytes = bytes;
            self.touch(&key);
        } else {
            self.lru.push_front(key.clone());
            self.entries.insert(
                key,
                Entry {
                    image: owned,
                    bytes,
                },
            );
            self.used += bytes;
        }
        while self.used > self.max_bytes {
            let Some(oldest) = self.lru.pop_back() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.used -= entry.bytes;
            }
        }
    }
    fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            bytes: self.used,
            max_bytes: self.max_bytes,
            hits: self.hits,
            misses: self.misses,
        }
    }
    fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.used = 0;
    }
}
#[derive(Default)]
struct State {
    generation: u64,
    closed: bool,
}
pub struct Loader<S> {
    source: Option<S>,
    workers: i64,
    cache: Mutex<TileCache>,
    state: Mutex<State>,
}
fn canceled() -> Error {
    Error::with_cause(Kind::Other, Some(Cause::Canceled), "context canceled")
}
impl Error {
    #[doc(hidden)]
    pub fn context_canceled() -> Error {
        canceled()
    }
}
fn join(errors: Vec<Error>) -> Option<Error> {
    let first = errors.first()?;
    let text = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    Some(Error::with_cause(first.kind, first.cause(), text))
}
impl<S: TileSource> Loader<S> {
    pub fn new(source: Option<S>, max_cache_bytes: i64, workers: i64) -> Loader<S> {
        Loader {
            source,
            workers: if workers <= 0 { 4 } else { workers },
            cache: Mutex::new(TileCache {
                max_bytes: max_cache_bytes.max(0),
                ..TileCache::default()
            }),
            state: Mutex::new(State::default()),
        }
    }
    pub fn workers(&self) -> i64 {
        self.workers
    }
    fn cache(&self) -> std::sync::MutexGuard<'_, TileCache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn request(
        &self,
        cancel: Option<&AtomicBool>,
        tiles: &[Tile],
        mut publish: impl FnMut(TileResult),
    ) -> Result<RequestOutcome, Error> {
        let Some(source) = &self.source else {
            return Err(Error::other("dicom/microscopy: tile loader has no source"));
        };
        let generation = {
            let mut state = self.state();
            if state.closed {
                return Err(Error::with_cause(
                    Kind::Other,
                    Some(Cause::LoaderClosed),
                    "dicom/microscopy: tile loader is closed",
                ));
            }
            state.generation += 1;
            state.generation
        };
        let tiles = super::deduplicate_tiles(tiles.to_vec(), 0);
        if tiles.is_empty() {
            return Ok(None);
        }
        let is_canceled = || cancel.is_some_and(|c| c.load(Ordering::SeqCst));
        let mut errors = Vec::new();
        for tile in tiles {
            if is_canceled() {
                break;
            }
            let key = tile.key();
            let mut result = TileResult {
                tile,
                generation,
                ..TileResult::default()
            };
            result.image = self.cache().get(&key);
            result.from_cache = result.image.is_some();
            if !result.from_cache {
                let fetched = if is_canceled() {
                    Err(canceled())
                } else {
                    source.fetch_tile(&result.tile)
                };
                match fetched {
                    Ok(image) => {
                        if let Some(image) = &image {
                            self.cache().put(key, image);
                        }
                        result.image = image;
                    }
                    Err(err) => result.err = Some(err),
                }
            }
            if let Some(err) = &result.err
                && err.cause() != Some(Cause::Canceled)
            {
                errors.push(err.clone());
            }
            {
                let state = self.state();
                if state.closed || generation != state.generation {
                    break;
                }
                publish(result);
            }
        }
        if is_canceled() {
            errors.insert(0, canceled());
        }
        Ok(join(errors))
    }
    pub fn cancel(&self) {
        self.state().generation += 1;
    }
    pub fn close(&self) {
        {
            let mut state = self.state();
            if !state.closed {
                state.closed = true;
                state.generation += 1;
            }
        }
        self.cache().clear();
    }
    pub fn cache_stats(&self) -> CacheStats {
        self.cache().stats()
    }
}
