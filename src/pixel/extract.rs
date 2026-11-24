use super::Metadata;
use super::frame::{
    Frame, FrameImage, FrameOptions, Rescale, Window, new_encapsulated_frame, new_native_frame,
};
use super::pixeldata::{
    self, PixelError, PixelErrorKind, extract_metadata, extract_native_frames, extract_view,
    is_jpip_referenced_transfer_syntax, is_video_transfer_syntax, normalized_photometric,
};
use crate::display::{self, LutOptions};
use crate::object::transfer;
use crate::object::{File, Object, Tag};
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_RESCALE_INTERCEPT: Tag = Tag::new(0x0028, 0x1052);
const TAG_RESCALE_SLOPE: Tag = Tag::new(0x0028, 0x1053);
pub fn window_from_object(obj: &Object) -> Option<Window> {
    let (voi, _) = display::voi_from_object(obj, LutOptions::default());
    (voi.width > 0.0).then_some(Window {
        center: voi.center,
        width: voi.width,
        function: voi.function,
        lut: None,
    })
}
pub fn window_at(obj: &Object, frame_index: i64) -> Option<Window> {
    let (voi, _) = display::voi_at(obj, frame_index, LutOptions::default());
    (voi.width > 0.0).then_some(Window {
        center: voi.center,
        width: voi.width,
        function: voi.function,
        lut: None,
    })
}
pub fn rescale_at(obj: &Object, frame_index: i64) -> Rescale {
    let (modality, _) = display::modality_lut_at(obj, frame_index, LutOptions::default());
    Rescale {
        slope: if modality.slope == 0.0 {
            1.0
        } else {
            modality.slope
        },
        intercept: modality.intercept,
    }
}
pub fn rescale_from_object(obj: &Object) -> Rescale {
    super::frame::rescale_from(
        obj.get_float(TAG_RESCALE_SLOPE).ok(),
        obj.get_float(TAG_RESCALE_INTERCEPT).ok(),
    )
}
fn decoded_frame_metadata(mut m: Metadata, uid: &str) -> Metadata {
    if m.samples_per_pixel != 3 {
        return m;
    }
    let uid = transfer::normalize_uid(uid);
    let photometric = normalized_photometric(&m.photometric_interpretation);
    let convert = match uid {
        "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51" => matches!(
            photometric.as_str(),
            "YBR_FULL" | "YBR_FULL_422" | "YBR_PARTIAL_422"
        ),
        _ if is_jpeg2000_or_xl(uid) => matches!(
            photometric.as_str(),
            "YBR_FULL" | "YBR_FULL_422" | "YBR_RCT" | "YBR_ICT"
        ),
        _ => false,
    };
    if convert {
        m.photometric_interpretation = "RGB".into();
        m.planar_configuration = 0;
        m.planar_configuration_present = true;
    }
    m
}
fn is_jpeg2000_or_xl(uid: &str) -> bool {
    matches!(
        uid,
        "1.2.840.10008.1.2.4.90"
            | "1.2.840.10008.1.2.4.91"
            | "1.2.840.10008.1.2.4.92"
            | "1.2.840.10008.1.2.4.93"
            | "1.2.840.10008.1.2.4.201"
            | "1.2.840.10008.1.2.4.202"
            | "1.2.840.10008.1.2.4.203"
            | "1.2.840.10008.1.2.4.110"
            | "1.2.840.10008.1.2.4.111"
            | "1.2.840.10008.1.2.4.112"
    )
}
pub fn frame_options_at(
    obj: &Object,
    big_endian: bool,
    index: i64,
) -> Result<FrameOptions, PixelError> {
    let mut opts = FrameOptions {
        big_endian,
        ..FrameOptions::default()
    };
    if normalized_photometric(&obj.get_string(TAG_PHOTOMETRIC).unwrap_or_default()) == "ARGB" {
        opts.palette = display::palette_from_object(obj, LutOptions::default())
            .map_err(|e| PixelError::new(PixelErrorKind::Display, e.to_string()))?;
    }
    if let Some(window) = window_at(obj, index) {
        opts.window.center = window.center;
        opts.window.width = window.width;
        opts.window.function = window.function;
    }
    opts.rescale = rescale_at(obj, index);
    Ok(opts)
}
pub fn extract_frames(file: &File) -> Result<Vec<Frame>, PixelError> {
    let ds = &file.dataset;
    let uid = file.transfer_syntax.uid;
    let mut palette = None;
    if normalized_photometric(&ds.get_string(TAG_PHOTOMETRIC).unwrap_or_default()) == "ARGB" {
        palette = display::palette_from_object(ds, LutOptions::default())
            .map_err(|e| PixelError::new(PixelErrorKind::Display, e.to_string()))?;
    }
    let options_at = |index: i64| -> FrameOptions {
        let mut opts = FrameOptions {
            big_endian: file.transfer_syntax.big_endian,
            palette: palette.clone(),
            ..FrameOptions::default()
        };
        if let Some(window) = window_at(ds, index) {
            opts.window.center = window.center;
            opts.window.width = window.width;
            opts.window.function = window.function;
        }
        opts.rescale = rescale_at(ds, index);
        opts
    };
    if is_jpip_referenced_transfer_syntax(uid) {
        let empty = pixeldata::PixelData::Native(&[]);
        return Err(pixeldata::decode_frames(uid, &empty, ds)
            .err()
            .unwrap_or_else(|| {
                PixelError::new(PixelErrorKind::Other, "dicom: JPIP decode returned frames")
            }));
    }
    if is_video_transfer_syntax(uid) {
        let pixels = extract_view(ds)?;
        return Err(pixeldata::decode_frames(uid, &pixels, ds)
            .err()
            .unwrap_or_else(|| {
                PixelError::new(PixelErrorKind::Other, "dicom: video decode returned frames")
            }));
    }
    match extract_native_frames(ds) {
        Ok(native) => {
            return Ok(native
                .data
                .into_iter()
                .enumerate()
                .map(|(i, data)| {
                    new_native_frame(
                        i as i64,
                        data,
                        native.metadata.clone(),
                        options_at(i as i64),
                    )
                })
                .collect());
        }
        Err(e) if e.kind != PixelErrorKind::EncapsulatedPixelData => return Err(e),
        Err(_) => {}
    }
    let mut metadata = extract_metadata(ds)?;
    let pixels = extract_view(ds)?;
    let decoded = pixeldata::decode_frames(uid, &pixels, ds)?;
    metadata = decoded_frame_metadata(metadata, uid);
    if metadata.samples_per_pixel == 3 && !decoded.photometric_interpretation.is_empty() {
        metadata.photometric_interpretation = decoded.photometric_interpretation.clone();
        metadata.planar_configuration = 0;
        metadata.planar_configuration_present = true;
    }
    Ok(decoded
        .data
        .into_iter()
        .enumerate()
        .map(|(i, data)| {
            new_encapsulated_frame(i as i64, data, metadata.clone(), options_at(i as i64))
        })
        .collect())
}
pub fn render_frame(file: &File, index: i64) -> Result<FrameImage, PixelError> {
    let frames = extract_frames(file)?;
    if index < 0 || index as usize >= frames.len() {
        return Err(PixelError::new(
            PixelErrorKind::Other,
            format!(
                "dicom: frame index {index} out of range [0,{})",
                frames.len()
            ),
        ));
    }
    frames
        .get(index as usize)
        .ok_or_else(|| PixelError::new(PixelErrorKind::Frame, "frame index out of range"))?
        .image()
        .map_err(|e| PixelError::new(PixelErrorKind::Frame, e.to_string()))
}
