use super::bounded::inspection_limits;
use super::read::g;
use super::reflect::{format_float32, model_admission};
use super::*;
model! {
    Summary "Summary" {
        label: String = "Label",
        name: String = "Name",
        intent: String = "Intent",
        geometry: String = "Geometry",
        approval_status: String = "ApprovalStatus",
        prescription: Vec<f64> = "Prescription",
        fraction_groups: Vec<FractionGroupSummary> = "FractionGroups",
        beams: Vec<BeamSummary> = "Beams",
        brachy_setups: i64 = "BrachySetups",
        warnings: i64 = "Warnings",
        profile: String = "Profile",
        ion_beams: Vec<IonBeamSummary> = "IonBeams",
        sources: i64 = "Sources",
        channels: i64 = "Channels",
    }
}
model! {
    FractionGroupSummary "FractionGroupSummary" {
        number: i64 = "Number",
        fractions_planned: Option<i64> = "FractionsPlanned",
        beams: i64 = "Beams",
        brachy_setups: i64 = "BrachySetups",
        meterset_per_fraction: Option<f64> = "MetersetPerFraction",
        meterset_unit: String = "MetersetUnit",
        dose_per_fraction: Option<f64> = "DosePerFraction",
    }
}
model! {
    BeamSummary "BeamSummary" {
        number: i64 = "Number",
        name: String = "Name",
        typ: String = "Type",
        radiation_type: String = "RadiationType",
        treatment_machine_name: String = "TreatmentMachineName",
        delivery_type: String = "DeliveryType",
        nominal_energy: Option<f64> = "NominalEnergy",
        control_points: i64 = "ControlPoints",
        arc: GantryArc = "Arc",
        collimator_angle: f64 = "CollimatorAngle",
        couch_angle: f64 = "CouchAngle",
        meterset: Option<f64> = "Meterset",
        meterset_unit: String = "MetersetUnit",
        mlc: String = "MLC",
        mlc_pairs: i64 = "MLCPairs",
        jaw_x: Option<f64> = "JawX",
        jaw_y: Option<f64> = "JawY",
        aperture: Option<f64> = "Aperture",
        wedges: i64 = "Wedges",
        blocks: i64 = "Blocks",
    }
}
model! {
    IonBeamSummary "IonBeamSummary" {
        number: i64 = "Number",
        name: String = "Name",
        radiation_type: String = "RadiationType",
        scan_mode: String = "ScanMode",
        unit: String = "Unit",
        control_points: i64 = "ControlPoints",
        range_shifters: i64 = "RangeShifters",
        spots: i64 = "Spots",
        energy: Option<f64> = "Energy",
    }
}
model! {
    InspectionTarget "InspectionTarget" {
        kind: String = "Kind",
        label: String = "Label",
        beam_number: i64 = "BeamNumber",
        setup_number: i64 = "SetupNumber",
        channel_number: i64 = "ChannelNumber",
    }
}
model! {
    InspectionValue "InspectionValue" {
        name: String = "Name",
        value: String = "Value",
        unit: String = "Unit",
        coordinates: String = "Coordinates",
    }
}
model! {
    InspectionRow "InspectionRow" {
        label: String = "Label",
        values: Vec<InspectionValue> = "Values",
    }
}
model! {
    InspectionPage "InspectionPage" {
        rows: Vec<InspectionRow> = "Rows",
        offset: i64 = "Offset",
        total: i64 = "Total",
    }
}
fn sum_of(refs: &[ReferencedBeam], value: impl Fn(&ReferencedBeam) -> Option<f64>) -> Option<f64> {
    if refs.is_empty() {
        return None;
    }
    let mut total = 0.0;
    for r in refs {
        total += value(r)?;
        if !finite(total) {
            return None;
        }
    }
    Some(total)
}
fn group_meterset_unit(p: &Plan, group: &FractionGroup) -> String {
    let mut unit = String::new();
    for reference in &group.referenced_beams {
        let mut u = "";
        for b in &p.beams {
            if b.number == reference.beam_number {
                u = &b.primary_dosimeter_unit;
            }
        }
        for b in &p.ion_beams {
            if b.number == reference.beam_number {
                u = &b.primary_dosimeter_unit;
            }
        }
        if u.is_empty() || !unit.is_empty() && u != unit {
            return "declared mixed/unknown units".to_string();
        }
        unit = u.to_string();
    }
    unit
}
fn val(n: &str, v: impl Into<String>, u: &str, c: &str) -> InspectionValue {
    InspectionValue {
        name: n.to_string(),
        value: v.into(),
        unit: u.to_string(),
        coordinates: c.to_string(),
    }
}
fn opt(v: Option<f64>) -> String {
    match v {
        None => "unavailable".to_string(),
        Some(x) => g(x),
    }
}
fn attr(v: &Attribute<f64>) -> String {
    opt(v.value)
}
fn count(v: &Attribute<i64>) -> String {
    match v.value {
        None => "unavailable".to_string(),
        Some(n) => n.to_string(),
    }
}
fn vec(v: &[f64]) -> String {
    if v.is_empty() {
        return "unavailable".to_string();
    }
    let text: Vec<String> = v.iter().map(|x| g(*x)).collect();
    text.join(", ")
}
struct Pager {
    page: InspectionPage,
    limit: i64,
}
impl Pager {
    fn emit(&mut self, label: String, build: impl FnOnce() -> Vec<InspectionValue>) {
        let n = self.page.total;
        self.page.total += 1;
        if n >= self.page.offset && n - self.page.offset < self.limit {
            self.page.rows.push(InspectionRow {
                label,
                values: build(),
            });
        }
    }
}
impl Plan {
    pub fn summary(&self) -> Summary {
        let p = self;
        let mut s = Summary {
            label: p.label.clone(),
            name: p.name.clone(),
            intent: p.intent.clone(),
            geometry: p.geometry.clone(),
            approval_status: p.approval_status.clone(),
            brachy_setups: p.brachy_setups.len() as i64,
            warnings: p.warnings.len() as i64,
            ..Summary::default()
        };
        s.profile = "Conventional RT Plan".to_string();
        if !p.ion_beams.is_empty() {
            s.profile = "Ion PROTON/ION NONE/MODULATED".to_string();
        }
        if !p.brachy_setups.is_empty() {
            s.profile = format!("Brachy {} STEPWISE/FIXED", p.brachy_treatment_type);
        }
        s.sources = p.sources.len() as i64;
        for v in &p.brachy_setups {
            s.channels += v.channels.len() as i64;
        }
        for reference in &p.dose_references {
            if let Some(dose) = reference.target_prescription_dose {
                s.prescription.push(dose);
            }
        }
        for group in &p.fraction_groups {
            let mut summary = FractionGroupSummary {
                number: group.number,
                fractions_planned: group.fractions_planned,
                beams: group.number_of_beams,
                brachy_setups: group.number_of_brachy_setups,
                ..FractionGroupSummary::default()
            };
            summary.meterset_unit = group_meterset_unit(p, group);
            summary.meterset_per_fraction = sum_of(&group.referenced_beams, |r| r.meterset);
            if summary.meterset_unit == "declared mixed/unknown units" {
                summary.meterset_per_fraction = None;
            }
            summary.dose_per_fraction = sum_of(&group.referenced_beams, |r| r.dose);
            s.fraction_groups.push(summary);
        }
        for beam in &p.beams {
            let mut b = BeamSummary {
                number: beam.number,
                name: beam.name.clone(),
                typ: beam.typ.clone(),
                radiation_type: beam.radiation_type.clone(),
                treatment_machine_name: beam.treatment_machine_name.clone(),
                delivery_type: beam.treatment_delivery_type.clone(),
                control_points: beam.control_points.len() as i64,
                arc: beam.arc(),
                meterset_unit: beam.primary_dosimeter_unit.clone(),
                wedges: beam.number_of_wedges,
                blocks: beam.number_of_blocks,
                ..BeamSummary::default()
            };
            if let Some(first) = beam.control_points.first() {
                b.nominal_energy = first.nominal_beam_energy;
                b.collimator_angle = first.beam_limiting_device_angle;
                b.couch_angle = first.patient_support_angle;
                let (x, y, ok_x, ok_y) = beam.jaw_opening(0);
                if ok_x {
                    b.jaw_x = Some(x);
                }
                if ok_y {
                    b.jaw_y = Some(y);
                }
                if let Ok(area) = beam.aperture_area(0) {
                    b.aperture = Some(area);
                }
            }
            if let Some(mlc) = beam.mlc() {
                b.mlc = mlc.typ.clone();
                b.mlc_pairs = mlc.pairs;
            }
            'found: for group in &p.fraction_groups {
                for reference in &group.referenced_beams {
                    if reference.beam_number == beam.number {
                        b.meterset = reference.meterset;
                        break 'found;
                    }
                }
            }
            s.beams.push(b);
        }
        for b in &p.ion_beams {
            let mut x = IonBeamSummary {
                number: b.number,
                name: b.name.clone(),
                radiation_type: b.radiation_type.clone(),
                scan_mode: b.scan_mode.clone(),
                unit: b.primary_dosimeter_unit.clone(),
                control_points: b.control_points.len() as i64,
                range_shifters: b.range_shifters.len() as i64,
                ..IonBeamSummary::default()
            };
            if let Some(first) = b.control_points.first() {
                x.energy = first.control_point.nominal_beam_energy;
            }
            for cp in &b.control_points {
                x.spots += cp.spot_weights.len() as i64;
            }
            s.ion_beams.push(x);
        }
        s
    }
    pub fn inspection_targets(
        &self,
        options: Option<Limits>,
    ) -> Result<Vec<InspectionTarget>, Error> {
        let l = inspection_limits(options)?;
        model_admission(self, &l)?;
        let mut out = Vec::new();
        for b in &self.ion_beams {
            out.push(InspectionTarget {
                kind: "ion".to_string(),
                beam_number: b.number,
                label: format!("Ion beam {} {}", b.number, b.name),
                ..InspectionTarget::default()
            });
        }
        if !self.sources.is_empty() {
            out.push(InspectionTarget {
                kind: "sources".to_string(),
                label: "Brachy sources".to_string(),
                ..InspectionTarget::default()
            });
        }
        for s in &self.brachy_setups {
            for c in &s.channels {
                out.push(InspectionTarget {
                    kind: "brachy".to_string(),
                    setup_number: s.number,
                    channel_number: c.number,
                    label: format!("Setup {} / channel {}", s.number, c.number),
                    ..InspectionTarget::default()
                });
            }
        }
        Ok(out)
    }
    pub fn inspect_page(
        &self,
        target: &InspectionTarget,
        offset: i64,
        limit: i64,
        options: Option<Limits>,
    ) -> Result<InspectionPage, Error> {
        if offset < 0 || !(1..=256).contains(&limit) {
            return Err(Error::bare(ErrorKind::InvalidObject));
        }
        let l = inspection_limits(options)?;
        model_admission(self, &l)?;
        let p = self;
        let mut pager = Pager {
            page: InspectionPage {
                offset,
                ..InspectionPage::default()
            },
            limit,
        };
        let mut matched = false;
        match target.kind.as_str() {
            "sources" => {
                matched = true;
                for s in &p.sources {
                    pager.emit(format!("Source {}", s.number), || {
                        vec![
                            val("Type", s.typ.as_str(), "", ""),
                            val("Isotope", s.isotope_name.as_str(), "", ""),
                            val("Half-life", attr(&s.isotope_half_life), "day", ""),
                            val(
                                "Reference air kerma rate",
                                attr(&s.reference_air_kerma_rate),
                                "µGy/h at 1 m",
                                "",
                            ),
                            val("Strength", attr(&s.strength), &s.strength_units, ""),
                            val(
                                "Strength reference",
                                format!(
                                    "{} {}",
                                    s.strength_reference_date, s.strength_reference_time
                                ),
                                "",
                                "declared precision",
                            ),
                        ]
                    });
                }
            }
            "brachy" => {
                for s in &p.brachy_setups {
                    if s.number != target.setup_number {
                        continue;
                    }
                    for c in &s.channels {
                        if c.number != target.channel_number {
                            continue;
                        }
                        matched = true;
                        pager.emit("Channel".to_string(), || {
                            vec![
                                val("Source", c.source_number.to_string(), "", "reference"),
                                val("Movement", c.movement.as_str(), "", ""),
                                val("Total time", attr(&c.total_time), "s", "one pulse for PDR"),
                                val("Length", attr(&c.length), "mm", "channel"),
                                val("Step size", attr(&c.step_size), "mm", "channel"),
                                val(
                                    "Final weight",
                                    attr(&c.final_cumulative_time_weight),
                                    "weight",
                                    "",
                                ),
                                val("Pulse interval", attr(&c.pulse_interval), "s", ""),
                                val("Pulses", count(&c.number_of_pulses), "count", ""),
                            ]
                        });
                        for x in &c.control_points {
                            pager.emit(format!("CP {}", x.index), || {
                                vec![
                                    val(
                                        "Time weight",
                                        attr(&x.cumulative_time_weight),
                                        "weight",
                                        "",
                                    ),
                                    val(
                                        "Relative position",
                                        g(x.relative_position),
                                        "mm",
                                        "distal-most possible position",
                                    ),
                                    val("3D position", vec(&x.position_3d), "mm", "patient"),
                                    val(
                                        "Orientation",
                                        vec(&x.orientation),
                                        "direction cosines",
                                        "patient",
                                    ),
                                ]
                            });
                        }
                    }
                }
            }
            "ion" => {
                for b in &p.ion_beams {
                    if b.number != target.beam_number {
                        continue;
                    }
                    matched = true;
                    pager.emit("Beam".to_string(), || {
                        vec![
                            val("Radiation", b.radiation_type.as_str(), "", ""),
                            val("Scan", b.scan_mode.as_str(), "", ""),
                            val(
                                "Dosimeter",
                                b.primary_dosimeter_unit.as_str(),
                                "",
                                "declared",
                            ),
                            val("Snout", b.snout_id.as_str(), "", "machine identifier"),
                            val(
                                "Virtual source-axis distances",
                                vec(&b.virtual_source_axis_distances),
                                "mm",
                                "IEC GANTRY X, Y",
                            ),
                        ]
                    });
                    for x in &b.control_points {
                        let c = &x.control_point;
                        pager.emit(format!("CP {}", c.index), || {
                            vec![
                                val(
                                    "Cumulative meterset",
                                    opt(c.cumulative_meterset_weight),
                                    "weight",
                                    "plan normalized",
                                ),
                                val(
                                    "Energy",
                                    opt(c.nominal_beam_energy),
                                    "MeV/nucleon",
                                    "nozzle entrance",
                                ),
                                val(
                                    "Gantry",
                                    format!(
                                        "{} {}",
                                        g(c.gantry_angle),
                                        c.gantry_rotation_direction
                                    ),
                                    "degree",
                                    "IEC GANTRY",
                                ),
                                val(
                                    "Gantry pitch",
                                    format!(
                                        "{} {}",
                                        attr(&x.gantry_pitch_angle),
                                        x.gantry_pitch_rotation_direction
                                    ),
                                    "degree",
                                    "IEC GANTRY",
                                ),
                                val(
                                    "BLD angle",
                                    format!(
                                        "{} {}",
                                        g(c.beam_limiting_device_angle),
                                        c.beam_limiting_device_rotation_direction
                                    ),
                                    "degree",
                                    "IEC BEAM LIMITING DEVICE",
                                ),
                                val(
                                    "Patient support",
                                    format!(
                                        "{} {}",
                                        g(c.patient_support_angle),
                                        c.patient_support_rotation_direction
                                    ),
                                    "degree",
                                    "IEC PATIENT SUPPORT",
                                ),
                                val(
                                    "Table pitch",
                                    format!(
                                        "{} {}",
                                        opt(c.table_top_pitch_angle),
                                        x.table_top_pitch_rotation_direction
                                    ),
                                    "degree",
                                    "IEC TABLE TOP",
                                ),
                                val(
                                    "Table roll",
                                    format!(
                                        "{} {}",
                                        opt(c.table_top_roll_angle),
                                        x.table_top_roll_rotation_direction
                                    ),
                                    "degree",
                                    "IEC TABLE TOP",
                                ),
                                val(
                                    "Table vertical/longitudinal/lateral",
                                    format!(
                                        "{}, {}, {}",
                                        opt(c.table_top_vertical_position),
                                        opt(c.table_top_longitudinal_position),
                                        opt(c.table_top_lateral_position)
                                    ),
                                    "mm",
                                    "IEC TABLE TOP",
                                ),
                                val("Isocentre", vec(&c.isocenter_position), "mm", "patient"),
                                val(
                                    "Snout position",
                                    attr(&x.snout_position),
                                    "mm",
                                    "from isocentre",
                                ),
                                val(
                                    "Meterset rate",
                                    attr(&x.meterset_rate),
                                    &format!("{}/min", b.primary_dosimeter_unit),
                                    "declared",
                                ),
                                val("Tune", x.scan_spot_tune_id.as_str(), "", "machine code"),
                                val("Paintings", count(&x.number_of_paintings), "count", ""),
                            ]
                        });
                        for rs in &x.range_shifter_settings {
                            pager.emit(
                                format!("CP {} / range shifter {}", c.index, rs.number),
                                || {
                                    vec![
                                        val("Setting", rs.setting.as_str(), "", "machine code"),
                                        val("Distance", attr(&rs.distance), "mm", "from isocentre"),
                                        val(
                                            "Water equivalent thickness",
                                            attr(&rs.water_equivalent_thickness),
                                            "mm",
                                            "",
                                        ),
                                    ]
                                },
                            );
                        }
                        for (j, w) in x.spot_weights.iter().enumerate() {
                            pager.emit(format!("CP {} / spot {j}", c.index), || {
                                let mut position = "unavailable".to_string();
                                if 2 * j + 1 < x.spot_positions.len() {
                                    position = format!(
                                        "{}, {}",
                                        format_float32(x.spot_positions[2 * j]),
                                        format_float32(x.spot_positions[2 * j + 1])
                                    );
                                }
                                vec![
                                    val("Position", position, "mm", "IEC GANTRY isocentric plane"),
                                    val("Meterset", format_float32(*w), "weight", ""),
                                ]
                            });
                        }
                        for d in &c.device_positions {
                            let n = d.positions.len() / 2;
                            for j in 0..n {
                                pager.emit(
                                    format!("CP {} / {} pair {}", c.index, d.device_type, j + 1),
                                    || {
                                        vec![val(
                                            "Banks 1, 2",
                                            format!(
                                                "{}, {}",
                                                g(d.positions[j]),
                                                g(d.positions[j + n])
                                            ),
                                            "mm",
                                            "IEC BEAM LIMITING DEVICE",
                                        )]
                                    },
                                );
                            }
                        }
                    }
                }
            }
            _ => return Err(Error::bare(ErrorKind::UnsupportedFeature)),
        }
        if !matched {
            return Err(path_error(
                ErrorKind::InvalidObject,
                "",
                "inspection entity was not found",
            ));
        }
        if offset > pager.page.total {
            return Err(path_error(
                ErrorKind::InvalidObject,
                "",
                "inspection offset is out of range",
            ));
        }
        Ok(pager.page)
    }
}
