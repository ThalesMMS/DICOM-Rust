use super::fixtures::{reference_synthetic_ct_dimensions, synthetic_ct_descriptor};
use super::{Error, Result};
use crate::fmath;
use crate::render::GeometryAffine;
use crate::render::view_state::{
    Interpolation, MprViewState, Projection, SLAB_MIP, SLAB_NONE, TransferLut, TransferSample,
    VIEW_STATE_CONTRACT_VERSION, VIEW_STATE_CONTRACT_VERSION_V4, VIEW_STATE_HEADER_SIZES,
    VR_MODE_DVR, ViewKind, ViewState, VrViewState, freeze_view_state, validate_view_state,
};
use crate::render::vr_transfer::{DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM, VrLightingMaterial};
use crate::textfmt::quote;
use std::collections::HashSet;
use std::f64::consts::{FRAC_PI_4, FRAC_PI_6};
pub const OPERATION_2D: &str = "2d";
pub const OPERATION_MPR: &str = "mpr";
pub const OPERATION_VR: &str = "vr";
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StateStep {
    pub id: String,
    pub operation: String,
    pub volume_generation: u64,
    pub view_generation: u64,
    pub presentation_generation: u64,
    pub slice_index: u32,
    pub view: Option<ViewState>,
}
pub fn frozen_mpr_sequence() -> Result<Vec<StateStep>> {
    freeze_sequence(build_mpr_sequence(1, 1)?)
}
pub fn frozen_vr_sequence() -> Result<Vec<StateStep>> {
    freeze_sequence(build_vr_sequence(1, 101)?)
}
pub fn frozen_mixed_sequence() -> Result<Vec<StateStep>> {
    let mpr = build_mpr_sequence(1, 201)?;
    let vr = build_vr_sequence(1, 301)?;
    let replacement_mpr = build_mpr_sequence(2, 401)?;
    let renamed = |sequence: &[StateStep], id: &str, name: &str| -> Result<StateStep> {
        let mut step = state_step_by_id(sequence, id)?;
        step.id = name.to_string();
        Ok(step)
    };
    let two_d = |id: &str, volume: u64, view: u64, slice: u32| StateStep {
        id: id.to_string(),
        operation: OPERATION_2D.to_string(),
        volume_generation: volume,
        view_generation: view,
        slice_index: slice,
        ..Default::default()
    };
    freeze_sequence(vec![
        two_d("mixed-01-2d-center", 1, 1, 150),
        renamed(&mpr, "mpr-01-axial", "mixed-02-mpr-axial")?,
        renamed(&vr, "vr-01-fit", "mixed-03-vr-fit")?,
        two_d("mixed-04-2d-jump", 1, 2, 37),
        renamed(&mpr, "mpr-04-oblique", "mixed-05-mpr-oblique")?,
        renamed(&vr, "vr-03-transfer", "mixed-06-vr-transfer")?,
        two_d("mixed-07-2d-replacement", 2, 1, 149),
        renamed(&replacement_mpr, "mpr-01-axial", "mixed-08-mpr-replacement")?,
    ])
}
fn state_step_by_id(sequence: &[StateStep], id: &str) -> Result<StateStep> {
    sequence
        .iter()
        .find(|step| step.id == id)
        .cloned()
        .ok_or_else(|| Error::other(format!("qualification: state step {} not found", quote(id))))
}
pub fn validate_state_sequence(sequence: &[StateStep]) -> Result<()> {
    if sequence.is_empty() {
        return Err(Error::other("qualification: empty state sequence"));
    }
    let mut seen: HashSet<&str> = HashSet::with_capacity(sequence.len());
    for (index, step) in sequence.iter().enumerate() {
        if step.id.is_empty() {
            return Err(Error::other(format!(
                "qualification: state step {index} has empty ID"
            )));
        }
        let id = quote(&step.id);
        if !seen.insert(&step.id) {
            return Err(Error::other(format!(
                "qualification: duplicate state step ID {id}"
            )));
        }
        if step.volume_generation == 0 || step.view_generation == 0 {
            return Err(Error::other(format!(
                "qualification: state step {id} has zero generation"
            )));
        }
        match step.operation.as_str() {
            OPERATION_2D => {
                if step.view.is_some() {
                    return Err(Error::other(format!(
                        "qualification: 2D step {id} unexpectedly has ViewState"
                    )));
                }
            }
            OPERATION_MPR | OPERATION_VR => {
                let Some(view) = &step.view else {
                    return Err(Error::other(format!(
                        "qualification: render step {id} lacks ViewState"
                    )));
                };
                validate_view_state(view).map_err(|e| {
                    Error::from(e).prefixed(format!("qualification: state step {id}"))
                })?;
                if view.volume_generation != step.volume_generation
                    || view.view_generation != step.view_generation
                    || view.presentation_generation != step.presentation_generation
                {
                    return Err(Error::other(format!(
                        "qualification: state step {id} generation mismatch"
                    )));
                }
                if step.operation == OPERATION_MPR && view.kind != ViewKind::MPR {
                    return Err(Error::other(format!(
                        "qualification: MPR step {id} has wrong view kind"
                    )));
                }
                if step.operation == OPERATION_VR && view.kind != ViewKind::VR {
                    return Err(Error::other(format!(
                        "qualification: VR step {id} has wrong view kind"
                    )));
                }
            }
            other => {
                return Err(Error::other(format!(
                    "qualification: state step {id} operation {}",
                    quote(other)
                )));
            }
        }
    }
    Ok(())
}
type V3 = [f64; 3];
fn build_mpr_sequence(
    volume_generation: u64,
    first_view_generation: u64,
) -> Result<Vec<StateStep>> {
    let descriptor =
        synthetic_ct_descriptor(reference_synthetic_ct_dimensions(), volume_generation)?;
    let affine = descriptor.index_to_patient_lps;
    let x_step = affine_vector(&affine, 1.0, 0.0, 0.0);
    let y_step = affine_vector(&affine, 0.0, 1.0, 0.0);
    let z_step = affine_vector(&affine, 0.0, 0.0, 1.0);
    let center = affine_point(
        &affine,
        f64::from(descriptor.dimensions[0] - 1) / 2.0,
        f64::from(descriptor.dimensions[1] - 1) / 2.0,
        f64::from(descriptor.dimensions[2] - 1) / 2.0,
    );
    const OUTPUT: u32 = 256;
    let half = f64::from(OUTPUT - 1) / 2.0;
    let origin_for =
        |u: V3, v: V3| -> V3 { subtract(subtract(center, scale(u, half)), scale(v, half)) };
    let oblique_u = add(
        scale(x_step, fmath::cos(FRAC_PI_6)),
        scale(z_step, fmath::sin(FRAC_PI_6)),
    );
    let oblique_v = y_step;
    let oblique_normal = unit_cross(oblique_u, oblique_v)?;
    let zero = [0.0; 3];
    #[allow(clippy::type_complexity)]
    let specs: [(u64, V3, V3, V3, Interpolation, i64, V3, f64, f64, u64); 5] = [
        (
            first_view_generation,
            origin_for(x_step, y_step),
            x_step,
            y_step,
            Interpolation::LINEAR,
            SLAB_NONE,
            zero,
            0.0,
            0.0,
            1,
        ),
        (
            first_view_generation + 1,
            origin_for(y_step, z_step),
            y_step,
            z_step,
            Interpolation::LINEAR,
            SLAB_NONE,
            zero,
            0.0,
            0.0,
            1,
        ),
        (
            first_view_generation + 2,
            origin_for(x_step, z_step),
            x_step,
            z_step,
            Interpolation::LINEAR,
            SLAB_NONE,
            zero,
            0.0,
            0.0,
            1,
        ),
        (
            first_view_generation + 3,
            origin_for(oblique_u, oblique_v),
            oblique_u,
            oblique_v,
            Interpolation::CUBIC,
            SLAB_NONE,
            zero,
            0.0,
            0.0,
            2,
        ),
        (
            first_view_generation + 4,
            origin_for(oblique_u, oblique_v),
            oblique_u,
            oblique_v,
            Interpolation::LINEAR,
            SLAB_MIP,
            oblique_normal,
            12.0,
            0.5,
            2,
        ),
    ];
    let ids = [
        "mpr-01-axial",
        "mpr-02-sagittal",
        "mpr-03-coronal",
        "mpr-04-oblique",
        "mpr-05-slab-mip",
    ];
    let mut result = Vec::with_capacity(specs.len());
    for (spec, id) in specs.into_iter().zip(ids) {
        let (
            generation,
            origin,
            u,
            v,
            interpolation,
            slab,
            slab_axis,
            thickness,
            spacing,
            presentation,
        ) = spec;
        let state = ViewState {
            contract_version: VIEW_STATE_CONTRACT_VERSION,
            struct_size: VIEW_STATE_HEADER_SIZES[VIEW_STATE_CONTRACT_VERSION as usize],
            kind: ViewKind::MPR,
            volume_generation,
            view_generation: generation,
            presentation_generation: presentation,
            output_width: OUTPUT,
            output_height: OUTPUT,
            mpr: MprViewState {
                pixel_origin_lps: origin,
                pixel_step_u_lps: u,
                pixel_step_v_lps: v,
                interpolation,
                slab_mode: slab,
                slab_axis_lps: slab_axis,
                slab_thickness_mm: thickness,
                slab_sample_spacing_mm: spacing,
                outside_value: -1024.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let frozen = freeze_view_state(&state)?;
        result.push(StateStep {
            id: id.to_string(),
            operation: OPERATION_MPR.to_string(),
            volume_generation: frozen.volume_generation,
            view_generation: frozen.view_generation,
            presentation_generation: frozen.presentation_generation,
            slice_index: 0,
            view: Some(frozen),
        });
    }
    Ok(result)
}
fn build_vr_sequence(volume_generation: u64, first_view_generation: u64) -> Result<Vec<StateStep>> {
    let descriptor =
        synthetic_ct_descriptor(reference_synthetic_ct_dimensions(), volume_generation)?;
    let affine = descriptor.index_to_patient_lps;
    let center = affine_point(
        &affine,
        f64::from(descriptor.dimensions[0] - 1) / 2.0,
        f64::from(descriptor.dimensions[1] - 1) / 2.0,
        f64::from(descriptor.dimensions[2] - 1) / 2.0,
    );
    let radius = 320.0f64;
    let cameras: [V3; 4] = [
        [center[0], center[1], center[2] + radius * 2.5],
        [
            center[0] + radius * 1.25,
            center[1],
            center[2] + radius * 2.1,
        ],
        [
            center[0] - radius * 0.8,
            center[1] + radius * 0.6,
            center[2] + radius * 2.2,
        ],
        [center[0], center[1], center[2] + radius * 2.5],
    ];
    let ids = ["vr-01-fit", "vr-02-orbit", "vr-03-transfer", "vr-04-settle"];
    let sample = |r: f32, g: f32, b: f32, a: f32| TransferSample { r, g, b, a };
    let mut result = Vec::with_capacity(cameras.len());
    for (index, (position, id)) in cameras.into_iter().zip(ids).enumerate() {
        let presentation = if index >= 2 { 2 } else { 1 };
        let material = if index >= 2 {
            VrLightingMaterial::glossy_vascular()
        } else {
            VrLightingMaterial::basic_clinical()
        };
        let state = ViewState {
            contract_version: VIEW_STATE_CONTRACT_VERSION_V4,
            struct_size: VIEW_STATE_HEADER_SIZES[VIEW_STATE_CONTRACT_VERSION_V4 as usize],
            kind: ViewKind::VR,
            volume_generation,
            view_generation: first_view_generation + index as u64,
            presentation_generation: presentation,
            output_width: 384,
            output_height: 384,
            vr: VrViewState {
                position_lps: position,
                focal_point_lps: center,
                view_up_lps: [0.0, 1.0, 0.0],
                projection: Projection::PERSPECTIVE,
                vertical_fov_radians: FRAC_PI_4,
                near_mm: 0.1,
                far_mm: 5000.0,
                mode: VR_MODE_DVR,
                sample_distance_mm: 0.7,
                opacity_unit_distance_mm: DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM,
                image_sample_distance: 1.0,
                shading_enabled: 1,
                ambient: material.ambient,
                diffuse: material.diffuse,
                specular: material.specular,
                specular_power: material.specular_power,
                gradient_opacity_scale: 220.0,
                background_rgba: [0.0, 0.0, 0.0, 1.0],
                transfer_lut: TransferLut {
                    domain_min: -1024.0,
                    domain_max: 3071.0,
                    samples: vec![
                        sample(0.0, 0.0, 0.0, 0.0),
                        sample(0.18, 0.08, 0.04, 0.08),
                        sample(0.72, 0.49, 0.32, 0.34),
                        sample(1.0, 0.92, 0.82, 0.82),
                    ],
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let frozen = freeze_view_state(&state)?;
        result.push(StateStep {
            id: id.to_string(),
            operation: OPERATION_VR.to_string(),
            volume_generation: frozen.volume_generation,
            view_generation: frozen.view_generation,
            presentation_generation: frozen.presentation_generation,
            slice_index: 0,
            view: Some(frozen),
        });
    }
    Ok(result)
}
fn freeze_sequence(source: Vec<StateStep>) -> Result<Vec<StateStep>> {
    source
        .into_iter()
        .map(|mut step| {
            if let Some(view) = &step.view {
                step.view = Some(freeze_view_state(view)?);
            }
            Ok(step)
        })
        .collect()
}
fn affine_point(affine: &GeometryAffine, x: f64, y: f64, z: f64) -> V3 {
    let a = &affine.0;
    [
        a[0] * x + a[1] * y + a[2] * z + a[3],
        a[4] * x + a[5] * y + a[6] * z + a[7],
        a[8] * x + a[9] * y + a[10] * z + a[11],
    ]
}
fn affine_vector(affine: &GeometryAffine, x: f64, y: f64, z: f64) -> V3 {
    let a = &affine.0;
    [
        a[0] * x + a[1] * y + a[2] * z,
        a[4] * x + a[5] * y + a[6] * z,
        a[8] * x + a[9] * y + a[10] * z,
    ]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn subtract(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(value: V3, factor: f64) -> V3 {
    [value[0] * factor, value[1] * factor, value[2] * factor]
}
fn unit_cross(a: V3, b: V3) -> Result<V3> {
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let length = fmath::hypot(fmath::hypot(cross[0], cross[1]), cross[2]);
    if length == 0.0 || !length.is_finite() {
        return Err(Error::other(
            "qualification: cannot normalize degenerate cross product",
        ));
    }
    Ok(scale(cross, 1.0 / length))
}
