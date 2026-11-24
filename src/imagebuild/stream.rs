use super::capture::write_with_pixels;
use super::{
    Error, Geometry, Options, Rescale, Slice, TAG_FRAME_OF_REFERENCE_UID, TAG_STUDY_INSTANCE_UID,
    build_instance_pixels, choose_rescale_range, invalid, local_now, source_has, validate,
    validate_instance_uids,
};
use crate::deid::trim_space;
use crate::object::inherit::new_uid;
use crate::object::{Tag, Value, Vr};
pub type Emit<'a> = dyn FnMut(i64, &[u8]) -> Result<(), Error> + 'a;
pub trait SliceSource {
    fn slices(&self) -> i64;
    fn slice(&mut self, index: i64) -> Result<Slice, Error>;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamOptions {
    pub max_pixel_buffer_bytes: i64,
    pub max_slices: i64,
    pub replayable: bool,
    pub stored_representation: Option<u16>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeriesWriteResult {
    pub instances: i64,
    pub series_instance_uid: String,
    pub study_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub pixel_buffer_bytes: i64,
}
pub fn write_series(
    geometry: &Geometry,
    source: Option<&mut dyn SliceSource>,
    opts: &Options,
    limits: StreamOptions,
    emit: Option<&mut Emit<'_>>,
) -> (SeriesWriteResult, Option<Error>) {
    let mut result = SeriesWriteResult::default();
    let err = write_series_inner(geometry, source, opts, limits, emit, &mut result).err();
    (result, err)
}
fn write_series_inner(
    geometry: &Geometry,
    source: Option<&mut dyn SliceSource>,
    opts: &Options,
    limits: StreamOptions,
    emit: Option<&mut Emit<'_>>,
    result: &mut SeriesWriteResult,
) -> Result<(), Error> {
    let (Some(source), Some(emit)) = (source, emit) else {
        return Err(invalid(
            "source, emitter and positive stream budgets required",
        ));
    };
    if limits.max_slices <= 0 || limits.max_pixel_buffer_bytes <= 0 {
        return Err(invalid(
            "source, emitter and positive stream budgets required",
        ));
    }
    let count = source.slices();
    if count <= 0 || count > limits.max_slices {
        return Err(invalid("slice count exceeds stream budget"));
    }
    if geometry.rows < 1
        || geometry.columns < 1
        || geometry.rows > u16::MAX as i64
        || geometry.columns > u16::MAX as i64
        || geometry.rows * geometry.columns > limits.max_pixel_buffer_bytes / 10
    {
        return Err(invalid("slice exceeds pixel buffer budget"));
    }
    result.pixel_buffer_bytes = geometry.rows * geometry.columns * 10;
    validate_instance_uids(&opts.sop_instance_uids, count as usize)?;
    let (rescale, signed) = if let Some(representation) = limits.stored_representation {
        let Some(fixed) = opts.rescale.filter(|_| representation <= 1) else {
            return Err(invalid(
                "one-pass representation requires fixed rescale and sign",
            ));
        };
        choose_rescale_range(fixed.intercept, fixed.intercept, true, Some(&fixed))?;
        (fixed, representation == 1)
    } else {
        if !limits.replayable {
            return Err(invalid(
                "global rescale requires replayable source or fixed parameters",
            ));
        }
        let (mut low, mut high, mut integral) = (f64::INFINITY, f64::NEG_INFINITY, true);
        for i in 0..count {
            let s = read_stream_slice(geometry, source, opts, i)?;
            for &v in &s.values {
                low = crate::fmath::min(low, v);
                high = crate::fmath::max(high, v);
                integral = integral && (v - v.round()).abs() < 1e-6;
            }
        }
        choose_rescale_range(low, high, integral, opts.rescale.as_ref())?
    };
    let mut series_uid = trim_space(&opts.series_instance_uid).to_string();
    let mut frame_uid = trim_space(&geometry.frame_of_reference_uid).to_string();
    let mut study_uid = String::new();
    if series_uid.is_empty() {
        series_uid = new_uid();
    }
    if frame_uid.is_empty()
        && opts.has_image_plane()
        && !source_has(&opts.common, TAG_FRAME_OF_REFERENCE_UID)
    {
        frame_uid = new_uid();
    }
    if !source_has(&opts.common, TAG_STUDY_INSTANCE_UID) {
        study_uid = new_uid();
    }
    result.series_instance_uid = series_uid.clone();
    result.study_instance_uid = study_uid.clone();
    result.frame_of_reference_uid = frame_uid.clone();
    let now = opts.content_time.unwrap_or_else(local_now);
    let check = |r: &Rescale| r.slope == 0.0 || !r.slope.is_finite() || !r.intercept.is_finite();
    if check(&rescale) {
        return Err(invalid("nonfinite global rescale"));
    }
    for i in 0..count {
        let s = read_stream_slice(geometry, source, opts, i)?;
        let (low, high) = if signed {
            (-32768.0, 32767.0)
        } else {
            (0.0, 65535.0)
        };
        for &v in &s.values {
            let stored = ((v - rescale.intercept) / rescale.slope).round();
            if !stored.is_finite() || stored < low || stored > high {
                return Err(invalid(format!(
                    "slice {} value does not fit selected rescale",
                    i + 1
                )));
            }
        }
        let file = build_instance_pixels(
            geometry,
            &s,
            i + 1,
            opts,
            rescale,
            signed,
            &series_uid,
            &study_uid,
            &frame_uid,
            now,
            None,
        )?;
        if result.study_instance_uid.is_empty() {
            result.study_instance_uid = file
                .dataset
                .get_uid(TAG_STUDY_INSTANCE_UID)
                .unwrap_or_default();
        }
        if result.frame_of_reference_uid.is_empty() {
            result.frame_of_reference_uid = file
                .dataset
                .get_uid(TAG_FRAME_OF_REFERENCE_UID)
                .unwrap_or_default();
        }
        let pixels = match file.dataset.get(Tag::PIXEL_DATA).map(|e| &e.value) {
            Some(Value::Raw(p)) => p.clone(),
            _ => Vec::new(),
        };
        let bytes = write_with_pixels(&file, Vr::OW, pixels)?;
        emit(i, &bytes)?;
        result.instances += 1;
    }
    Ok(())
}
fn read_stream_slice(
    geometry: &Geometry,
    source: &mut dyn SliceSource,
    opts: &Options,
    index: i64,
) -> Result<Slice, Error> {
    let s = source.slice(index)?;
    if s.valid.is_some() {
        return Err(invalid("stream does not admit a validity mask"));
    }
    let mut validation_opts = opts.clone();
    validation_opts.sop_instance_uids = Vec::new();
    validate(geometry, std::slice::from_ref(&s), &validation_opts)?;
    if s.values.iter().any(|v| !v.is_finite()) {
        return Err(invalid("nonfinite slice value"));
    }
    Ok(s)
}
