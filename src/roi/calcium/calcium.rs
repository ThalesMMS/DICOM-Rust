#![allow(clippy::neg_cmp_op_on_partial_ord)]
use crate::codes::{Code, sct};
use crate::fmath;
use crate::numtext::format_float;
use crate::roi::{Cancellation, Error, ctx_err};
use crate::textfmt::{float_v, quote};
use std::borrow::Cow;
pub const DEFAULT_THRESHOLD_HU: f64 = 130.0;
pub const DEFAULT_MIN_LESION_AREA_MM2: f64 = 1.0;
pub const DEFAULT_REFERENCE_THICKNESS_MM: f64 = 3.0;
pub const ERR_INVALID_GEOMETRY: &str = "calcium: invalid geometry";
pub const ERR_IRREGULAR_SPACING: &str = "calcium: slices are not evenly spaced";
pub const ERR_INVALID_OPTIONS: &str = "calcium: invalid options";
pub const ERR_UNKNOWN_LESION: &str = "calcium: unknown lesion";
pub const ERR_UNKNOWN_VESSEL: &str = "calcium: unknown vessel";
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Vessel(pub Cow<'static, str>);
pub const UNASSIGNED: Vessel = Vessel(Cow::Borrowed(""));
pub const LM: Vessel = Vessel(Cow::Borrowed("LM"));
pub const LAD: Vessel = Vessel(Cow::Borrowed("LAD"));
pub const LCX: Vessel = Vessel(Cow::Borrowed("LCX"));
pub const RCA: Vessel = Vessel(Cow::Borrowed("RCA"));
pub const OTHER: Vessel = Vessel(Cow::Borrowed("Other"));
pub const TOTAL: Vessel = Vessel(Cow::Borrowed("Total"));
const VESSELS: [Vessel; 5] = [LM, LAD, LCX, RCA, OTHER];
pub fn vessels() -> Vec<Vessel> {
    VESSELS.to_vec()
}
impl Vessel {
    pub fn new(name: impl Into<String>) -> Vessel {
        Vessel(Cow::Owned(name.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn valid(&self) -> bool {
        VESSELS.iter().any(|known| known == self)
    }
    pub fn name(&self) -> String {
        match self.as_str() {
            "LM" => "Left main",
            "LAD" => "Left anterior descending",
            "LCX" => "Left circumflex",
            "RCA" => "Right coronary",
            "Other" => "Other coronary",
            "Total" => "Total",
            "" => "Unassigned",
            other => other,
        }
        .to_string()
    }
    pub fn site(&self) -> Code<'static> {
        match self.as_str() {
            "LM" => sct::LeftMainCoronaryArtery,
            "LAD" => sct::LeftAnteriorDescendingCoronaryArtery,
            "LCX" => sct::CircumflexCoronaryArtery,
            "RCA" => sct::RightCoronaryArtery,
            _ => sct::CoronaryArtery,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Geometry {
    pub columns: i64,
    pub rows: i64,
    pub pixel_spacing: [f64; 2],
    pub slice_positions: Vec<f64>,
    pub slice_thickness_mm: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Options {
    pub threshold_hu: f64,
    pub min_lesion_area_mm2: f64,
    pub reference_thickness_mm: f64,
    pub four_connected: bool,
    pub mass_calibration: f64,
}
impl Options {
    fn resolved(mut self) -> Result<Options, Error> {
        for value in [
            self.threshold_hu,
            self.min_lesion_area_mm2,
            self.reference_thickness_mm,
            self.mass_calibration,
        ] {
            if !value.is_finite() {
                return Err(Error::wrap(ERR_INVALID_OPTIONS, "non-finite value"));
            }
        }
        if self.min_lesion_area_mm2 < 0.0
            || self.reference_thickness_mm < 0.0
            || self.mass_calibration < 0.0
        {
            return Err(Error::wrap(ERR_INVALID_OPTIONS, "negative value"));
        }
        if self.threshold_hu == 0.0 {
            self.threshold_hu = DEFAULT_THRESHOLD_HU;
        }
        if self.min_lesion_area_mm2 == 0.0 {
            self.min_lesion_area_mm2 = DEFAULT_MIN_LESION_AREA_MM2;
        }
        if self.reference_thickness_mm == 0.0 {
            self.reference_thickness_mm = DEFAULT_REFERENCE_THICKNESS_MM;
        }
        Ok(self)
    }
}
pub fn density_weight(peak_hu: f64) -> i64 {
    if peak_hu >= 400.0 {
        4
    } else if peak_hu >= 300.0 {
        3
    } else if peak_hu >= 200.0 {
        2
    } else {
        1
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lesion {
    pub id: i64,
    pub slice: i64,
    pub group: i64,
    pub pixels: Vec<i64>,
    pub centroid_x: f64,
    pub centroid_y: f64,
    pub area_mm2: f64,
    pub peak_hu: f64,
    pub mean_hu: f64,
    pub weight: i64,
    pub agatston: f64,
    pub volume_mm3: f64,
    pub vessel: Vessel,
}
impl Lesion {
    pub fn mass_mg(&self, c: f64) -> f64 {
        c * self.volume_mm3 / 1000.0 * self.mean_hu
    }
}
pub type SliceReader<'a> = dyn FnMut(i64) -> Result<Cow<'a, [f32]>, Error> + 'a;
pub fn slice_values<'a>(
    slices: &'a [Vec<f32>],
) -> impl FnMut(i64) -> Result<Cow<'a, [f32]>, Error> + 'a {
    move |z| {
        if z < 0 || z >= slices.len() as i64 {
            return Err(Error::msg(format!("calcium: slice {z} out of range")));
        }
        Ok(Cow::Borrowed(slices[z as usize].as_slice()))
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Analysis {
    pub geometry: Geometry,
    pub options: Options,
    pub increment_mm: f64,
    pub normalization_factor: f64,
    pub lesions: Vec<Lesion>,
    by_slice: Vec<Vec<i64>>,
}
pub fn increment(g: &Geometry) -> Result<f64, Error> {
    let positions = &g.slice_positions;
    if positions.is_empty() {
        return Err(Error::wrap(ERR_INVALID_GEOMETRY, "no slices"));
    }
    if positions.iter().any(|p| !p.is_finite()) {
        return Err(Error::wrap(
            ERR_INVALID_GEOMETRY,
            "non-finite slice position",
        ));
    }
    if positions.len() == 1 {
        if !(g.slice_thickness_mm > 0.0) || g.slice_thickness_mm.is_infinite() {
            return Err(Error::wrap(
                ERR_INVALID_GEOMETRY,
                "a single slice needs its slice thickness",
            ));
        }
        return Ok(g.slice_thickness_mm);
    }
    let mut gaps = vec![0.0f64; positions.len() - 1];
    let ascending = positions[1] > positions[0];
    for i in 0..gaps.len() {
        let mut gap = positions[i + 1] - positions[i];
        if !ascending {
            gap = -gap;
        }
        if !(gap > 0.0) {
            return Err(Error::wrap(
                ERR_INVALID_GEOMETRY,
                format!(
                    "slice positions are not strictly monotonic at slice {}",
                    i + 1
                ),
            ));
        }
        gaps[i] = gap;
    }
    let increment = (positions[positions.len() - 1] - positions[0]).abs() / gaps.len() as f64;
    let tolerance = fmath::max(0.01, 0.05 * increment);
    for (i, &gap) in gaps.iter().enumerate() {
        if (gap - increment).abs() > tolerance {
            return Err(Error::wrap(
                ERR_IRREGULAR_SPACING,
                format!(
                    "gap of {} mm after slice {i}, increment {} mm",
                    format_float(gap, 'g', 3),
                    format_float(increment, 'g', 3)
                ),
            ));
        }
    }
    Ok(increment)
}
pub fn detect(
    ctx: Option<&dyn Cancellation>,
    g: &Geometry,
    read: Option<&mut SliceReader<'_>>,
    opts: Options,
) -> Result<Analysis, Error> {
    let opts = opts.resolved()?;
    if g.columns <= 0 || g.rows <= 0 {
        return Err(Error::wrap(
            ERR_INVALID_GEOMETRY,
            format!("{}×{} pixels", g.columns, g.rows),
        ));
    }
    let spacing = g.pixel_spacing;
    if !(spacing[0] > 0.0)
        || !(spacing[1] > 0.0)
        || spacing[0].is_infinite()
        || spacing[1].is_infinite()
    {
        return Err(Error::wrap(
            ERR_INVALID_GEOMETRY,
            format!(
                "pixel spacing [{} {}]",
                float_v(spacing[0]),
                float_v(spacing[1])
            ),
        ));
    }
    let Some(read) = read else {
        return Err(Error::wrap(ERR_INVALID_GEOMETRY, "no slice reader"));
    };
    let increment_mm = increment(g)?;
    let mut analysis = Analysis {
        geometry: g.clone(),
        options: opts,
        increment_mm,
        normalization_factor: increment_mm / opts.reference_thickness_mm,
        lesions: Vec::new(),
        by_slice: vec![Vec::new(); g.slice_positions.len()],
    };
    let pixel_area = spacing[0] * spacing[1];
    let plane = g.columns.wrapping_mul(g.rows);
    let mut visited = vec![false; plane as usize];
    let (mut stack, mut pixels): (Vec<i64>, Vec<i64>) = (Vec::new(), Vec::new());
    for z in 0..g.slice_positions.len() as i64 {
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        let values = match read(z) {
            Ok(values) => values,
            Err(err) => {
                return Err(Error::with_message(
                    err.sentinel_text(),
                    format!("calcium: read slice {z}: {err}"),
                ));
            }
        };
        if values.len() as i64 != plane {
            return Err(Error::wrap(
                ERR_INVALID_GEOMETRY,
                format!("slice {z} has {} values, want {plane}", values.len()),
            ));
        }
        visited.fill(false);
        let above = |index: i64| f64::from(values[index as usize]) >= opts.threshold_hu;
        for start in 0..plane {
            if visited[start as usize] || !above(start) {
                continue;
            }
            pixels.clear();
            stack.clear();
            stack.push(start);
            visited[start as usize] = true;
            while let Some(index) = stack.pop() {
                pixels.push(index);
                let (x, y) = (index % g.columns, index / g.columns);
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        if (dx == 0 && dy == 0) || (opts.four_connected && dx != 0 && dy != 0) {
                            continue;
                        }
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= g.columns || ny >= g.rows {
                            continue;
                        }
                        let neighbour = ny * g.columns + nx;
                        if visited[neighbour as usize] || !above(neighbour) {
                            continue;
                        }
                        visited[neighbour as usize] = true;
                        stack.push(neighbour);
                    }
                }
            }
            let area = pixels.len() as f64 * pixel_area;
            if area < opts.min_lesion_area_mm2 * 0.999999999 {
                continue;
            }
            analysis.add_lesion(z, &pixels, &values, area);
        }
    }
    analysis.group_across_slices();
    Ok(analysis)
}
impl Analysis {
    fn add_lesion(&mut self, z: i64, pixels: &[i64], values: &[f32], area: f64) {
        let mut sorted = pixels.to_vec();
        sorted.sort_unstable();
        let columns = self.geometry.columns;
        let mut peak_hu = f64::NEG_INFINITY;
        let (mut sum, mut sum_x, mut sum_y) = (0.0f64, 0.0f64, 0.0f64);
        for &index in &sorted {
            let value = f64::from(values[index as usize]);
            sum += value;
            peak_hu = fmath::max(peak_hu, value);
            sum_x += (index % columns) as f64;
            sum_y += (index / columns) as f64;
        }
        let n = sorted.len() as f64;
        let id = self.lesions.len() as i64;
        let weight = density_weight(peak_hu);
        self.lesions.push(Lesion {
            id,
            slice: z,
            group: id,
            pixels: sorted,
            centroid_x: sum_x / n,
            centroid_y: sum_y / n,
            area_mm2: area,
            peak_hu,
            mean_hu: sum / n,
            weight,
            agatston: area * weight as f64 * self.normalization_factor,
            volume_mm3: area * self.increment_mm,
            vessel: UNASSIGNED,
        });
        self.by_slice[z as usize].push(id);
    }
    fn group_across_slices(&mut self) {
        let mut parent: Vec<usize> = (0..self.lesions.len()).collect();
        fn find(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        for z in 1..self.by_slice.len() {
            for &current in &self.by_slice[z] {
                for &previous in &self.by_slice[z - 1] {
                    if overlap(
                        &self.lesions[current as usize].pixels,
                        &self.lesions[previous as usize].pixels,
                    ) {
                        let (mut ra, mut rb) = (
                            find(&mut parent, current as usize),
                            find(&mut parent, previous as usize),
                        );
                        if ra < rb {
                            (ra, rb) = (rb, ra);
                        }
                        parent[ra] = rb;
                    }
                }
            }
        }
        for i in 0..self.lesions.len() {
            self.lesions[i].group = find(&mut parent, i) as i64;
        }
    }
    pub fn slice_lesions(&self, z: i64) -> Vec<i64> {
        if z < 0 || z >= self.by_slice.len() as i64 {
            return Vec::new();
        }
        self.by_slice[z as usize].clone()
    }
    pub fn lesion_at(&self, z: i64, x: i64, y: i64) -> Option<i64> {
        if z < 0
            || z >= self.by_slice.len() as i64
            || x < 0
            || y < 0
            || x >= self.geometry.columns
            || y >= self.geometry.rows
        {
            return None;
        }
        let index = y * self.geometry.columns + x;
        self.by_slice[z as usize].iter().copied().find(|&id| {
            self.lesions[id as usize]
                .pixels
                .binary_search(&index)
                .is_ok()
        })
    }
    fn check_assignment(&self, id: i64, vessel: &Vessel) -> Result<(), Error> {
        if id < 0 || id >= self.lesions.len() as i64 {
            return Err(Error::wrap(ERR_UNKNOWN_LESION, id));
        }
        if *vessel != UNASSIGNED && !vessel.valid() {
            return Err(Error::wrap(ERR_UNKNOWN_VESSEL, quote(vessel.as_str())));
        }
        Ok(())
    }
    pub fn assign(&mut self, id: i64, vessel: Vessel) -> Result<(), Error> {
        self.check_assignment(id, &vessel)?;
        self.lesions[id as usize].vessel = vessel;
        Ok(())
    }
    pub fn assign_group(&mut self, id: i64, vessel: Vessel) -> Result<i64, Error> {
        self.check_assignment(id, &vessel)?;
        let group = self.lesions[id as usize].group;
        let mut count = 0i64;
        for lesion in &mut self.lesions {
            if lesion.group == group {
                lesion.vessel = vessel.clone();
                count += 1;
            }
        }
        Ok(count)
    }
    pub fn set_mass_calibration(&mut self, c: f64) -> Result<(), Error> {
        if !c.is_finite() || c < 0.0 {
            return Err(Error::wrap(
                ERR_INVALID_OPTIONS,
                format!("mass calibration {}", float_v(c)),
            ));
        }
        self.options.mass_calibration = c;
        Ok(())
    }
    pub fn score(&self) -> Score {
        let mut score = Score::unscored();
        let calibration = self.options.mass_calibration;
        score.mass_calibrated = calibration > 0.0;
        score.mass_calibration = calibration;
        score.threshold_hu = self.options.threshold_hu;
        score.min_lesion_area_mm2 = self.options.min_lesion_area_mm2;
        score.increment_mm = self.increment_mm;
        score.slice_thickness_mm = self.geometry.slice_thickness_mm;
        score.normalization_factor = self.normalization_factor;
        score.slices = self.geometry.slice_positions.len() as i64;
        for lesion in &self.lesions {
            let Some(i) = VESSELS.iter().position(|v| *v == lesion.vessel) else {
                score.unassigned.add(lesion, calibration);
                continue;
            };
            score.vessels[i].add(lesion, calibration);
            score.total.add(lesion, calibration);
        }
        score
    }
}
fn overlap(a: &[i64], b: &[i64]) -> bool {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => return true,
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    false
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VesselScore {
    pub vessel: Vessel,
    pub lesions: i64,
    pub agatston: f64,
    pub volume_mm3: f64,
    pub mass_mg: f64,
}
impl VesselScore {
    fn of(vessel: Vessel) -> VesselScore {
        VesselScore {
            vessel,
            ..VesselScore::default()
        }
    }
    fn add(&mut self, lesion: &Lesion, calibration: f64) {
        self.lesions += 1;
        self.agatston += lesion.agatston;
        self.volume_mm3 += lesion.volume_mm3;
        self.mass_mg += lesion.mass_mg(calibration);
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Score {
    pub vessels: Vec<VesselScore>,
    pub total: VesselScore,
    pub unassigned: VesselScore,
    pub mass_calibrated: bool,
    pub mass_calibration: f64,
    pub threshold_hu: f64,
    pub min_lesion_area_mm2: f64,
    pub increment_mm: f64,
    pub slice_thickness_mm: f64,
    pub normalization_factor: f64,
    pub slices: i64,
}
impl Score {
    #[doc(hidden)]
    pub fn unscored() -> Score {
        Score {
            vessels: VESSELS.iter().cloned().map(VesselScore::of).collect(),
            total: VesselScore::of(TOTAL),
            unassigned: VesselScore::of(UNASSIGNED),
            ..Score::default()
        }
    }
    pub fn vessel(&self, vessel: &Vessel) -> VesselScore {
        if *vessel == TOTAL {
            return self.total.clone();
        }
        if *vessel == UNASSIGNED {
            return self.unassigned.clone();
        }
        self.vessels
            .iter()
            .find(|score| score.vessel == *vessel)
            .cloned()
            .unwrap_or_else(|| VesselScore::of(vessel.clone()))
    }
}
