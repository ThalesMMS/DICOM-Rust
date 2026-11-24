use super::regional::{
    Region, RegionReader, RegionWriter, RegionalFilter, RegionalOptions, RegionalResult,
    filter_regions, plan_regions, regional_params_pub, regional_valid_pub, regional_window_pub,
    worker_frozen,
};
use super::slab::DEFAULT_SLAB_WORKING_BYTES;
use super::volume::{Ctx, ErrorKind, FilterError, ctx_err, voxel_count};
use crate::render::parallel::Cancellation;
use crate::render::{ErrorKind as RenderKind, RenderError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
const ENTRY_SIZE: i64 = 232;
fn closed_err() -> FilterError {
    FilterError::new(
        ErrorKind::LocalChainClosed,
        "imagefilter: local chain closed",
    )
}
fn superseded_err() -> FilterError {
    FilterError::new(
        ErrorKind::LocalChainSuperseded,
        "imagefilter: local chain superseded",
    )
}
fn budget_err(required: i64, available: i64) -> FilterError {
    FilterError::new(
        ErrorKind::LocalChainBudget,
        format!("imagefilter: local chain budget: need {required} bytes, have {available}"),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LocalChainIdentity {
    pub source_id: [u8; 32],
    pub geometry_id: [u8; 32],
    pub generation: u64,
    pub dims: [i64; 3],
    pub spacing_mm: [f64; 3],
}
#[derive(Clone, Default)]
pub struct LocalChainSource {
    pub identity: LocalChainIdentity,
    pub reader: Option<Arc<dyn RegionReader + Send + Sync>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalChainOptions {
    pub max_working_bytes: i64,
    pub cache_bytes: i64,
    pub max_read_bytes: i64,
    pub max_cache_entries: i64,
    pub tile_size: [i64; 3],
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalChainStagePlan {
    pub filter: RegionalFilter,
    pub input: Region,
    pub output: Region,
    pub regional: RegionalResult,
    pub output_bytes: i64,
}
impl Default for LocalChainStagePlan {
    fn default() -> Self {
        LocalChainStagePlan {
            filter: RegionalFilter::Other,
            input: Region::default(),
            output: Region::default(),
            regional: RegionalResult::default(),
            output_bytes: 0,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LocalChainPlan {
    pub identity: LocalChainIdentity,
    pub stages: usize,
    pub stage: [LocalChainStagePlan; 3],
    pub source: Region,
    pub requested: Region,
    pub composite_halo: [i64; 3],
    pub working_bytes: i64,
    pub source_read_bytes: i64,
    pub cache_payload_limit: i64,
    pub cache_metadata_bytes: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalChainStats {
    pub computed: [bool; 3],
    pub cache_hit_stage: i64,
    pub source_read_bytes: i64,
    pub internal_read_bytes: i64,
    pub cache_bytes_after: i64,
    pub evictions: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct LocalChainResult {
    pub region: Region,
    pub data: Vec<f32>,
    pub plan: LocalChainPlan,
    pub stats: LocalChainStats,
}
#[derive(Clone, Copy, PartialEq)]
struct Key {
    identity: LocalChainIdentity,
    region: Region,
    prefix: [u64; 3],
    stage: usize,
}
struct Entry {
    key: Key,
    data: Arc<Vec<f32>>,
    stamp: u64,
}
struct State {
    source: LocalChainSource,
    stages: [(RegionalFilter, u64); 3],
    n: usize,
    opts: LocalChainOptions,
    revision: u64,
    active: Option<Arc<AtomicBool>>,
    closed: bool,
    cache: Vec<Entry>,
    bytes: i64,
    clock: u64,
    evicted: i64,
}
pub struct LocalChain {
    state: Mutex<State>,
    gate: Mutex<()>,
}
fn add(parts: &[i64]) -> Result<i64, FilterError> {
    let mut total: i64 = 0;
    for &p in parts {
        if p < 0 || total > i64::MAX - p {
            return Err(FilterError::invalid("chain byte overflow"));
        }
        total += p;
    }
    Ok(total)
}
fn region_bytes(r: Region) -> Result<i64, FilterError> {
    super::regional::checked_product_pub(&[r.size[0], r.size[1], r.size[2], 4])
}
fn identity_valid(s: &LocalChainSource) -> Result<(), FilterError> {
    if s.reader.is_none() {
        return Err(FilterError::invalid("nil chain source"));
    }
    if voxel_count(s.identity.dims).is_none() {
        return Err(FilterError::invalid("chain dimensions"));
    }
    for (a, spacing) in s.identity.spacing_mm.iter().copied().enumerate() {
        if spacing.is_nan()
            || spacing.is_infinite()
            || spacing < 0.0
            || (s.identity.dims[a] > 1 && spacing == 0.0)
        {
            return Err(FilterError::invalid("chain spacing"));
        }
    }
    Ok(())
}
fn chain_filter(
    f: &RegionalFilter,
    id: &LocalChainIdentity,
) -> Result<RegionalFilter, FilterError> {
    let f = match f {
        RegionalFilter::Bilateral(_) | RegionalFilter::NonLocalMeans(_) => *f,
        RegionalFilter::NilBilateral => return Err(FilterError::invalid("nil chain bilateral")),
        RegionalFilter::NilNonLocalMeans => return Err(FilterError::invalid("nil chain NLM")),
        _ => {
            return Err(FilterError::invalid(
                "chain admits only bilateral/NLM; global filters need an external materialized barrier",
            ));
        }
    };
    let f = match worker_frozen(&f) {
        RegionalFilter::Bilateral(mut o) => {
            if o.max_kernel_samples == 0 {
                o.max_kernel_samples = 1 << 16;
            }
            RegionalFilter::Bilateral(o)
        }
        RegionalFilter::NonLocalMeans(mut o) => {
            if o.beta == 0.0 {
                o.beta = 1.0;
            }
            if o.block_step == 0 {
                o.block_step = 2;
            }
            if o.max_search_samples == 0 {
                o.max_search_samples = 1 << 16;
            }
            RegionalFilter::NonLocalMeans(o)
        }
        other => other,
    };
    regional_params_pub(&f, id.dims, id.spacing_mm)?;
    Ok(f)
}
pub fn new_local_chain(
    source: LocalChainSource,
    filters: &[RegionalFilter],
    opts: LocalChainOptions,
) -> Result<LocalChain, FilterError> {
    identity_valid(&source)?;
    let mut opts = opts;
    if filters.len() < 2
        || filters.len() > 3
        || opts.max_working_bytes < 0
        || opts.cache_bytes < 0
        || opts.max_read_bytes < 0
        || opts.max_cache_entries < 0
        || opts.max_cache_entries > 128
    {
        return Err(FilterError::invalid("chain stage count/bounds"));
    }
    if opts.tile_size.iter().any(|t| *t < 0) {
        return Err(FilterError::invalid("chain tile size"));
    }
    if opts.max_working_bytes == 0 {
        opts.max_working_bytes = DEFAULT_SLAB_WORKING_BYTES;
    }
    if opts.cache_bytes == 0 {
        opts.max_cache_entries = 0;
    } else if opts.max_cache_entries == 0 {
        opts.max_cache_entries = 8;
    }
    let metadata = opts.max_cache_entries * ENTRY_SIZE;
    let required = add(&[opts.cache_bytes, metadata, 4096])?;
    if required > opts.max_working_bytes {
        return Err(budget_err(required, opts.max_working_bytes));
    }
    let mut stages = [(RegionalFilter::Other, 0u64); 3];
    for (i, f) in filters.iter().enumerate() {
        stages[i] = (chain_filter(f, &source.identity)?, 1);
    }
    Ok(LocalChain {
        state: Mutex::new(State {
            source,
            stages,
            n: filters.len(),
            opts,
            revision: 1,
            active: None,
            closed: false,
            cache: Vec::new(),
            bytes: 0,
            clock: 0,
            evicted: 0,
        }),
        gate: Mutex::new(()),
    })
}
impl State {
    fn plan(&self, requested: Region) -> Result<LocalChainPlan, (LocalChainPlan, FilterError)> {
        let mut p = LocalChainPlan {
            identity: self.source.identity,
            stages: self.n,
            requested,
            cache_payload_limit: self.opts.cache_bytes,
            cache_metadata_bytes: self.opts.max_cache_entries * ENTRY_SIZE,
            ..Default::default()
        };
        macro_rules! tryp {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(e) => return Err((p, e)),
                }
            };
        }
        if !regional_valid_pub(requested, p.identity.dims) {
            return Err((p, FilterError::invalid("chain requested region")));
        }
        let mut region = requested;
        let output_bytes = tryp!(region_bytes(requested));
        let mut total = tryp!(add(&[
            self.opts.cache_bytes,
            p.cache_metadata_bytes,
            4096,
            output_bytes
        ]));
        for i in (0..self.n).rev() {
            let f = self.stages[i].0;
            let k = tryp!(regional_params_pub(
                &f,
                p.identity.dims,
                p.identity.spacing_mm
            ));
            for a in 0..3 {
                if p.composite_halo[a] > i64::MAX - k[a] {
                    return Err((p, FilterError::invalid("composite halo overflow")));
                }
                p.composite_halo[a] += k[a];
            }
            let input = regional_window_pub(region, p.identity.dims, k);
            let regional = match plan_regions(
                &f,
                p.identity.dims,
                p.identity.spacing_mm,
                region,
                RegionalOptions {
                    max_working_bytes: self.opts.max_working_bytes,
                    tile_size: self.opts.tile_size,
                    max_read_bytes: 0,
                },
            ) {
                Ok(r) => r,
                Err((_, e)) => return Err((p, e)),
            };
            let bytes = tryp!(region_bytes(region));
            p.stage[i] = LocalChainStagePlan {
                filter: f,
                input,
                output: region,
                regional,
                output_bytes: bytes,
            };
            total = tryp!(add(&[total, bytes, regional.working_bytes]));
            region = input;
        }
        p.source = region;
        p.source_read_bytes = tryp!(region_bytes(region));
        p.working_bytes = tryp!(add(&[total, p.source_read_bytes]));
        if p.working_bytes > self.opts.max_working_bytes {
            let e = budget_err(p.working_bytes, self.opts.max_working_bytes);
            return Err((p, e));
        }
        if self.opts.max_read_bytes > 0 && p.source_read_bytes > self.opts.max_read_bytes {
            let e = budget_err(p.source_read_bytes, self.opts.max_read_bytes);
            return Err((p, e));
        }
        Ok(p)
    }
    fn key(&self, stage: usize, region: Region) -> Key {
        let mut k = Key {
            identity: self.source.identity,
            region,
            prefix: [0; 3],
            stage,
        };
        for i in 0..=stage {
            k.prefix[i] = self.stages[i].1;
        }
        k
    }
    fn remove(&mut self, i: usize, eviction: bool) {
        self.bytes -= self.cache[i].data.len() as i64 * 4;
        self.cache.remove(i);
        if eviction {
            self.evicted += 1;
        }
    }
    fn clear(&mut self, first: usize) {
        for i in (0..self.cache.len()).rev() {
            if self.cache[i].key.stage >= first {
                self.remove(i, false);
            }
        }
    }
    fn changed(&mut self) {
        self.revision += 1;
        if let Some(a) = &self.active {
            a.store(true, Ordering::SeqCst);
        }
    }
    fn cache_put(&mut self, key: Key, data: Arc<Vec<f32>>) {
        let bytes = data.len() as i64 * 4;
        if bytes > self.opts.cache_bytes || self.opts.max_cache_entries == 0 {
            return;
        }
        if self.cache.iter().any(|e| e.key == key) {
            return;
        }
        while self.bytes > self.opts.cache_bytes - bytes
            || self.cache.len() as i64 >= self.opts.max_cache_entries
        {
            let mut oldest = 0;
            for i in 0..self.cache.len() {
                if self.cache[i].stamp < self.cache[oldest].stamp {
                    oldest = i;
                }
            }
            self.remove(oldest, true);
        }
        self.clock += 1;
        self.cache.push(Entry {
            key,
            data,
            stamp: self.clock,
        });
        self.bytes += bytes;
    }
}
struct ActiveCtx<'a> {
    parent: Ctx<'a>,
    flag: Arc<AtomicBool>,
}
impl Cancellation for ActiveCtx<'_> {
    fn err(&self) -> Option<RenderError> {
        if let Some(e) = self.parent.and_then(|p| p.err()) {
            return Some(e);
        }
        if self.flag.load(Ordering::SeqCst) {
            return Some(RenderError::new(RenderKind::Canceled, "context canceled"));
        }
        None
    }
}
struct ChainBuffer {
    region: Region,
    data: Vec<f32>,
}
impl ChainBuffer {
    fn copy_region(
        &mut self,
        ctx: Ctx<'_>,
        r: Region,
        data: &mut [f32],
        src: Option<&[f32]>,
    ) -> Result<(), FilterError> {
        let len = src.map_or(data.len(), |s| s.len());
        match voxel_count(r.size) {
            Some(n) if len as i64 == n => {}
            _ => return Err(FilterError::invalid("chain region buffer")),
        }
        let b = self.region;
        for a in 0..3 {
            if r.origin[a] < b.origin[a]
                || r.size[a] > b.size[a]
                || r.origin[a] - b.origin[a] > b.size[a] - r.size[a]
            {
                return Err(FilterError::invalid("chain missing halo"));
            }
        }
        let s = r.size.map(|v| v as usize);
        for z in 0..s[2] {
            for y in 0..s[1] {
                ctx_err(ctx)?;
                let bi =
                    (((z as i64 + r.origin[2] - b.origin[2]) * b.size[1] + y as i64 + r.origin[1]
                        - b.origin[1])
                        * b.size[0]
                        + r.origin[0]
                        - b.origin[0]) as usize;
                let ri = (z * s[1] + y) * s[0];
                match src {
                    Some(src) => self.data[bi..bi + s[0]].copy_from_slice(&src[ri..ri + s[0]]),
                    None => data[ri..ri + s[0]].copy_from_slice(&self.data[bi..bi + s[0]]),
                }
            }
        }
        Ok(())
    }
}
struct BufferReader<'a>(&'a ChainBuffer);
impl RegionReader for BufferReader<'_> {
    fn read_region(&self, ctx: Ctx<'_>, r: Region, dst: &mut [f32]) -> Result<(), FilterError> {
        let b = self.0;
        match voxel_count(r.size) {
            Some(n) if dst.len() as i64 == n => {}
            _ => return Err(FilterError::invalid("chain region buffer")),
        }
        for a in 0..3 {
            if r.origin[a] < b.region.origin[a]
                || r.size[a] > b.region.size[a]
                || r.origin[a] - b.region.origin[a] > b.region.size[a] - r.size[a]
            {
                return Err(FilterError::invalid("chain missing halo"));
            }
        }
        let s = r.size.map(|v| v as usize);
        let o = b.region;
        for z in 0..s[2] {
            for y in 0..s[1] {
                ctx_err(ctx)?;
                let bi =
                    (((z as i64 + r.origin[2] - o.origin[2]) * o.size[1] + y as i64 + r.origin[1]
                        - o.origin[1])
                        * o.size[0]
                        + r.origin[0]
                        - o.origin[0]) as usize;
                let ri = (z * s[1] + y) * s[0];
                dst[ri..ri + s[0]].copy_from_slice(&b.data[bi..bi + s[0]]);
            }
        }
        Ok(())
    }
}
impl RegionWriter for ChainBuffer {
    fn write_region(&mut self, ctx: Ctx<'_>, r: Region, src: &[f32]) -> Result<(), FilterError> {
        self.copy_region(ctx, r, &mut [], Some(src))
    }
}
impl LocalChain {
    fn lock(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }
    pub fn plan(&self, requested: Region) -> Result<LocalChainPlan, (LocalChainPlan, FilterError)> {
        let s = self.lock();
        if s.closed {
            return Err((LocalChainPlan::default(), closed_err()));
        }
        s.plan(requested)
    }
    pub fn set_stage(&self, index: i64, filter: &RegionalFilter) -> Result<(), FilterError> {
        let mut s = self.lock();
        if s.closed {
            return Err(closed_err());
        }
        if index < 0 || index >= s.n as i64 {
            return Err(FilterError::invalid("chain stage index"));
        }
        let f = chain_filter(filter, &s.source.identity)?;
        let i = index as usize;
        if f == s.stages[i].0 {
            return Ok(());
        }
        s.stages[i].0 = f;
        s.stages[i].1 += 1;
        s.clear(i);
        s.changed();
        Ok(())
    }
    pub fn replace_source(&self, source: LocalChainSource) -> Result<(), FilterError> {
        identity_valid(&source)?;
        let mut s = self.lock();
        if s.closed {
            return Err(closed_err());
        }
        for i in 0..s.n {
            chain_filter(&s.stages[i].0, &source.identity)?;
        }
        s.source = source;
        s.clear(0);
        s.changed();
        Ok(())
    }
    pub fn clear_cache(&self) {
        self.lock().clear(0);
    }
    pub fn close(&self) {
        let mut s = self.lock();
        s.closed = true;
        s.clear(0);
        s.changed();
        s.source = LocalChainSource::default();
        s.stages = [(RegionalFilter::Other, 0); 3];
        s.cache = Vec::new();
    }
    pub fn cache_bytes(&self) -> i64 {
        self.lock().bytes
    }
    pub fn run(&self, ctx: Ctx<'_>, requested: Region) -> Result<LocalChainResult, FilterError> {
        let _gate = match self.gate.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut s = self.lock();
        if s.closed {
            return Err(closed_err());
        }
        let p = s.plan(requested).map_err(|(_, e)| e)?;
        ctx_err(ctx)?;
        let (revision, source, opts) = (s.revision, s.source.clone(), s.opts);
        let flag = Arc::new(AtomicBool::new(false));
        s.active = Some(flag.clone());
        let active = ActiveCtx { parent: ctx, flag };
        let actx: Ctx<'_> = Some(&active);
        let mut keys = [s.key(0, Region::default()); 3];
        for (i, k) in keys.iter_mut().enumerate().take(p.stages) {
            *k = s.key(i, p.stage[i].output);
        }
        let mut stats = LocalChainStats {
            cache_hit_stage: -1,
            ..Default::default()
        };
        let mut start = 0;
        let mut input: Option<ChainBuffer> = None;
        let mut i = p.stages;
        while i > 0 && input.is_none() {
            i -= 1;
            let mut hit = None;
            for j in 0..s.cache.len() {
                if s.cache[j].key == keys[i] {
                    hit = Some(j);
                    break;
                }
            }
            if let Some(j) = hit {
                s.clock += 1;
                let clock = s.clock;
                s.cache[j].stamp = clock;
                input = Some(ChainBuffer {
                    region: p.stage[i].output,
                    data: s.cache[j].data.as_ref().clone(),
                });
                stats.cache_hit_stage = i as i64;
                start = i + 1;
            }
        }
        drop(s);
        let r = self.run_stages(
            &active, actx, &p, revision, source, opts, input, start, keys, stats,
        );
        self.lock().active = None;
        r
    }
    #[allow(clippy::too_many_arguments)]
    fn run_stages(
        &self,
        _active: &ActiveCtx<'_>,
        actx: Ctx<'_>,
        p: &LocalChainPlan,
        revision: u64,
        source: LocalChainSource,
        opts: LocalChainOptions,
        input: Option<ChainBuffer>,
        start: usize,
        keys: [Key; 3],
        mut stats: LocalChainStats,
    ) -> Result<LocalChainResult, FilterError> {
        let finish_error = |cause: FilterError| -> FilterError {
            let s = self.lock();
            if s.closed {
                return closed_err();
            }
            if revision != s.revision {
                return superseded_err();
            }
            cause
        };
        if let Err(e) = ctx_err(actx) {
            return Err(finish_error(e));
        }
        let mut input = match input {
            Some(b) => b,
            None => {
                let count = voxel_count(p.source.size).unwrap_or(0) as usize;
                let mut b = ChainBuffer {
                    region: p.source,
                    data: vec![0.0; count],
                };
                let reader = source.reader.as_ref().map(|r| r.as_ref());
                if let Some(reader) = reader
                    && let Err(e) = reader.read_region(actx, p.source, &mut b.data)
                {
                    return Err(finish_error(e));
                }
                stats.source_read_bytes = p.source_read_bytes;
                for (i, v) in b.data.iter().enumerate() {
                    if i % 4096 == 0
                        && let Err(e) = ctx_err(actx)
                    {
                        return Err(finish_error(e));
                    }
                    if !v.is_finite() {
                        return Err(finish_error(FilterError::invalid("nonfinite chain source")));
                    }
                }
                b
            }
        };
        let mut pending: [Option<Arc<Vec<f32>>>; 3] = [None, None, None];
        for i in start..p.stages {
            let stage = &p.stage[i];
            let count = voxel_count(stage.output.size).unwrap_or(0) as usize;
            let mut out = ChainBuffer {
                region: stage.output,
                data: vec![0.0; count],
            };
            let reader = BufferReader(&input);
            let (res, r) = filter_regions(
                actx,
                &stage.filter,
                crate::imagefilter::RegionalIo {
                    src: Some(&reader),
                    dst: Some(&mut out),
                },
                p.identity.dims,
                p.identity.spacing_mm,
                stage.output,
                RegionalOptions {
                    max_working_bytes: opts.max_working_bytes,
                    tile_size: stage.regional.tile_size,
                    max_read_bytes: 0,
                },
            );
            if let Err(e) = r {
                return Err(finish_error(e));
            }
            stats.computed[i] = true;
            stats.internal_read_bytes += res.read_bytes;
            pending[i] = Some(Arc::new(out.data.clone()));
            input = out;
        }
        let mut data = vec![0f32; input.data.len()];
        let mut i = 0;
        while i < data.len() {
            if let Err(e) = ctx_err(actx) {
                return Err(finish_error(e));
            }
            let end = i + 4096.min(data.len() - i);
            data[i..end].copy_from_slice(&input.data[i..end]);
            i = end;
        }
        let mut s = self.lock();
        if s.closed {
            return Err(closed_err());
        }
        if revision != s.revision {
            return Err(superseded_err());
        }
        ctx_err(actx)?;
        let before = s.evicted;
        for (i, out) in pending.iter().enumerate() {
            if let Some(out) = out {
                s.cache_put(keys[i], out.clone());
            }
        }
        stats.cache_bytes_after = s.bytes;
        stats.evictions = s.evicted - before;
        Ok(LocalChainResult {
            region: p.requested,
            data,
            plan: *p,
            stats,
        })
    }
}
