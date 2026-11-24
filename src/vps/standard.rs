use super::*;
use crate::siemens::trim_space;
const TAG_CROPPING_SPECIFICATION_INDEX: Tag = Tag::new(0x0070, 0x1205);
const TAG_RENDERING_METHOD: Tag = Tag::new(0x0070, 0x120D);
const TAG_GLOBAL_CROPPING_SPECIFICATION_INDEX: Tag = Tag::new(0x0070, 0x120C);
const TAG_VOLUME_CROPPING_SEQUENCE: Tag = Tag::new(0x0070, 0x1301);
const TAG_VOLUME_CROPPING_METHOD: Tag = Tag::new(0x0070, 0x1302);
const TAG_BOUNDING_BOX_CROP: Tag = Tag::new(0x0070, 0x1303);
const TAG_OBLIQUE_CROPPING_PLANE_SEQUENCE: Tag = Tag::new(0x0070, 0x1304);
const TAG_CROPPING_SPECIFICATION_NUMBER: Tag = Tag::new(0x0070, 0x1309);
const TAG_MPR_STYLE: Tag = Tag::new(0x0070, 0x1501);
const TAG_MPR_THICKNESS_TYPE: Tag = Tag::new(0x0070, 0x1502);
const TAG_MPR_SLAB_THICKNESS: Tag = Tag::new(0x0070, 0x1503);
const TAG_MPR_TOP_LEFT: Tag = Tag::new(0x0070, 0x1505);
const TAG_MPR_WIDTH_DIRECTION: Tag = Tag::new(0x0070, 0x1507);
const TAG_MPR_WIDTH: Tag = Tag::new(0x0070, 0x1508);
const TAG_MPR_HEIGHT_DIRECTION: Tag = Tag::new(0x0070, 0x1511);
const TAG_MPR_HEIGHT: Tag = Tag::new(0x0070, 0x1512);
const TAG_RENDER_PROJECTION: Tag = Tag::new(0x0070, 0x1602);
const TAG_VIEWPOINT_POSITION: Tag = Tag::new(0x0070, 0x1603);
const TAG_VIEWPOINT_LOOK_AT: Tag = Tag::new(0x0070, 0x1604);
const TAG_VIEWPOINT_UP: Tag = Tag::new(0x0070, 0x1605);
const TAG_RENDER_FIELD_OF_VIEW: Tag = Tag::new(0x0070, 0x1606);
const TAG_SHADING_STYLE: Tag = Tag::new(0x0070, 0x1701);
const TAG_AMBIENT_REFLECTION: Tag = Tag::new(0x0070, 0x1702);
const TAG_LIGHT_DIRECTION: Tag = Tag::new(0x0070, 0x1703);
const TAG_DIFFUSE_REFLECTION: Tag = Tag::new(0x0070, 0x1704);
const TAG_SPECULAR_REFLECTION: Tag = Tag::new(0x0070, 0x1705);
const TAG_SHININESS: Tag = Tag::new(0x0070, 0x1706);
const TAG_CLASSIFICATION_SEQUENCE: Tag = Tag::new(0x0070, 0x1801);
const TAG_COMPONENT_TYPE: Tag = Tag::new(0x0070, 0x1802);
const TAG_COMPONENT_INPUT_SEQUENCE: Tag = Tag::new(0x0070, 0x1803);
const TAG_PRESENTATION_INPUT_INDEX: Tag = Tag::new(0x0070, 0x1804);
const TAG_COMPOSITOR_SEQUENCE: Tag = Tag::new(0x0070, 0x1805);
const TAG_PRESENTATION_CREATION_DATE: Tag = Tag::new(0x0070, 0x0082);
const TAG_PRESENTATION_CREATION_TIME: Tag = Tag::new(0x0070, 0x0083);
const TAG_ANATOMIC_REGION_SEQUENCE: Tag = Tag::new(0x0008, 0x2218);
const TAG_VIEW_CODE_SEQUENCE: Tag = Tag::new(0x0054, 0x0220);
const TAG_IMAGE_LATERALITY: Tag = Tag::new(0x0020, 0x0062);
const TAG_VOLUME_STREAM_SEQUENCE: Tag = Tag::new(0x0070, 0x1A08);
const TAG_RGBA_TRANSFER_FUNCTION_DESCRIPTION: Tag = Tag::new(0x0070, 0x1A09);
const TAG_WINDOW_CENTER: Tag = Tag::new(0x0028, 0x1050);
const TAG_WINDOW_WIDTH: Tag = Tag::new(0x0028, 0x1051);
const TAG_VOI_LUT_FUNCTION: Tag = Tag::new(0x0028, 0x1056);
const TAG_VOI_LUT_SEQUENCE: Tag = Tag::new(0x0028, 0x3010);
const TAG_PIXEL_PRESENTATION: Tag = Tag::new(0x0008, 0x9205);
const TAG_PRESENTATION_LUT_SHAPE: Tag = Tag::new(0x2050, 0x0020);
const TAG_RGB_TRANSFER_FUNCTION: Tag = Tag::new(0x0028, 0x140F);
const TAG_ALPHA_TRANSFER_FUNCTION: Tag = Tag::new(0x0028, 0x1410);
const TAG_RED_PALETTE_DESCRIPTOR: Tag = Tag::new(0x0028, 0x1101);
const TAG_GREEN_PALETTE_DESCRIPTOR: Tag = Tag::new(0x0028, 0x1102);
const TAG_BLUE_PALETTE_DESCRIPTOR: Tag = Tag::new(0x0028, 0x1103);
const TAG_ALPHA_PALETTE_DESCRIPTOR: Tag = Tag::new(0x0028, 0x1104);
const TAG_RED_PALETTE_DATA: Tag = Tag::new(0x0028, 0x1201);
const TAG_GREEN_PALETTE_DATA: Tag = Tag::new(0x0028, 0x1202);
const TAG_BLUE_PALETTE_DATA: Tag = Tag::new(0x0028, 0x1203);
const TAG_ALPHA_PALETTE_DATA: Tag = Tag::new(0x0028, 0x1204);
const TAG_ICC_PROFILE: Tag = Tag::new(0x0028, 0x2000);
const TAG_COLOR_SPACE: Tag = Tag::new(0x0028, 0x2002);
const TAG_REFERENCED_SPATIAL_REGISTRATION_SEQUENCE: Tag = Tag::new(0x0070, 0x0404);
const MAX_CLASSIFICATION_PALETTE_ENTRIES: i64 = 4096;
pub(super) fn has_standard_state(state: &State) -> bool {
    state.mpr_geometry.is_some() || state.volume_render_geometry.is_some()
}
pub(super) fn has_standard_geometry(obj: &Object, sop_class_uid: &str) -> bool {
    if is_mpr_sop_class(sop_class_uid) {
        return obj.has(TAG_MPR_STYLE)
            || obj.has(TAG_MPR_TOP_LEFT)
            || obj.has(TAG_MPR_WIDTH_DIRECTION);
    }
    obj.has(TAG_RENDER_PROJECTION)
        || obj.has(TAG_VIEWPOINT_POSITION)
        || obj.has(TAG_VIEWPOINT_LOOK_AT)
}
pub(super) fn is_mpr_sop_class(uid: &str) -> bool {
    uid == GRAYSCALE_PLANAR_MPR_VOLUMETRIC_PRESENTATION_STATE_STORAGE
        || uid == COMPOSITING_PLANAR_MPR_VOLUMETRIC_PRESENTATION_STATE_STORAGE
}
fn vec_values(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
pub(super) fn write_standard(state: &State) -> Result<File, Error> {
    validate_standard_state(state)?;
    let identification = state
        .identification
        .elements(DEFAULT_CONTENT_LABEL)
        .map_err(Error::invalid)?;
    let mut date = state.creation_date.clone();
    let mut clock = state.creation_time.clone();
    if date.is_empty() || clock.is_empty() {
        let now = chrono::Local::now();
        if date.is_empty() {
            date = now.format("%Y%m%d").to_string();
        }
        if clock.is_empty() {
            clock = now.format("%H%M%S").to_string();
        }
    }
    let mut elements = vec![
        d::ui(d::TAG_SOP_CLASS_UID, &state.sop_class_uid),
        d::ui(d::TAG_SOP_INSTANCE_UID, &state.sop_instance_uid),
        d::cs(d::TAG_MODALITY, "PR"),
        d::ui(d::TAG_STUDY_INSTANCE_UID, &state.study_instance_uid),
        d::ui(d::TAG_SERIES_INSTANCE_UID, &state.series_instance_uid),
        d::ui(d::TAG_FRAME_OF_REFERENCE_UID, &state.frame_of_reference_uid),
        input_set_sequence(&state.inputs),
        standard_input_sequence(&state.inputs),
        d::str_elem(TAG_PRESENTATION_CREATION_DATE, Vr::DA, &date),
        d::str_elem(TAG_PRESENTATION_CREATION_TIME, Vr::TM, &clock),
        d::seq(TAG_ANATOMIC_REGION_SEQUENCE, Vec::new()),
        d::seq(TAG_VIEW_CODE_SEQUENCE, Vec::new()),
        d::cs(TAG_IMAGE_LATERALITY, ""),
    ];
    elements.extend(identification);
    if !is_mpr_sop_class(&state.sop_class_uid) {
        elements.push(d::seq(TAG_COMPOSITOR_SEQUENCE, Vec::new()));
    }
    if state.global_crop_specification_numbers.is_empty() {
        elements.push(d::cs(TAG_GLOBAL_CROP, "NO"));
    } else {
        elements.push(d::cs(TAG_GLOBAL_CROP, "YES"));
        elements.push(us_ints(
            TAG_GLOBAL_CROPPING_SPECIFICATION_INDEX,
            &state.global_crop_specification_numbers,
        ));
    }
    if !state.cropping_specifications.is_empty() {
        elements.push(cropping_sequence(&state.cropping_specifications));
    }
    if let Some(g) = &state.mpr_geometry {
        elements.extend(encode_mpr_geometry(g));
    } else if let Some(g) = &state.volume_render_geometry {
        elements.extend(encode_volume_geometry(g));
        elements.push(d::cs(TAG_RENDERING_METHOD, &state.rendering_method));
    }
    if let Some(display) = &state.display {
        elements.extend(encode_display(display, &state.sop_class_uid));
    }
    if let Some(shading) = &state.shading {
        elements.extend(encode_shading(shading));
    }
    finish(state, elements)
}
pub(super) fn read_standard(
    obj: &Object,
    sop_class_uid: &str,
    options: ReadOptions,
) -> Result<State, Error> {
    if obj.has(TAG_REFERENCED_SPATIAL_REGISTRATION_SEQUENCE) {
        return Err(Error::unsupported(
            "Referenced Spatial Registration Sequence",
        ));
    }
    let mut state = State {
        sop_class_uid: sop_class_uid.to_string(),
        sop_instance_uid: d::clean_uid(obj, d::TAG_SOP_INSTANCE_UID),
        study_instance_uid: d::clean_uid(obj, d::TAG_STUDY_INSTANCE_UID),
        series_instance_uid: d::clean_uid(obj, d::TAG_SERIES_INSTANCE_UID),
        frame_of_reference_uid: d::clean_uid(obj, d::TAG_FRAME_OF_REFERENCE_UID),
        identification: d::read_identification(obj),
        creation_date: d::clean_string(obj, TAG_PRESENTATION_CREATION_DATE),
        creation_time: d::clean_string(obj, TAG_PRESENTATION_CREATION_TIME),
        ..Default::default()
    };
    state.inputs = read_standard_inputs(obj)?;
    state.global_crop_specification_numbers = read_crop_selection(
        obj,
        TAG_GLOBAL_CROP,
        TAG_GLOBAL_CROPPING_SPECIFICATION_INDEX,
        "Global Crop",
    )?;
    state.cropping_specifications = read_cropping_specifications(obj, options)?;
    if is_mpr_sop_class(sop_class_uid) {
        state.mpr_geometry = Some(read_mpr_geometry(obj)?);
    } else {
        let geometry = read_volume_geometry(obj);
        state.rendering_method = clean_cs(obj, TAG_RENDERING_METHOD);
        state.volume_render_geometry = Some(geometry?);
    }
    state.display = Some(read_display(obj, sop_class_uid, options)?);
    state.shading = read_shading(obj)?;
    validate_standard_state_with_frame_requirement(&state, false)?;
    state.camera = camera_from_volume_geometry(state.volume_render_geometry.as_ref());
    Ok(state)
}
pub(super) fn validate_standard_state(state: &State) -> Result<(), Error> {
    validate_standard_state_with_frame_requirement(state, true)
}
fn blank(s: &str) -> bool {
    trim_space(s).is_empty()
}
fn one_of(value: &str, choices: &[&str]) -> bool {
    choices.contains(&value)
}
fn validate_standard_state_with_frame_requirement(
    state: &State,
    require_frame_of_reference: bool,
) -> Result<(), Error> {
    if blank(&state.sop_instance_uid)
        || blank(&state.study_instance_uid)
        || blank(&state.series_instance_uid)
    {
        return Err(Error::invalid(
            "standard VPS is missing an SOP, Study, or Series Instance UID",
        ));
    }
    if require_frame_of_reference && blank(&state.frame_of_reference_uid) {
        return Err(Error::invalid(
            "standard VPS is missing Frame of Reference UID",
        ));
    }
    let mpr = is_mpr_sop_class(&state.sop_class_uid);
    let mut mpr_thickness = "";
    if mpr {
        match (&state.mpr_geometry, &state.volume_render_geometry) {
            (Some(g), None) => {
                validate_mpr_geometry(g)?;
                mpr_thickness = &g.thickness_type;
            }
            _ => {
                return Err(Error::invalid(format!(
                    "SOP class {} requires exactly one PLANAR MPR geometry",
                    state.sop_class_uid
                )));
            }
        }
    } else {
        match (&state.volume_render_geometry, &state.mpr_geometry) {
            (Some(g), None) => validate_volume_geometry(g)?,
            _ => {
                return Err(Error::invalid(format!(
                    "SOP class {} requires exactly one volume render geometry",
                    state.sop_class_uid
                )));
            }
        }
        if !one_of(
            &state.rendering_method,
            &[
                RENDERING_VOLUME_RENDERED,
                RENDERING_MAXIMUM_IP,
                RENDERING_MINIMUM_IP,
            ],
        ) {
            return Err(Error::unsupported(format!(
                "Rendering Method {}",
                quote(&state.rendering_method)
            )));
        }
    }
    if state.inputs.is_empty() {
        return Err(Error::invalid(
            "Volumetric Presentation State Input Sequence is empty",
        ));
    }
    if state.inputs.len() != 1 {
        return Err(Error::unsupported(format!(
            "{} presentation inputs; the supported standard subset requires exactly one",
            state.inputs.len()
        )));
    }
    let mut sets: Vec<&str> = Vec::new();
    let mut input_numbers: Vec<i64> = Vec::new();
    for (i, input) in state.inputs.iter().enumerate() {
        if input.number != i as i64 + 1 || input.number > u16::MAX as i64 {
            return Err(Error::invalid(format!(
                "input numbers must be contiguous from 1; item {} is {}",
                i + 1,
                input.number
            )));
        }
        if blank(&input.input_set_uid) {
            return Err(Error::invalid(format!(
                "input {} is missing Input Set UID",
                input.number
            )));
        }
        sets.push(&input.input_set_uid);
        input_numbers.push(input.number);
        if input.referenced_instances.is_empty() {
            return Err(Error::invalid(format!(
                "input set {} has no referenced instances",
                input.input_set_uid
            )));
        }
        for r in &input.referenced_instances {
            if blank(&r.sop_class_uid) || blank(&r.sop_instance_uid) {
                return Err(Error::invalid(format!(
                    "input {} has an incomplete SOP instance reference",
                    input.number
                )));
            }
        }
        if let Some(voi) = &input.voi
            && (!finite(voi.window_center)
                || !finite(voi.window_width)
                || voi.window_width <= 0.0
                || voi.function == VoiFunction::Linear && voi.window_width < 1.0)
        {
            return Err(Error::invalid(format!(
                "input {} has invalid Window Center/Width",
                input.number
            )));
        }
        if mpr
            && mpr_thickness == MPR_THICKNESS_SLAB
            && !one_of(
                &input.rendering_method,
                &[
                    RENDERING_AVERAGE_IP,
                    RENDERING_MAXIMUM_IP,
                    RENDERING_MINIMUM_IP,
                ],
            )
        {
            return Err(Error::unsupported(format!(
                "input {} Rendering Method {}",
                input.number,
                quote(&input.rendering_method)
            )));
        }
        if mpr && mpr_thickness == MPR_THICKNESS_THIN && !input.rendering_method.is_empty() {
            return Err(Error::unsupported(format!(
                "Rendering Method on THIN MPR input {}",
                input.number
            )));
        }
        if !mpr && !input.rendering_method.is_empty() {
            return Err(Error::unsupported(format!(
                "input-level Rendering Method on volume render input {}",
                input.number
            )));
        }
    }
    validate_crop(state)?;
    validate_display(
        state.display.as_ref(),
        &state.sop_class_uid,
        &sets,
        &input_numbers,
    )?;
    if let Some(s) = &state.shading {
        if !one_of(&s.style, &[SHADING_SINGLE_SIDED, SHADING_DOUBLE_SIDED]) {
            return Err(Error::unsupported(format!(
                "Shading Style {}",
                quote(&s.style)
            )));
        }
        for (name, value) in [
            ("ambient", s.ambient),
            ("diffuse", s.diffuse),
            ("specular", s.specular),
            ("shininess", s.shininess),
        ] {
            if !finite(value) || !(0.0..=1.0).contains(&value) {
                return Err(Error::invalid(format!(
                    "{name} shading intensity {} is outside [0,1]",
                    g(value)
                )));
            }
        }
        if (s.diffuse > 0.0 || s.specular > 0.0) && !unit_vec(s.light_direction) {
            return Err(Error::invalid(
                "Light Direction must be a unit vector when diffuse or specular shading is enabled",
            ));
        }
    }
    Ok(())
}
fn validate_mpr_geometry(g: &MprGeometry) -> Result<(), Error> {
    if !one_of(&g.thickness_type, &[MPR_THICKNESS_THIN, MPR_THICKNESS_SLAB]) {
        return Err(Error::unsupported(format!(
            "MPR Thickness Type {}",
            quote(&g.thickness_type)
        )));
    }
    if g.thickness_type == MPR_THICKNESS_SLAB
        && (!finite(g.slab_thickness) || g.slab_thickness <= 0.0)
    {
        return Err(Error::invalid("MPR slab thickness must be positive"));
    }
    if !finite_vec(g.top_left)
        || !unit_vec(g.width_direction)
        || !unit_vec(g.height_direction)
        || g.width_direction.dot(g.height_direction).abs() > 1e-4
        || !finite(g.width)
        || g.width <= 0.0
        || !finite(g.height)
        || g.height <= 0.0
    {
        return Err(Error::invalid("invalid MPR plane geometry"));
    }
    Ok(())
}
fn validate_volume_geometry(g: &VolumeRenderGeometry) -> Result<(), Error> {
    if !one_of(
        &g.projection,
        &[PROJECTION_ORTHOGRAPHIC, PROJECTION_PERSPECTIVE],
    ) {
        return Err(Error::unsupported(format!(
            "Render Projection {}",
            quote(&g.projection)
        )));
    }
    let forward = g.look_at.sub(g.position);
    if !finite_vec(g.position)
        || !finite_vec(g.look_at)
        || !unit_vec(g.up)
        || forward.length() <= 1e-9
        || forward.normalize().dot(g.up).abs() > 1.0 - 1e-6
    {
        return Err(Error::invalid(
            "invalid viewpoint position/look-at/up geometry",
        ));
    }
    if g.field_of_view.iter().any(|v| !finite(*v)) {
        return Err(Error::invalid(
            "Render Field of View contains a non-finite value",
        ));
    }
    let f = &g.field_of_view;
    if !(f[0] < f[1] && f[3] < f[2] && f[4] > 0.0 && f[4] < f[5]) {
        return Err(Error::invalid("invalid Render Field of View bounds"));
    }
    Ok(())
}
fn validate_crop(state: &State) -> Result<(), Error> {
    let mut specs: Vec<i64> = Vec::new();
    for spec in &state.cropping_specifications {
        if spec.number <= 0 || spec.number > u16::MAX as i64 || specs.contains(&spec.number) {
            return Err(Error::invalid(format!(
                "invalid or duplicate cropping specification number {}",
                spec.number
            )));
        }
        specs.push(spec.number);
        if spec.method != CROP_BOUNDING_BOX {
            return Err(Error::unsupported(format!(
                "Volume Cropping Method {}",
                quote(&spec.method)
            )));
        }
        let b = &spec.bounding_box;
        if b.iter().any(|v| !finite(*v)) {
            return Err(Error::invalid(format!(
                "crop {} contains a non-finite bound",
                spec.number
            )));
        }
        if b[0] == b[1] && b[2] == b[3] && b[4] == b[5] {
            return Err(Error::invalid(format!(
                "crop {} bounding box has coincident corners",
                spec.number
            )));
        }
    }
    let check = |scope: &str, values: &[i64]| -> Result<(), Error> {
        for number in values {
            if !specs.contains(number) {
                return Err(Error::invalid(format!(
                    "{scope} references missing crop {number}"
                )));
            }
        }
        Ok(())
    };
    check("Global Crop", &state.global_crop_specification_numbers)?;
    for input in &state.inputs {
        check(
            &format!("input {}", input.number),
            &input.crop_specification_numbers,
        )?;
    }
    Ok(())
}
fn validate_display(
    display: Option<&Display>,
    sop_class_uid: &str,
    sets: &[&str],
    input_numbers: &[i64],
) -> Result<(), Error> {
    let Some(display) = display else {
        return Err(Error::invalid("standard VPS is missing its display module"));
    };
    if sop_class_uid == GRAYSCALE_PLANAR_MPR_VOLUMETRIC_PRESENTATION_STATE_STORAGE {
        if display.pixel_presentation != PIXEL_PRESENTATION_MONOCHROME {
            return Err(Error::unsupported(format!(
                "grayscale MPR Pixel Presentation {}",
                quote(&display.pixel_presentation)
            )));
        }
        if !one_of(
            &display.presentation_lut_shape,
            &[PRESENTATION_LUT_IDENTITY, PRESENTATION_LUT_INVERSE],
        ) {
            return Err(Error::unsupported(format!(
                "Presentation LUT Shape {}",
                quote(&display.presentation_lut_shape)
            )));
        }
        if !display.classification.is_empty() {
            return Err(Error::unsupported("classification in MONOCHROME MPR"));
        }
        return Ok(());
    }
    if display.pixel_presentation != PIXEL_PRESENTATION_TRUE_COLOR {
        return Err(Error::unsupported(format!(
            "Pixel Presentation {}",
            quote(&display.pixel_presentation)
        )));
    }
    if display.icc_profile.is_empty() || to_upper(trim_space(&display.color_space)) != "SRGB" {
        return Err(Error::unsupported(
            "TRUE_COLOR display without an embedded sRGB ICC profile",
        ));
    }
    if !valid_rgb_icc_profile(&display.icc_profile) {
        return Err(Error::invalid("embedded ICC profile is not an RGB profile"));
    }
    if display.classification.len() != 1 {
        return Err(Error::unsupported(format!(
            "{} classification components",
            display.classification.len()
        )));
    }
    let component = &display.classification[0];
    if component.input_numbers.len() != 1 {
        return Err(Error::unsupported(format!(
            "classification component with {} inputs",
            component.input_numbers.len()
        )));
    }
    let mpr = is_mpr_sop_class(sop_class_uid);
    validate_classification_transfer(component, mpr)?;
    if !input_numbers.contains(&component.input_numbers[0]) {
        return Err(Error::invalid(format!(
            "classification references unknown input {}",
            component.input_numbers[0]
        )));
    }
    if !mpr && (display.input_set_uid.is_empty() || !sets.contains(&display.input_set_uid.as_str()))
    {
        return Err(Error::invalid(format!(
            "volume stream references unknown input set {}",
            display.input_set_uid
        )));
    }
    Ok(())
}
pub(super) fn validate_classification_transfer(
    component: &ClassificationComponent,
    mpr: bool,
) -> Result<(), Error> {
    let has_rgb_palette = !component.red_palette.is_empty()
        || !component.green_palette.is_empty()
        || !component.blue_palette.is_empty();
    match component.rgb_transfer_function.as_str() {
        RGB_TRANSFER_EQUAL => {
            if component.palette_descriptor != [0u16; 3] || has_rgb_palette {
                return Err(Error::invalid(
                    "EQUAL_RGB classification carries palette data",
                ));
            }
        }
        RGB_TRANSFER_TABLE => {
            let count = classification_palette_count(component.palette_descriptor)?;
            if component.red_palette.len() != count
                || component.green_palette.len() != count
                || component.blue_palette.len() != count
            {
                return Err(Error::invalid(format!(
                    "RGB palette lengths {}/{}/{} do not match descriptor count {count}",
                    component.red_palette.len(),
                    component.green_palette.len(),
                    component.blue_palette.len()
                )));
            }
        }
        other => {
            return Err(Error::unsupported(format!(
                "RGB LUT Transfer Function {}",
                quote(other)
            )));
        }
    }
    match component.alpha_transfer_function.as_str() {
        "" => {
            if !mpr {
                return Err(Error::invalid(
                    "volume classification is missing Alpha LUT Transfer Function",
                ));
            }
        }
        "NONE" | "IDENTITY" => {
            if !component.alpha_palette.is_empty() {
                return Err(Error::invalid(
                    "non-TABLE alpha classification carries palette data",
                ));
            }
        }
        ALPHA_TRANSFER_TABLE => {
            if component.rgb_transfer_function != RGB_TRANSFER_TABLE {
                return Err(Error::unsupported(
                    "TABLE alpha without a TABLE RGB descriptor",
                ));
            }
            let count = classification_palette_count(component.palette_descriptor)?;
            if component.alpha_palette.len() != count {
                return Err(Error::invalid(format!(
                    "alpha palette length {} does not match descriptor count {count}",
                    component.alpha_palette.len()
                )));
            }
        }
        other => {
            return Err(Error::unsupported(format!(
                "Alpha LUT Transfer Function {}",
                quote(other)
            )));
        }
    }
    Ok(())
}
fn descriptor_text(d: [u16; 3]) -> String {
    format!("[{} {} {}]", d[0], d[1], d[2])
}
fn classification_palette_count(descriptor: [u16; 3]) -> Result<usize, Error> {
    let count = descriptor[0] as i64;
    if count == 0 {
        return Err(Error::unsupported("65536-entry classification palette"));
    }
    if !(2..=MAX_CLASSIFICATION_PALETTE_ENTRIES).contains(&count) || descriptor[1] != 0 {
        return Err(Error::invalid(format!(
            "invalid classification palette descriptor {}",
            descriptor_text(descriptor)
        )));
    }
    if descriptor[2] == 8 {
        return Err(Error::unsupported("8-bit classification palette"));
    }
    if descriptor[2] != 16 {
        return Err(Error::invalid(format!(
            "invalid classification palette descriptor {}",
            descriptor_text(descriptor)
        )));
    }
    Ok(count as usize)
}
fn voi_function_name(function: VoiFunction) -> &'static str {
    match function {
        VoiFunction::Linear => "LINEAR",
        VoiFunction::LinearExact => "LINEAR_EXACT",
        VoiFunction::Sigmoid => "SIGMOID",
    }
}
fn standard_input_sequence(inputs: &[Input]) -> Element {
    let items = inputs
        .iter()
        .map(|input| {
            let mut elements = vec![
                d::us(TAG_VPS_INPUT_NUMBER, &[input.number as u16]),
                d::ui(TAG_VPS_INPUT_SET_UID, &input.input_set_uid),
            ];
            if input.crop_specification_numbers.is_empty() {
                elements.push(d::cs(TAG_CROP, "NO"));
            } else {
                elements.push(d::cs(TAG_CROP, "YES"));
                elements.push(us_ints(
                    TAG_CROPPING_SPECIFICATION_INDEX,
                    &input.crop_specification_numbers,
                ));
            }
            if let Some(voi) = &input.voi {
                elements.push(d::ds(TAG_WINDOW_CENTER, &[voi.window_center]));
                elements.push(d::ds(TAG_WINDOW_WIDTH, &[voi.window_width]));
                if voi.function != VoiFunction::Linear {
                    elements.push(d::cs(TAG_VOI_LUT_FUNCTION, voi_function_name(voi.function)));
                }
            }
            if !input.rendering_method.is_empty() {
                elements.push(d::cs(TAG_RENDERING_METHOD, &input.rendering_method));
            }
            d::data_set(elements)
        })
        .collect();
    d::seq(TAG_VPS_INPUT_SEQUENCE, items)
}
fn cropping_sequence(specs: &[CroppingSpecification]) -> Element {
    let items = specs
        .iter()
        .map(|spec| {
            let b = &spec.bounding_box;
            d::data_set(vec![
                d::us(TAG_CROPPING_SPECIFICATION_NUMBER, &[spec.number as u16]),
                d::cs(TAG_VOLUME_CROPPING_METHOD, &spec.method),
                d::fd(TAG_BOUNDING_BOX_CROP, &[b[0], b[2], b[4], b[1], b[3], b[5]]),
            ])
        })
        .collect();
    d::seq(TAG_VOLUME_CROPPING_SEQUENCE, items)
}
fn encode_mpr_geometry(g: &MprGeometry) -> Vec<Element> {
    let mut elements = vec![
        d::cs(TAG_MPR_STYLE, "PLANAR"),
        d::cs(TAG_MPR_THICKNESS_TYPE, &g.thickness_type),
        d::fd(TAG_MPR_TOP_LEFT, &vec_values(g.top_left)),
        d::fd(TAG_MPR_WIDTH_DIRECTION, &vec_values(g.width_direction)),
        d::fd(TAG_MPR_WIDTH, &[g.width]),
        d::fd(TAG_MPR_HEIGHT_DIRECTION, &vec_values(g.height_direction)),
        d::fd(TAG_MPR_HEIGHT, &[g.height]),
    ];
    if g.thickness_type == MPR_THICKNESS_SLAB {
        elements.push(d::fd(TAG_MPR_SLAB_THICKNESS, &[g.slab_thickness]));
    }
    elements
}
fn encode_volume_geometry(g: &VolumeRenderGeometry) -> Vec<Element> {
    vec![
        d::cs(TAG_RENDER_PROJECTION, &g.projection),
        d::fd(TAG_VIEWPOINT_POSITION, &vec_values(g.position)),
        d::fd(TAG_VIEWPOINT_LOOK_AT, &vec_values(g.look_at)),
        d::fd(TAG_VIEWPOINT_UP, &vec_values(g.up)),
        d::fd(TAG_RENDER_FIELD_OF_VIEW, &g.field_of_view),
    ]
}
fn encode_display(display: &Display, sop_class_uid: &str) -> Vec<Element> {
    let mut elements = vec![d::cs(TAG_PIXEL_PRESENTATION, &display.pixel_presentation)];
    if sop_class_uid == GRAYSCALE_PLANAR_MPR_VOLUMETRIC_PRESENTATION_STATE_STORAGE {
        elements.push(d::cs(
            TAG_PRESENTATION_LUT_SHAPE,
            &display.presentation_lut_shape,
        ));
        return elements;
    }
    let component_items: Vec<DataSet> = display
        .classification
        .iter()
        .map(|component| {
            let input_items = component
                .input_numbers
                .iter()
                .map(|&input| {
                    d::data_set(vec![d::us(TAG_PRESENTATION_INPUT_INDEX, &[input as u16])])
                })
                .collect();
            let mut e = vec![
                d::cs(TAG_COMPONENT_TYPE, "ONE_TO_RGBA"),
                d::seq(TAG_COMPONENT_INPUT_SEQUENCE, input_items),
                d::cs(TAG_RGB_TRANSFER_FUNCTION, &component.rgb_transfer_function),
            ];
            if !component.description.is_empty() {
                e.push(d::lo(
                    TAG_RGBA_TRANSFER_FUNCTION_DESCRIPTION,
                    &component.description,
                ));
            }
            if !component.alpha_transfer_function.is_empty() {
                e.push(d::cs(
                    TAG_ALPHA_TRANSFER_FUNCTION,
                    &component.alpha_transfer_function,
                ));
            }
            if component.rgb_transfer_function == RGB_TRANSFER_TABLE {
                let descriptor = &component.palette_descriptor;
                e.push(d::us(TAG_RED_PALETTE_DESCRIPTOR, descriptor));
                e.push(d::us(TAG_GREEN_PALETTE_DESCRIPTOR, descriptor));
                e.push(d::us(TAG_BLUE_PALETTE_DESCRIPTOR, descriptor));
                e.push(word_element(TAG_RED_PALETTE_DATA, &component.red_palette));
                e.push(word_element(
                    TAG_GREEN_PALETTE_DATA,
                    &component.green_palette,
                ));
                e.push(word_element(TAG_BLUE_PALETTE_DATA, &component.blue_palette));
            }
            if component.alpha_transfer_function == ALPHA_TRANSFER_TABLE {
                e.push(d::us(
                    TAG_ALPHA_PALETTE_DESCRIPTOR,
                    &[
                        component.palette_descriptor[0],
                        component.palette_descriptor[1],
                        8,
                    ],
                ));
                e.push(byte_alpha_palette(&component.alpha_palette));
            }
            d::data_set(e)
        })
        .collect();
    let classification = d::seq(TAG_CLASSIFICATION_SEQUENCE, component_items);
    if is_mpr_sop_class(sop_class_uid) {
        elements.push(classification);
    } else {
        elements.push(d::seq(
            TAG_VOLUME_STREAM_SEQUENCE,
            vec![d::data_set(vec![
                d::ui(TAG_VPS_INPUT_SET_UID, &display.input_set_uid),
                classification,
            ])],
        ));
    }
    elements.push(d::raw(TAG_ICC_PROFILE, Vr::OB, display.icc_profile.clone()));
    elements.push(d::cs(TAG_COLOR_SPACE, &display.color_space));
    elements
}
fn byte_alpha_palette(values: &[u16]) -> Element {
    let mut data = vec![0u16; values.len().div_ceil(2)];
    for (i, &value) in values.iter().enumerate() {
        data[i / 2] |= (((value as u32 + 128) / 257) as u16) << (8 * (i % 2));
    }
    word_element(TAG_ALPHA_PALETTE_DATA, &data)
}
fn encode_shading(s: &Shading) -> Vec<Element> {
    let mut elements = vec![
        d::cs(TAG_SHADING_STYLE, &s.style),
        d::fd(TAG_AMBIENT_REFLECTION, &[s.ambient]),
    ];
    if s.diffuse > 0.0 || s.specular > 0.0 {
        elements.push(d::fd(TAG_LIGHT_DIRECTION, &vec_values(s.light_direction)));
    }
    if s.diffuse > 0.0 {
        elements.push(d::fd(TAG_DIFFUSE_REFLECTION, &[s.diffuse]));
    }
    if s.specular > 0.0 {
        elements.push(d::fd(TAG_SPECULAR_REFLECTION, &[s.specular]));
    }
    if s.shininess > 0.0 {
        elements.push(d::fd(TAG_SHININESS, &[s.shininess]));
    }
    elements
}
fn floats(obj: &Object, tag: Tag) -> Vec<f64> {
    d::floats(Some(obj), tag).unwrap_or_default()
}
fn ints(obj: &Object, tag: Tag) -> Vec<i64> {
    d::ints(Some(obj), tag).unwrap_or_default()
}
fn read_standard_inputs(obj: &Object) -> Result<Vec<Input>, Error> {
    let mut sets: Vec<(String, Vec<ReferencedInstance>)> = Vec::new();
    let set_items = d::sequence(obj, TAG_VPS_INPUT_SET_SEQUENCE);
    if set_items.is_empty() {
        return Err(Error::invalid(
            "Volumetric Presentation Input Set Sequence is missing or empty",
        ));
    }
    for (index, set) in set_items.iter().enumerate() {
        let uid = d::clean_uid(set, TAG_VPS_INPUT_SET_UID);
        if uid.is_empty() || sets.iter().any(|s| s.0 == uid) {
            return Err(Error::invalid(format!(
                "input set item {} has missing or duplicate UID",
                index + 1
            )));
        }
        let kind = clean_cs(set, TAG_PRESENTATION_INPUT_TYPE);
        if kind != "VOLUME" {
            return Err(Error::unsupported(format!(
                "Presentation Input Type {}",
                quote(&kind)
            )));
        }
        if set.has(TAG_REFERENCED_SPATIAL_REGISTRATION_SEQUENCE) {
            return Err(Error::unsupported(format!(
                "Referenced Spatial Registration Sequence in input set {uid}"
            )));
        }
        let mut refs = Vec::new();
        read_refs(set, &mut refs);
        if refs.is_empty() {
            return Err(Error::invalid(format!(
                "input set {uid} has no Referenced Image Sequence"
            )));
        }
        sets.push((uid, refs));
    }
    let items = d::sequence(obj, TAG_VPS_INPUT_SEQUENCE);
    if items.is_empty() {
        return Err(Error::invalid(
            "Volumetric Presentation State Input Sequence is missing or empty",
        ));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in &items {
        let mut input = Input {
            number: d::int(item, TAG_VPS_INPUT_NUMBER),
            input_set_uid: d::clean_uid(item, TAG_VPS_INPUT_SET_UID),
            ..Default::default()
        };
        if let Some(set) = sets.iter().find(|s| s.0 == input.input_set_uid) {
            input.referenced_instances = set.1.clone();
        }
        input.crop_specification_numbers = read_crop_selection(
            item,
            TAG_CROP,
            TAG_CROPPING_SPECIFICATION_INDEX,
            &format!("input {} Crop", input.number),
        )?;
        if item.has(TAG_VOI_LUT_SEQUENCE) {
            return Err(Error::unsupported(format!(
                "VOI LUT Sequence on input {}",
                input.number
            )));
        }
        let centers = floats(item, TAG_WINDOW_CENTER);
        let widths = floats(item, TAG_WINDOW_WIDTH);
        if !centers.is_empty() || !widths.is_empty() {
            if centers.len() != 1 || widths.len() != 1 {
                return Err(Error::unsupported(format!(
                    "multi-valued or incomplete Window Center/Width on input {}",
                    input.number
                )));
            }
            let function = match clean_cs(item, TAG_VOI_LUT_FUNCTION).as_str() {
                "" | "LINEAR" => VoiFunction::Linear,
                "LINEAR_EXACT" => VoiFunction::LinearExact,
                "SIGMOID" => VoiFunction::Sigmoid,
                _ => return Err(Error::unsupported("VOI LUT Function")),
            };
            input.voi = Some(Voi {
                window_center: centers[0],
                window_width: widths[0],
                function,
            });
        } else if item.has(TAG_VOI_LUT_FUNCTION) {
            return Err(Error::invalid(
                "VOI LUT Function without Window Center/Width",
            ));
        }
        input.rendering_method = clean_cs(item, TAG_RENDERING_METHOD);
        out.push(input);
    }
    Ok(out)
}
fn read_crop_selection(
    obj: &Object,
    flag_tag: Tag,
    index_tag: Tag,
    label: &str,
) -> Result<Vec<i64>, Error> {
    match clean_cs(obj, flag_tag).as_str() {
        "NO" => {
            if obj.has(index_tag) {
                return Err(Error::invalid(format!(
                    "{label} is NO but has crop indexes"
                )));
            }
            Ok(Vec::new())
        }
        "YES" => {
            let values = ints(obj, index_tag);
            if values.is_empty() {
                return Err(Error::invalid(format!(
                    "{label} is YES without crop indexes"
                )));
            }
            Ok(values)
        }
        _ => Err(Error::invalid(format!("{label} must be YES or NO"))),
    }
}
fn read_cropping_specifications(
    obj: &Object,
    options: ReadOptions,
) -> Result<Vec<CroppingSpecification>, Error> {
    let items = d::sequence(obj, TAG_VOLUME_CROPPING_SEQUENCE);
    let mut out = Vec::with_capacity(items.len());
    for item in &items {
        let method = clean_cs(item, TAG_VOLUME_CROPPING_METHOD);
        if method != CROP_BOUNDING_BOX {
            if item.has(TAG_OBLIQUE_CROPPING_PLANE_SEQUENCE) {
                return Err(Error::unsupported(format!(
                    "Volume Cropping Method {} with oblique planes",
                    quote(&method)
                )));
            }
            return Err(Error::unsupported(format!(
                "Volume Cropping Method {}",
                quote(&method)
            )));
        }
        let v = floats(item, TAG_BOUNDING_BOX_CROP);
        if v.len() != 6 {
            return Err(Error::invalid("BOUNDING_BOX crop must contain six values"));
        }
        let bounding_box = if options.legacy_interleaved_bounding_box {
            [v[0], v[1], v[2], v[3], v[4], v[5]]
        } else {
            [v[0], v[3], v[1], v[4], v[2], v[5]]
        };
        out.push(CroppingSpecification {
            number: d::int(item, TAG_CROPPING_SPECIFICATION_NUMBER),
            method,
            bounding_box,
        });
    }
    Ok(out)
}
fn required_vec(obj: &Object, tag: Tag, name: &str) -> Result<Vec3, Error> {
    let values = floats(obj, tag);
    if values.len() != 3 {
        return Err(Error::invalid(format!("{name} must contain three values")));
    }
    Ok(Vec3::new(values[0], values[1], values[2]))
}
fn required_scalar(obj: &Object, tag: Tag, name: &str) -> Result<f64, Error> {
    let values = floats(obj, tag);
    if values.len() != 1 {
        return Err(Error::invalid(format!("{name} must contain one value")));
    }
    Ok(values[0])
}
fn optional_scalar(obj: &Object, tag: Tag, name: &str) -> Result<Option<f64>, Error> {
    if !obj.has(tag) {
        return Ok(None);
    }
    match d::lookup_floats(Some(obj), tag) {
        Ok(values) if values.len() == 1 => Ok(Some(values[0])),
        _ => Err(Error::invalid(format!(
            "{name} must contain one numeric value"
        ))),
    }
}
fn read_mpr_geometry(obj: &Object) -> Result<MprGeometry, Error> {
    let style = clean_cs(obj, TAG_MPR_STYLE);
    if style != "PLANAR" {
        return Err(Error::unsupported(format!(
            "Multi-Planar Reconstruction Style {}",
            quote(&style)
        )));
    }
    let mut g = MprGeometry {
        thickness_type: clean_cs(obj, TAG_MPR_THICKNESS_TYPE),
        ..Default::default()
    };
    g.top_left = required_vec(obj, TAG_MPR_TOP_LEFT, "MPR Top Left Hand Corner")?;
    g.width_direction = required_vec(obj, TAG_MPR_WIDTH_DIRECTION, "MPR View Width Direction")?;
    g.width = required_scalar(obj, TAG_MPR_WIDTH, "MPR View Width")?;
    g.height_direction = required_vec(obj, TAG_MPR_HEIGHT_DIRECTION, "MPR View Height Direction")?;
    g.height = required_scalar(obj, TAG_MPR_HEIGHT, "MPR View Height")?;
    if g.thickness_type == MPR_THICKNESS_SLAB {
        g.slab_thickness = required_scalar(obj, TAG_MPR_SLAB_THICKNESS, "MPR Slab Thickness")?;
    }
    Ok(g)
}
fn read_volume_geometry(obj: &Object) -> Result<VolumeRenderGeometry, Error> {
    let mut g = VolumeRenderGeometry {
        projection: clean_cs(obj, TAG_RENDER_PROJECTION),
        ..Default::default()
    };
    g.position = required_vec(obj, TAG_VIEWPOINT_POSITION, "Viewpoint Position")?;
    g.look_at = required_vec(obj, TAG_VIEWPOINT_LOOK_AT, "Viewpoint LookAt Point")?;
    g.up = required_vec(obj, TAG_VIEWPOINT_UP, "Viewpoint Up Direction")?;
    let values = floats(obj, TAG_RENDER_FIELD_OF_VIEW);
    if values.len() != 6 {
        return Err(Error::invalid(
            "Render Field of View must contain six values",
        ));
    }
    g.field_of_view.copy_from_slice(&values);
    Ok(g)
}
fn read_display(obj: &Object, sop_class_uid: &str, options: ReadOptions) -> Result<Display, Error> {
    let mut display = Display {
        pixel_presentation: clean_cs(obj, TAG_PIXEL_PRESENTATION),
        ..Default::default()
    };
    if sop_class_uid == GRAYSCALE_PLANAR_MPR_VOLUMETRIC_PRESENTATION_STATE_STORAGE {
        display.presentation_lut_shape = clean_cs(obj, TAG_PRESENTATION_LUT_SHAPE);
        return Ok(display);
    }
    let mut stream: Option<Object> = None;
    if !is_mpr_sop_class(sop_class_uid) {
        let mut streams = d::sequence(obj, TAG_VOLUME_STREAM_SEQUENCE);
        if streams.len() != 1 {
            return Err(Error::unsupported(format!(
                "{} Volume Stream Sequence items",
                streams.len()
            )));
        }
        let first = streams.remove(0);
        display.input_set_uid = d::clean_uid(&first, TAG_VPS_INPUT_SET_UID);
        stream = Some(first);
    }
    let owner = stream.as_ref().unwrap_or(obj);
    if !d::sequence(obj, TAG_COMPOSITOR_SEQUENCE).is_empty() {
        return Err(Error::unsupported(
            "Presentation State Compositor Component Sequence",
        ));
    }
    for item in &d::sequence(owner, TAG_CLASSIFICATION_SEQUENCE) {
        let kind = clean_cs(item, TAG_COMPONENT_TYPE);
        if kind != "ONE_TO_RGBA" {
            return Err(Error::unsupported(format!(
                "classification Component Type {}",
                quote(&kind)
            )));
        }
        let mut component = ClassificationComponent {
            rgb_transfer_function: clean_cs(item, TAG_RGB_TRANSFER_FUNCTION),
            alpha_transfer_function: clean_cs(item, TAG_ALPHA_TRANSFER_FUNCTION),
            description: d::clean_string(item, TAG_RGBA_TRANSFER_FUNCTION_DESCRIPTION),
            ..Default::default()
        };
        for input in &d::sequence(item, TAG_COMPONENT_INPUT_SEQUENCE) {
            component
                .input_numbers
                .push(d::int(input, TAG_PRESENTATION_INPUT_INDEX));
        }
        if component.rgb_transfer_function == RGB_TRANSFER_TABLE {
            component.palette_descriptor = read_classification_palette_descriptor(item)?;
            component.red_palette = read_classification_palette(item, TAG_RED_PALETTE_DATA, "red")?;
            component.green_palette =
                read_classification_palette(item, TAG_GREEN_PALETTE_DATA, "green")?;
            component.blue_palette =
                read_classification_palette(item, TAG_BLUE_PALETTE_DATA, "blue")?;
        }
        if component.alpha_transfer_function == ALPHA_TRANSFER_TABLE {
            component.alpha_palette =
                read_classification_alpha(item, component.palette_descriptor, options)?;
        }
        display.classification.push(component);
    }
    if let Some(raw) = obj.get(TAG_ICC_PROFILE).and_then(raw_bytes) {
        display.icc_profile = raw.to_vec();
    }
    display.color_space = clean_cs(obj, TAG_COLOR_SPACE);
    Ok(display)
}
fn read_classification_palette_descriptor(obj: &Object) -> Result<[u16; 3], Error> {
    let red = ints(obj, TAG_RED_PALETTE_DESCRIPTOR);
    let green = ints(obj, TAG_GREEN_PALETTE_DESCRIPTOR);
    let blue = ints(obj, TAG_BLUE_PALETTE_DESCRIPTOR);
    if red.len() != 3 || green.len() != 3 || blue.len() != 3 {
        return Err(Error::invalid(
            "TABLE classification requires three RGB palette descriptors",
        ));
    }
    let mut descriptor = [0u16; 3];
    for index in 0..3 {
        if red[index] < 0
            || red[index] > u16::MAX as i64
            || green[index] != red[index]
            || blue[index] != red[index]
        {
            return Err(Error::invalid("RGB palette descriptors differ or overflow"));
        }
        descriptor[index] = red[index] as u16;
    }
    Ok(descriptor)
}
fn read_classification_palette(obj: &Object, tag: Tag, name: &str) -> Result<Vec<u16>, Error> {
    let Some(element) = obj.get(tag) else {
        return Err(Error::invalid(format!(
            "TABLE classification is missing {name} palette data"
        )));
    };
    match raw_bytes(element) {
        Some(raw) if raw.len() % 2 == 0 => {
            let big = obj.value_big_endian();
            Ok(raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if big {
                        u16::from_be_bytes([c[0], c[1]])
                    } else {
                        u16::from_le_bytes([c[0], c[1]])
                    }
                })
                .collect())
        }
        _ => Err(Error::invalid(format!(
            "{name} palette data is not 16-bit OW"
        ))),
    }
}
fn read_classification_alpha(
    obj: &Object,
    rgb: [u16; 3],
    options: ReadOptions,
) -> Result<Vec<u16>, Error> {
    let descriptor = ints(obj, TAG_ALPHA_PALETTE_DESCRIPTOR);
    if options.legacy_missing_alpha_descriptor && !obj.has(TAG_ALPHA_PALETTE_DESCRIPTOR) {
        return read_classification_palette(obj, TAG_ALPHA_PALETTE_DATA, "alpha");
    }
    if descriptor.len() != 3 || descriptor[0] != rgb[0] as i64 || descriptor[1] != rgb[1] as i64 {
        return Err(Error::invalid("alpha palette descriptor differs from RGB"));
    }
    if descriptor[2] == 16 && options.legacy_sixteen_bit_alpha {
        return read_classification_palette(obj, TAG_ALPHA_PALETTE_DATA, "alpha");
    }
    if descriptor[2] != 8 {
        return Err(Error::invalid(
            "alpha palette descriptor requires eight-bit entries",
        ));
    }
    let Some(element) = obj.get(TAG_ALPHA_PALETTE_DATA) else {
        return Err(Error::invalid(
            "TABLE classification is missing alpha palette data",
        ));
    };
    let mut data: Option<Vec<u8>> = raw_bytes(element).map(<[u8]>::to_vec);
    if let Some(data) = &mut data
        && obj.value_big_endian()
    {
        for pair in data.as_chunks_mut::<2>().0 {
            pair.swap(0, 1);
        }
    }
    let count = rgb[0] as usize;
    match data {
        Some(data)
            if data.len() == count
                || (count % 2 == 1 && data.len() == count + 1 && data[count] == 0) =>
        {
            Ok(data[..count].iter().map(|&b| b as u16 * 257).collect())
        }
        _ => Err(Error::invalid(
            "alpha palette length does not match descriptor",
        )),
    }
}
fn read_shading(obj: &Object) -> Result<Option<Shading>, Error> {
    if !obj.has(TAG_SHADING_STYLE) {
        if obj.has(TAG_AMBIENT_REFLECTION)
            || obj.has(TAG_LIGHT_DIRECTION)
            || obj.has(TAG_DIFFUSE_REFLECTION)
            || obj.has(TAG_SPECULAR_REFLECTION)
            || obj.has(TAG_SHININESS)
        {
            return Err(Error::invalid("incomplete Render Shading module"));
        }
        return Ok(None);
    }
    let mut s = Shading {
        style: clean_cs(obj, TAG_SHADING_STYLE),
        ..Default::default()
    };
    s.ambient = required_scalar(obj, TAG_AMBIENT_REFLECTION, "Ambient Reflection Intensity")?;
    let diffuse = optional_scalar(obj, TAG_DIFFUSE_REFLECTION, "Diffuse Reflection Intensity")?;
    let specular = optional_scalar(
        obj,
        TAG_SPECULAR_REFLECTION,
        "Specular Reflection Intensity",
    )?;
    s.diffuse = diffuse.unwrap_or(0.0);
    s.specular = specular.unwrap_or(0.0);
    s.shininess = optional_scalar(obj, TAG_SHININESS, "Shininess")?.unwrap_or(0.0);
    if diffuse.is_some() || specular.is_some() {
        s.light_direction = required_vec(obj, TAG_LIGHT_DIRECTION, "Light Direction")?;
    }
    Ok(Some(s))
}
fn camera_from_volume_geometry(g: Option<&VolumeRenderGeometry>) -> Camera {
    let Some(g) = g else {
        return Camera::default();
    };
    let forward = g.look_at.sub(g.position).normalize();
    let up = g.up.normalize();
    let mut base_right = forward.cross(Vec3::new(0.0, 0.0, 1.0));
    if base_right.length() < 1e-6 {
        base_right = Vec3::new(1.0, 0.0, 0.0);
    }
    let base_right = base_right.normalize();
    let base_up = base_right.cross(forward).normalize();
    let roll = fmath::atan2(base_right.dot(up), base_up.dot(up));
    let vertical = g.field_of_view[2] - g.field_of_view[3];
    let mut fov = 0.0;
    if g.projection == PROJECTION_PERSPECTIVE && g.field_of_view[5] > 0.0 {
        fov = 2.0 * fmath::atan(vertical / (2.0 * g.field_of_view[5]));
    }
    Camera {
        target: g.look_at,
        yaw: fmath::atan2(forward.x, forward.y),
        pitch: fmath::asin(fmath::max(-1.0, fmath::min(1.0, forward.z))),
        roll,
        distance: g.look_at.sub(g.position).length(),
        fov_y: fov,
    }
}
fn valid_rgb_icc_profile(profile: &[u8]) -> bool {
    profile.len() >= 128 && &profile[16..20] == b"RGB " && &profile[36..40] == b"acsp"
}
