use super::*;
use crate::fmath;
model! {
    GantryArc "GantryArc" {
        start: f64 = "Start",
        stop: f64 = "Stop",
        extent: f64 = "Extent",
        direction: String = "Direction",
    }
}
pub(crate) fn jaw(cp: &ControlPoint, types: &[&str]) -> (f64, f64, bool) {
    for device_type in types {
        if let Some(d) = cp.device(device_type)
            && d.positions.len() == 2
        {
            return (d.positions[0], d.positions[1], true);
        }
    }
    (0.0, 0.0, false)
}
impl Beam {
    pub fn arc(&self) -> GantryArc {
        let cps = &self.control_points;
        if cps.is_empty() {
            return GantryArc {
                direction: ROTATION_NONE.to_string(),
                ..GantryArc::default()
            };
        }
        let mut arc = GantryArc {
            start: cps[0].gantry_angle,
            stop: cps[cps.len() - 1].gantry_angle,
            extent: 0.0,
            direction: ROTATION_NONE.to_string(),
        };
        for i in 0..cps.len() - 1 {
            let (from, to) = (cps[i].gantry_angle, cps[i + 1].gantry_angle);
            let direction = cps[i].gantry_rotation_direction.as_str();
            let step = match direction {
                ROTATION_CLOCKWISE => (to - from + 360.0) % 360.0,
                ROTATION_COUNTER_CLOCKWISE => (from - to + 360.0) % 360.0,
                _ => {
                    let step = (to - from).abs();
                    if step > 180.0 { 360.0 - step } else { step }
                }
            };
            if step == 0.0 {
                continue;
            }
            arc.extent += step;
            if direction == ROTATION_CLOCKWISE || direction == ROTATION_COUNTER_CLOCKWISE {
                if arc.direction == ROTATION_NONE {
                    arc.direction = direction.to_string();
                } else if arc.direction != direction {
                    arc.direction = "MIXED".to_string();
                }
            }
        }
        arc
    }
    pub fn is_arc(&self) -> bool {
        self.arc().extent > 0.0
    }
    pub fn jaw_opening(&self, control_point: i64) -> (f64, f64, bool, bool) {
        if control_point < 0 || control_point >= self.control_points.len() as i64 {
            return (0.0, 0.0, false, false);
        }
        let cp = &self.control_points[control_point as usize];
        let (x1, x2, ok_x) = jaw(cp, &[DEVICE_X, DEVICE_ASYM_X]);
        let (y1, y2, ok_y) = jaw(cp, &[DEVICE_Y, DEVICE_ASYM_Y]);
        (x2 - x1, y2 - y1, ok_x, ok_y)
    }
    pub fn aperture_area(&self, control_point: i64) -> Result<f64, Error> {
        let no = |detail: String| Error {
            kind: ErrorKind::NoAperture,
            text: format!("{}: {detail}", ErrorKind::NoAperture.text()),
            validation: false,
            problems: Vec::new(),
        };
        if control_point < 0 || control_point >= self.control_points.len() as i64 {
            return Err(no(format!("control point {control_point} does not exist")));
        }
        let Some(mlc) = self.mlc() else {
            return Err(Error::bare(ErrorKind::NoAperture));
        };
        let cp = &self.control_points[control_point as usize];
        let leaves = match cp.device(&mlc.typ) {
            Some(l)
                if l.positions.len() as i64 == 2 * mlc.pairs
                    && mlc.leaf_boundaries.len() as i64 == mlc.pairs + 1 =>
            {
                l
            }
            _ => {
                return Err(no(format!(
                    "control point {control_point} has no position for {}",
                    mlc.typ
                )));
            }
        };
        let (mut travel_types, mut across_types) =
            ([DEVICE_X, DEVICE_ASYM_X], [DEVICE_Y, DEVICE_ASYM_Y]);
        if mlc.typ == DEVICE_MLC_Y {
            std::mem::swap(&mut travel_types, &mut across_types);
        }
        let (mut travel_low, mut travel_high) = (f64::NEG_INFINITY, f64::INFINITY);
        let (lo, hi, ok) = jaw(cp, &travel_types);
        if ok {
            (travel_low, travel_high) = (lo, hi);
        }
        let (mut across_low, mut across_high) = (f64::NEG_INFINITY, f64::INFINITY);
        let (lo, hi, ok) = jaw(cp, &across_types);
        if ok {
            (across_low, across_high) = (lo, hi);
        }
        let pairs = mlc.pairs.max(0) as usize;
        let mut area = 0.0;
        for k in 0..pairs {
            let gap = fmath::min(leaves.positions[pairs + k], travel_high)
                - fmath::max(leaves.positions[k], travel_low);
            let width = fmath::min(mlc.leaf_boundaries[k + 1], across_high)
                - fmath::max(mlc.leaf_boundaries[k], across_low);
            if gap > 0.0 && width > 0.0 {
                area += gap * width;
            }
        }
        Ok(area)
    }
}
