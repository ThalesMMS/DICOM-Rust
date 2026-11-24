use super::object::{
    ObjectError, ObjectErrorKind, lut_from_item, print_density_from_object, raw_uint16, trim_space,
    u16_at,
};
use super::{
    BitmapShutter, CircleShutter, LutOptions, Overlay, PolygonShutter, Presentation, RectShutter,
    Shutter, ShutterPoint,
};
use crate::object::{Element, Object, Tag, Vr};
use crate::{numtext, textfmt};
use std::collections::HashSet;
const INVALID_SHUTTER: &str = "dicom/display: invalid display shutter geometry";
const UNSUPPORTED_SHUTTER: &str = "dicom/display: unsupported display shutter shape";
const UNSUPPORTED_GSPS: &str =
    "dicom/display: Grayscale Softcopy Presentation State is not supported";
const INVALID_OVERLAY: &str = "dicom: invalid overlay";
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_PRESENTATION_LUT_SEQUENCE: Tag = Tag::new(0x2050, 0x0010);
const TAG_PRESENTATION_LUT_SHAPE: Tag = Tag::new(0x2050, 0x0020);
const PRESENTATION_STATE_UID_PREFIX: &str = "1.2.840.10008.5.1.4.1.1.11.";
const TAG_SHUTTER_SHAPE: Tag = Tag::new(0x0018, 0x1600);
const TAG_SHUTTER_LEFT: Tag = Tag::new(0x0018, 0x1602);
const TAG_SHUTTER_RIGHT: Tag = Tag::new(0x0018, 0x1604);
const TAG_SHUTTER_UPPER: Tag = Tag::new(0x0018, 0x1606);
const TAG_SHUTTER_LOWER: Tag = Tag::new(0x0018, 0x1608);
const TAG_SHUTTER_CIRCLE_CENTER: Tag = Tag::new(0x0018, 0x1610);
const TAG_SHUTTER_CIRCLE_RADIUS: Tag = Tag::new(0x0018, 0x1612);
const TAG_SHUTTER_POLYGON_VERTICES: Tag = Tag::new(0x0018, 0x1620);
const TAG_SHUTTER_PRESENTATION_VALUE: Tag = Tag::new(0x0018, 0x1622);
const TAG_SHUTTER_OVERLAY_GROUP: Tag = Tag::new(0x0018, 0x1623);
const TAG_SHUTTER_PRESENTATION_COLOR: Tag = Tag::new(0x0018, 0x1624);
const OVERLAY_ROWS: u16 = 0x0010;
const OVERLAY_COLUMNS: u16 = 0x0011;
const OVERLAY_FRAMES: u16 = 0x0015;
const OVERLAY_DESCRIPTION: u16 = 0x0022;
const OVERLAY_TYPE: u16 = 0x0040;
const OVERLAY_SUBTYPE: u16 = 0x0045;
const OVERLAY_ORIGIN: u16 = 0x0050;
const OVERLAY_FRAME_ORIGIN: u16 = 0x0051;
const OVERLAY_BITS_ALLOCATED: u16 = 0x0100;
const OVERLAY_BIT_POSITION: u16 = 0x0102;
const OVERLAY_LABEL: u16 = 0x1500;
const ROI_AREA: u16 = 0x1301;
const ROI_MEAN: u16 = 0x1302;
const ROI_STANDARD_DEVIATION: u16 = 0x1303;
const OVERLAY_DATA: u16 = 0x3000;
pub(super) const OVERLAY_GROUP_FIRST: u16 = 0x6000;
pub(super) const OVERLAY_GROUP_LAST: u16 = 0x601E;
const TAG_IMAGE_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_IMAGE_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_IMAGE_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_IMAGE_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_IMAGE_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_IMAGE_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const MAX_OVERLAY_US: i64 = (1 << 16) - 1;
const MIN_OVERLAY_SS: i64 = -(1 << 15);
const MAX_OVERLAY_SS: i64 = (1 << 15) - 1;
pub(super) fn hex_0x(v: u16) -> String {
    format!("0x{v:02x}")
}
fn shutter_error(detail: impl std::fmt::Display) -> ObjectError {
    ObjectError::new(
        ObjectErrorKind::InvalidShutter,
        format!("{INVALID_SHUTTER}: {detail}"),
    )
}
fn overlay_error(detail: impl std::fmt::Display) -> ObjectError {
    ObjectError::new(
        ObjectErrorKind::InvalidOverlay,
        format!("{INVALID_OVERLAY}: {detail}"),
    )
}
fn plane_bits(rows: i64, columns: i64) -> Option<u64> {
    if rows <= 0 || columns <= 0 {
        return None;
    }
    (rows as u64).checked_mul(columns as u64)
}
fn planes_fit(data_bytes: usize, rows: i64, columns: i64, frames: i64) -> bool {
    let Some(plane) = plane_bits(rows, columns) else {
        return false;
    };
    if frames <= 0 {
        return false;
    }
    match plane.checked_mul(frames as u64) {
        Some(total) => u128::from(total) <= data_bytes as u128 * 8,
        None => false,
    }
}
impl Overlay {
    pub fn frame_count(&self) -> i64 {
        self.frames.max(1)
    }
    pub fn validate(&self) -> Result<(), ObjectError> {
        let reason = if self.rows < 1
            || self.rows > MAX_OVERLAY_US
            || self.columns < 1
            || self.columns > MAX_OVERLAY_US
        {
            format!(
                "Overlay Rows/Columns {}x{} outside 1-{MAX_OVERLAY_US}",
                self.rows, self.columns
            )
        } else if self.origin_row < MIN_OVERLAY_SS
            || self.origin_row > MAX_OVERLAY_SS
            || self.origin_col < MIN_OVERLAY_SS
            || self.origin_col > MAX_OVERLAY_SS
        {
            format!(
                "Overlay Origin ({},{}) outside the SS range",
                self.origin_row, self.origin_col
            )
        } else if self.frame_origin < 0 || self.frame_origin > MAX_OVERLAY_US {
            format!(
                "Image Frame Origin {} outside the US range",
                self.frame_origin
            )
        } else if self.embedded && (self.bit_position < 0 || self.bit_position > 15) {
            format!(
                "Overlay Bit Position {} outside a 16-bit sample",
                self.bit_position
            )
        } else if !planes_fit(self.data.len(), self.rows, self.columns, self.frame_count()) {
            format!(
                "Overlay Data has {} bytes, too few for {} frame(s) of {}x{} bits",
                self.data.len(),
                self.frame_count(),
                self.rows,
                self.columns
            )
        } else {
            return Ok(());
        };
        Err(overlay_error(format_args!(
            "group {}: {reason}",
            hex_0x(self.group)
        )))
    }
    pub fn frame(&self, index: i64) -> Option<Overlay> {
        if index < 0
            || index >= self.frame_count()
            || !planes_fit(self.data.len(), self.rows, self.columns, index + 1)
        {
            return None;
        }
        let plane = plane_bits(self.rows, self.columns)?;
        let start = plane * index as u64;
        let mut out = self.clone();
        out.frames = 1;
        out.frame_origin = 1;
        let n = plane.div_ceil(8) as usize;
        let first = (start / 8) as usize;
        let shift = (start % 8) as u32;
        out.data = (0..n)
            .map(|j| {
                let mut b = self.data[first + j] >> shift;
                if shift != 0 && first + j + 1 < self.data.len() {
                    b |= self.data[first + j + 1] << (8 - shift);
                }
                b
            })
            .collect();
        let rest = plane % 8;
        if rest != 0 {
            let last = out.data.len() - 1;
            out.data[last] &= (1u8 << rest) - 1;
        }
        Some(out)
    }
    pub fn for_image_frame(&self, image_frame: i64) -> Option<Overlay> {
        let origin = self.frame_origin.max(1);
        self.frame(image_frame - (origin - 1))
    }
    pub fn unpacked(&self) -> Option<Vec<u8>> {
        if !planes_fit(self.data.len(), self.rows, self.columns, 1) {
            return None;
        }
        let plane = plane_bits(self.rows, self.columns)?;
        if plane > i64::MAX as u64 {
            return None;
        }
        Some(
            (0..plane as usize)
                .map(|i| {
                    if self.data[i / 8] & (1 << (i % 8)) != 0 {
                        255
                    } else {
                        0
                    }
                })
                .collect(),
        )
    }
}
pub fn presentation_from_object(
    obj: &Object,
    opts: LutOptions,
) -> (Presentation, Option<ObjectError>) {
    let mut pres = Presentation::default();
    if let Some(sop) = obj.get_uid(TAG_SOP_CLASS_UID)
        && trim_space(&sop).starts_with(PRESENTATION_STATE_UID_PREFIX)
    {
        return (
            pres,
            Some(ObjectError::new(
                ObjectErrorKind::UnsupportedGsps,
                UNSUPPORTED_GSPS,
            )),
        );
    }
    if let Some(items) = obj.get_sequence(TAG_PRESENTATION_LUT_SEQUENCE)
        && let Some(first) = items.first()
    {
        match lut_from_item(Some(first), false, opts) {
            Ok(lut) => pres.lut = Some(lut),
            Err(e) => return (pres, Some(e)),
        }
    }
    if let Some(shape) = obj.get_string(TAG_PRESENTATION_LUT_SHAPE) {
        match textfmt::to_upper(trim_space(&shape)).as_str() {
            "INVERSE" => pres.invert = true,
            "LIN OD" if pres.lut.is_none() => {
                match print_density_from_object(obj).lin_od_presentation_lut(8) {
                    Ok(lut) => {
                        pres.lut = Some(lut);
                        pres.lin_od = true;
                    }
                    Err(e) => {
                        return (
                            pres,
                            Some(ObjectError::new(
                                ObjectErrorKind::Calibration,
                                e.to_string(),
                            )),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    let shutter = match shutter_from_object(obj) {
        Ok(s) => s,
        Err(e) => return (pres, Some(e)),
    };
    let skip: HashSet<u16> = shutter
        .as_ref()
        .and_then(|s| s.bitmap.as_ref())
        .map(|b| b.group)
        .into_iter()
        .collect();
    pres.shutter = shutter;
    let (overlays, err) = overlays_from(obj, &skip);
    pres.overlays = overlays;
    (pres, err)
}
pub fn validate_shutter(shutter: Option<&Shutter>) -> Result<(), ObjectError> {
    let Some(shutter) = shutter else {
        return Ok(());
    };
    let mut shapes = 0;
    if let Some(r) = &shutter.rectangle {
        shapes += 1;
        if r.left > r.right || r.upper > r.lower {
            return Err(shutter_error(format_args!(
                "invalid rectangular shutter {{Left:{} Right:{} Upper:{} Lower:{}}}",
                r.left, r.right, r.upper, r.lower
            )));
        }
    }
    if let Some(c) = &shutter.circle {
        shapes += 1;
        if c.radius <= 0 {
            return Err(shutter_error("circular shutter radius must be positive"));
        }
    }
    if let Some(p) = &shutter.polygon {
        shapes += 1;
        if p.vertices.len() < 3 {
            return Err(shutter_error(
                "polygonal shutter requires at least three vertices",
            ));
        }
    }
    if let Some(b) = &shutter.bitmap {
        shapes += 1;
        if !(OVERLAY_GROUP_FIRST..=OVERLAY_GROUP_LAST).contains(&b.group) || b.group % 2 != 0 {
            return Err(shutter_error(format_args!(
                "invalid bitmap shutter overlay group {}",
                hex_0x(b.group)
            )));
        }
        if let Err(e) = b.overlay.validate() {
            return Err(shutter_error(format_args!("bitmap shutter overlay: {e}")));
        }
    }
    if shapes == 0 {
        return Err(shutter_error("shutter has no shape"));
    }
    if shapes > 3 {
        return Err(shutter_error(format_args!(
            "Shutter Shape VM is 1-3, got {shapes} shapes"
        )));
    }
    Ok(())
}
fn any_tag(obj: &Object, tags: &[Tag]) -> bool {
    tags.iter().any(|&t| obj.has(t))
}
fn raw_uint16s(obj: &Object, tag: Tag) -> Option<Vec<u16>> {
    let raw = obj.get_raw(tag)?;
    if raw.is_empty() || raw.len() % 2 != 0 {
        return None;
    }
    Some(
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16_at(obj, p))
            .collect(),
    )
}
pub fn shutter_from_object(obj: &Object) -> Result<Option<Shutter>, ObjectError> {
    let shapes = obj.get_strings(TAG_SHUTTER_SHAPE).unwrap_or_default();
    if shapes.is_empty() {
        if any_tag(
            obj,
            &[
                TAG_SHUTTER_LEFT,
                TAG_SHUTTER_RIGHT,
                TAG_SHUTTER_UPPER,
                TAG_SHUTTER_LOWER,
                TAG_SHUTTER_CIRCLE_CENTER,
                TAG_SHUTTER_CIRCLE_RADIUS,
                TAG_SHUTTER_POLYGON_VERTICES,
                TAG_SHUTTER_PRESENTATION_VALUE,
                TAG_SHUTTER_OVERLAY_GROUP,
                TAG_SHUTTER_PRESENTATION_COLOR,
            ],
        ) {
            return Err(shutter_error(
                "shutter attributes are present without Shutter Shape",
            ));
        }
        return Ok(None);
    }
    let mut sh = Shutter::default();
    if let Some(p) = raw_uint16(obj, TAG_SHUTTER_PRESENTATION_VALUE) {
        sh.presentation_p_value = p;
        sh.presentation_p_value_defined = true;
        sh.presentation_value = (f64::from(p) * 255.0 / 65535.0).round() as u8;
    }
    match raw_uint16s(obj, TAG_SHUTTER_PRESENTATION_COLOR) {
        Some(c) if c.len() == 3 => {
            sh.presentation_color_cielab = [c[0], c[1], c[2]];
            sh.presentation_color_cielab_defined = true;
        }
        _ if obj.has(TAG_SHUTTER_PRESENTATION_COLOR) => {
            return Err(shutter_error(
                "shutter presentation color requires three CIELab values",
            ));
        }
        _ => {}
    }
    for shape in &shapes {
        match textfmt::to_upper(trim_space(shape)).as_str() {
            "RECTANGULAR" => sh.rectangle = Some(rect_shutter(obj)?),
            "CIRCULAR" => sh.circle = Some(circle_shutter(obj)?),
            "POLYGONAL" => sh.polygon = Some(polygon_shutter(obj)?),
            "BITMAP" => sh.bitmap = Some(bitmap_shutter(obj)?),
            _ => {
                return Err(ObjectError::new(
                    ObjectErrorKind::UnsupportedShutter,
                    format!("{UNSUPPORTED_SHUTTER}: {}", textfmt::quote(shape)),
                ));
            }
        }
    }
    if sh.rectangle.is_none()
        && any_tag(
            obj,
            &[
                TAG_SHUTTER_LEFT,
                TAG_SHUTTER_RIGHT,
                TAG_SHUTTER_UPPER,
                TAG_SHUTTER_LOWER,
            ],
        )
    {
        return Err(shutter_error(
            "rectangular geometry is present without RECTANGULAR Shutter Shape",
        ));
    }
    if sh.circle.is_none() && any_tag(obj, &[TAG_SHUTTER_CIRCLE_CENTER, TAG_SHUTTER_CIRCLE_RADIUS])
    {
        return Err(shutter_error(
            "circular geometry is present without CIRCULAR Shutter Shape",
        ));
    }
    if sh.polygon.is_none() && obj.has(TAG_SHUTTER_POLYGON_VERTICES) {
        return Err(shutter_error(
            "polygon geometry is present without POLYGONAL Shutter Shape",
        ));
    }
    if sh.bitmap.is_none() && obj.has(TAG_SHUTTER_OVERLAY_GROUP) {
        return Err(shutter_error(
            "shutter overlay group is present without BITMAP Shutter Shape",
        ));
    }
    validate_shutter(Some(&sh))?;
    Ok(Some(sh))
}
fn rect_shutter(obj: &Object) -> Result<RectShutter, ObjectError> {
    match (
        obj.get_int(TAG_SHUTTER_LEFT),
        obj.get_int(TAG_SHUTTER_RIGHT),
        obj.get_int(TAG_SHUTTER_UPPER),
        obj.get_int(TAG_SHUTTER_LOWER),
    ) {
        (Ok(left), Ok(right), Ok(upper), Ok(lower)) => Ok(RectShutter {
            left,
            right,
            upper,
            lower,
        }),
        _ => Err(shutter_error("rectangular shutter requires four edges")),
    }
}
fn circle_shutter(obj: &Object) -> Result<CircleShutter, ObjectError> {
    match (
        obj.get_ints(TAG_SHUTTER_CIRCLE_CENTER),
        obj.get_int(TAG_SHUTTER_CIRCLE_RADIUS),
    ) {
        (Ok(center), Ok(radius)) if center.len() >= 2 => Ok(CircleShutter {
            center_row: center[0],
            center_col: center[1],
            radius,
        }),
        _ => Err(shutter_error("circular shutter requires center and radius")),
    }
}
fn polygon_shutter(obj: &Object) -> Result<PolygonShutter, ObjectError> {
    match obj.get_ints(TAG_SHUTTER_POLYGON_VERTICES) {
        Ok(values) if values.len() >= 6 && values.len() % 2 == 0 => Ok(PolygonShutter {
            vertices: values
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| ShutterPoint {
                    row: p[0],
                    col: p[1],
                })
                .collect(),
        }),
        _ => Err(shutter_error(
            "polygonal shutter requires >=3 (row,column) vertices",
        )),
    }
}
fn bitmap_shutter(obj: &Object) -> Result<BitmapShutter, ObjectError> {
    let group = raw_uint16(obj, TAG_SHUTTER_OVERLAY_GROUP)
        .ok_or_else(|| shutter_error("bitmap shutter requires Shutter Overlay Group"))?;
    if !(OVERLAY_GROUP_FIRST..=OVERLAY_GROUP_LAST).contains(&group) || group % 2 != 0 {
        return Err(shutter_error(format_args!(
            "invalid shutter overlay group {}",
            hex_0x(group)
        )));
    }
    let mut overlay = match overlay_from_group(obj, group) {
        Err(e) => return Err(shutter_error(format_args!("bitmap shutter overlay: {e}"))),
        Ok(None) => {
            return Err(shutter_error(
                "bitmap shutter requires a valid referenced overlay",
            ));
        }
        Ok(Some(o)) => o,
    };
    let plane = plane_bits(overlay.rows, overlay.columns).unwrap_or(0);
    overlay.data.truncate(plane.div_ceil(8) as usize);
    overlay.frames = 1;
    overlay.frame_origin = 1;
    Ok(BitmapShutter { group, overlay })
}
pub fn overlays_from_object(obj: &Object) -> (Vec<Overlay>, Option<ObjectError>) {
    overlays_from(obj, &HashSet::new())
}
fn overlays_from(obj: &Object, skip: &HashSet<u16>) -> (Vec<Overlay>, Option<ObjectError>) {
    let mut overlays = Vec::new();
    let mut errs = Vec::new();
    for group in (OVERLAY_GROUP_FIRST..=OVERLAY_GROUP_LAST).step_by(2) {
        if skip.contains(&group) {
            continue;
        }
        match overlay_from_group(obj, group) {
            Err(e) => errs.push(Some(e)),
            Ok(Some(o)) => overlays.push(o),
            Ok(None) => {}
        }
    }
    (overlays, ObjectError::join(errs))
}
fn overlay_from_group(obj: &Object, group: u16) -> Result<Option<Overlay>, ObjectError> {
    let Some(data) = obj.get_raw(Tag::new(group, OVERLAY_DATA)) else {
        return Ok(None);
    };
    if data.is_empty() {
        return Ok(None);
    }
    if let Some(bits) = overlay_uint16(obj, group, OVERLAY_BITS_ALLOCATED)
        && bits != 1
    {
        return Ok(None);
    }
    let rows = overlay_uint16(obj, group, OVERLAY_ROWS).unwrap_or(0);
    let columns = overlay_uint16(obj, group, OVERLAY_COLUMNS).unwrap_or(0);
    let (origin_row, origin_col) = overlay_origin(obj, group);
    let mut overlay = Overlay {
        rows: i64::from(rows),
        columns: i64::from(columns),
        origin_row,
        origin_col,
        data,
        ..Overlay::default()
    };
    describe_overlay(obj, group, &mut overlay);
    overlay.validate()?;
    let swap = obj
        .get(Tag::new(group, OVERLAY_DATA))
        .is_some_and(|e: &Element| e.vr() == Vr::OW && obj.value_big_endian());
    if swap {
        for pair in overlay.data.as_chunks_mut::<2>().0 {
            pair.swap(0, 1);
        }
    }
    Ok(Some(overlay))
}
fn overlay_uint16(obj: &Object, group: u16, element: u16) -> Option<u16> {
    raw_uint16(obj, Tag::new(group, element))
}
fn overlay_origin(obj: &Object, group: u16) -> (i64, i64) {
    match obj.get_raw(Tag::new(group, OVERLAY_ORIGIN)) {
        Some(raw) if raw.len() >= 4 => (
            i64::from(u16_at(obj, &raw[0..2]) as i16),
            i64::from(u16_at(obj, &raw[2..4]) as i16),
        ),
        _ => (1, 1),
    }
}
fn overlay_string(obj: &Object, group: u16, element: u16) -> String {
    obj.get_string(Tag::new(group, element))
        .map(|v| trim_space(&v).to_owned())
        .unwrap_or_default()
}
fn atoi(s: &str) -> Option<i64> {
    numtext::parse_int(s).ok()
}
fn describe_overlay(obj: &Object, group: u16, overlay: &mut Overlay) {
    overlay.group = group;
    overlay.overlay_type = textfmt::to_upper(&overlay_string(obj, group, OVERLAY_TYPE));
    overlay.subtype = overlay_string(obj, group, OVERLAY_SUBTYPE);
    overlay.label = overlay_string(obj, group, OVERLAY_LABEL);
    overlay.description = overlay_string(obj, group, OVERLAY_DESCRIPTION);
    overlay.frames = 1;
    if let Some(frames) = atoi(&overlay_string(obj, group, OVERLAY_FRAMES))
        && frames > 0
    {
        overlay.frames = frames;
    }
    overlay.frame_origin = 1;
    if let Some(origin) = overlay_uint16(obj, group, OVERLAY_FRAME_ORIGIN)
        && origin > 0
    {
        overlay.frame_origin = i64::from(origin);
    }
    let area = numtext::parse_int_value(&overlay_string(obj, group, ROI_AREA));
    let mean = numtext::parse_float_value(&overlay_string(obj, group, ROI_MEAN));
    let sd = numtext::parse_float_value(&overlay_string(obj, group, ROI_STANDARD_DEVIATION));
    if area.1.is_none() || mean.1.is_none() || sd.1.is_none() {
        overlay.has_roi_statistics = true;
        overlay.roi_area = area.0;
        overlay.roi_mean = mean.0;
        overlay.roi_standard_deviation = sd.0;
    }
}
fn stored_bits(obj: &Object) -> Option<(u16, u16)> {
    let stored = raw_uint16(obj, TAG_IMAGE_BITS_STORED)?;
    let high = raw_uint16(obj, TAG_IMAGE_HIGH_BIT)?;
    if stored == 0 || u32::from(high) + 1 < u32::from(stored) {
        return None;
    }
    Some((high + 1 - stored, high))
}
pub fn embedded_overlays(
    obj: &Object,
    native: &[u8],
    big_endian: bool,
) -> (Vec<Overlay>, Option<ObjectError>) {
    let rows = raw_uint16(obj, TAG_IMAGE_ROWS);
    let columns = raw_uint16(obj, TAG_IMAGE_COLUMNS);
    let bits_allocated = raw_uint16(obj, TAG_IMAGE_BITS_ALLOCATED);
    let mut image_frames = 1;
    if let Some(value) = obj.get_string(TAG_IMAGE_NUMBER_OF_FRAMES)
        && let Some(n) = atoi(trim_space(&value))
        && n > 0
    {
        image_frames = n;
    }
    let mut out = Vec::new();
    for group in (OVERLAY_GROUP_FIRST..=OVERLAY_GROUP_LAST).step_by(2) {
        match overlay_uint16(obj, group, OVERLAY_BITS_ALLOCATED) {
            Some(bits) if bits > 1 && !obj.has(Tag::new(group, OVERLAY_DATA)) => {}
            _ => continue,
        }
        let (Some(rows), Some(columns), Some(bits_allocated)) = (rows, columns, bits_allocated)
        else {
            return (out, Some(embedded_layout_error(group)));
        };
        if rows == 0 || columns == 0 || (bits_allocated != 8 && bits_allocated != 16) {
            return (out, Some(embedded_layout_error(group)));
        }
        let position = match overlay_uint16(obj, group, OVERLAY_BIT_POSITION) {
            Some(p) if p < bits_allocated => p,
            _ => {
                return (
                    out,
                    Some(overlay_error(format_args!(
                        "embedded overlay {} bit position",
                        hex_0x(group)
                    ))),
                );
            }
        };
        if let Some((low, high)) = stored_bits(obj)
            && position >= low
            && position <= high
        {
            return (
                out,
                Some(overlay_error(format_args!(
                    "embedded overlay {} bit position {position} lies in the stored bits {low}-{high}",
                    hex_0x(group)
                ))),
            );
        }
        let mut overlay = Overlay {
            rows: i64::from(rows),
            columns: i64::from(columns),
            embedded: true,
            bit_position: i64::from(position),
            ..Overlay::default()
        };
        (overlay.origin_row, overlay.origin_col) = overlay_origin(obj, group);
        describe_overlay(obj, group, &mut overlay);
        if !obj.has(Tag::new(group, OVERLAY_FRAMES)) {
            overlay.frames = image_frames;
        }
        let width = usize::from(bits_allocated / 8);
        let samples = (native.len() / width) as u64;
        let plane = u64::from(rows) * u64::from(columns);
        let first = ((overlay.frame_origin - 1) as u64).wrapping_mul(plane);
        let total = (overlay.frame_count() as u64).checked_mul(plane);
        let Some(total) = total.filter(|&t| first <= samples && t <= samples - first) else {
            return (
                out,
                Some(overlay_error(format_args!(
                    "embedded overlay {} covers {} frame(s) from image frame {}, pixel data has {samples} samples of {plane} per frame",
                    hex_0x(group),
                    overlay.frame_count(),
                    overlay.frame_origin
                ))),
            );
        };
        let mut data = vec![0u8; total.div_ceil(8) as usize];
        let base = first as usize;
        for i in 0..total as usize {
            let offset = (base + i) * width;
            let sample = if width == 2 {
                let pair = [native[offset], native[offset + 1]];
                if big_endian {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            } else {
                u16::from(native[offset])
            };
            if sample & (1 << position) != 0 {
                data[i / 8] |= 1 << (i % 8);
            }
        }
        overlay.data = data;
        if let Err(e) = overlay.validate() {
            return (out, Some(e));
        }
        out.push(overlay);
    }
    (out, None)
}
fn embedded_layout_error(group: u16) -> ObjectError {
    overlay_error(format_args!(
        "embedded overlay {} needs non-empty 8- or 16-bit image samples",
        hex_0x(group)
    ))
}
pub fn embedded_overlays_for_frame(
    obj: &Object,
    frame_samples: &[u8],
    big_endian: bool,
    image_frame: i64,
) -> (Vec<Overlay>, Option<ObjectError>) {
    if image_frame < 0 {
        return (Vec::new(), None);
    }
    let (Some(rows), Some(columns), Some(bits_allocated)) = (
        raw_uint16(obj, TAG_IMAGE_ROWS),
        raw_uint16(obj, TAG_IMAGE_COLUMNS),
        raw_uint16(obj, TAG_IMAGE_BITS_ALLOCATED),
    ) else {
        return (Vec::new(), None);
    };
    if bits_allocated != 8 && bits_allocated != 16 {
        return (Vec::new(), None);
    }
    let need = u64::from(rows) * u64::from(columns) * u64::from(bits_allocated / 8);
    if (frame_samples.len() as u64) < need {
        return (
            Vec::new(),
            Some(overlay_error(format_args!(
                "frame has {} bytes, needs {need}",
                frame_samples.len()
            ))),
        );
    }
    let single = frame_samples[..need as usize].to_vec();
    let (all, err) = embedded_overlays(&single_frame_view(obj), &single, big_endian);
    if let Some(err) = err {
        return (Vec::new(), Some(err));
    }
    let mut out = Vec::new();
    for mut overlay in all {
        let (frames, origin) = embedded_coverage(obj, overlay.group);
        let index = image_frame - (origin - 1);
        if frames > 0 && (index < 0 || index >= frames) {
            continue;
        }
        overlay.frames = 1;
        overlay.frame_origin = 1;
        out.push(overlay);
    }
    (out, None)
}
fn single_frame_view(obj: &Object) -> Object {
    let elements = obj
        .elements()
        .iter()
        .filter(|e| {
            let tag = e.tag();
            if tag == TAG_IMAGE_NUMBER_OF_FRAMES || tag == TAG_PIXEL_DATA {
                return false;
            }
            !((OVERLAY_GROUP_FIRST..=OVERLAY_GROUP_LAST).contains(&tag.group)
                && (tag.element == OVERLAY_FRAMES || tag.element == OVERLAY_FRAME_ORIGIN))
        })
        .cloned()
        .collect();
    let mut view = Object::from_elements(elements);
    view.set_value_big_endian(obj.value_big_endian());
    view
}
fn embedded_coverage(obj: &Object, group: u16) -> (i64, i64) {
    let mut frames = 0;
    let mut origin = 1;
    if let Some(value) = atoi(&overlay_string(obj, group, OVERLAY_FRAMES))
        && value > 0
    {
        frames = value;
    }
    if let Some(value) = overlay_uint16(obj, group, OVERLAY_FRAME_ORIGIN)
        && value > 0
    {
        origin = i64::from(value);
    }
    (frames, origin)
}
