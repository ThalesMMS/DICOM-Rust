use super::{
    Code, Context, Error, ImageReference, Kind, Limits, Measurement, MeasurementValues, Result,
    Results, Track, TrackSet, TrackSetStatistic, TrackStatistic, check, validate,
};
use crate::fmath;
use crate::mesh::Mesh;
use std::collections::{HashMap, HashSet};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TrackId {
    pub set_number: u32,
    pub track_index: i64,
}
pub type RegionKind = u8;
pub const SPHERE: RegionKind = 0;
pub const BOX: RegionKind = 1;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Region {
    pub kind: RegionKind,
    pub center: [f64; 3],
    pub radius_mm: f64,
    pub min: [f64; 3],
    pub max: [f64; 3],
}
#[derive(Clone, Debug, Default)]
pub struct SelectionOptions {
    pub include: Vec<Region>,
    pub exclude: Vec<Region>,
    pub require_all_includes: bool,
}
#[derive(Clone, Debug, Default)]
pub struct TrackAnalysis {
    pub id: TrackId,
    pub length_mm: f64,
    pub measurements: Vec<Measurement>,
    pub statistics: Vec<TrackStatistic>,
}
#[derive(Clone, Debug, Default)]
pub struct SetAnalysis {
    pub set_number: u32,
    pub statistics: Vec<TrackSetStatistic>,
}
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub tracks: Vec<TrackAnalysis>,
    pub total_length_mm: f64,
    pub references: Vec<ImageReference>,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub sets: Vec<SetAnalysis>,
}
pub type Style = u8;
pub const LINE: Style = 0;
pub const TUBE: Style = 1;
pub const RIBBON: Style = 2;
pub type ColorMode = u8;
pub const DIRECTION: ColorMode = 0;
pub const SCALAR: ColorMode = 1;
#[derive(Clone, Debug, Default)]
pub struct ScalarMapping {
    pub measurement_index: i64,
    pub quantity: Code,
    pub units: Code,
    pub min: f64,
    pub max: f64,
}
#[derive(Clone, Debug, Default)]
pub struct DisplayOptions {
    pub style: Style,
    pub radius_mm: f64,
    pub tube_sides: i64,
    pub point_stride: i64,
    pub max_tracks: i64,
    pub max_points: i64,
    pub max_triangles: i64,
    pub max_bytes: i64,
    pub color: ColorMode,
    pub scalar: Option<ScalarMapping>,
}
#[derive(Clone, Debug, Default)]
pub struct Polyline {
    pub owner: TrackId,
    pub points: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 3]>,
    pub source_point_indices: Vec<i64>,
}
#[derive(Clone, Debug, Default)]
pub struct Surface {
    pub owner: TrackId,
    pub mesh: Mesh,
    pub colors: Vec<[f32; 3]>,
    pub source_point_indices: Vec<i64>,
}
#[derive(Clone, Debug, Default)]
pub struct DisplayStats {
    pub input_tracks: i64,
    pub displayed_tracks: i64,
    pub input_points: i64,
    pub displayed_points: i64,
    pub vertices: i64,
    pub triangles: i64,
    pub bytes: i64,
    pub degenerate_tracks: Vec<TrackId>,
    pub lod: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Display {
    pub lines: Vec<Polyline>,
    pub surfaces: Vec<Surface>,
    pub stats: DisplayStats,
}
fn options() -> Error {
    Error::sentinel(Kind::GeomInvalidOptions)
}
fn data(detail: &str) -> Error {
    Error::new(Kind::GeomInvalidData, detail)
}
type V3 = [f64; 3];
fn finite(v: f64) -> bool {
    !v.is_nan() && !v.is_infinite()
}
fn vec(p: [f32; 3]) -> V3 {
    [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, b: f64) -> V3 {
    [a[0] * b, a[1] * b, a[2] * b]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> f64 {
    fmath::hypot(fmath::hypot(a[0], a[1]), a[2])
}
fn unit(a: V3) -> V3 {
    let n = norm(a);
    if n == 0.0 {
        return [0.0; 3];
    }
    mul(a, 1.0 / n)
}
fn operation<'a>(ctx: &Context, r: Option<&'a Results>) -> Result<&'a Results> {
    validate(ctx, r, Limits::default())?;
    let Some(r) = r else {
        return Err(super::invalid("nil Results"));
    };
    for s in &r.track_sets {
        for t in &s.tracks {
            for (i, p) in t.points.iter().enumerate() {
                if i & 255 == 0 {
                    check(ctx)?;
                }
                if p.iter().any(|v| !finite(f64::from(*v))) {
                    return Err(data("nonfinite point"));
                }
            }
        }
    }
    check(ctx)?;
    Ok(r)
}
struct Selected<'a> {
    id: TrackId,
    set: &'a TrackSet,
    track: &'a Track,
}
fn resolve<'a>(ctx: &Context, r: &'a Results, ids: &[TrackId]) -> Result<Vec<Selected<'a>>> {
    let mut sets: HashMap<u32, &TrackSet> = HashMap::with_capacity(r.track_sets.len());
    for s in &r.track_sets {
        sets.insert(s.number, s);
    }
    let mut seen = HashSet::with_capacity(ids.len());
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        check(ctx)?;
        let track = sets.get(&id.set_number).and_then(|s| {
            usize::try_from(id.track_index)
                .ok()
                .and_then(|i| s.tracks.get(i))
                .map(|t| (*s, t))
        });
        let Some((set, track)) = track.filter(|_| !seen.contains(id)) else {
            return Err(Error::new(
                Kind::GeomInvalidOptions,
                format!(
                    "missing or duplicate track {{{} {}}}",
                    id.set_number, id.track_index
                ),
            ));
        };
        seen.insert(*id);
        out.push(Selected {
            id: *id,
            set,
            track,
        });
    }
    Ok(out)
}
pub fn analyze(ctx: &Context, r: Option<&Results>, ids: &[TrackId]) -> Result<Analysis> {
    let r = operation(ctx, r)?;
    let tracks = resolve(ctx, r, ids)?;
    let mut a = Analysis {
        sop_instance_uid: r.sop_instance_uid.clone(),
        study_instance_uid: r.study_instance_uid.clone(),
        series_instance_uid: r.series_instance_uid.clone(),
        frame_of_reference_uid: r.frame_of_reference_uid.clone(),
        ..Analysis::default()
    };
    let mut seen_sets = HashSet::new();
    for s in &tracks {
        if seen_sets.insert(s.id.set_number) {
            a.sets.push(SetAnalysis {
                set_number: s.id.set_number,
                statistics: s.set.statistics.clone().unwrap_or_default(),
            });
        }
        let index = s.id.track_index as usize;
        let mut t = TrackAnalysis {
            id: s.id,
            ..TrackAnalysis::default()
        };
        for i in 1..s.track.points.len() {
            if i & 255 == 0 {
                check(ctx)?;
            }
            t.length_mm += norm(sub(vec(s.track.points[i]), vec(s.track.points[i - 1])));
        }
        for m in s.set.measurements.iter().flatten() {
            let v = &m.tracks[index];
            t.measurements.push(Measurement {
                quantity: m.quantity.clone(),
                units: m.units.clone(),
                tracks: vec![MeasurementValues {
                    values: v.values.clone(),
                    point_indices: v.point_indices.clone(),
                }],
            });
        }
        for m in s.set.track_statistics.iter().flatten() {
            t.statistics.push(TrackStatistic {
                quantity: m.quantity.clone(),
                modifier: m.modifier.clone(),
                units: m.units.clone(),
                values: vec![m.values[index]],
            });
        }
        a.total_length_mm += t.length_mm;
        a.tracks.push(t);
    }
    a.references = r.references.clone();
    check(ctx)?;
    Ok(a)
}
pub fn select(ctx: &Context, r: Option<&Results>, o: &SelectionOptions) -> Result<Vec<TrackId>> {
    if o.include.len() + o.exclude.len() > 1024 {
        return Err(Error::sentinel(Kind::GeomLimitExceeded));
    }
    if !o.include.iter().chain(&o.exclude).all(valid_region) {
        return Err(Error::new(Kind::GeomInvalidOptions, "region"));
    }
    let r = operation(ctx, r)?;
    let mut out = Vec::new();
    for s in &r.track_sets {
        for (ti, t) in s.tracks.iter().enumerate() {
            let mut include = o.include.is_empty() || o.require_all_includes;
            for q in &o.include {
                let hit = intersects(ctx, &t.points, q)?;
                if o.require_all_includes {
                    include = include && hit;
                    if !include {
                        break;
                    }
                } else {
                    include = include || hit;
                    if include {
                        break;
                    }
                }
            }
            if !include {
                continue;
            }
            let mut excluded = false;
            for q in &o.exclude {
                if intersects(ctx, &t.points, q)? {
                    excluded = true;
                    break;
                }
            }
            if !excluded {
                out.push(TrackId {
                    set_number: s.number,
                    track_index: ti as i64,
                });
            }
        }
    }
    check(ctx)?;
    Ok(out)
}
fn valid_region(q: &Region) -> bool {
    if [q.center, q.min, q.max]
        .iter()
        .flatten()
        .any(|v| !finite(*v))
        || !finite(q.radius_mm)
    {
        return false;
    }
    match q.kind {
        SPHERE => q.radius_mm > 0.0 && q.min == [0.0; 3] && q.max == [0.0; 3],
        BOX => q.center == [0.0; 3] && q.radius_mm == 0.0 && (0..3).all(|i| q.min[i] <= q.max[i]),
        _ => false,
    }
}
fn intersects(ctx: &Context, points: &[[f32; 3]], q: &Region) -> Result<bool> {
    for i in 1..points.len() {
        if i & 255 == 0 {
            check(ctx)?;
        }
        if segment_hit(vec(points[i - 1]), vec(points[i]), q) {
            return Ok(true);
        }
    }
    check(ctx)?;
    Ok(false)
}
fn segment_hit(a: V3, b: V3, q: &Region) -> bool {
    let d = sub(b, a);
    if q.kind == SPHERE {
        let mut t = 0.0;
        let dd = dot(d, d);
        if dd > 0.0 {
            t = fmath::max(0.0, fmath::min(1.0, dot(sub(q.center, a), d) / dd));
        }
        return norm(sub(add(a, mul(d, t)), q.center)) <= q.radius_mm;
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for k in 0..3 {
        if d[k] == 0.0 {
            if a[k] < q.min[k] || a[k] > q.max[k] {
                return false;
            }
            continue;
        }
        let (mut x, mut y) = ((q.min[k] - a[k]) / d[k], (q.max[k] - a[k]) / d[k]);
        if x > y {
            std::mem::swap(&mut x, &mut y);
        }
        lo = fmath::max(lo, x);
        hi = fmath::min(hi, y);
        if lo > hi {
            return false;
        }
    }
    true
}
fn display_options(mut o: DisplayOptions) -> Result<DisplayOptions> {
    if o.style > RIBBON
        || o.color > SCALAR
        || o.point_stride < 0
        || !finite(o.radius_mm)
        || o.radius_mm < 0.0
    {
        return Err(options());
    }
    if o.radius_mm == 0.0 {
        o.radius_mm = 0.5;
    }
    if o.tube_sides == 0 {
        o.tube_sides = 6;
    }
    if o.tube_sides < 3 || o.tube_sides > 32 {
        return Err(options());
    }
    for (v, max) in [
        (&mut o.max_tracks, 100_000),
        (&mut o.max_points, 1_000_000),
        (&mut o.max_triangles, 2_000_000),
        (&mut o.max_bytes, 256 << 20),
    ] {
        if *v < 0 || *v > max {
            return Err(options());
        }
        if *v == 0 {
            *v = max;
        }
    }
    if o.point_stride == 0 {
        o.point_stride = 1;
    }
    if o.color == SCALAR {
        match &o.scalar {
            Some(s)
                if s.measurement_index >= 0
                    && !s.quantity.value.is_empty()
                    && !s.quantity.scheme.is_empty()
                    && !s.units.value.is_empty()
                    && s.units.scheme == "UCUM"
                    && finite(s.min)
                    && finite(s.max)
                    && finite(s.max - s.min)
                    && s.min < s.max => {}
            _ => return Err(options()),
        }
    } else if o.scalar.is_some() {
        return Err(options());
    }
    Ok(o)
}
struct Prepared<'a> {
    selected: Selected<'a>,
    indices: Vec<usize>,
    sampled: Vec<usize>,
    values: Vec<f32>,
    closed: bool,
    target: usize,
}
fn uniform_index(i: usize, n: usize, total: usize) -> usize {
    if n == 1 {
        return 0;
    }
    ((i as i64) * (total as i64 - 1) / (n as i64 - 1)) as usize
}
impl Prepared<'_> {
    fn point(&self, k: usize) -> [f32; 3] {
        self.selected.track.points[self.indices[k]]
    }
    fn resample(&mut self, ctx: &Context) -> Result<()> {
        check(ctx)?;
        let mut sampled: Vec<usize> = Vec::with_capacity(self.target);
        for i in 0..self.target {
            if i & 255 == 0 {
                check(ctx)?;
            }
            let k = uniform_index(i, self.target, self.indices.len());
            if let Some(&last) = sampled.last()
                && self.point(k) == self.point(last)
            {
                if i == self.target - 1 && sampled.len() > 1 {
                    let at = sampled.len() - 1;
                    sampled[at] = k;
                }
                continue;
            }
            sampled.push(k);
        }
        if sampled.len() == 1 {
            sampled = vec![0, 1, self.indices.len() - 1];
        }
        self.sampled = sampled;
        check(ctx)
    }
    fn minimum_points(&self) -> usize {
        if self.closed {
            return self.indices.len().min(4);
        }
        2
    }
}
fn planned(ps: &[Prepared<'_>], o: &DisplayOptions) -> (i64, i64, i64, i64) {
    let (mut points, mut vertices, mut triangles, mut bytes) = (0i64, 0i64, 0i64, 0i64);
    for p in ps {
        let n = p.sampled.len() as i64;
        points += n;
        let (mut v, mut t) = (n, 0i64);
        if o.style == TUBE {
            v = n * o.tube_sides;
            t = (n - 1) * o.tube_sides * 2;
        }
        if o.style == RIBBON {
            v = n * 2;
            t = (n - 1) * 2;
        }
        vertices += v;
        triangles += t;
        let mut per_vertex = 24 + 8;
        if o.style != LINE {
            per_vertex += 12;
        }
        bytes += v * per_vertex + t * 12;
    }
    (points, vertices, triangles, bytes)
}
fn scalar_values(ctx: &Context, s: &Selected<'_>, m: &ScalarMapping) -> Result<Vec<f32>> {
    let measurements = s.set.measurements.as_deref().unwrap_or_default();
    let Some(measurement) = measurements.get(m.measurement_index as usize) else {
        return Err(data("missing measurement"));
    };
    if !measurement.quantity.same_code(&m.quantity) || !measurement.units.same_code(&m.units) {
        return Err(data("measurement quantity/units mismatch"));
    }
    let v = &measurement.tracks[s.id.track_index as usize];
    if v.values.len() != s.track.points.len() {
        return Err(data("sparse scalar measurement"));
    }
    let mut out = vec![0f32; v.values.len()];
    let mut seen = vec![false; out.len()];
    for (i, &value) in v.values.iter().enumerate() {
        if i & 255 == 0 {
            check(ctx)?;
        }
        if !finite(f64::from(value)) {
            return Err(data("nonfinite scalar"));
        }
        let ix = match &v.point_indices {
            Some(indices) => indices[i] as usize - 1,
            None => i,
        };
        if seen[ix] {
            return Err(data("repeated scalar index"));
        }
        seen[ix] = true;
        out[ix] = value;
    }
    Ok(out)
}
fn tangent(points: &[[f32; 3]], indices: &[usize], i: usize, closed: bool) -> V3 {
    let last = indices.len() - 1;
    let (a, b) = if closed && (i == 0 || i == last) {
        (indices.len() - 2, 1)
    } else {
        (i.saturating_sub(1), (i + 1).min(last))
    };
    let mut v = sub(vec(points[indices[b]]), vec(points[indices[a]]));
    if norm(v) == 0.0 {
        v = if i + 1 < indices.len() {
            sub(vec(points[indices[i + 1]]), vec(points[indices[i]]))
        } else {
            sub(vec(points[indices[i]]), vec(points[indices[i - 1]]))
        };
    }
    unit(v)
}
fn build_polyline(ctx: &Context, p: &Prepared<'_>, o: &DisplayOptions) -> Result<Polyline> {
    check(ctx)?;
    let n = p.sampled.len();
    let mut line = Polyline {
        owner: p.selected.id,
        points: Vec::with_capacity(n),
        colors: Vec::with_capacity(n),
        source_point_indices: Vec::with_capacity(n),
    };
    let points = &p.selected.track.points;
    for (j, &k) in p.sampled.iter().enumerate() {
        if j & 255 == 0 {
            check(ctx)?;
        }
        let ix = p.indices[k];
        line.points.push(points[ix]);
        line.source_point_indices.push(ix as i64);
        match (&o.scalar, o.color) {
            (Some(scalar), SCALAR) => {
                let v = fmath::max(
                    0.0,
                    fmath::min(
                        1.0,
                        (f64::from(p.values[ix]) - scalar.min) / (scalar.max - scalar.min),
                    ),
                );
                line.colors.push([v as f32, 0.0, (1.0 - v) as f32]);
            }
            _ => {
                let v = tangent(points, &p.indices, k, p.closed);
                line.colors
                    .push([v[0].abs() as f32, v[1].abs() as f32, v[2].abs() as f32]);
            }
        }
    }
    check(ctx)?;
    Ok(line)
}
fn tessellate(ctx: &Context, line: &Polyline, closed: bool, o: &DisplayOptions) -> Result<Surface> {
    let ribbon = o.style == RIBBON;
    let sides = if ribbon { 2 } else { o.tube_sides as usize };
    let n = line.points.len();
    let mut vertices = Vec::with_capacity(n * sides);
    let mut normals = Vec::with_capacity(n * sides);
    let mut colors = Vec::with_capacity(n * sides);
    let mut source = Vec::with_capacity(n * sides);
    let indices: Vec<usize> = (0..n).collect();
    let mut normal = [0.0; 3];
    let mut first_normal = [0.0; 3];
    for (i, &p) in line.points.iter().enumerate() {
        if i & 255 == 0 {
            check(ctx)?;
        }
        let t = tangent(&line.points, &indices, i, closed);
        let mut projected = sub(normal, mul(t, dot(normal, t)));
        if norm(projected) < 1e-10 {
            let axis = if t[0].abs() > 0.8 {
                [0.0, 1.0, 0.0]
            } else {
                [1.0, 0.0, 0.0]
            };
            projected = cross(t, axis);
        }
        normal = unit(projected);
        if i == 0 {
            first_normal = normal;
        }
        if closed && i == n - 1 {
            normal = first_normal;
        }
        let binormal = unit(cross(t, normal));
        for side in 0..sides {
            let mut offset = normal;
            if ribbon {
                if side == 1 {
                    offset = mul(normal, -1.0);
                }
            } else {
                let angle = std::f64::consts::TAU * side as f64 / sides as f64;
                offset = add(
                    mul(normal, fmath::cos(angle)),
                    mul(binormal, fmath::sin(angle)),
                );
            }
            let position = add(vec(p), mul(offset, o.radius_mm));
            let v = [position[0] as f32, position[1] as f32, position[2] as f32];
            if v.iter().any(|x| !finite(f64::from(*x))) {
                return Err(data("output coordinate overflow"));
            }
            let nv = if ribbon { binormal } else { offset };
            vertices.push(v);
            normals.push([nv[0] as f32, nv[1] as f32, nv[2] as f32]);
            colors.push(line.colors[i]);
            source.push(line.source_point_indices[i]);
        }
    }
    let mut triangles = Vec::new();
    for i in 0..n.saturating_sub(1) {
        if i & 255 == 0 {
            check(ctx)?;
        }
        let segments = if ribbon { 1 } else { sides };
        for side in 0..segments {
            let a = (i * sides + side) as u32;
            let b = (i * sides + (side + 1) % sides) as u32;
            let (c, d) = (a + sides as u32, b + sides as u32);
            triangles.push([a, b, c]);
            triangles.push([b, d, c]);
        }
    }
    Ok(Surface {
        owner: line.owner,
        mesh: Mesh {
            vertices,
            normals: Some(normals),
            triangles,
        },
        colors,
        source_point_indices: source,
    })
}
pub fn build_display(
    ctx: &Context,
    r: Option<&Results>,
    ids: &[TrackId],
    o: &DisplayOptions,
) -> Result<Display> {
    let o = display_options(o.clone())?;
    let r = operation(ctx, r)?;
    let selected = resolve(ctx, r, ids)?;
    let mut d = Display::default();
    d.stats.input_tracks = selected.len() as i64;
    let mut ready: Vec<Prepared<'_>> = Vec::with_capacity(selected.len());
    for s in selected {
        check(ctx)?;
        d.stats.input_points += s.track.points.len() as i64;
        let values = match (&o.scalar, o.color) {
            (Some(scalar), SCALAR) => scalar_values(ctx, &s, scalar)?,
            _ => Vec::new(),
        };
        let points = &s.track.points;
        let mut indices = Vec::new();
        for (i, v) in points.iter().enumerate() {
            if i & 255 == 0 {
                check(ctx)?;
            }
            if i == 0 || *v != points[i - 1] {
                indices.push(i);
            }
        }
        if indices.len() < 2 {
            d.stats.degenerate_tracks.push(s.id);
            continue;
        }
        let closed = indices.len() >= 3 && points[indices[0]] == points[indices[indices.len() - 1]];
        ready.push(Prepared {
            selected: s,
            indices,
            sampled: Vec::new(),
            values,
            closed,
            target: 0,
        });
    }
    let max_tracks = o.max_tracks as usize;
    if ready.len() > max_tracks {
        let total = ready.len();
        let picks: Vec<usize> = (0..max_tracks)
            .map(|i| uniform_index(i, max_tracks, total))
            .collect();
        let mut slots: Vec<Option<Prepared<'_>>> = ready.into_iter().map(Some).collect();
        let mut sample = Vec::with_capacity(max_tracks);
        for (i, &pick) in picks.iter().enumerate() {
            if i & 255 == 0 {
                check(ctx)?;
            }
            let last_use = picks[i + 1..].iter().all(|&next| next != pick);
            let taken = if last_use {
                slots[pick].take()
            } else {
                slots[pick].as_ref().map(|p| Prepared {
                    selected: Selected {
                        id: p.selected.id,
                        set: p.selected.set,
                        track: p.selected.track,
                    },
                    indices: p.indices.clone(),
                    sampled: p.sampled.clone(),
                    values: p.values.clone(),
                    closed: p.closed,
                    target: p.target,
                })
            };
            if let Some(p) = taken {
                sample.push(p);
            }
        }
        ready = sample;
        d.stats.lod = true;
    }
    for p in ready.iter_mut() {
        check(ctx)?;
        let n = p.indices.len();
        let target = 2 + (n - 2) / o.point_stride as usize;
        p.target = target.max(p.minimum_points()).min(n);
        p.resample(ctx)?;
    }
    loop {
        check(ctx)?;
        let (points, vertices, triangles, bytes) = planned(&ready, &o);
        if points <= o.max_points && triangles <= o.max_triangles && bytes <= o.max_bytes {
            d.stats.displayed_points = points;
            d.stats.vertices = vertices;
            d.stats.triangles = triangles;
            d.stats.bytes = bytes;
            break;
        }
        let mut changed = false;
        for (i, p) in ready.iter_mut().enumerate() {
            if i & 255 == 0 {
                check(ctx)?;
            }
            let minimum = p.minimum_points();
            if p.target > minimum {
                p.target = minimum + (p.target - minimum) / 2;
                p.resample(ctx)?;
                changed = true;
            }
        }
        if !changed {
            return Err(Error::new(
                Kind::GeomLimitExceeded,
                "minimum selected display",
            ));
        }
        d.stats.lod = true;
    }
    for p in &ready {
        check(ctx)?;
        if p.target < p.indices.len() {
            d.stats.lod = true;
        }
        let line = build_polyline(ctx, p, &o)?;
        if o.style == LINE {
            d.lines.push(line);
        } else {
            d.surfaces.push(tessellate(ctx, &line, p.closed, &o)?);
        }
    }
    d.stats.displayed_tracks = ready.len() as i64;
    check(ctx)?;
    Ok(d)
}
