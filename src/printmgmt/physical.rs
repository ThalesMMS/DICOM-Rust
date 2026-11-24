use super::*;
use std::sync::Arc;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Support {
    #[default]
    Unknown,
    Supported,
    Unsupported,
    Other,
}
impl Support {
    pub fn valid(self) -> bool {
        self != Support::Other
    }
}
pub const DECIMATE: &str = "DECIMATE";
pub const CROP: &str = "CROP";
pub const FAIL: &str = "FAIL";
pub const MAX_PHYSICAL_SIZE_MM: f64 = 10000.0;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImageBoxPresentation {
    pub requested_width_mm: f64,
    pub decimate_crop_behavior: String,
    pub magnification_type: String,
}
impl ImageBoxPresentation {
    pub fn specified(&self) -> bool {
        self.requested_width_mm != 0.0
            || !self.decimate_crop_behavior.is_empty()
            || !self.magnification_type.is_empty()
    }
    pub fn validate(&self) -> Result<()> {
        if self.requested_width_mm != 0.0
            && (!finite_positive(self.requested_width_mm)
                || self.requested_width_mm > MAX_PHYSICAL_SIZE_MM)
        {
            return Err(invalid(&[TAG_REQUESTED_IMAGE_SIZE]));
        }
        if !matches!(
            self.decimate_crop_behavior.as_str(),
            "" | DECIMATE | CROP | FAIL
        ) {
            return Err(invalid(&[TAG_REQUESTED_DECIMATE_CROP]));
        }
        if !matches!(
            self.magnification_type.as_str(),
            "" | "NONE" | "REPLICATE" | "BILINEAR" | "CUBIC"
        ) {
            return Err(invalid(&[TAG_MAGNIFICATION_TYPE]));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhysicalCapabilities {
    pub requested_size: Support,
    pub decimate: Support,
    pub crop: Support,
    pub fail: Support,
}
impl PhysicalCapabilities {
    pub fn validate(&self) -> Result<()> {
        if !self.requested_size.valid()
            || !self.decimate.valid()
            || !self.crop.valid()
            || !self.fail.valid()
        {
            return Err(Kind::InvalidAttributes.into());
        }
        Ok(())
    }
    pub fn behavior(&self, b: &str) -> Support {
        match b {
            DECIMATE => self.decimate,
            CROP => self.crop,
            FAIL => self.fail,
            _ => Support::Unknown,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ImageBoxArea {
    pub width_mm: f64,
    pub height_mm: f64,
    pub native_pixel_pitch_mm: f64,
}
impl ImageBoxArea {
    pub fn validate(&self) -> Result<()> {
        if !finite_positive(self.width_mm)
            || !finite_positive(self.height_mm)
            || self.width_mm > MAX_PHYSICAL_SIZE_MM
            || self.height_mm > MAX_PHYSICAL_SIZE_MM
            || (self.native_pixel_pitch_mm != 0.0
                && (!finite_positive(self.native_pixel_pitch_mm)
                    || self.native_pixel_pitch_mm > MAX_PHYSICAL_SIZE_MM))
        {
            return Err(Kind::PhysicalGeometry.into());
        }
        Ok(())
    }
}
pub type AreaFn =
    Arc<dyn Fn(&Ctx, &FilmBoxPresentation, i64, i64, i64) -> Result<ImageBoxArea> + Send + Sync>;
#[derive(Clone, Default)]
pub struct PhysicalServiceOptions {
    pub capabilities: PhysicalCapabilities,
    pub area: Option<AreaFn>,
    pub default_behavior: String,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhysicalPlan {
    pub resolved: bool,
    pub outcome: String,
    pub requested_width_mm: f64,
    pub requested_height_mm: f64,
    pub accepted_width_mm: f64,
    pub accepted_height_mm: f64,
    pub source_left: f64,
    pub source_top: f64,
    pub source_right: f64,
    pub source_bottom: f64,
    pub magnification_type: String,
}
pub(crate) fn finite_positive(v: f64) -> bool {
    v > 0.0 && v.is_finite()
}
fn c603(tags: &[Tag]) -> Error {
    failure(0xC603, "", Kind::PhysicalGeometry.into(), tags)
}
pub fn plan_image_box(
    rows: i64,
    columns: i64,
    aspect: [i64; 2],
    p: &ImageBoxPresentation,
    a: &ImageBoxArea,
    film_magnification: &str,
    default_behavior: &str,
) -> Result<PhysicalPlan> {
    p.validate()?;
    a.validate()?;
    if rows < 1 || columns < 1 || rows > 65535 || columns > 65535 {
        return Err(Kind::PhysicalGeometry.into());
    }
    let aspect = if aspect == [0, 0] { [1, 1] } else { aspect };
    if aspect[0] < 1 || aspect[1] < 1 || aspect[0] > 2147483647 || aspect[1] > 2147483647 {
        return Err(Kind::PhysicalGeometry.into());
    }
    let ratio = (rows as f64 / columns as f64) * (aspect[0] as f64 / aspect[1] as f64);
    if !finite_positive(ratio) {
        return Err(Kind::PhysicalGeometry.into());
    }
    let magnification = if p.magnification_type.is_empty() {
        film_magnification.to_string()
    } else {
        p.magnification_type.clone()
    };
    if !matches!(
        magnification.as_str(),
        "" | "NONE" | "REPLICATE" | "BILINEAR" | "CUBIC"
    ) {
        return Err(invalid(&[TAG_MAGNIFICATION_TYPE]));
    }
    let mut width = p.requested_width_mm;
    if width == 0.0 {
        if !p.decimate_crop_behavior.is_empty() || magnification == "NONE" {
            if a.native_pixel_pitch_mm == 0.0 {
                return Err(Kind::PhysicalGeometry.into());
            }
            width = columns as f64 * a.native_pixel_pitch_mm;
        } else {
            width = crate::fmath::min(a.width_mm, a.height_mm / ratio);
        }
    }
    let height = width * ratio;
    if !finite_positive(width) || !finite_positive(height) {
        return Err(Kind::PhysicalGeometry.into());
    }
    let mut out = PhysicalPlan {
        resolved: true,
        requested_width_mm: width,
        requested_height_mm: height,
        accepted_width_mm: width,
        accepted_height_mm: height,
        source_right: 1.0,
        source_bottom: 1.0,
        magnification_type: magnification.clone(),
        ..Default::default()
    };
    if width <= a.width_mm && height <= a.height_mm {
        return Ok(out);
    }
    let mut behavior = p.decimate_crop_behavior.clone();
    if behavior.is_empty() {
        behavior = default_behavior.to_string();
    }
    if behavior.is_empty() {
        behavior = FAIL.into();
    }
    match behavior.as_str() {
        FAIL => {
            return Err(c603(&[
                TAG_REQUESTED_IMAGE_SIZE,
                TAG_REQUESTED_DECIMATE_CROP,
            ]));
        }
        DECIMATE => {
            if magnification == "NONE" {
                return Err(c603(&[TAG_REQUESTED_DECIMATE_CROP, TAG_MAGNIFICATION_TYPE]));
            }
            let factor = crate::fmath::min(a.width_mm / width, a.height_mm / height);
            out.accepted_width_mm = width * factor;
            out.accepted_height_mm = height * factor;
        }
        CROP => {
            let mut keep_columns = columns;
            let mut keep_rows = rows;
            if width > a.width_mm {
                keep_columns = (columns as f64 * (a.width_mm / width)).floor() as i64;
            }
            if height > a.height_mm {
                keep_rows = (rows as f64 * (a.height_mm / height)).floor() as i64;
            }
            if keep_columns < 1 || keep_rows < 1 {
                return Err(c603(&[
                    TAG_REQUESTED_IMAGE_SIZE,
                    TAG_REQUESTED_DECIMATE_CROP,
                ]));
            }
            let left = (columns - keep_columns) / 2;
            let top = (rows - keep_rows) / 2;
            out.source_left = left as f64 / columns as f64;
            out.source_top = top as f64 / rows as f64;
            out.source_right = (left + keep_columns) as f64 / columns as f64;
            out.source_bottom = (top + keep_rows) as f64 / rows as f64;
            out.accepted_width_mm = width * (keep_columns as f64 / columns as f64);
            out.accepted_height_mm = height * (keep_rows as f64 / rows as f64);
        }
        _ => return Err(invalid(&[TAG_REQUESTED_DECIMATE_CROP])),
    }
    if !finite_positive(out.accepted_width_mm) || !finite_positive(out.accepted_height_mm) {
        return Err(Kind::PhysicalGeometry.into());
    }
    out.outcome = behavior;
    Ok(out)
}
fn ds_pattern(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - s;
    }
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s {
            return false;
        }
    }
    i == b.len()
}
fn is_pattern(s: &str) -> bool {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}
pub(crate) fn cs_pattern(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_' || b == b' ')
}
pub(crate) fn resident_text_values(
    ds: &Object,
    tag: Tag,
    vr: Vr,
    vm: usize,
    limit: usize,
) -> Result<Option<Vec<String>>> {
    let Some(e) = ds.get(tag) else {
        return Ok(None);
    };
    if e.vr() != vr {
        return Err(invalid(&[tag]));
    }
    let values: Vec<String> = match &e.value {
        Value::Raw(raw) => {
            let mut maximum = vm * limit + vm - 1;
            if vm > 1 {
                maximum += 1;
            }
            if raw.len() > maximum {
                return Err(invalid(&[tag]));
            }
            String::from_utf8_lossy(raw)
                .split('\\')
                .map(str::to_string)
                .collect()
        }
        Value::Strings(v) => v.clone(),
        _ => return Err(invalid(&[tag])),
    };
    if values.len() != vm {
        return Err(invalid(&[tag]));
    }
    let mut total = vm - 1;
    for v in &values {
        if v.len() > limit + 1 || v.contains('\\') {
            return Err(invalid(&[tag]));
        }
        total += v.len();
    }
    let mut out = Vec::with_capacity(vm);
    for (i, v) in values.iter().enumerate() {
        if v.len() > limit
            && !(vm > 1
                && i == vm - 1
                && v.len() == limit + 1
                && v.ends_with(' ')
                && total.is_multiple_of(2))
        {
            return Err(invalid(&[tag]));
        }
        let v = v.trim_matches(' ');
        let valid = match vr {
            Vr::DS => ds_pattern(v),
            Vr::IS => is_pattern(v),
            Vr::CS => cs_pattern(v),
            _ => false,
        };
        if !valid {
            return Err(invalid(&[tag]));
        }
        out.push(v.to_string());
    }
    Ok(Some(out))
}
pub(crate) fn presentation_text(ds: &Object, tag: Tag, vr: Vr) -> Result<(String, bool)> {
    match resident_text_values(ds, tag, vr, 1, 16)? {
        None => Ok((String::new(), false)),
        Some(v) => Ok((v[0].clone(), true)),
    }
}
pub(crate) fn parse_image_presentation(
    r: &mut AttributeReader<'_>,
    current: &ImageBoxPresentation,
) -> Result<ImageBoxPresentation> {
    let mut p = current.clone();
    for t in [
        TAG_REQUESTED_IMAGE_SIZE,
        TAG_REQUESTED_DECIMATE_CROP,
        TAG_MAGNIFICATION_TYPE,
    ] {
        r.mark(t);
    }
    let (value, ok) = presentation_text(r.ds, TAG_REQUESTED_IMAGE_SIZE, Vr::DS)?;
    if ok {
        match crate::numtext::parse_float(&value) {
            Ok(n) if finite_positive(n) && n <= MAX_PHYSICAL_SIZE_MM => p.requested_width_mm = n,
            _ => return Err(invalid(&[TAG_REQUESTED_IMAGE_SIZE])),
        }
    }
    let (value, ok) = presentation_text(r.ds, TAG_REQUESTED_DECIMATE_CROP, Vr::CS)?;
    if ok {
        p.decimate_crop_behavior = value.trim_matches(' ').to_string();
    }
    let (value, ok) = presentation_text(r.ds, TAG_MAGNIFICATION_TYPE, Vr::CS)?;
    if ok {
        p.magnification_type = value.trim_matches(' ').to_string();
    }
    p.validate()?;
    Ok(p)
}
pub fn build_image_box_with_presentation(
    position: i64,
    image: Option<&Image>,
    polarity: &str,
    p: &ImageBoxPresentation,
) -> Result<Object> {
    p.validate()?;
    let mut ds = build_image_box(position, image, polarity)?;
    if p.requested_width_mm != 0.0 {
        let mut value = crate::numtext::format_float(p.requested_width_mm, 'g', -1);
        if value.len() > 16 {
            value = crate::numtext::format_float(p.requested_width_mm, 'g', 9);
        }
        ds.put(text(TAG_REQUESTED_IMAGE_SIZE, Vr::DS, &value));
    }
    if !p.decimate_crop_behavior.is_empty() {
        ds.put(text(
            TAG_REQUESTED_DECIMATE_CROP,
            Vr::CS,
            &p.decimate_crop_behavior,
        ));
    }
    if !p.magnification_type.is_empty() {
        ds.put(text(TAG_MAGNIFICATION_TYPE, Vr::CS, &p.magnification_type));
    }
    Ok(ds)
}
