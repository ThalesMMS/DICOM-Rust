use super::codestream::{
    Codestream, Component, QUANT_DERIVED, QUANT_NONE, Quantization, WAVELET_53,
};
use super::dwt::{SubbandGrid, inverse_dwt};
use super::geometry::{Band, CodeBlock, Limits, TileComponent, ceil_div_int};
use super::packet::{CodeBlockData, PacketFilter};
use super::par;
use super::tier1::{decode_block, magnitude_bits, subband_index};
use super::{CoreError, Rect, Result, errorf};
use crate::codec::jpeg2000::j2kpart2::Profile;
use crate::fmath;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Image {
    pub width: i64,
    pub height: i64,
    pub components: Vec<Vec<i32>>,
    pub depths: Vec<i64>,
    pub signed: Vec<bool>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodeOptions {
    pub annex_j: Option<Profile>,
    pub reduce: i64,
    pub region: Rect,
    pub layers: i64,
}
impl Codestream {
    fn output_scale(&self, reduce: i64) -> (i64, i64) {
        let (mut sx, mut sy) = (0, 0);
        for comp in &self.components {
            if sx == 0 || comp.dx < sx {
                sx = comp.dx;
            }
            if sy == 0 || comp.dy < sy {
                sy = comp.dy;
            }
        }
        (sx.max(1) << reduce, sy.max(1) << reduce)
    }
    pub fn output_size(&self, reduce: i64) -> (i64, i64) {
        let (sx, sy) = self.output_scale(reduce);
        (
            ceil_div_int(self.width as i64, sx)
                .saturating_sub(ceil_div_int(self.x_offset as i64, sx)),
            ceil_div_int(self.height as i64, sy)
                .saturating_sub(ceil_div_int(self.y_offset as i64, sy)),
        )
    }
    pub(crate) fn output_origin(&self, reduce: i64) -> (i64, i64) {
        let (sx, sy) = self.output_scale(reduce);
        (
            ceil_div_int(self.x_offset as i64, sx),
            ceil_div_int(self.y_offset as i64, sy),
        )
    }
}
pub fn decode(c: &Codestream, limits: Limits, options: DecodeOptions) -> Result<Image> {
    for component in &c.components {
        if component.depth > 16 && options.annex_j.is_none() {
            return Err(CoreError::Unsupported("components above 16 bits".into()));
        }
    }
    if let Some(profile) = options.annex_j
        && (!profile.valid()
            || profile.frames != c.components.len() as i64
            || options.reduce != 0
            || !options.region.is_empty()
            || options.layers != 0)
    {
        return Err(CoreError::Unsupported(
            "Annex J reconstruction profile".into(),
        ));
    }
    if options.reduce < 0 || options.reduce > 32 {
        return Err(errorf(format_args!(
            "a reduction of {} resolution levels",
            options.reduce
        )));
    }
    if options.layers < 0 {
        return Err(errorf(format_args!("{} quality layers", options.layers)));
    }
    let (width, height) = c.output_size(options.reduce);
    if width <= 0 || height <= 0 {
        return Err(errorf(format_args!("an image of {width}x{height} samples")));
    }
    if width > 1 << 16 || height > 1 << 16 || width.saturating_mul(height) > 1 << 28 {
        return Err(errorf(format_args!(
            "an image of {width}x{height} samples is larger than this decoder accepts"
        )));
    }
    let mut region = Rect::new(0, 0, width, height);
    if !options.region.is_empty() {
        if !options.region.is_in(&region) {
            return Err(errorf(format_args!(
                "a region {} outside the {width}x{height} image",
                options.region
            )));
        }
        region = options.region;
    }
    let (width, height) = (region.dx(), region.dy());
    limits.normalized().check(c)?;
    let mut img = Image {
        width,
        height,
        ..Image::default()
    };
    for comp in &c.components {
        img.components
            .push(vec![0; width.saturating_mul(height) as usize]);
        img.depths.push(comp.depth);
        img.signed.push(comp.signed);
    }
    if let Some(profile) = options.annex_j {
        img.depths.fill(profile.bits);
        img.signed.fill(profile.signed);
    }
    let (sx, sy) = c.output_scale(options.reduce);
    let (origin_x, origin_y) = (
        ceil_div_int(c.x_offset as i64, sx),
        ceil_div_int(c.y_offset as i64, sy),
    );
    for tile in 0..c.num_tiles() {
        let (tx0, ty0, tx1, ty1) = c.tile_bounds(tile);
        let covered = Rect::new(
            ceil_div_int(tx0, sx).saturating_sub(origin_x),
            ceil_div_int(ty0, sy).saturating_sub(origin_y),
            ceil_div_int(tx1, sx).saturating_sub(origin_x),
            ceil_div_int(ty1, sy).saturating_sub(origin_y),
        );
        if !covered.overlaps(&region) {
            continue;
        }
        decode_tile(
            c,
            tile,
            &mut img,
            options.reduce,
            options.layers,
            (region.min_x, region.min_y),
            options.annex_j.as_ref(),
        )?;
    }
    Ok(img)
}
fn f32_to_i32(v: f32) -> i32 {
    fmath::f64_to_i32(v as f64)
}
fn decode_tile(
    c: &Codestream,
    tile: i64,
    img: &mut Image,
    reduce: i64,
    layers: i64,
    origin: (i64, i64),
    annex_j: Option<&Profile>,
) -> Result<()> {
    let ncomp = c.components.len() as i64;
    for comp in 0..ncomp {
        let style = c.coding_for_tile(tile, comp);
        if reduce > style.levels {
            return Err(errorf(format_args!(
                "a reduction of {reduce} levels, but tile {tile} component {comp} has {}",
                style.levels
            )));
        }
    }
    let data = c.read_tile_packets_filtered(tile, PacketFilter { layers, reduce })?;
    let threads = decode_threads();
    let mut work: Vec<ComponentWork<'_>> = Vec::with_capacity(ncomp as usize);
    for comp in 0..ncomp {
        let style = c.coding_for_tile(tile, comp);
        let Some(tc) = data.components.get(comp as usize) else {
            return Err(errorf(format_args!("tile {tile} has no component {comp}")));
        };
        let mut tc = tc.clone();
        let keep = tc.resolutions.len().saturating_sub(reduce as usize);
        let Some(last) = keep.checked_sub(1).and_then(|k| tc.resolutions.get(k)) else {
            return Err(errorf(format_args!(
                "a reduction of {reduce} levels leaves tile {tile} component {comp} no resolution"
            )));
        };
        let (x0, y0, x1, y1) = (last.x0, last.y0, last.x1, last.y1);
        tc.resolutions.truncate(keep);
        tc.x0 = x0;
        tc.y0 = y0;
        tc.x1 = x1;
        tc.y1 = y1;
        let quant = c.quant_for_tile(tile, comp);
        let roi_shift = c.roi_shift_for_tile(tile, comp);
        let (pending, pre_error) =
            pending_blocks(comp, &tc, quant, style.levels, roi_shift, &data.blocks);
        work.push(ComponentWork {
            comp,
            tc,
            quant,
            levels: style.levels,
            reversible: style.wavelet == WAVELET_53,
            roi_shift,
            pending,
            pre_error,
            decoded: Vec::new(),
            samples: Ok(Vec::new()),
        });
    }
    let batch: Vec<(&CodeBlockData, i64)> = work
        .iter()
        .flat_map(|w| w.pending.iter().map(|(block, _, mb)| (*block, *mb)))
        .collect();
    let mut decoded = decode_blocks(&batch, threads).into_iter();
    for w in work.iter_mut() {
        w.decoded = decoded.by_ref().take(w.pending.len()).collect();
    }
    let side_by_side = threads.min(work.len()).max(1);
    let within = threads.checked_div(side_by_side).unwrap_or(1).max(1);
    par::each(&mut work, side_by_side, |_, w| {
        w.samples = w.synthesize(c, within);
    });
    let mut planes: Vec<Vec<f32>> = Vec::with_capacity(ncomp as usize);
    let mut reduced: Vec<TileComponent> = Vec::with_capacity(ncomp as usize);
    for w in work {
        planes.push(w.samples?);
        reduced.push(w.tc);
    }
    let style = c.coding_for_tile(tile, 0);
    if style.mct && planes.len() >= 3 {
        inverse_component_transform(&mut planes, style.wavelet == WAVELET_53)?;
    }
    if annex_j.is_some() {
        for component in 1..planes.len() {
            let (before, after) = planes.split_at_mut(component);
            let (Some(previous), Some(this)) = (before.last(), after.first_mut()) else {
                continue;
            };
            for (v, p) in this.iter_mut().zip(previous.iter()) {
                *v += *p;
            }
        }
    }
    let (sx, sy) = c.output_scale(reduce);
    let (tx0, ty0, tx1, ty1) = c.tile_bounds(tile);
    let origin_x = ceil_div_int(c.x_offset as i64, sx).saturating_add(origin.0);
    let origin_y = ceil_div_int(c.y_offset as i64, sy).saturating_add(origin.1);
    let outputs = planes
        .iter()
        .zip(&reduced)
        .zip(&c.components)
        .zip(&mut img.components);
    for (((plane, tc), &component), out) in outputs {
        if tc.width() <= 0 || tc.height() <= 0 {
            continue;
        }
        let sxc = component.dx << reduce;
        let syc = component.dy << reduce;
        let mut output_component: Component = component;
        if let Some(p) = annex_j {
            output_component.depth = p.bits;
            output_component.signed = p.signed;
        }
        let shift: i32 = if output_component.signed {
            0
        } else {
            1i32.wrapping_shl(output_component.depth.saturating_sub(1) as u32)
        };
        let (lo, hi) = sample_range(&output_component);
        let tw = tc.width() as usize;
        let mut columns: Vec<(usize, usize)> = Vec::new();
        for ox in ceil_div_int(tx0, sx)..ceil_div_int(tx1, sx) {
            let col = ox.saturating_sub(origin_x);
            if col < 0 || col >= img.width {
                continue;
            }
            let xc = floor_div_int(ox.saturating_mul(sx), sxc)
                .max(tc.x0)
                .min(tc.x1.saturating_sub(1));
            columns.push((col as usize, xc.saturating_sub(tc.x0) as usize));
        }
        let Some(&(first_col, first_x)) = columns.first() else {
            continue;
        };
        let count = columns.len();
        let one_run = columns.iter().enumerate().all(|(k, &(col, x))| {
            col.checked_sub(k) == Some(first_col) && x.checked_sub(k) == Some(first_x)
        });
        let width = img.width as usize;
        let first_row = ceil_div_int(ty0, sy).saturating_sub(origin_y).max(0);
        let end_row = ceil_div_int(ty1, sy)
            .saturating_sub(origin_y)
            .min(img.height);
        if first_row >= end_row {
            continue;
        }
        let Some(rows) = out.get_mut(
            (first_row as usize).saturating_mul(width)..(end_row as usize).saturating_mul(width),
        ) else {
            continue;
        };
        let columns = columns.as_slice();
        let row_threads = par::worth(threads.min(8), rows.len(), OUTPUT_SAMPLES_PER_THREAD);
        par::row_runs(rows, width, row_threads, |first, run| {
            for (k, target) in run.chunks_exact_mut(width).enumerate() {
                let oy = first_row
                    .saturating_add(first.saturating_add(k) as i64)
                    .saturating_add(origin_y);
                let yc = floor_div_int(oy.saturating_mul(sy), syc)
                    .max(tc.y0)
                    .min(tc.y1.saturating_sub(1));
                let start = (yc.saturating_sub(tc.y0) as usize).saturating_mul(tw);
                let Some(source) = plane.get(start..start.saturating_add(tw)) else {
                    continue;
                };
                if one_run {
                    let span = |s: usize| s..s.saturating_add(count);
                    let (Some(source), Some(target)) =
                        (source.get(span(first_x)), target.get_mut(span(first_col)))
                    else {
                        continue;
                    };
                    for (t, &s) in target.iter_mut().zip(source) {
                        *t = round_to_i32(s).wrapping_add(shift).clamp(lo, hi);
                    }
                } else {
                    for &(col, x) in columns {
                        if let (Some(t), Some(&s)) = (target.get_mut(col), source.get(x)) {
                            *t = round_to_i32(s).wrapping_add(shift).clamp(lo, hi);
                        }
                    }
                }
            }
        });
    }
    Ok(())
}
const OUTPUT_SAMPLES_PER_THREAD: usize = 1 << 17;
struct ComponentWork<'a> {
    comp: i64,
    tc: TileComponent,
    quant: &'a Quantization,
    levels: i64,
    reversible: bool,
    roi_shift: i64,
    pending: Vec<(&'a CodeBlockData, Band, i64)>,
    pre_error: Option<CoreError>,
    decoded: Vec<Result<Vec<i32>>>,
    samples: Result<Vec<f32>>,
}
impl ComponentWork<'_> {
    fn synthesize(&mut self, c: &Codestream, threads: usize) -> Result<Vec<f32>> {
        let bands = self.subbands(c, threads)?;
        inverse_dwt(&self.tc, bands, self.reversible, threads)
    }
}
#[inline(always)]
fn round_to_i32(sample: f32) -> i32 {
    const UNIT_STEP: f32 = 8_388_608.0;
    let magnitude = sample.abs();
    let rounded = if magnitude < UNIT_STEP {
        ((magnitude + UNIT_STEP) - UNIT_STEP).copysign(sample)
    } else {
        sample
    };
    if (-2_147_483_648.0..2_147_483_648.0).contains(&rounded) {
        rounded as i32
    } else {
        i32::MIN
    }
}
fn floor_div_int(a: i64, b: i64) -> i64 {
    if a >= 0 {
        return a.checked_div(b).unwrap_or(0);
    }
    a.saturating_neg()
        .saturating_add(b)
        .saturating_sub(1)
        .checked_div(b)
        .unwrap_or(0)
        .saturating_neg()
}
fn pending_blocks<'a>(
    comp: i64,
    tc: &TileComponent,
    quant: &Quantization,
    levels: i64,
    roi_shift: i64,
    blocks: &'a [CodeBlockData],
) -> (Vec<(&'a CodeBlockData, Band, i64)>, Option<CoreError>) {
    let mut pending: Vec<(&CodeBlockData, Band, i64)> = Vec::new();
    let mut pre_error = None;
    for block in blocks {
        if block.component != comp {
            continue;
        }
        let Some(res) = usize::try_from(block.resolution)
            .ok()
            .and_then(|r| tc.resolutions.get(r))
        else {
            continue;
        };
        let mut band = Band::default();
        for candidate in &res.bands {
            if candidate.kind == block.band_type {
                band = candidate.clone_shape();
            }
        }
        let mut mb = match magnitude_bits(quant, &band, block.resolution, levels) {
            Ok(mb) => mb,
            Err(e) => {
                pre_error = Some(e);
                break;
            }
        };
        if roi_shift > 0 {
            if mb
                .saturating_add(roi_shift)
                .saturating_sub(1)
                .saturating_sub(block.zero_bit_planes)
                > 29
            {
                pre_error = Some(CoreError::Unsupported(format!(
                    "region of interest shift {roi_shift} past the 30 bit planes a code-block holds here"
                )));
                break;
            }
            mb = mb.saturating_add(roi_shift);
        }
        pending.push((block, band, mb));
    }
    (pending, pre_error)
}
impl ComponentWork<'_> {
    fn subbands(
        &mut self,
        c: &Codestream,
        threads: usize,
    ) -> Result<Vec<[Option<SubbandGrid>; 4]>> {
        let (comp, tc, quant, levels, reversible, roi_shift) = (
            self.comp,
            &self.tc,
            self.quant,
            self.levels,
            self.reversible,
            self.roi_shift,
        );
        let mut bands: Vec<[Option<SubbandGrid>; 4]> = Vec::with_capacity(tc.resolutions.len());
        for res in &tc.resolutions {
            let mut entry: [Option<SubbandGrid>; 4] = [None, None, None, None];
            for band in &res.bands {
                let (w, h) = (band.width(), band.height());
                if let Some(slot) = entry.get_mut(band.kind as usize) {
                    *slot = Some(SubbandGrid {
                        x0: band.x0,
                        y0: band.y0,
                        x1: band.x1,
                        y1: band.y1,
                        values: vec![0.0; w.saturating_mul(h).max(0) as usize],
                    });
                }
            }
            bands.push(entry);
        }
        let pending = std::mem::take(&mut self.pending);
        let decoded = std::mem::take(&mut self.decoded);
        let pre_error = self.pre_error.take();
        place_blocks(
            c, comp, quant, levels, reversible, roi_shift, &pending, decoded, &mut bands, threads,
        )?;
        match pre_error {
            Some(e) => Err(e),
            None => Ok(bands),
        }
    }
}
struct Placement {
    block: CodeBlock,
    coefficients: Vec<i32>,
    step: f32,
}
struct BandRows<'a> {
    values: &'a mut [f32],
    x0: i64,
    width: i64,
    y0: i64,
    y1: i64,
    blocks: Vec<Placement>,
}
impl BandRows<'_> {
    fn fill(&mut self, reversible: bool, roi_shift: i64) {
        for placement in self.blocks.iter_mut() {
            if roi_shift > 0 {
                descale_roi(&mut placement.coefficients, roi_shift);
            }
            let block = &placement.block;
            let bw = block.width();
            let first = block.x0.max(self.x0);
            let count = block
                .x1
                .min(self.x0.saturating_add(self.width))
                .saturating_sub(first);
            if count <= 0 {
                continue;
            }
            let count = count as usize;
            for y in 0..block.height() {
                let row = block.y0.saturating_add(y);
                if row < self.y0 || row >= self.y1 {
                    continue;
                }
                let from = y
                    .saturating_mul(bw)
                    .saturating_add(first.saturating_sub(block.x0));
                let at = row
                    .saturating_sub(self.y0)
                    .saturating_mul(self.width)
                    .saturating_add(first.saturating_sub(self.x0));
                let span = |s: i64| (s as usize)..(s as usize).saturating_add(count);
                let (Some(source), Some(target)) = (
                    placement.coefficients.get(span(from)),
                    self.values.get_mut(span(at)),
                ) else {
                    continue;
                };
                if reversible {
                    for (t, &v) in target.iter_mut().zip(source) {
                        *t = (v / 2) as f32;
                    }
                } else {
                    let step = placement.step;
                    for (t, &v) in target.iter_mut().zip(source) {
                        *t = v as f32 * step / 2.0;
                    }
                }
            }
        }
    }
}
fn band_rows(grid: &mut SubbandGrid, mut blocks: Vec<Placement>) -> Vec<BandRows<'_>> {
    let (x0, width, y0, y1) = (grid.x0, grid.width(), grid.y0, grid.y1);
    if width <= 0 || y1 <= y0 {
        return Vec::new();
    }
    let mut starts: Vec<i64> = blocks
        .iter()
        .map(|p| p.block.y0)
        .filter(|&y| y > y0 && y < y1)
        .collect();
    starts.sort_unstable();
    starts.dedup();
    let crossed = blocks.iter().any(|p| {
        let next = starts.partition_point(|&start| start <= p.block.y0);
        starts.get(next).is_some_and(|&start| start < p.block.y1)
    });
    if crossed {
        starts.clear();
    } else {
        blocks.sort_by_key(|p| p.block.y0.max(y0));
    }
    let mut runs = Vec::with_capacity(starts.len().saturating_add(1));
    let mut rest = grid.values.as_mut_slice();
    let mut top = y0;
    let mut blocks = blocks.into_iter().peekable();
    for end in starts.into_iter().chain([y1]) {
        let at = (end.saturating_sub(top).saturating_mul(width) as usize).min(rest.len());
        let (values, below) = rest.split_at_mut(at);
        rest = below;
        let mut inside = Vec::new();
        while let Some(p) = blocks.next_if(|p| p.block.y0 < end) {
            inside.push(p);
        }
        runs.push(BandRows {
            values,
            x0,
            width,
            y0: top,
            y1: end,
            blocks: inside,
        });
        top = end;
    }
    runs
}
#[allow(clippy::too_many_arguments)]
fn place_blocks(
    c: &Codestream,
    comp: i64,
    quant: &Quantization,
    levels: i64,
    reversible: bool,
    roi_shift: i64,
    pending: &[(&CodeBlockData, Band, i64)],
    decoded: Vec<Result<Vec<i32>>>,
    bands: &mut [[Option<SubbandGrid>; 4]],
    threads: usize,
) -> Result<()> {
    let mut per_band: Vec<[Vec<Placement>; 4]> = bands.iter().map(|_| Default::default()).collect();
    let mut samples = 0usize;
    for ((block, band, _), coefficients) in pending.iter().zip(decoded) {
        let block = *block;
        let coefficients = coefficients?;
        let depth = c.components.get(comp as usize).map_or(0, |k| k.depth);
        let step = step_size(quant, band, block.resolution, levels, depth)?;
        let (r, t) = (block.resolution as usize, block.band_type as usize);
        let present = bands
            .get(r)
            .and_then(|b| b.get(t))
            .is_some_and(|g| g.is_some());
        let Some(placements) = per_band
            .get_mut(r)
            .and_then(|b| b.get_mut(t))
            .filter(|_| present)
        else {
            return Err(errorf(
                "a code-block in a band the resolution does not have",
            ));
        };
        samples = samples.saturating_add(coefficients.len());
        placements.push(Placement {
            block: block.block,
            coefficients,
            step,
        });
    }
    let mut runs: Vec<BandRows<'_>> = Vec::new();
    for (grids, placements) in bands.iter_mut().zip(per_band) {
        for (grid, blocks) in grids.iter_mut().zip(placements) {
            if let Some(grid) = grid {
                runs.extend(band_rows(grid, blocks));
            }
        }
    }
    let threads = par::worth(threads.min(8), samples, PLACED_SAMPLES_PER_THREAD);
    par::each(&mut runs, threads, |_, run| run.fill(reversible, roi_shift));
    Ok(())
}
const PLACED_SAMPLES_PER_THREAD: usize = 1 << 17;
thread_local! {
    static DECODE_THREADS: std::cell::Cell<usize> = const { std::cell::Cell::new(1) };
}
pub fn with_decode_threads<R>(threads: usize, f: impl FnOnce() -> R) -> R {
    let before = DECODE_THREADS.with(|t| t.replace(threads.max(1)));
    struct Restore(usize);
    impl Drop for Restore {
        fn drop(&mut self) {
            DECODE_THREADS.with(|t| t.set(self.0));
        }
    }
    let _restore = Restore(before);
    f()
}
fn decode_threads() -> usize {
    DECODE_THREADS.with(|t| t.get())
}
const BLOCK_BYTES_PER_THREAD: usize = 4096;
fn decode_blocks(pending: &[(&CodeBlockData, i64)], threads: usize) -> Vec<Result<Vec<i32>>> {
    let bytes: usize = pending
        .iter()
        .flat_map(|(block, _)| block.segments.iter())
        .map(|segment| segment.data.len())
        .sum();
    let threads = par::worth(threads, bytes, BLOCK_BYTES_PER_THREAD).min(pending.len());
    if threads <= 1 {
        return pending
            .iter()
            .map(|(block, mb)| decode_block(block, *mb))
            .collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let work = || {
        let mut out = Vec::new();
        loop {
            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let Some((block, mb)) = pending.get(i) else {
                return out;
            };
            out.push((i, decode_block(block, *mb)));
        }
    };
    let mut results: Vec<Option<Result<Vec<i32>>>> = (0..pending.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let helpers: Vec<_> = (1..threads).map(|_| scope.spawn(work)).collect();
        let mut done = work();
        for helper in helpers {
            match helper.join() {
                Ok(more) => done.extend(more),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
        for (i, r) in done {
            if let Some(slot) = results.get_mut(i) {
                *slot = Some(r);
            }
        }
    });
    results
        .into_iter()
        .map(|r| r.unwrap_or_else(|| Err(errorf("a code-block was not decoded"))))
        .collect()
}
impl Band {
    fn clone_shape(&self) -> Band {
        Band {
            kind: self.kind,
            x0: self.x0,
            y0: self.y0,
            x1: self.x1,
            y1: self.y1,
            level: self.level,
            precincts: Vec::new(),
        }
    }
}
fn descale_roi(coefficients: &mut [i32], shift: i64) {
    if shift >= 30 {
        return;
    }
    let threshold: i32 = 1 << shift.saturating_add(1);
    let below = (threshold >> 1).saturating_sub(1);
    for v in coefficients.iter_mut() {
        let magnitude = v.wrapping_abs();
        if magnitude < threshold {
            continue;
        }
        let mut scaled = magnitude >> shift;
        if magnitude & below != 0 {
            scaled |= 1;
        }
        if *v < 0 {
            scaled = scaled.wrapping_neg();
        }
        *v = scaled;
    }
}
fn step_size(
    q: &Quantization,
    band: &Band,
    resolution: i64,
    levels: i64,
    depth: i64,
) -> Result<f32> {
    if q.style == QUANT_NONE {
        return Ok(1.0);
    }
    let index = subband_index(resolution, band.kind);
    let (exponent, mantissa);
    if q.style == QUANT_DERIVED {
        let (Some(&e), Some(&m)) = (q.exponents.first(), q.mantissas.first()) else {
            return Err(errorf("a derived quantization with no values"));
        };
        exponent = e.saturating_add(band.level).saturating_sub(levels);
        mantissa = m;
    } else {
        let at = |v: &[i64]| usize::try_from(index).ok().and_then(|i| v.get(i)).copied();
        let (Some(e), Some(m)) = (at(&q.exponents), at(&q.mantissas)) else {
            return Err(errorf(format_args!(
                "subband {index} has no quantization step, of {} given",
                q.exponents.len()
            )));
        };
        exponent = e;
        mantissa = m;
    }
    let power = depth.saturating_sub(exponent);
    if !(-31..=31).contains(&power) {
        return Err(errorf(format_args!("a quantization step of 2^{power}")));
    }
    Ok(fmath::ldexp(1.0 + mantissa as f64 / 2048.0, power) as f32)
}
fn inverse_component_transform(planes: &mut [Vec<f32>], reversible: bool) -> Result<()> {
    if planes.len() < 3 {
        return Err(errorf(format_args!(
            "a component transform needs three components, not {}",
            planes.len()
        )));
    }
    let [first, second, third, ..] = planes else {
        return Err(errorf(format_args!(
            "a component transform needs three components, not {}",
            planes.len()
        )));
    };
    let n = first.len();
    if second.len() != n || third.len() != n {
        return Err(errorf(
            "the three transformed components are different sizes",
        ));
    }
    let samples = first
        .iter_mut()
        .zip(second.iter_mut())
        .zip(third.iter_mut());
    if reversible {
        for ((p0, p1), p2) in samples {
            let (y0, y1, y2) = (f32_to_i32(*p0), f32_to_i32(*p1), f32_to_i32(*p2));
            let g = y0.wrapping_sub(y1.wrapping_add(y2) >> 2);
            *p0 = y2.wrapping_add(g) as f32;
            *p1 = g as f32;
            *p2 = y1.wrapping_add(g) as f32;
        }
    } else {
        for ((p0, p1), p2) in samples {
            let (y0, y1, y2) = (*p0, *p1, *p2);
            *p0 = y0 + 1.402f32 * y2;
            *p1 = y0 - 0.344136f32 * y1 - 0.714136f32 * y2;
            *p2 = y0 + 1.772f32 * y1;
        }
    }
    Ok(())
}
fn sample_range(comp: &Component) -> (i32, i32) {
    let half = 1i32.wrapping_shl(comp.depth.saturating_sub(1) as u32);
    if comp.signed {
        return (half.wrapping_neg(), half.wrapping_sub(1));
    }
    (0, (1i64 << comp.depth).wrapping_sub(1) as i32)
}
#[cfg(test)]
mod parallel_tests {
    use super::*;
    #[test]
    fn output_samples_round_halves_to_even() {
        let plain = |s: f32| fmath::f64_to_i32((s as f64).round_ties_even());
        for s in [
            0.0f32,
            -0.0,
            0.5,
            -0.5,
            1.5,
            -1.5,
            2.5,
            0.49999997,
            8_388_607.5,
            -8_388_607.5,
            8_388_608.0,
            16_777_216.0,
            2_147_483_520.0,
            2_147_483_648.0,
            -2_147_483_648.0,
            -2_147_483_904.0,
            3.0e9,
            -3.0e9,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            f32::MIN_POSITIVE,
            1.0e-40,
        ] {
            assert_eq!(round_to_i32(s), plain(s), "{s}");
        }
        let mut seed = 0x1234_5678_9ABC_DEF0u64;
        for _ in 0..2_000_000 {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let bits = (seed >> 32) as u32;
            let any = f32::from_bits(bits);
            assert_eq!(round_to_i32(any), plain(any), "{any}");
            let near = ((bits >> 8) as i32 - (1 << 23)) as f32 / 4.0;
            assert_eq!(round_to_i32(near), plain(near), "{near}");
        }
    }
    use crate::codec::common::Metadata;
    use crate::codec::jpeg2000::encoder::{Encoder, Options};
    #[test]
    fn a_decode_over_threads_equals_the_serial_decode() {
        let (w, h) = (640u16, 520u16);
        let mut seed = 12345u32;
        let mut grey = Vec::with_capacity(usize::from(w) * usize::from(h) * 2);
        let mut colour = Vec::with_capacity(usize::from(w) * usize::from(h) * 3);
        for y in 0..u32::from(h) {
            for x in 0..u32::from(w) {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let v = (x * 7 + y * 3 + (seed >> 22)) & 0x0FFF;
                grey.extend_from_slice(&(v as u16).to_le_bytes());
                colour.extend_from_slice(&[(v >> 4) as u8, (x + y) as u8, (seed >> 24) as u8]);
            }
        }
        let metadata = |samples: u16, bits: u16, photometric: &str| Metadata {
            rows: h,
            columns: w,
            samples_per_pixel: samples,
            bits_allocated: if bits > 8 { 16 } else { 8 },
            bits_stored: bits,
            high_bit: bits - 1,
            number_of_frames: 1,
            photometric_interpretation: photometric.into(),
            planar_configuration_present: samples > 1,
            ..Metadata::default()
        };
        let grey_metadata = metadata(1, 12, "MONOCHROME2");
        let colour_metadata = metadata(3, 8, "RGB");
        for (uid, lossy, frame, m) in [
            ("1.2.840.10008.1.2.4.90", false, &grey, &grey_metadata),
            ("1.2.840.10008.1.2.4.91", true, &grey, &grey_metadata),
            ("1.2.840.10008.1.2.4.90", false, &colour, &colour_metadata),
            ("1.2.840.10008.1.2.4.91", true, &colour, &colour_metadata),
        ] {
            let encoder = Encoder::new(&Options {
                transfer_syntax_uid: uid.into(),
                color_transform: m.samples_per_pixel == 3,
                levels: 5,
                step_size: if lossy { 4.0 } else { 0.0 },
                allow_lossy: lossy,
            })
            .expect("an encoder");
            let encoded = encoder.encode_frame(frame, m).expect("encoded");
            let stream = crate::codec::jpeg2000::j2kcore::parse_codestream(&encoded.data)
                .expect("a codestream");
            let serial =
                decode(&stream, Limits::default(), DecodeOptions::default()).expect("serial");
            assert_eq!(serial.components.len(), usize::from(m.samples_per_pixel));
            for threads in [2, 3, 8] {
                let parallel = with_decode_threads(threads, || {
                    decode(&stream, Limits::default(), DecodeOptions::default())
                })
                .expect("parallel");
                assert_eq!(
                    parallel, serial,
                    "{uid}, {} components, on {threads} threads",
                    m.samples_per_pixel
                );
            }
            assert_eq!(DECODE_THREADS.with(|t| t.get()), 1);
        }
    }
}
