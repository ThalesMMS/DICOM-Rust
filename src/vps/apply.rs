use super::standard::{
    has_standard_state, is_mpr_sop_class, validate_classification_transfer, validate_standard_state,
};
use super::{
    ALPHA_TRANSFER_IDENTITY, ALPHA_TRANSFER_NONE, ALPHA_TRANSFER_TABLE, Camera,
    ClassificationComponent, CroppingSpecification, Error, Input, MprGeometry,
    PRESENTATION_LUT_INVERSE, RENDERING_AVERAGE_IP, RENDERING_MAXIMUM_IP, RENDERING_MINIMUM_IP,
    RENDERING_VOLUME_RENDERED, RGB_TRANSFER_TABLE, Shading, State, Voi, VolumeRenderGeometry,
    preset_name,
};
use crate::render::vr_gradient_opacity::VrGradientOpacity;
use crate::render::vr_transfer::{
    DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM, TfAlphaStop, TfColorStop, VrMode, VrPreset,
    VrTransferFunction, default_vr_preset, vr_preset_by_name,
};
use crate::textfmt::quote;
#[derive(Debug, Clone, Default)]
pub struct AppliedState {
    pub inputs: Vec<Input>,
    pub preset: VrPreset,
    pub camera: Camera,
    pub global_crop_specification_numbers: Vec<i64>,
    pub cropping_specifications: Vec<CroppingSpecification>,
    pub mpr_geometry: Option<MprGeometry>,
    pub volume_render_geometry: Option<VolumeRenderGeometry>,
    pub voi: Option<Voi>,
    pub shading: Option<Shading>,
}
pub fn apply(state: Option<&State>) -> Result<AppliedState, Error> {
    let Some(state) = state else {
        return Err(Error::invalid("state is nil"));
    };
    if has_standard_state(state) {
        validate_standard_state(state)?;
    }
    let preset = applied_preset(state)?;
    let voi = state.inputs.iter().find_map(|input| input.voi);
    Ok(AppliedState {
        inputs: state.inputs.clone(),
        preset,
        camera: state.camera,
        global_crop_specification_numbers: state.global_crop_specification_numbers.clone(),
        cropping_specifications: state.cropping_specifications.clone(),
        mpr_geometry: state.mpr_geometry.clone(),
        volume_render_geometry: state.volume_render_geometry.clone(),
        voi,
        shading: state.shading.clone(),
    })
}
fn applied_preset(state: &State) -> Result<VrPreset, Error> {
    if !has_standard_state(state) {
        return Ok(vr_preset_by_name(preset_name(&state.render_preset_name))
            .unwrap_or_else(default_vr_preset));
    }
    let mut method = state.rendering_method.as_str();
    if state.mpr_geometry.is_some()
        && let Some(first) = state.inputs.first()
    {
        method = first.rendering_method.as_str();
    }
    let mut preset = default_vr_preset();
    preset.name = "DICOM VPS".into();
    if let Some(display) = &state.display
        && display.classification.len() == 1
    {
        let component = &display.classification[0];
        preset.tf =
            classification_transfer_function(component, is_mpr_sop_class(&state.sop_class_uid))?;
        let description = component.description.trim_matches(char::is_whitespace);
        if !description.is_empty() {
            preset.name = description.to_string();
        }
        preset.gradient_opacity_scale = 0.0;
        preset.gradient_opacity = VrGradientOpacity::default();
    }
    preset.mode = match method {
        "" | RENDERING_VOLUME_RENDERED => VrMode::DVR,
        RENDERING_MAXIMUM_IP => VrMode::MIP,
        RENDERING_MINIMUM_IP => VrMode::MIN_IP,
        RENDERING_AVERAGE_IP => VrMode::AVERAGE,
        other => {
            return Err(Error::unsupported(format!(
                "Rendering Method {}",
                quote(other)
            )));
        }
    };
    if state
        .display
        .as_ref()
        .is_some_and(|d| d.presentation_lut_shape == PRESENTATION_LUT_INVERSE)
    {
        preset.inverse = true;
    }
    if state.shading.is_some() {
        preset.shading_default = true;
    }
    Ok(preset)
}
fn classification_transfer_function(
    component: &ClassificationComponent,
    mpr: bool,
) -> Result<VrTransferFunction, Error> {
    validate_classification_transfer(component, mpr)?;
    let table = component.rgb_transfer_function == RGB_TRANSFER_TABLE;
    let count = if table {
        usize::from(component.palette_descriptor[0])
    } else {
        2
    };
    let denominator = f64::from(u16::MAX);
    let mut colors = vec![TfColorStop::default(); count];
    let mut alpha = vec![TfAlphaStop::default(); count];
    for index in 0..count {
        let position = index as f64 / (count as f64 - 1.0);
        colors[index].hu = position;
        alpha[index].hu = position;
        if table {
            colors[index].r = f64::from(component.red_palette[index]) / denominator;
            colors[index].g = f64::from(component.green_palette[index]) / denominator;
            colors[index].b = f64::from(component.blue_palette[index]) / denominator;
        } else {
            colors[index].r = position;
            colors[index].g = position;
            colors[index].b = position;
        }
        match component.alpha_transfer_function.as_str() {
            "" | ALPHA_TRANSFER_NONE => alpha[index].a = 1.0,
            ALPHA_TRANSFER_IDENTITY => alpha[index].a = position,
            ALPHA_TRANSFER_TABLE => {
                alpha[index].a = f64::from(component.alpha_palette[index]) / denominator;
            }
            _ => {}
        }
    }
    Ok(VrTransferFunction::new_normalized(
        &colors,
        &alpha,
        DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM,
    ))
}
