use super::tensor::{
    TENSOR_NUMERICAL, TENSOR_VALID, TensorResult, TensorSource, budget_add, tensor_error,
    tensor_grid,
};
use super::{Error, Kind};
use crate::codes::dcm;
use crate::derivation::{Code as DerivationCode, Derivation, Source};
use crate::derivedio as dio;
use crate::numtext::format_float;
use crate::object::{DataSet, File, Tag, Value, is_valid_uid};
use crate::parametricmap::{self, Code, Document, DocumentFrame, Reference};
use crate::render::SliceGeometry;
use std::collections::BTreeMap;
pub mod tensor_map {
    pub const FA: u8 = 0;
    pub const MD: u8 = 1;
    pub const XX: u8 = 2;
    #[doc(hidden)]
    pub const YY: u8 = 3;
    pub const ZZ: u8 = 4;
    pub const XY: u8 = 5;
    pub const XZ: u8 = 6;
    pub const YZ: u8 = 7;
}
#[derive(Debug, Clone, Default)]
pub struct TensorMapOptions {
    pub identification: Document,
    pub max_bytes: u64,
}
const TAG_PER_FRAME: Tag = Tag::new(0x5200, 0x9230);
const TAG_DERIVATION_IMAGE_SEQ: Tag = Tag::new(0x0008, 0x9124);
const TAG_REFERENCED_SERIES_SEQ: Tag = Tag::new(0x0008, 0x1115);
const TAG_REFERENCED_INSTANCE_SEQ: Tag = Tag::new(0x0008, 0x114a);
fn code(c: crate::codes::Code<'_>) -> Code {
    Code::new(c.value, "DCM", c.meaning)
}
fn finite(x: f64) -> bool {
    x.is_finite()
}
fn foreign(e: impl std::fmt::Display) -> Error {
    Error::with_text(Kind::Other, e.to_string())
}
pub fn write_tensor_map(
    result: &TensorResult,
    quantity: u8,
    o: &TensorMapOptions,
) -> Result<File, Error> {
    let g = result.geometry();
    let n = tensor_grid(g)?;
    if n != result.voxels.len() || quantity > tensor_map::YZ {
        return Err(tensor_error("map quantity or result size"));
    }
    let max_bytes = if o.max_bytes == 0 {
        512 << 20
    } else {
        o.max_bytes
    };
    let mut budget = 0u64;
    let planes = g.size[2] as u64;
    if n as u64 > u64::MAX / 24
        || planes > u64::MAX / 8192
        || !budget_add(&mut budget, n as u64 * 24, max_bytes)
        || !budget_add(&mut budget, planes * 8192, max_bytes)
    {
        return Err(tensor_error("map byte budget"));
    }
    let sources = result.sources();
    if sources.len() != result.encodings().len() || sources.len() < 7 {
        return Err(tensor_error("map missing fit provenance"));
    }
    let mut series: BTreeMap<String, BTreeMap<String, TensorSource>> = BTreeMap::new();
    for volume in sources {
        if volume.len() as i64 != g.size[2] {
            return Err(tensor_error(
                "every volume needs source for every output plane",
            ));
        }
        for s in volume {
            if !is_valid_uid(s.study_instance_uid.as_bytes())
                || s.study_instance_uid != o.identification.study_instance_uid
                || !is_valid_uid(s.series_instance_uid.as_bytes())
                || !is_valid_uid(s.sop_class_uid.as_bytes())
                || !is_valid_uid(s.sop_instance_uid.as_bytes())
                || s.frame_number < 0
            {
                return Err(tensor_error("invalid map source"));
            }
            if !budget_add(&mut budget, 1024, max_bytes) {
                return Err(tensor_error("source map byte budget"));
            }
            let instances = series.entry(s.series_instance_uid.clone()).or_default();
            if let Some(prev) = instances.get(&s.sop_instance_uid)
                && prev.sop_class_uid != s.sop_class_uid
            {
                return Err(tensor_error("source SOP class conflict"));
            }
            instances.insert(s.sop_instance_uid.clone(), s.clone());
        }
    }
    let mut doc = o.identification.clone();
    if !is_valid_uid(doc.sop_instance_uid.as_bytes())
        || !is_valid_uid(doc.study_instance_uid.as_bytes())
        || !is_valid_uid(doc.series_instance_uid.as_bytes())
    {
        return Err(tensor_error("map identification UID"));
    }
    doc.rows = g.size[1];
    doc.columns = g.size[0];
    doc.frame_of_reference_uid = g.frame_of_reference_uid.clone();
    doc.content_qualification = "RESEARCH".into();
    doc.modality = "MR".into();
    doc.frames = Vec::new();
    let codes = [
        dcm::FractionalAnisotropy,
        dcm::MeanDiffusivity,
        dcm::VolumetricDiffusionDxxComponent,
        dcm::VolumetricDiffusionDyyComponent,
        dcm::VolumetricDiffusionDzzComponent,
        dcm::VolumetricDiffusionDxyComponent,
        dcm::VolumetricDiffusionDxzComponent,
        dcm::VolumetricDiffusionDyzComponent,
    ];
    doc.quantity = code(codes[quantity as usize]);
    doc.units = if quantity == tensor_map::FA {
        Code::new("1", "UCUM", "no units")
    } else {
        Code::new("mm2/s", "UCUM", "square millimeter per second")
    };
    doc.derivation_code = code(dcm::DiffusionImageAnalysis);
    doc.derived_pixel_contrast = "QUANTITY".into();
    doc.lut_label = "DTI".into();
    let opt = result.options();
    let counts: Vec<String> = result.counts.iter().map(i64::to_string).collect();
    doc.derivation_description = format!(
        "DTI unweighted anchored log-linear OLS; LPS; mean b0; floor={} min={} negative={} tol={} max={}; volumes={} dup={} clamp={} invalid=[{}]",
        format_float(opt.noise_floor, 'g', -1),
        format_float(opt.min_signal, 'g', -1),
        opt.negative_policy,
        format_float(opt.negative_tolerance, 'g', -1),
        format_float(opt.max_diffusivity, 'g', -1),
        result.encodings().len(),
        result.duplicate_encodings,
        result.clamped,
        counts.join(" ")
    );
    let keys: Vec<String> = series.keys().cloned().collect();
    doc.source_series_instance_uid = keys[0].clone();
    let row = crate::render::Vec3::new(g.direction[0][0], g.direction[1][0], g.direction[2][0]);
    let col = crate::render::Vec3::new(g.direction[0][1], g.direction[1][1], g.direction[2][1]);
    let normal = row.cross(col);
    let plane = (g.size[0] * g.size[1]) as usize;
    for z in 0..g.size[2] as usize {
        let mut values = vec![0f32; plane];
        for (i, value_out) in values.iter_mut().enumerate() {
            let v = &result.voxels[z * plane + i];
            if v.status > TENSOR_NUMERICAL {
                return Err(tensor_error("unknown voxel status"));
            }
            let mut value = f64::NAN;
            if v.status == TENSOR_VALID {
                value = match quantity {
                    tensor_map::FA => v.fa,
                    tensor_map::MD => v.md,
                    q => v.tensor[q as usize - 2],
                };
                if !finite(value)
                    || !finite(f64::from(value as f32))
                    || quantity == tensor_map::FA && !(0.0..=1.0).contains(&value)
                    || quantity == tensor_map::MD && value < 0.0
                {
                    return Err(tensor_error("invalid finite map value"));
                }
            }
            *value_out = value as f32;
        }
        let mut frame = DocumentFrame {
            geometry: SliceGeometry {
                origin: g.origin.add(normal.scale(z as f64 * g.spacing.z)),
                row_dir: row,
                col_dir: col,
                normal,
                row_spacing: g.spacing.y,
                col_spacing: g.spacing.x,
                rows: g.size[1],
                columns: g.size[0],
            },
            values,
            sources: Vec::new(),
        };
        for volume in sources {
            let s = &volume[z];
            frame.sources.push(Reference {
                sop_class_uid: s.sop_class_uid.clone(),
                sop_instance_uid: s.sop_instance_uid.clone(),
                frames: Vec::new(),
            });
        }
        doc.frames.push(frame);
    }
    let mut file = parametricmap::write(&doc).map_err(foreign)?;
    if file
        .dataset
        .get_string(Tag::new(0x0020, 0x0052))
        .unwrap_or_default()
        != g.frame_of_reference_uid
    {
        return Err(tensor_error("inherited common frame mismatch"));
    }
    if file
        .dataset
        .get_string(Tag::new(0x0020, 0x000d))
        .unwrap_or_default()
        != doc.study_instance_uid
    {
        return Err(tensor_error("inherited common study mismatch"));
    }
    let mut frames: Vec<DataSet> = match file.dataset.get(TAG_PER_FRAME).map(|e| &e.value) {
        Some(Value::Sequence(items)) if items.len() as i64 == g.size[2] => items.clone(),
        _ => return Err(tensor_error("map functional groups")),
    };
    let purpose = DerivationCode {
        value: "121322".into(),
        scheme: "DCM".into(),
        meaning: "Source image for image processing operation".into(),
    };
    for (z, frame) in frames.iter_mut().enumerate() {
        let mut d = Derivation {
            description: doc.derivation_description.clone(),
            codes: vec![DerivationCode {
                value: doc.derivation_code.value.clone(),
                scheme: doc.derivation_code.scheme.clone(),
                meaning: doc.derivation_code.meaning.clone(),
            }],
            sources: Vec::new(),
        };
        for volume in sources {
            let s = &volume[z];
            d.sources.push(Source {
                sop_class_uid: s.sop_class_uid.clone(),
                sop_instance_uid: s.sop_instance_uid.clone(),
                frames: if s.frame_number > 0 {
                    vec![s.frame_number]
                } else {
                    Vec::new()
                },
                purpose: Some(purpose.clone()),
            });
        }
        let item = d.item().map_err(foreign)?;
        let item = dio::data_set(item.elements);
        let element = dio::seq(TAG_DERIVATION_IMAGE_SEQ, vec![item]);
        frame
            .elements
            .retain(|e| e.tag() != TAG_DERIVATION_IMAGE_SEQ);
        frame.elements.push(element);
        frame
            .elements
            .sort_by_key(|e| (e.tag().group, e.tag().element));
    }
    file.dataset.put(dio::seq(TAG_PER_FRAME, frames));
    let mut series_items = Vec::with_capacity(keys.len());
    for key in &keys {
        let refs: Vec<DataSet> = series[key]
            .values()
            .map(|s| {
                dio::data_set(vec![
                    dio::ui(Tag::new(0x0008, 0x1150), &s.sop_class_uid),
                    dio::ui(Tag::new(0x0008, 0x1155), &s.sop_instance_uid),
                ])
            })
            .collect();
        series_items.push(dio::data_set(vec![
            dio::ui(Tag::new(0x0020, 0x000e), key),
            dio::seq(TAG_REFERENCED_INSTANCE_SEQ, refs),
        ]));
    }
    file.dataset
        .put(dio::seq(TAG_REFERENCED_SERIES_SEQ, series_items));
    Ok(file)
}
