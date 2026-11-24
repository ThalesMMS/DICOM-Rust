use super::model::read_modality_lut;
use super::{
    DisplayedArea, Error, Kind, ModalityLut, OverlayActivation, PRESENTATION_LUT_IDENTITY,
    PRESENTATION_LUT_INVERSE, PRESENTATION_SIZE_SCALE_TO_FIT, ReferencedImage, SoftcopyVoi, State,
    TAG_LATERALITY, TAG_PRESENTATION_LUT_SHAPE, invalid,
};
use crate::derivation::DerivedCommon;
use crate::derivedio as dio;
use crate::display::{LutOptions, VoiLut, overlays_from_object, shutter_from_object, vois_at};
use crate::object::inherit::new_uid;
use crate::object::{Object, Tag};
use crate::textfmt::to_upper;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoiSelection {
    #[default]
    PreferLut,
    PreferWindow,
    Ignore,
}
pub const OVERLAY_LAYER: &str = "OVERLAY";
#[derive(Clone, Debug, Default)]
pub struct FromImageOptions {
    pub sop_instance_uid: String,
    pub series_instance_uid: String,
    pub voi: VoiSelection,
    pub ignore_shutter: bool,
    pub ignore_overlays: bool,
    pub ignore_presentation_lut_shape: bool,
}
const TAG_PIXEL_SPACING: Tag = Tag::new(0x0028, 0x0030);
const TAG_IMAGER_PIXEL_SPACING: Tag = Tag::new(0x0018, 0x1164);
const TAG_PIXEL_ASPECT_RATIO: Tag = Tag::new(0x0028, 0x0034);
pub fn from_image(image: Option<&Object>, opts: &FromImageOptions) -> Result<State, Error> {
    let Some(image) = image else {
        return Err(invalid("image is nil"));
    };
    let reference = ReferencedImage {
        series_instance_uid: dio::clean_uid(image, dio::TAG_SERIES_INSTANCE_UID),
        sop_class_uid: dio::clean_uid(image, dio::TAG_SOP_CLASS_UID),
        sop_instance_uid: dio::clean_uid(image, dio::TAG_SOP_INSTANCE_UID),
        frames: Vec::new(),
    };
    let study_uid = dio::clean_uid(image, dio::TAG_STUDY_INSTANCE_UID);
    if study_uid.is_empty()
        || reference.series_instance_uid.is_empty()
        || reference.sop_class_uid.is_empty()
        || reference.sop_instance_uid.is_empty()
    {
        return Err(Error::new(
            Kind::MissingReference,
            "the image needs its study, series, SOP Class and SOP Instance UIDs",
        ));
    }
    let rows = dio::int(image, dio::TAG_ROWS);
    let columns = dio::int(image, dio::TAG_COLUMNS);
    if rows <= 0 || columns <= 0 {
        return Err(invalid("the image has no Rows and Columns"));
    }
    if dio::int(image, dio::TAG_SAMPLES_PER_PIXEL) > 1 {
        return Err(Error::new(
            Kind::UnsupportedSopClass,
            "a grayscale presentation state needs a grayscale image",
        ));
    }
    let mut state = State {
        common: DerivedCommon {
            source: Some(image.clone()),
            ..DerivedCommon::default()
        },
        sop_instance_uid: opts.sop_instance_uid.clone(),
        study_instance_uid: study_uid,
        series_instance_uid: opts.series_instance_uid.clone(),
        series_number: 1,
        referenced_images: vec![reference],
        ..State::default()
    };
    if state.sop_instance_uid.is_empty() {
        state.sop_instance_uid = new_uid();
    }
    if state.series_instance_uid.is_empty() {
        state.series_instance_uid = new_uid();
    }
    let laterality = dio::clean_string(image, TAG_LATERALITY);
    if laterality == "L" || laterality == "R" {
        state.laterality = laterality;
    }
    let mut area = DisplayedArea {
        defined: true,
        top_left_x: 1,
        top_left_y: 1,
        bottom_right_x: columns,
        bottom_right_y: rows,
        presentation_size_mode: PRESENTATION_SIZE_SCALE_TO_FIT.into(),
        ..DisplayedArea::default()
    };
    if let Some(spacing) = image_pixel_spacing(image) {
        area.pixel_spacing = spacing;
    } else if let Some(ratio) = dio::ints(Some(image), TAG_PIXEL_ASPECT_RATIO)
        && ratio.len() == 2
        && ratio[0] > 0
        && ratio[1] > 0
    {
        area.pixel_aspect_ratio = ratio;
    }
    state.displayed_area = area;
    if let Some(modality) = image_modality_lut(image)? {
        state.modality_lut = Some(modality);
    }
    if opts.voi != VoiSelection::Ignore
        && let Some(voi) = image_voi(image, opts.voi)?
    {
        state.softcopy_voi = voi;
    }
    if !opts.ignore_shutter {
        state.shutter = shutter_from_object(image)
            .map_err(|e| Error::other(format!("dicom/gsps: image shutter: {e}")))?;
    }
    if !opts.ignore_overlays {
        let (overlays, _) = overlays_from_object(image);
        for overlay in overlays {
            if let Some(shutter) = &state.shutter
                && let Some(bitmap) = &shutter.bitmap
                && bitmap.group == overlay.group
            {
                continue;
            }
            state.overlay_activations.push(OverlayActivation {
                group: overlay.group,
                layer: OVERLAY_LAYER.into(),
            });
        }
    }
    let mut shape = String::new();
    if !opts.ignore_presentation_lut_shape {
        let value = to_upper(&dio::clean_string(image, TAG_PRESENTATION_LUT_SHAPE));
        if value == PRESENTATION_LUT_IDENTITY || value == PRESENTATION_LUT_INVERSE {
            shape = value;
        }
    }
    if shape.is_empty() {
        shape = PRESENTATION_LUT_IDENTITY.into();
        let photometric = to_upper(&dio::clean_string(
            image,
            dio::TAG_PHOTOMETRIC_INTERPRETATION,
        ));
        if photometric == "MONOCHROME1" {
            shape = PRESENTATION_LUT_INVERSE.into();
        }
    }
    state.presentation_lut_shape = shape;
    Ok(state)
}
fn image_pixel_spacing(image: &Object) -> Option<Vec<f64>> {
    for tag in [TAG_PIXEL_SPACING, TAG_IMAGER_PIXEL_SPACING] {
        if let Some(values) = dio::floats(Some(image), tag)
            && values.len() == 2
            && values[0] > 0.0
            && values[1] > 0.0
        {
            return Some(values);
        }
    }
    None
}
fn image_modality_lut(image: &Object) -> Result<Option<ModalityLut>, Error> {
    let Some(mut modality) = read_modality_lut(image)? else {
        return Ok(None);
    };
    if modality.lut.is_none() && modality.slope == 0.0 {
        modality.slope = 1.0;
    }
    if modality.lut.is_none() && modality.rescale_type.is_empty() {
        modality.rescale_type = "US".into();
    }
    if modality.lut.is_some() && modality.lut_type.is_empty() {
        modality.lut_type = "US".into();
    }
    Ok(Some(modality))
}
fn image_voi(image: &Object, selection: VoiSelection) -> Result<Option<SoftcopyVoi>, Error> {
    let (transforms, err) = vois_at(image, 0, LutOptions::default());
    if let Some(err) = err
        && transforms.is_empty()
    {
        return Err(Error::other(format!("dicom/gsps: image VOI: {err}")));
    }
    let mut window: Option<&VoiLut> = None;
    let mut lut: Option<&VoiLut> = None;
    for t in &transforms {
        if t.lut.is_some() && lut.is_none() {
            lut = Some(t);
        } else if t.lut.is_none() && window.is_none() {
            window = Some(t);
        }
    }
    let mut chosen = lut;
    if (selection == VoiSelection::PreferWindow && window.is_some()) || chosen.is_none() {
        chosen = window;
    }
    Ok(chosen.map(|c| SoftcopyVoi {
        window_center: c.center,
        window_width: c.width,
        function: c.function,
        lut: c.lut.clone(),
        explanation: c.explanation.clone(),
        referenced_images: Vec::new(),
    }))
}
