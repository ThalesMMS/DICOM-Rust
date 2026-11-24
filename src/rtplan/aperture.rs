use super::bounded::inspection_limits;
use super::geometry::jaw;
use super::read::{POSITION_TOLERANCE, Reader};
use super::reflect::model_admission;
use super::validate::validate_plan;
use super::*;
use crate::fmath;
pub const APERTURE_COORDINATES: &str = "IEC BEAM LIMITING DEVICE; machine isocentric plane; mm";
model! {
    JawGeometry "JawGeometry" {
        device_type: String = "DeviceType",
        axis: i64 = "Axis",
        positions: [f64; 2] = "Positions",
    }
}
model! {
    LeafOpening "LeafOpening" {
        device_type: String = "DeviceType",
        pair: i64 = "Pair",
        min: [f64; 2] = "Min",
        max: [f64; 2] = "Max",
        open: bool = "Open",
    }
}
model! {
    FractionMeterset "FractionMeterset" {
        group_number: i64 = "GroupNumber",
        meterset: Option<f64> = "Meterset",
        unit: String = "Unit",
    }
}
model! {
    ApertureGeometry "ApertureGeometry" {
        beam_number: i64 = "BeamNumber",
        beam_name: String = "BeamName",
        control_point_count: i64 = "ControlPointCount",
        state: ControlPoint = "State",
        jaws: Vec<JawGeometry> = "Jaws",
        leaves: Vec<LeafOpening> = "Leaves",
        blocks: Vec<Block> = "Blocks",
        derived_area: Option<f64> = "DerivedArea",
        area_basis: String = "AreaBasis",
        limitations: Vec<String> = "Limitations",
        fraction_metersets: Vec<FractionMeterset> = "FractionMetersets",
    }
}
pub(crate) fn invalid(detail: &str) -> Error {
    Error {
        kind: ErrorKind::InvalidObject,
        text: format!("{}: {detail}", ErrorKind::InvalidObject.text()),
        validation: false,
        problems: Vec::new(),
    }
}
impl Plan {
    pub fn inspect_aperture(
        &self,
        beam_number: i64,
        control_point: i64,
        options: Option<Limits>,
    ) -> Result<ApertureGeometry, Error> {
        if control_point < 0 {
            return Err(Error::bare(ErrorKind::InvalidObject));
        }
        let l = inspection_limits(options)?;
        model_admission(self, &l)?;
        let mut r = Reader::new(l.max_problems);
        validate_plan(&mut r, self);
        if !r.errors.is_empty() {
            return Err(Error::validation(r.errors));
        }
        for b in &self.beams {
            if b.number != beam_number {
                continue;
            }
            if control_point >= b.control_points.len() as i64 {
                return Err(invalid("control point does not exist"));
            }
            let cp = &b.control_points[control_point as usize];
            let mut geometry = ApertureGeometry {
                beam_number: b.number,
                beam_name: b.name.clone(),
                control_point_count: b.control_points.len() as i64,
                state: cp.clone(),
                ..ApertureGeometry::default()
            };
            for group in &self.fraction_groups {
                for reference in &group.referenced_beams {
                    if reference.beam_number == b.number {
                        geometry.fraction_metersets.push(FractionMeterset {
                            group_number: group.number,
                            meterset: reference.meterset,
                            unit: b.primary_dosimeter_unit.clone(),
                        });
                    }
                }
            }
            for device in &b.devices {
                let position = match cp.device(&device.typ) {
                    Some(p)
                        if device.pairs >= 1 && p.positions.len() as i64 == 2 * device.pairs =>
                    {
                        p
                    }
                    _ => {
                        return Err(invalid(
                            "selected control point has incomplete device positions",
                        ));
                    }
                };
                let pairs = device.pairs as usize;
                for k in 0..pairs {
                    if position.positions[k] > position.positions[pairs + k] + POSITION_TOLERANCE {
                        return Err(invalid("selected leaf/jaw banks cross"));
                    }
                }
                if !device.is_mlc() {
                    let axis = i64::from(device.typ == DEVICE_Y || device.typ == DEVICE_ASYM_Y);
                    geometry.jaws.push(JawGeometry {
                        device_type: device.typ.clone(),
                        axis,
                        positions: [position.positions[0], position.positions[1]],
                    });
                }
            }
            let (x_low, x_high, x_ok) = jaw(cp, &[DEVICE_X, DEVICE_ASYM_X]);
            let (y_low, y_high, y_ok) = jaw(cp, &[DEVICE_Y, DEVICE_ASYM_Y]);
            let mut mlcs = 0;
            for device in &b.devices {
                if !device.is_mlc() {
                    continue;
                }
                mlcs += 1;
                let Some(position) = cp.device(&device.typ) else {
                    continue;
                };
                let pairs = device.pairs as usize;
                for k in 0..pairs {
                    let mut gap = LeafOpening {
                        device_type: device.typ.clone(),
                        pair: k as i64 + 1,
                        min: [position.positions[k], device.leaf_boundaries[k]],
                        max: [position.positions[pairs + k], device.leaf_boundaries[k + 1]],
                        open: false,
                    };
                    if device.typ == DEVICE_MLC_Y {
                        gap.min.swap(0, 1);
                        gap.max.swap(0, 1);
                    }
                    if x_ok {
                        gap.min[0] = fmath::max(gap.min[0], x_low);
                        gap.max[0] = fmath::min(gap.max[0], x_high);
                    }
                    if y_ok {
                        gap.min[1] = fmath::max(gap.min[1], y_low);
                        gap.max[1] = fmath::min(gap.max[1], y_high);
                    }
                    gap.open = gap.max[0] > gap.min[0] && gap.max[1] > gap.min[1];
                    geometry.leaves.push(gap);
                }
            }
            if mlcs == 1 {
                match b.aperture_area(control_point) {
                    Ok(area) if finite(area) => {
                        geometry.derived_area = Some(area);
                        geometry.area_basis =
                            "MLC clipped by available jaws, excluding blocks".to_string();
                    }
                    _ => return Err(invalid("derived MLC area is unavailable or non-finite")),
                }
            } else if mlcs == 0 && x_ok && y_ok {
                let area = (x_high - x_low) * (y_high - y_low);
                if !finite(area) {
                    return Err(invalid("derived jaw area is non-finite"));
                }
                geometry.derived_area = Some(area);
                geometry.area_basis = "jaws only, excluding blocks".to_string();
            } else {
                geometry.limitations.push(
                    "Combined aperture area unavailable for this device arrangement.".to_string(),
                );
            }
            for block in &b.blocks {
                geometry.blocks.push(block.clone());
                if block.contour.is_empty() {
                    geometry
                        .limitations
                        .push(format!("Block {} outline unavailable.", block.number));
                }
            }
            if !b.blocks.is_empty() {
                geometry.limitations.push("Block contours are declared outlines; composite aperture, transmission and block-corrected area are not calculated.".to_string());
            }
            if !x_ok || !y_ok {
                geometry.limitations.push(
                    "One or both jaw axes are unavailable; available device geometry only."
                        .to_string(),
                );
            }
            return Ok(geometry);
        }
        Err(invalid("conventional beam does not exist"))
    }
}
