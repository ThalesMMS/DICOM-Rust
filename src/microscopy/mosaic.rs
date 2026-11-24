use super::Rect;
use crate::dicomjson::jsonwire::{append_string, format_float};
use crate::fmath;
use crate::optimize::Context;
use crate::registration::{
    Image, MaskedFftOptions, MaskedFftResult, MaskedFftSearch, Matrix3, RegError, Sentinel,
    estimate_masked_fft_translation, masked_fft_workspace,
};
use crate::render::Vec3;
use sha2::{Digest, Sha256};
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MosaicKind {
    Admission,
    Limit,
    Inconclusive,
    Canceled,
    Registration,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MosaicError {
    pub kind: MosaicKind,
    text: String,
    registration: Option<RegError>,
}
const ERR_MOSAIC: &str = "microscopy: mosaic admission";
const ERR_MOSAIC_LIMIT: &str = "microscopy: mosaic quota";
const ERR_MOSAIC_INCONCLUSIVE: &str = "microscopy: inconclusive mosaic";
impl MosaicError {
    fn admission(detail: &str) -> MosaicError {
        MosaicError {
            kind: MosaicKind::Admission,
            text: format!("{ERR_MOSAIC}: {detail}"),
            registration: None,
        }
    }
    fn limit(detail: &str) -> MosaicError {
        MosaicError {
            kind: MosaicKind::Limit,
            text: format!("{ERR_MOSAIC_LIMIT}: {detail}"),
            registration: None,
        }
    }
    fn canceled() -> MosaicError {
        MosaicError {
            kind: MosaicKind::Canceled,
            text: "context canceled".to_string(),
            registration: None,
        }
    }
    fn registration(err: RegError) -> MosaicError {
        let kind = if err.is(Sentinel::Canceled) {
            MosaicKind::Canceled
        } else {
            MosaicKind::Registration
        };
        MosaicError {
            kind,
            text: err.to_string(),
            registration: Some(err),
        }
    }
    pub fn is(&self, kind: MosaicKind) -> bool {
        self.kind == kind
    }
    #[doc(hidden)]
    pub fn registration_error(&self) -> Option<&RegError> {
        self.registration.as_ref()
    }
}
impl fmt::Display for MosaicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for MosaicError {}
#[derive(Clone, Debug, Default)]
pub struct MosaicTile<'a> {
    pub id: String,
    pub grid: (i64, i64),
    pub image: Option<&'a Image>,
    pub mask: Option<&'a Image>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MosaicOptions {
    pub anchor_id: String,
    pub max_correction_mm: f64,
    pub min_overlap: i64,
    pub min_overlap_fraction: f64,
    pub min_score: f64,
    pub ambiguity_tolerance: f64,
    pub max_cycle_error_mm: f64,
    pub max_gain_ratio: f64,
    pub max_offset_fraction: f64,
    pub max_tiles: i64,
    pub max_samples: u64,
    pub max_bytes: u64,
    pub max_fft_work: u64,
    pub max_work: u64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MosaicPlacement {
    pub id: String,
    pub grid: (i64, i64),
    pub size: [i64; 3],
    pub source_origin: Vec3,
    pub origin: Vec3,
    pub source_digest: [u8; 32],
    pub component: i64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MosaicLink {
    pub a: i64,
    pub b: i64,
    pub registration: MaskedFftResult,
    pub offset: Vec3,
    pub accepted: bool,
    pub reason: String,
    pub gain: f64,
    pub offset_intensity: f64,
    pub offset_fraction: f64,
    pub residual_mm: f64,
    pub cycle_closure_mm: f64,
    pub residual_measured: bool,
    pub cycle_closure_measured: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MosaicLayout {
    pub id: String,
    pub anchor_id: String,
    pub tiles: Vec<MosaicPlacement>,
    pub links: Vec<MosaicLink>,
    pub conclusive: bool,
    pub components: i64,
    pub cycle_rank: i64,
    pub max_residual_mm: f64,
    pub max_cycle_closure_mm: f64,
    pub max_displacement_mm: f64,
    pub diagnostics: Vec<String>,
    pub spacing: Vec3,
    pub direction: Matrix3,
    pub anchor_origin: Vec3,
    pub scratch_bytes: u64,
    pub fft_work: u64,
    pub work: u64,
    seal: [u8; 32],
}
struct MosaicRun<'a> {
    ctx: Context<'a>,
    max_work: u64,
    work: u64,
}
impl MosaicRun<'_> {
    fn check(&self) -> Result<(), MosaicError> {
        self.ctx.err().map_err(|_| MosaicError::canceled())
    }
    fn step(&mut self, n: u64) -> Result<(), MosaicError> {
        self.check()?;
        if n > self.max_work - self.work {
            return Err(MosaicError::limit("logical work"));
        }
        self.work += n;
        Ok(())
    }
}
fn finite_positive(value: f64) -> bool {
    value > 0.0 && value.is_finite()
}
fn length(v: Vec3) -> f64 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}
fn mosaic_options(mut o: MosaicOptions) -> Result<MosaicOptions, MosaicError> {
    if !finite_positive(o.max_correction_mm) {
        return Err(MosaicError::admission(
            "explicit positive search bound required",
        ));
    }
    if o.min_overlap == 0 {
        o.min_overlap = 16;
    }
    if o.min_overlap_fraction == 0.0 {
        o.min_overlap_fraction = 0.1;
    }
    if o.min_score == 0.0 {
        o.min_score = 0.9;
    }
    if o.ambiguity_tolerance == 0.0 {
        o.ambiguity_tolerance = 1e-8;
    }
    if o.max_cycle_error_mm == 0.0 {
        o.max_cycle_error_mm = 1e-6;
    }
    if o.max_gain_ratio == 0.0 {
        o.max_gain_ratio = 1.25;
    }
    if o.max_offset_fraction == 0.0 {
        o.max_offset_fraction = 0.1;
    }
    if o.max_tiles == 0 {
        o.max_tiles = 128;
    }
    if o.max_samples == 0 {
        o.max_samples = 8 << 20;
    }
    if o.max_bytes == 0 {
        o.max_bytes = 256 << 20;
    }
    if o.max_fft_work == 0 {
        o.max_fft_work = 1 << 32;
    }
    if o.max_work == 0 {
        o.max_work = 64 << 20;
    }
    if o.min_overlap < 3
        || !finite_positive(o.min_overlap_fraction)
        || o.min_overlap_fraction > 1.0
        || !finite_positive(o.min_score)
        || o.min_score > 1.0
        || !finite_positive(o.ambiguity_tolerance)
        || o.ambiguity_tolerance > 2.0
        || !finite_positive(o.max_cycle_error_mm)
        || !o.max_gain_ratio.is_finite()
        || o.max_gain_ratio < 1.0
        || !finite_positive(o.max_offset_fraction)
        || o.max_tiles < 2
        || o.max_tiles > 1024
        || o.max_samples > i32::MAX as u64
        || o.anchor_id.len() > 256
    {
        return Err(MosaicError::admission("options"));
    }
    Ok(o)
}
fn size_i64(image: &Image) -> [i64; 3] {
    let s = image.size();
    [s[0] as i64, s[1] as i64, s[2] as i64]
}
fn source_digest(run: &mut MosaicRun<'_>, t: &MosaicTile<'_>) -> Result<[u8; 32], MosaicError> {
    let Some(image) = t.image else {
        return Err(MosaicError::admission("tile identity/grid/image"));
    };
    let mut h = Sha256::new();
    let put = |h: &mut Sha256, v: u64| h.update(v.to_le_bytes());
    for v in image.size() {
        put(&mut h, v as u64);
    }
    let (origin, spacing) = (image.origin(), image.spacing());
    for v in [
        origin.x, origin.y, origin.z, spacing.x, spacing.y, spacing.z,
    ] {
        put(&mut h, v.to_bits());
    }
    for row in image.direction().0 {
        for v in row {
            put(&mut h, v.to_bits());
        }
    }
    for im in [Some(image), t.mask] {
        let Some(im) = im else {
            put(&mut h, 0);
            continue;
        };
        put(&mut h, im.data().len() as u64);
        for v in im.data() {
            run.step(1)?;
            if !v.is_finite() {
                return Err(MosaicError::admission("nonfinite input"));
            }
            put(&mut h, u64::from(v.to_bits()));
        }
    }
    Ok(h.finalize().into())
}
fn illumination(
    run: &mut MosaicRun<'_>,
    f: &Image,
    m: &Image,
    fm: &Image,
    mm: &Image,
    s: [i64; 3],
) -> Result<(f64, f64, f64), MosaicError> {
    let mut n = 0.0f64;
    let (mut mf, mut mt, mut cff, mut cft) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let (fs, ms) = (size_i64(f), size_i64(m));
    for y in 0..fs[1] {
        for x in 0..fs[0] {
            run.step(1)?;
            let (j, k) = (x + s[0], y + s[1]);
            if j < 0
                || k < 0
                || j >= ms[0]
                || k >= ms[1]
                || fm.at(x as usize, y as usize, 0) <= 0.0
                || mm.at(j as usize, k as usize, 0) <= 0.0
            {
                continue;
            }
            let a = f64::from(f.at(x as usize, y as usize, 0));
            let b = f64::from(m.at(j as usize, k as usize, 0));
            n += 1.0;
            let (da, db) = (a - mf, b - mt);
            mf += da / n;
            mt += db / n;
            cff += da * (a - mf);
            cft += da * (b - mt);
            lo = fmath::min(lo, a);
            hi = fmath::max(hi, a);
        }
    }
    if cff <= 0.0 || hi <= lo {
        return Err(MosaicError::admission("unresolved illumination"));
    }
    let gain = cft / cff;
    let offset = mt - gain * mf;
    let fraction = offset.abs() / (hi - lo);
    if !gain.is_finite() || !offset.is_finite() || !fraction.is_finite() {
        return Err(MosaicError::admission("illumination arithmetic"));
    }
    Ok((gain, offset, fraction))
}
pub fn register_mosaic(
    ctx: Context<'_>,
    tiles: &[MosaicTile<'_>],
    options: &MosaicOptions,
) -> Result<MosaicLayout, MosaicError> {
    ctx.err().map_err(|_| MosaicError::canceled())?;
    let o = mosaic_options(options.clone())?;
    let mut run = MosaicRun {
        ctx,
        max_work: o.max_work,
        work: 0,
    };
    if tiles.len() < 2 || tiles.len() as i64 > o.max_tiles {
        return Err(MosaicError::limit("tile count"));
    }
    let n = tiles.len() as u64;
    let mut base = n * n * 32 + n * 4096;
    if base >= o.max_bytes {
        return Err(MosaicError::limit("graph scratch"));
    }
    let mut result = MosaicLayout {
        tiles: vec![MosaicPlacement::default(); tiles.len()],
        ..MosaicLayout::default()
    };
    let mut generated: Vec<Option<Image>> = Vec::with_capacity(tiles.len());
    let mut samples = 0u64;
    let first_size = tiles[0].image.map(size_i64);
    let first_origin = tiles[0].image.map(Image::origin);
    for (i, t) in tiles.iter().enumerate() {
        run.step(1)?;
        let Some(image) = t.image else {
            return Err(MosaicError::admission("tile identity/grid/image"));
        };
        if t.id.is_empty()
            || t.id.len() > 256
            || t.grid.0 < 0
            || t.grid.1 < 0
            || t.grid.0 > 1_000_000
            || t.grid.1 > 1_000_000
        {
            return Err(MosaicError::admission("tile identity/grid/image"));
        }
        if tiles[..i]
            .iter()
            .any(|prev| prev.id == t.id || prev.grid == t.grid)
        {
            return Err(MosaicError::admission("duplicate tile identity/grid"));
        }
        let size = size_i64(image);
        if size[2] != 1 || size[0] < 2 || size[1] < 2 {
            return Err(MosaicError::admission("scalar 2D dimensions"));
        }
        if size[0] as u64 > o.max_samples / size[1] as u64 {
            return Err(MosaicError::limit("tile samples"));
        }
        let count = size[0] as u64 * size[1] as u64;
        if count > o.max_samples - samples || count != image.data().len() as u64 {
            return Err(MosaicError::limit("total samples"));
        }
        samples += count;
        if i == 0 {
            result.spacing = image.spacing();
            result.direction = image.direction();
        }
        if image.spacing() != result.spacing
            || image.direction() != result.direction
            || Some(size) != first_size
        {
            return Err(MosaicError::admission(
                "incompatible dimensions/spacing/direction",
            ));
        }
        let d = result.direction.0;
        for a in 0..3 {
            for b in 0..3 {
                let mut dot = 0.0f64;
                for row in &d {
                    dot += row[a] * row[b];
                }
                let want = if a == b { 1.0 } else { 0.0 };
                if !dot.is_finite() || (dot - want).abs() > 1e-8 {
                    return Err(MosaicError::admission("nonorthonormal plane"));
                }
            }
        }
        let normal = Vec3::new(d[0][2], d[1][2], d[2][2]);
        let delta = image.origin().sub(first_origin.unwrap_or_default());
        if (delta.x * normal.x + delta.y * normal.y + delta.z * normal.z).abs() > 1e-8 {
            return Err(MosaicError::admission("different focal plane"));
        }
        match t.mask {
            None => {
                if count > (o.max_bytes - base) / 4 {
                    return Err(MosaicError::limit("mask scratch"));
                }
                base += count * 4;
                let mut data = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    run.step(1)?;
                    data.push(1.0f32);
                }
                let mask = Image::new(size, data, image.origin(), result.spacing, result.direction)
                    .map_err(MosaicError::registration)?;
                generated.push(Some(mask));
            }
            Some(mask) => {
                if size_i64(mask) != size
                    || mask.origin() != image.origin()
                    || mask.spacing() != result.spacing
                    || mask.direction() != result.direction
                    || mask.data().len() as u64 != count
                {
                    return Err(MosaicError::admission("independent mask grid"));
                }
                generated.push(None);
            }
        }
        let digest = source_digest(&mut run, t)?;
        result.tiles[i] = MosaicPlacement {
            id: t.id.clone(),
            grid: t.grid,
            size,
            source_origin: image.origin(),
            origin: image.origin(),
            source_digest: digest,
            component: 0,
        };
    }
    let images: Vec<&Image> = tiles.iter().filter_map(|t| t.image).collect();
    let masks: Vec<&Image> = tiles
        .iter()
        .zip(&generated)
        .filter_map(|(t, g)| g.as_ref().or(t.mask))
        .collect();
    if images.len() != tiles.len() || masks.len() != tiles.len() {
        return Err(MosaicError::admission("tile identity/grid/image"));
    }
    let mut anchor = 0usize;
    for (i, t) in tiles.iter().enumerate() {
        if o.anchor_id.is_empty() {
            let at = tiles[anchor].grid;
            if t.grid.1 < at.1 || (t.grid.1 == at.1 && t.grid.0 < at.0) {
                anchor = i;
            }
        } else if t.id == o.anchor_id {
            anchor = i;
        }
    }
    if !o.anchor_id.is_empty() && tiles[anchor].id != o.anchor_id {
        return Err(MosaicError::admission("missing anchor"));
    }
    result.anchor_id = tiles[anchor].id.clone();
    result.anchor_origin = images[anchor].origin();
    result.scratch_bytes = base;
    for a in 0..tiles.len() {
        for b in a + 1..tiles.len() {
            run.step(1)?;
            let (dx, dy) = (
                tiles[b].grid.0 - tiles[a].grid.0,
                tiles[b].grid.1 - tiles[a].grid.1,
            );
            if dx.abs() + dy.abs() != 1 {
                continue;
            }
            if result.fft_work >= o.max_fft_work || base >= o.max_bytes {
                return Err(MosaicError::limit("FFT work/scratch"));
            }
            let bound = o.max_correction_mm;
            let fft_options = MaskedFftOptions {
                min_overlap: o.min_overlap,
                min_overlap_fraction: o.min_overlap_fraction,
                ambiguity_tolerance: o.ambiguity_tolerance,
                search: Some(MaskedFftSearch {
                    min: Vec3::new(-bound, -bound, -bound),
                    max: Vec3::new(bound, bound, bound),
                }),
                max_input_voxels: o.max_samples * 2,
                max_bytes: o.max_bytes - base,
                max_fft_work: o.max_fft_work - result.fft_work,
                ..MaskedFftOptions::default()
            };
            let inputs = (
                Some(images[a]),
                Some(images[b]),
                Some(masks[a]),
                Some(masks[b]),
            );
            let (bytes, work, admission) =
                masked_fft_workspace(inputs.0, inputs.1, inputs.2, inputs.3, fft_options);
            if let Some(e) = admission {
                return Err(MosaicError {
                    kind: MosaicKind::Limit,
                    text: format!("{ERR_MOSAIC_LIMIT}: FFT admission: {e}"),
                    registration: Some(e),
                });
            }
            result.fft_work += work;
            result.scratch_bytes = result.scratch_bytes.max(base + bytes);
            let mut link = MosaicLink {
                a: a as i64,
                b: b as i64,
                ..MosaicLink::default()
            };
            match estimate_masked_fft_translation(
                ctx,
                inputs.0,
                inputs.1,
                inputs.2,
                inputs.3,
                fft_options,
            ) {
                Err(e) => {
                    if e.is(Sentinel::MaskedFftNoOverlap) || e.is(Sentinel::MaskedFftVariance) {
                        link.reason = e.to_string();
                    } else {
                        return Err(MosaicError::registration(e));
                    }
                }
                Ok(proposal) => {
                    link.registration = proposal;
                    let s = proposal.index_shift;
                    link.offset = result.direction.mul_vec(Vec3::new(
                        -(s[0] as f64) * result.spacing.x,
                        -(s[1] as f64) * result.spacing.y,
                        0.0,
                    ));
                    if proposal.ambiguous {
                        link.reason = "ambiguous correlation peaks".into();
                    } else if !proposal.usable {
                        link.reason = "unusable linear boundary".into();
                    } else if proposal.search_boundary {
                        link.reason = "search bound reached".into();
                    } else if proposal.score < o.min_score {
                        link.reason = "correlation below threshold".into();
                    } else {
                        let (gain, offset, fraction) =
                            illumination(&mut run, images[a], images[b], masks[a], masks[b], s)?;
                        link.gain = gain;
                        link.offset_intensity = offset;
                        link.offset_fraction = fraction;
                        if link.gain < 1.0 / o.max_gain_ratio
                            || link.gain > o.max_gain_ratio
                            || link.offset_fraction > o.max_offset_fraction
                        {
                            link.reason = "incompatible illumination".into();
                        } else {
                            link.accepted = true;
                        }
                    }
                }
            }
            result.links.push(link);
        }
    }
    solve(&mut run, &o, &mut result, anchor)?;
    result.work = run.work;
    result.seal = mosaic_seal(&result)?;
    result.id = format!("mosaic-v1:{}", hex(&result.seal));
    run.check()?;
    Ok(result)
}
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
fn solve(
    run: &mut MosaicRun<'_>,
    o: &MosaicOptions,
    m: &mut MosaicLayout,
    anchor: usize,
) -> Result<(), MosaicError> {
    let n = m.tiles.len();
    let mut seen = vec![false; n];
    let mut queue: Vec<usize> = Vec::with_capacity(n);
    let mut roots = vec![anchor];
    roots.extend((0..n).filter(|i| *i != anchor));
    let accepted = m.links.iter().filter(|e| e.accepted).count() as i64;
    let mut tree_edge = vec![false; m.links.len()];
    for root in roots {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        m.tiles[root].component = m.components;
        queue.clear();
        queue.push(root);
        let mut at = 0;
        while at < queue.len() {
            let a = queue[at];
            at += 1;
            for link_index in 0..m.links.len() {
                run.step(1)?;
                let e = &m.links[link_index];
                if !e.accepted {
                    continue;
                }
                let mut b = e.b as usize;
                let mut offset = e.offset;
                if e.b as usize == a {
                    b = e.a as usize;
                    offset = Vec3::default().sub(offset);
                } else if e.a as usize != a {
                    continue;
                }
                if !seen[b] {
                    tree_edge[link_index] = true;
                    seen[b] = true;
                    m.tiles[b].component = m.components;
                    m.tiles[b].origin = m.tiles[a].origin.add(offset);
                    queue.push(b);
                }
            }
        }
        m.components += 1;
    }
    m.cycle_rank = accepted - n as i64 + m.components;
    for i in 0..m.links.len() {
        if !m.links[i].accepted || tree_edge[i] {
            continue;
        }
        let (a, b, offset) = (
            m.links[i].a as usize,
            m.links[i].b as usize,
            m.links[i].offset,
        );
        let closure = length(m.tiles[b].origin.sub(m.tiles[a].origin).sub(offset));
        m.links[i].cycle_closure_measured = true;
        m.links[i].cycle_closure_mm = closure;
        m.max_cycle_closure_mm = fmath::max(m.max_cycle_closure_mm, closure);
    }
    if m.components != 1 {
        m.diagnostics
            .push("accepted links do not connect all tiles".into());
        return run.check();
    }
    let unknown = n - 1;
    let mut index = vec![-1i64; n];
    let mut at = 0i64;
    for (i, slot) in index.iter_mut().enumerate() {
        if i != anchor {
            *slot = at;
            at += 1;
        }
    }
    let mut l = vec![0.0f64; unknown * unknown];
    let mut b = vec![[0.0f64; 3]; unknown];
    for e in &m.links {
        run.step(1)?;
        if !e.accepted {
            continue;
        }
        let (a, c) = (index[e.a as usize], index[e.b as usize]);
        let v = [e.offset.x, e.offset.y, e.offset.z];
        if a >= 0 {
            let a = a as usize;
            l[a * unknown + a] += 1.0;
            for axis in 0..3 {
                b[a][axis] -= v[axis];
            }
        }
        if c >= 0 {
            let c = c as usize;
            l[c * unknown + c] += 1.0;
            for axis in 0..3 {
                b[c][axis] += v[axis];
            }
        }
        if a >= 0 && c >= 0 {
            let (a, c) = (a as usize, c as usize);
            l[a * unknown + c] -= 1.0;
            l[c * unknown + a] -= 1.0;
        }
    }
    for i in 0..unknown {
        for j in 0..=i {
            let mut s = l[i * unknown + j];
            for k in 0..j {
                run.step(1)?;
                s -= l[i * unknown + k] * l[j * unknown + k];
            }
            if i == j {
                if !finite_positive(s) {
                    return Err(MosaicError::admission("singular graph"));
                }
                l[i * unknown + j] = s.sqrt();
            } else {
                l[i * unknown + j] = s / l[j * unknown + j];
            }
        }
    }
    for i in 0..unknown {
        for j in 0..i {
            run.step(1)?;
            for axis in 0..3 {
                b[i][axis] -= l[i * unknown + j] * b[j][axis];
            }
        }
        for value in &mut b[i] {
            *value /= l[i * unknown + i];
        }
    }
    for i in (0..unknown).rev() {
        for j in i + 1..unknown {
            run.step(1)?;
            for axis in 0..3 {
                b[i][axis] -= l[j * unknown + i] * b[j][axis];
            }
        }
        for value in &mut b[i] {
            *value /= l[i * unknown + i];
        }
    }
    for i in 0..n {
        if i != anchor {
            let p = b[index[i] as usize];
            m.tiles[i].origin = m.anchor_origin.add(Vec3::new(p[0], p[1], p[2]));
        }
        let d = length(m.tiles[i].origin.sub(m.tiles[i].source_origin));
        if !d.is_finite() {
            return Err(MosaicError::admission("graph arithmetic"));
        }
        m.max_displacement_mm = fmath::max(m.max_displacement_mm, d);
    }
    for i in 0..m.links.len() {
        if !m.links[i].accepted {
            continue;
        }
        let (a, c, offset) = (
            m.links[i].a as usize,
            m.links[i].b as usize,
            m.links[i].offset,
        );
        let residual = length(m.tiles[c].origin.sub(m.tiles[a].origin).sub(offset));
        m.links[i].residual_mm = residual;
        m.links[i].residual_measured = true;
        m.max_residual_mm = fmath::max(m.max_residual_mm, residual);
    }
    if m.max_residual_mm > o.max_cycle_error_mm || m.max_cycle_closure_mm > o.max_cycle_error_mm {
        m.diagnostics
            .push("global residual or fundamental-cycle closure exceeds tolerance".into());
    }
    if m.max_displacement_mm > o.max_correction_mm {
        m.diagnostics
            .push("anchored displacement exceeds correction bound".into());
    }
    m.conclusive = m.diagnostics.is_empty();
    run.check()
}
struct Json {
    out: String,
    finite: bool,
}
impl Json {
    fn float(&mut self, v: f64) {
        if !v.is_finite() {
            self.finite = false;
        }
        self.out.push_str(&format_float(v));
    }
    fn key(&mut self, first: bool, name: &str) {
        if !first {
            self.out.push(',');
        }
        self.out.push('"');
        self.out.push_str(name);
        self.out.push_str("\":");
    }
    fn vec3(&mut self, v: Vec3) {
        self.out.push_str("{\"X\":");
        self.float(v.x);
        self.out.push_str(",\"Y\":");
        self.float(v.y);
        self.out.push_str(",\"Z\":");
        self.float(v.z);
        self.out.push('}');
    }
    fn ints(&mut self, values: &[i64]) {
        self.out.push('[');
        for (i, v) in values.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            self.out.push_str(&v.to_string());
        }
        self.out.push(']');
    }
    fn raw(&mut self, text: &str) {
        self.out.push_str(text);
    }
    fn boolean(&mut self, v: bool) {
        self.out.push_str(if v { "true" } else { "false" });
    }
}
fn mosaic_seal(m: &MosaicLayout) -> Result<[u8; 32], MosaicError> {
    let mut j = Json {
        out: String::new(),
        finite: true,
    };
    j.raw("{\"ID\":\"\",\"AnchorID\":");
    append_string(&mut j.out, &m.anchor_id);
    j.key(false, "Tiles");
    j.raw("[");
    for (i, t) in m.tiles.iter().enumerate() {
        if i > 0 {
            j.raw(",");
        }
        j.raw("{\"ID\":");
        append_string(&mut j.out, &t.id);
        j.raw(&format!(
            ",\"Grid\":{{\"X\":{},\"Y\":{}}},\"Size\":",
            t.grid.0, t.grid.1
        ));
        j.ints(&t.size);
        j.key(false, "SourceOrigin");
        j.vec3(t.source_origin);
        j.key(false, "Origin");
        j.vec3(t.origin);
        j.key(false, "SourceDigest");
        let digest: Vec<i64> = t.source_digest.iter().map(|b| i64::from(*b)).collect();
        j.ints(&digest);
        j.raw(&format!(",\"Component\":{}}}", t.component));
    }
    j.raw("]");
    j.key(false, "Links");
    if m.links.is_empty() {
        j.raw("null");
    } else {
        j.raw("[");
        for (i, e) in m.links.iter().enumerate() {
            if i > 0 {
                j.raw(",");
            }
            let r = &e.registration;
            j.raw(&format!(
                "{{\"A\":{},\"B\":{},\"Registration\":{{\"IndexShift\":",
                e.a, e.b
            ));
            j.ints(&r.index_shift);
            j.key(false, "TranslationLPS");
            j.vec3(r.translation_lps);
            j.key(false, "Score");
            j.float(r.score);
            j.raw(&format!(",\"Overlap\":{}", r.overlap));
            j.key(false, "OverlapFraction");
            j.float(r.overlap_fraction);
            j.key(false, "Ambiguous");
            j.boolean(r.ambiguous);
            j.key(false, "SearchBoundary");
            j.boolean(r.search_boundary);
            j.key(false, "LinearBoundary");
            j.boolean(r.linear_boundary);
            j.key(false, "Usable");
            j.boolean(r.usable);
            j.key(false, "PaddedSize");
            j.ints(&r.padded_size);
            j.raw(&format!(
                ",\"ScratchBytes\":{},\"FFTWork\":{}}}",
                r.scratch_bytes, r.fft_work
            ));
            j.key(false, "Offset");
            j.vec3(e.offset);
            j.key(false, "Accepted");
            j.boolean(e.accepted);
            j.key(false, "Reason");
            append_string(&mut j.out, &e.reason);
            for (name, v) in [
                ("Gain", e.gain),
                ("OffsetIntensity", e.offset_intensity),
                ("OffsetFraction", e.offset_fraction),
                ("ResidualMM", e.residual_mm),
                ("CycleClosureMM", e.cycle_closure_mm),
            ] {
                j.key(false, name);
                j.float(v);
            }
            j.key(false, "ResidualMeasured");
            j.boolean(e.residual_measured);
            j.key(false, "CycleClosureMeasured");
            j.boolean(e.cycle_closure_measured);
            j.raw("}");
        }
        j.raw("]");
    }
    j.key(false, "Conclusive");
    j.boolean(m.conclusive);
    j.raw(&format!(
        ",\"Components\":{},\"CycleRank\":{}",
        m.components, m.cycle_rank
    ));
    for (name, v) in [
        ("MaxResidualMM", m.max_residual_mm),
        ("MaxCycleClosureMM", m.max_cycle_closure_mm),
        ("MaxDisplacementMM", m.max_displacement_mm),
    ] {
        j.key(false, name);
        j.float(v);
    }
    j.key(false, "Diagnostics");
    if m.diagnostics.is_empty() {
        j.raw("null");
    } else {
        j.raw("[");
        for (i, d) in m.diagnostics.iter().enumerate() {
            if i > 0 {
                j.raw(",");
            }
            append_string(&mut j.out, d);
        }
        j.raw("]");
    }
    j.key(false, "Spacing");
    j.vec3(m.spacing);
    j.key(false, "Direction");
    j.raw("[");
    for (i, row) in m.direction.0.iter().enumerate() {
        if i > 0 {
            j.raw(",");
        }
        j.raw("[");
        for (k, v) in row.iter().enumerate() {
            if k > 0 {
                j.raw(",");
            }
            j.float(*v);
        }
        j.raw("]");
    }
    j.raw("]");
    j.key(false, "AnchorOrigin");
    j.vec3(m.anchor_origin);
    j.raw(&format!(
        ",\"ScratchBytes\":{},\"FFTWork\":{},\"Work\":{}}}",
        m.scratch_bytes, m.fft_work, m.work
    ));
    if !j.finite {
        return Err(MosaicError::admission("derivation metadata"));
    }
    Ok(Sha256::digest(j.out.as_bytes()).into())
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MosaicRegionOptions {
    pub region: Rect,
    pub output_size: (i64, i64),
    pub background: f32,
    pub max_pixels: u64,
    pub max_input_samples: u64,
    pub max_bytes: u64,
    pub max_work: u64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MosaicRegion {
    pub layout_id: String,
    pub region: Rect,
    pub output_size: (i64, i64),
    pub origin: Vec3,
    pub spacing: Vec3,
    pub pixels: Vec<f32>,
    pub coverage: Vec<u16>,
    pub reserved_bytes: u64,
    pub work: u64,
}
fn check_seal(m: &MosaicLayout) -> Result<(), MosaicError> {
    if m.tiles.len() < 2
        || m.tiles.len() > 1024
        || m.links.len() > 2 * m.tiles.len()
        || m.diagnostics.len() > m.tiles.len()
        || m.id.len() > 80
        || m.anchor_id.len() > 256
    {
        return Err(MosaicError::admission("layout metadata"));
    }
    if m.tiles.iter().any(|t| t.id.len() > 256) {
        return Err(MosaicError::admission("tile metadata"));
    }
    if m.links.iter().any(|e| e.reason.len() > 512) {
        return Err(MosaicError::admission("link metadata"));
    }
    if m.diagnostics.iter().any(|d| d.len() > 512) {
        return Err(MosaicError::admission("diagnostics"));
    }
    let s = mosaic_seal(m)?;
    if s != m.seal || m.id != format!("mosaic-v1:{}", hex(&s)) {
        return Err(MosaicError::admission("modified/unbound layout"));
    }
    Ok(())
}
fn mosaic_index(m: &MosaicLayout, p: Vec3) -> (f64, f64) {
    let d = p.sub(m.anchor_origin);
    let a = &m.direction.0;
    (
        (a[0][0] * d.x + a[1][0] * d.y + a[2][0] * d.z) / m.spacing.x,
        (a[0][1] * d.x + a[1][1] * d.y + a[2][1] * d.z) / m.spacing.y,
    )
}
pub fn mosaic_bounds(m: &MosaicLayout) -> Result<Rect, MosaicError> {
    check_seal(m)?;
    let (mut lo_x, mut lo_y) = (f64::INFINITY, f64::INFINITY);
    let (mut hi_x, mut hi_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for t in &m.tiles {
        let (x, y) = mosaic_index(m, t.origin);
        lo_x = fmath::min(lo_x, x.floor());
        lo_y = fmath::min(lo_y, y.floor());
        hi_x = fmath::max(hi_x, (x + t.size[0] as f64).ceil());
        hi_y = fmath::max(hi_y, (y + t.size[1] as f64).ceil());
    }
    for v in [lo_x, lo_y, hi_x, hi_y] {
        if !v.is_finite() || v.abs() > f64::from(i32::MAX) {
            return Err(MosaicError::admission("canvas index capacity"));
        }
    }
    if hi_x - lo_x > i64::MAX as f64 || hi_y - lo_y > i64::MAX as f64 {
        return Err(MosaicError::limit("native canvas dimensions"));
    }
    Ok(Rect::new(
        lo_x as i64,
        lo_y as i64,
        hi_x as i64,
        hi_y as i64,
    ))
}
fn mosaic_sample(t: &MosaicTile<'_>, image: &Image, x: f64, y: f64) -> Option<f64> {
    let s = size_i64(image);
    if x < -0.5 || y < -0.5 || x >= s[0] as f64 - 0.5 || y >= s[1] as f64 - 0.5 {
        return None;
    }
    let x = fmath::max(0.0, fmath::min((s[0] - 1) as f64, x));
    let y = fmath::max(0.0, fmath::min((s[1] - 1) as f64, y));
    let (x0, y0) = (x.floor() as i64, y.floor() as i64);
    let (x1, y1) = ((x0 + 1).min(s[0] - 1), (y0 + 1).min(s[1] - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let mut sum = 0.0f64;
    for (px, py, w) in [
        (x0, y0, (1.0 - fx) * (1.0 - fy)),
        (x1, y0, fx * (1.0 - fy)),
        (x0, y1, (1.0 - fx) * fy),
        (x1, y1, fx * fy),
    ] {
        if w == 0.0 {
            continue;
        }
        if let Some(mask) = t.mask
            && mask.at(px as usize, py as usize, 0) <= 0.0
        {
            return None;
        }
        sum += w * f64::from(image.at(px as usize, py as usize, 0));
    }
    Some(sum)
}
pub fn compose_mosaic_region(
    ctx: Context<'_>,
    tiles: &[MosaicTile<'_>],
    m: &MosaicLayout,
    options: &MosaicRegionOptions,
) -> Result<MosaicRegion, MosaicError> {
    ctx.err().map_err(|_| MosaicError::canceled())?;
    let mut o = *options;
    if o.max_pixels == 0 {
        o.max_pixels = 4 << 20;
    }
    if o.max_input_samples == 0 {
        o.max_input_samples = 8 << 20;
    }
    if o.max_bytes == 0 {
        o.max_bytes = 64 << 20;
    }
    if o.max_work == 0 {
        o.max_work = 64 << 20;
    }
    if !o.background.is_finite() || o.region.is_empty() || o.max_input_samples > i32::MAX as u64 {
        return Err(MosaicError::admission("region options"));
    }
    let bound = i64::from(i32::MAX);
    for v in [
        o.region.min_x,
        o.region.min_y,
        o.region.max_x,
        o.region.max_y,
    ] {
        if !(-bound..=bound).contains(&v) {
            return Err(MosaicError::admission("region index capacity"));
        }
    }
    if o.output_size == (0, 0) {
        o.output_size = (o.region.dx(), o.region.dy());
    }
    if o.output_size.0 < 1
        || o.output_size.1 < 1
        || o.output_size.0 > bound
        || o.output_size.1 > bound
    {
        return Err(MosaicError::admission("output size"));
    }
    let (w, h) = (o.output_size.0 as u64, o.output_size.1 as u64);
    if w > o.max_pixels / h {
        return Err(MosaicError::limit("region pixels"));
    }
    let pixels = w * h;
    if tiles.len() != m.tiles.len() || tiles.len() > 1024 {
        return Err(MosaicError::admission("source count"));
    }
    let metadata = tiles.len() as u64 * 8192 + 512;
    if metadata > o.max_bytes || pixels > (o.max_bytes - metadata) / 14 || pixels > i64::MAX as u64
    {
        return Err(MosaicError::limit("regional payload/scratch"));
    }
    check_seal(m)?;
    if !m.conclusive {
        return Err(MosaicError {
            kind: MosaicKind::Inconclusive,
            text: ERR_MOSAIC_INCONCLUSIVE.to_string(),
            registration: None,
        });
    }
    let mut run = MosaicRun {
        ctx,
        max_work: o.max_work,
        work: 0,
    };
    let mut samples = 0u64;
    let mut images: Vec<&Image> = Vec::with_capacity(tiles.len());
    for (i, t) in tiles.iter().enumerate() {
        let placement = &m.tiles[i];
        let Some(image) = t.image.filter(|image| {
            t.id == placement.id
                && t.grid == placement.grid
                && size_i64(image) == placement.size
                && image.origin() == placement.source_origin
                && image.spacing() == m.spacing
                && image.direction() == m.direction
        }) else {
            return Err(MosaicError::admission("changed source geometry/identity"));
        };
        let n = image.data().len() as u64;
        if n > o.max_input_samples - samples {
            return Err(MosaicError::limit("composition input samples"));
        }
        samples += n;
        if let Some(mask) = t.mask
            && (mask.size() != image.size()
                || mask.origin() != image.origin()
                || mask.spacing() != m.spacing
                || mask.direction() != m.direction
                || mask.data().len() != image.data().len())
        {
            return Err(MosaicError::admission("changed mask geometry"));
        }
        let digest = source_digest(&mut run, t)?;
        if digest != placement.source_digest {
            return Err(MosaicError::admission("changed source pixels/mask"));
        }
        images.push(image);
    }
    let (sx, sy) = (
        o.region.dx() as f64 / w as f64,
        o.region.dy() as f64 / h as f64,
    );
    let mut out = MosaicRegion {
        layout_id: m.id.clone(),
        region: o.region,
        output_size: o.output_size,
        spacing: Vec3::new(m.spacing.x * sx, m.spacing.y * sy, m.spacing.z),
        pixels: vec![0.0; pixels as usize],
        coverage: vec![0; pixels as usize],
        reserved_bytes: metadata + 14 * pixels,
        ..MosaicRegion::default()
    };
    out.origin = m.anchor_origin.add(m.direction.mul_vec(Vec3::new(
        (o.region.min_x as f64 + (sx - 1.0) / 2.0) * m.spacing.x,
        (o.region.min_y as f64 + (sy - 1.0) / 2.0) * m.spacing.y,
        0.0,
    )));
    let mut sums = vec![0.0f64; pixels as usize];
    for (tile, t) in tiles.iter().enumerate() {
        let (ox, oy) = mosaic_index(m, m.tiles[tile].origin);
        for y in 0..o.output_size.1 {
            for x in 0..o.output_size.0 {
                run.step(1)?;
                let px = o.region.min_x as f64 + (x as f64 + 0.5) * sx - 0.5;
                let py = o.region.min_y as f64 + (y as f64 + 0.5) * sy - 0.5;
                let Some(value) = mosaic_sample(t, images[tile], px - ox, py - oy) else {
                    continue;
                };
                let id = (y * w as i64 + x) as usize;
                sums[id] += value;
                out.coverage[id] = out.coverage[id].wrapping_add(1);
            }
        }
    }
    for i in 0..out.pixels.len() {
        run.step(1)?;
        if out.coverage[i] == 0 {
            out.pixels[i] = o.background;
        } else {
            let value = (sums[i] / f64::from(out.coverage[i])) as f32;
            if !value.is_finite() {
                return Err(MosaicError::admission("composite overflow"));
            }
            out.pixels[i] = value;
        }
    }
    run.check()?;
    out.work = run.work;
    Ok(out)
}
