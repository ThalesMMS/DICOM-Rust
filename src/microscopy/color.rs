use super::{
    Cause, ColorDiagnostic, Error, Instance, InstanceRef, Kind, MAX_INSTANCE_PROFILE_BYTES,
    MAX_OPTICAL_PATH_PROFILE_BYTES, MAX_OPTICAL_PATHS, Tile, TileFormat, TileImage, TileSource,
};
use crate::icc::{CacheOptions, CacheStats, IccError, Transform, TransformCache};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
pub const MAX_COLOR_SOURCES: usize = 4096;
pub const MAX_COLOR_PROFILE_BYTES: usize = 32 << 20;
pub const COLOR_CONVERTED: &str = "converted";
pub const COLOR_IDENTITY: &str = "sRGB";
pub const COLOR_MISSING_PROFILE: &str = "missing-profile";
pub const COLOR_INVALID_PROFILE: &str = "invalid-profile";
pub const COLOR_UNSUPPORTED_PROFILE: &str = "unsupported-profile";
pub const COLOR_UNKNOWN_PATH: &str = "unknown-path";
pub const COLOR_MISSING_PATH: &str = "missing-path";
pub const COLOR_RESOURCE_LIMITED: &str = "resource-limited";
#[derive(Clone, Debug, Default)]
pub struct ColorResult {
    pub transform: Option<Arc<Transform>>,
    pub profile_description: Vec<u8>,
    pub diagnostic: ColorDiagnostic,
    pub cause: Option<Error>,
}
impl ColorResult {
    pub(crate) fn diagnostic_error(&self) -> Error {
        let (kind, cause) = self
            .cause
            .as_ref()
            .map_or((Kind::Other, None), |e| (e.kind, e.cause()));
        let text = format!(
            "optical path {}: {}",
            crate::textfmt::quote(&self.diagnostic.optical_path),
            self.diagnostic.state
        );
        Error::with_cause(kind, cause, text).with_diagnostic(self.diagnostic.clone())
    }
}
fn icc_error(err: &IccError) -> Error {
    let cause = match err {
        IccError::CacheResourceLimit => Cause::IccCacheResourceLimit,
        IccError::Unsupported(_) => Cause::IccUnsupportedProfile,
        IccError::Invalid(_) => Cause::Icc,
    };
    Error::with_cause(Kind::Other, Some(cause), err.to_string())
}
#[derive(Default)]
struct Profiles {
    profiles: HashMap<InstanceRef, HashMap<String, Vec<u8>>>,
    bytes: usize,
}
pub struct ColorResolver {
    state: RwLock<Profiles>,
    cache: Arc<TransformCache>,
}
impl ColorResolver {
    pub fn new(cache: Option<Arc<TransformCache>>) -> ColorResolver {
        ColorResolver {
            state: RwLock::new(Profiles::default()),
            cache: cache.unwrap_or_else(|| Arc::new(TransformCache::new(CacheOptions::default()))),
        }
    }
    pub fn add(&self, i: &Instance) -> Result<(), Error> {
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        if state.profiles.contains_key(&i.level.source) {
            return Err(Error::invalid("duplicate color source"));
        }
        if state.profiles.len() >= MAX_COLOR_SOURCES || i.optical_paths.len() > MAX_OPTICAL_PATHS {
            return Err(Error::bare(Kind::ColorResourceLimit));
        }
        let mut paths: HashMap<String, Vec<u8>> = HashMap::with_capacity(i.optical_paths.len());
        let mut count = 0usize;
        for path in &i.optical_paths {
            if path.identifier.is_empty() {
                return Err(Error::bare(Kind::MissingOpticalPath));
            }
            if path.identifier.len() > 64 {
                return Err(Error::bare(Kind::ColorResourceLimit));
            }
            if paths.contains_key(&path.identifier) {
                return Err(Error::invalid("duplicate optical path"));
            }
            if path.icc_profile.len() > MAX_OPTICAL_PATH_PROFILE_BYTES
                || path.icc_profile.len() > MAX_INSTANCE_PROFILE_BYTES - count
            {
                return Err(Error::bare(Kind::ColorResourceLimit));
            }
            count += path.icc_profile.len();
            paths.insert(path.identifier.clone(), Vec::new());
        }
        if count > MAX_COLOR_PROFILE_BYTES.saturating_sub(state.bytes) {
            return Err(Error::bare(Kind::ColorResourceLimit));
        }
        for path in &i.optical_paths {
            paths.insert(path.identifier.clone(), path.icc_profile.clone());
        }
        state.profiles.insert(i.level.source.clone(), paths);
        state.bytes += count;
        Ok(())
    }
    pub fn resolve_frame(&self, i: &Instance, frame: i64) -> ColorResult {
        match i.frame_optical_path(frame) {
            Ok(id) => self.resolve_tile(&Tile {
                source: i.level.source.clone(),
                frame_number: frame,
                optical_path: id,
                ..Tile::default()
            }),
            Err(err) => ColorResult {
                diagnostic: ColorDiagnostic {
                    source: i.level.source.clone(),
                    optical_path: String::new(),
                    state: COLOR_MISSING_PATH,
                },
                cause: Some(err),
                ..ColorResult::default()
            },
        }
    }
    pub fn resolve_tile(&self, tile: &Tile) -> ColorResult {
        let mut d = ColorDiagnostic {
            source: tile.source.clone(),
            optical_path: tile.optical_path.clone(),
            state: "",
        };
        let fail = |mut d: ColorDiagnostic, state, kind| {
            d.state = state;
            ColorResult {
                diagnostic: d,
                cause: Some(kind),
                ..ColorResult::default()
            }
        };
        if tile.optical_path.is_empty() {
            return fail(d, COLOR_MISSING_PATH, Error::bare(Kind::MissingOpticalPath));
        }
        let data = {
            let state = self.state.read().unwrap_or_else(|e| e.into_inner());
            state
                .profiles
                .get(&tile.source)
                .and_then(|paths| paths.get(&tile.optical_path))
                .cloned()
        };
        let Some(data) = data else {
            return fail(d, COLOR_UNKNOWN_PATH, Error::bare(Kind::UnknownOpticalPath));
        };
        if data.is_empty() {
            let cause = Error::with_cause(
                Kind::Other,
                Some(Cause::MissingOpticalPathProfile),
                "dicom/microscopy: optical path ICC absent",
            );
            return fail(d, COLOR_MISSING_PROFILE, cause);
        }
        let value = self.cache.resolve(&data);
        let mut cause = None;
        d.state = match &value.err {
            Some(err) => {
                cause = Some(icc_error(err));
                match err {
                    IccError::CacheResourceLimit => COLOR_RESOURCE_LIMITED,
                    IccError::Unsupported(_) => COLOR_UNSUPPORTED_PROFILE,
                    IccError::Invalid(_) => COLOR_INVALID_PROFILE,
                }
            }
            None if value.transform.as_ref().is_some_and(|t| t.identity()) => COLOR_IDENTITY,
            None => COLOR_CONVERTED,
        };
        ColorResult {
            transform: value.transform,
            profile_description: value.description,
            diagnostic: d,
            cause,
        }
    }
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }
    pub fn clear(&self) {
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        *state = Profiles::default();
        self.cache.clear();
    }
}
pub struct ColorManagedTileSource<S> {
    pub source: Option<S>,
    pub colors: Arc<ColorResolver>,
}
impl<S: TileSource> TileSource for ColorManagedTileSource<S> {
    fn fetch_tile(&self, tile: &Tile) -> Result<Option<TileImage>, Error> {
        let result = self.colors.resolve_tile(tile);
        if matches!(
            result.diagnostic.state,
            COLOR_UNKNOWN_PATH | COLOR_MISSING_PATH | COLOR_RESOURCE_LIMITED
        ) {
            return Err(result.diagnostic_error());
        }
        let Some(source) = &self.source else {
            return Err(Error::other("microscopy tile source is nil"));
        };
        let Some(img) = source.fetch_tile(tile)? else {
            return Ok(None);
        };
        let Some(transform) = result.transform.as_ref().filter(|t| !t.identity()) else {
            return Ok(Some(img));
        };
        let (w, h) = (img.width as i64, img.height as i64);
        if w <= 0 || h <= 0 || w > (16 << 20) / h {
            return Err(Error::bare(Kind::ColorResourceLimit));
        }
        let owned = match img.format {
            TileFormat::Rgba64 | TileFormat::Nrgba64 => {
                let mut owned = img.to_rgba64();
                transform.apply_rgba64(&mut owned.pix, owned.width * 8, owned.width, owned.height);
                owned
            }
            _ => {
                let mut owned = img.to_nrgba();
                transform.apply_rgba8(&mut owned.pix, owned.width * 4, owned.width, owned.height);
                owned
            }
        };
        Ok(Some(owned))
    }
}
