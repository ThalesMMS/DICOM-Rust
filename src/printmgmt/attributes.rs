use super::*;
use crate::object::{DataSet, ElementHeader, UNDEFINED_LENGTH};
use std::collections::HashSet;
pub(crate) struct AttributeReader<'a> {
    pub(crate) ds: &'a Object,
    pub(crate) used: HashSet<Tag>,
}
impl<'a> AttributeReader<'a> {
    pub(crate) fn new(ds: &'a Object) -> Self {
        AttributeReader {
            ds,
            used: HashSet::new(),
        }
    }
    pub(crate) fn has(&self, tag: Tag) -> bool {
        self.ds.get(tag).is_some()
    }
    pub(crate) fn mark(&mut self, tag: Tag) {
        self.used.insert(tag);
    }
    pub(crate) fn text(&mut self, tag: Tag) -> Result<(String, bool)> {
        self.used.insert(tag);
        if !self.has(tag) {
            return Ok((String::new(), false));
        }
        let value = match self.ds.lookup_string(tag) {
            Ok(v) => v,
            Err(_) => match self.ds.lookup_strings(tag) {
                Ok(values) => values.join("\\"),
                Err(_) => return Err(invalid(&[tag])),
            },
        };
        Ok((value.trim().to_string(), true))
    }
    pub(crate) fn enumerated(&mut self, tag: Tag, allowed: &[&str]) -> Result<String> {
        let (value, ok) = self.text(tag)?;
        if !ok || value.is_empty() {
            return Ok(value);
        }
        if allowed.contains(&value.as_str()) {
            return Ok(value);
        }
        Err(invalid(&[tag]))
    }
    pub(crate) fn number(&mut self, tag: Tag) -> Result<(i64, bool)> {
        self.used.insert(tag);
        uint16_of(self.ds, tag)
    }
    pub(crate) fn ignored(&self) -> Vec<Tag> {
        self.ds
            .elements()
            .iter()
            .map(|e| e.tag())
            .filter(|t| !self.used.contains(t))
            .collect()
    }
}
pub(crate) fn uint16_of(ds: &Object, tag: Tag) -> Result<(i64, bool)> {
    let Some(e) = ds.get(tag) else {
        return Ok((0, false));
    };
    match &e.value {
        Value::Raw(raw) if raw.len() == 2 => {
            let v = if ds.value_big_endian() {
                u16::from_be_bytes([raw[0], raw[1]])
            } else {
                u16::from_le_bytes([raw[0], raw[1]])
            };
            return Ok((v as i64, true));
        }
        Value::Raw(raw) if raw.is_empty() => return Ok((0, false)),
        Value::Nil => return Ok((0, false)),
        _ => {}
    }
    Err(invalid(&[tag]))
}
const FILM_SIZES: [&str; 12] = [
    "8INX10IN",
    "8_5INX11IN",
    "10INX12IN",
    "10INX14IN",
    "11INX14IN",
    "11INX17IN",
    "14INX14IN",
    "14INX17IN",
    "24CMX24CM",
    "24CMX30CM",
    "A4",
    "A3",
];
const MEDIUMS: [&str; 5] = [
    "PAPER",
    "CLEAR FILM",
    "BLUE FILM",
    "MAMMO CLEAR FILM",
    "MAMMO BLUE FILM",
];
pub(crate) fn parse_film_session(
    ds: Option<&Object>,
    current: &FilmSessionPresentation,
    max_copies: i64,
) -> Result<(FilmSessionPresentation, bool, Vec<Tag>)> {
    let mut out = current.clone();
    let Some(ds) = ds else {
        return Ok((out, false, Vec::new()));
    };
    let mut r = AttributeReader::new(ds);
    r.mark(TAG_SPECIFIC_CHARACTER_SET);
    let (copies, ok) = r.text(TAG_NUMBER_OF_COPIES)?;
    if ok && !copies.is_empty() {
        match parse_decimal_int(&copies) {
            Some(n) if n >= 1 && n <= max_copies => out.number_of_copies = n,
            _ => return Err(invalid(&[TAG_NUMBER_OF_COPIES])),
        }
    }
    let v = r.enumerated(TAG_PRINT_PRIORITY, &["HIGH", "MED", "LOW"])?;
    if !v.is_empty() {
        out.print_priority = v;
    }
    let v = r.enumerated(TAG_MEDIUM_TYPE, &MEDIUMS)?;
    if !v.is_empty() {
        out.medium_type = v;
    }
    let (destination, ok) = r.text(TAG_FILM_DESTINATION)?;
    if ok && !destination.is_empty() {
        if destination != "MAGAZINE"
            && destination != "PROCESSOR"
            && !destination.starts_with("BIN_")
        {
            return Err(invalid(&[TAG_FILM_DESTINATION]));
        }
        out.film_destination = destination;
    }
    let (label, ok) = r.text(TAG_FILM_SESSION_LABEL)?;
    if ok {
        out.label = label;
    }
    let (owner, ok) = r.text(TAG_OWNER_ID)?;
    if ok {
        out.owner_id = owner;
    }
    let memory = r.has(TAG_MEMORY_ALLOCATION);
    r.mark(TAG_MEMORY_ALLOCATION);
    Ok((out, memory, r.ignored()))
}
pub(crate) fn parse_decimal_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok()
}
pub(crate) fn parse_display_format(value: &str, max_boxes: i64) -> Result<(i64, i64)> {
    let bad = || invalid(&[TAG_IMAGE_DISPLAY_FORMAT]);
    let Some((kind, layout)) = value.split_once('\\') else {
        return Err(bad());
    };
    if kind != "STANDARD" {
        return Err(bad());
    }
    let Some((c, r)) = layout.split_once(',') else {
        return Err(bad());
    };
    match (bounded_count(c, max_boxes), bounded_count(r, max_boxes)) {
        (Some(columns), Some(rows)) if box_count_within(columns, rows, max_boxes) => {
            Ok((columns, rows))
        }
        _ => Err(bad()),
    }
}
pub(crate) fn bounded_count(text: &str, limit: i64) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() || limit < 1 {
        return None;
    }
    let mut n: i64 = 0;
    for c in text.bytes() {
        if !c.is_ascii_digit() {
            return None;
        }
        let digit = (c - b'0') as i64;
        if digit > limit || n > (limit - digit) / 10 {
            return None;
        }
        n = n * 10 + digit;
    }
    (n >= 1).then_some(n)
}
pub(crate) fn box_count_within(columns: i64, rows: i64, max_boxes: i64) -> bool {
    columns >= 1 && rows >= 1 && max_boxes >= 1 && columns <= max_boxes / rows
}
pub(crate) fn parse_film_box_presentation_native(
    r: &mut AttributeReader<'_>,
    current: &FilmBoxPresentation,
    color: bool,
    allow_lut: bool,
) -> Result<FilmBoxPresentation> {
    let mut out = current.clone();
    if r.has(TAG_REFERENCED_PRESENTATION_LUT) && !allow_lut {
        return Err(invalid(&[TAG_REFERENCED_PRESENTATION_LUT]));
    }
    if r.has(TAG_ICC_PROFILE) && !color {
        return Err(invalid(&[TAG_ICC_PROFILE]));
    }
    let v = r.enumerated(TAG_FILM_ORIENTATION, &["PORTRAIT", "LANDSCAPE"])?;
    if !v.is_empty() {
        out.film_orientation = v;
    }
    let v = r.enumerated(TAG_FILM_SIZE_ID, &FILM_SIZES)?;
    if !v.is_empty() {
        out.film_size_id = v;
    }
    let v = r.enumerated(
        TAG_MAGNIFICATION_TYPE,
        &["REPLICATE", "BILINEAR", "CUBIC", "NONE"],
    )?;
    if !v.is_empty() {
        out.magnification_type = v;
    }
    let v = r.enumerated(TAG_TRIM, &["YES", "NO"])?;
    if !v.is_empty() {
        out.trim = v;
    }
    for (tag, field) in [
        (TAG_SMOOTHING_TYPE, &mut out.smoothing_type),
        (TAG_BORDER_DENSITY, &mut out.border_density),
        (TAG_EMPTY_IMAGE_DENSITY, &mut out.empty_image_density),
        (TAG_CONFIGURATION_INFORMATION, &mut out.configuration),
    ] {
        let (value, ok) = r.text(tag)?;
        if ok {
            *field = value;
        }
    }
    for (tag, field, present) in [
        (
            TAG_MIN_DENSITY,
            &mut out.min_density,
            &mut out.min_density_set,
        ),
        (
            TAG_MAX_DENSITY,
            &mut out.max_density,
            &mut out.max_density_set,
        ),
        (
            TAG_ILLUMINATION,
            &mut out.illumination,
            &mut out.illumination_set,
        ),
        (
            TAG_REFLECTED_AMBIENT_LIGHT,
            &mut out.reflected_ambient_light,
            &mut out.reflected_ambient_light_set,
        ),
    ] {
        let (value, ok) = r.number(tag)?;
        if ok {
            *field = value;
            *present = true;
        }
    }
    if out.min_density != 0 && out.max_density != 0 && out.min_density >= out.max_density {
        return Err(invalid(&[TAG_MIN_DENSITY, TAG_MAX_DENSITY]));
    }
    Ok(out)
}
pub(crate) fn parse_image(
    item: &Object,
    color: bool,
    max_dimension: i64,
    max_bytes: i64,
) -> Result<Image> {
    let sequence = if color {
        TAG_BASIC_COLOR_IMAGE_SEQUENCE
    } else {
        TAG_BASIC_GRAYSCALE_IMAGE_SEQUENCE
    };
    let get = |tag: Tag| -> Result<i64> {
        match uint16_of(item, tag) {
            Ok((v, true)) => Ok(v),
            _ => Err(invalid(&[sequence, tag])),
        }
    };
    let samples = get(TAG_SAMPLES_PER_PIXEL)?;
    let rows = get(TAG_ROWS)?;
    let columns = get(TAG_COLUMNS)?;
    let allocated = get(TAG_BITS_ALLOCATED)?;
    let stored = get(TAG_BITS_STORED)?;
    let high = get(TAG_HIGH_BIT)?;
    let representation = get(TAG_PIXEL_REPRESENTATION)?;
    let photometric = item
        .lookup_string(TAG_PHOTOMETRIC_INTERPRETATION)
        .map_err(|_| invalid(&[sequence, TAG_PHOTOMETRIC_INTERPRETATION]))?;
    let photometric = photometric.trim().to_string();
    if rows < 1 || columns < 1 || rows > max_dimension || columns > max_dimension {
        return Err(invalid(&[sequence, TAG_ROWS, TAG_COLUMNS]));
    }
    if representation != 0 || high != stored - 1 {
        return Err(invalid(&[sequence, TAG_PIXEL_REPRESENTATION, TAG_HIGH_BIT]));
    }
    let mut image = Image {
        color,
        rows,
        columns,
        bits: stored,
        photometric: photometric.clone(),
        ..Default::default()
    };
    let mut bytes_per_sample = 1;
    if color {
        let planar = get(TAG_PLANAR_CONFIGURATION)?;
        if samples != 3 || photometric != "RGB" || planar != 1 || allocated != 8 || stored != 8 {
            return Err(invalid(&[
                sequence,
                TAG_SAMPLES_PER_PIXEL,
                TAG_PHOTOMETRIC_INTERPRETATION,
                TAG_PLANAR_CONFIGURATION,
                TAG_BITS_ALLOCATED,
            ]));
        }
    } else {
        if samples != 1
            || (photometric != "MONOCHROME1" && photometric != "MONOCHROME2")
            || !((allocated == 8 && stored == 8) || (allocated == 16 && stored == 12))
        {
            return Err(invalid(&[
                sequence,
                TAG_SAMPLES_PER_PIXEL,
                TAG_PHOTOMETRIC_INTERPRETATION,
                TAG_BITS_ALLOCATED,
                TAG_BITS_STORED,
            ]));
        }
        bytes_per_sample = allocated / 8;
    }
    match resident_text_values(item, TAG_PIXEL_ASPECT_RATIO, Vr::IS, 2, 12) {
        Err(_) => return Err(invalid(&[sequence, TAG_PIXEL_ASPECT_RATIO])),
        Ok(Some(values)) => {
            for (i, v) in values.iter().enumerate() {
                match parse_int32_text(v) {
                    Some(n) if n >= 1 => image.pixel_aspect_ratio[i] = n,
                    _ => return Err(invalid(&[sequence, TAG_PIXEL_ASPECT_RATIO])),
                }
            }
        }
        Ok(None) => {}
    }
    let want = rows * columns * samples * bytes_per_sample;
    if want > max_bytes {
        return Err(failure(
            STATUS_INSUFFICIENT_PRINTER_MEMORY,
            "",
            Kind::InvalidAttributes.into(),
            &[sequence, TAG_PIXEL_DATA],
        ));
    }
    let pixels = item.get_raw(TAG_PIXEL_DATA);
    let ok = pixels
        .as_ref()
        .is_some_and(|p| p.len() as i64 == want || (want % 2 == 1 && p.len() as i64 == want + 1));
    let Some(pixels) = pixels.filter(|_| ok) else {
        return Err(invalid(&[sequence, TAG_PIXEL_DATA]));
    };
    image.pixel_data = pixels[..want as usize].to_vec();
    if item.value_big_endian() && bytes_per_sample == 2 {
        for pair in image.pixel_data.as_chunks_mut::<2>().0 {
            pair.swap(0, 1);
        }
    }
    Ok(image)
}
pub(crate) fn parse_int32_text(v: &str) -> Option<i64> {
    let digits = v.strip_prefix(['+', '-']).unwrap_or(v);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    v.parse::<i32>().ok().map(i64::from)
}
pub(crate) fn text(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
pub(crate) fn us(tag: Tag, value: i64) -> Element {
    raw_element(tag, Vr::US, (value as u16).to_le_bytes().to_vec())
}
pub(crate) fn sequence_of(tag: Tag, items: Vec<DataSet>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr: Vr::SQ,
            length: UNDEFINED_LENGTH,
            length_set: true,
        },
        value: Value::Sequence(items),
    }
}
pub(crate) fn reference_item(class_uid: &str, instance_uid: &str) -> DataSet {
    DataSet {
        elements: vec![
            text(TAG_REFERENCED_SOP_CLASS_UID, Vr::UI, class_uid),
            text(TAG_REFERENCED_SOP_INSTANCE_UID, Vr::UI, instance_uid),
        ],
        item_offset: None,
    }
}
pub fn build_film_session(p: &FilmSessionPresentation) -> Object {
    let mut elements = Vec::new();
    if p.number_of_copies > 0 {
        elements.push(text(
            TAG_NUMBER_OF_COPIES,
            Vr::IS,
            &p.number_of_copies.to_string(),
        ));
    }
    for (tag, vr, value) in [
        (
            TAG_SPECIFIC_CHARACTER_SET,
            Vr::CS,
            &p.specific_character_set,
        ),
        (TAG_PRINT_PRIORITY, Vr::CS, &p.print_priority),
        (TAG_MEDIUM_TYPE, Vr::CS, &p.medium_type),
        (TAG_FILM_DESTINATION, Vr::CS, &p.film_destination),
        (TAG_FILM_SESSION_LABEL, Vr::LO, &p.label),
        (TAG_OWNER_ID, Vr::SH, &p.owner_id),
    ] {
        if !value.is_empty() {
            elements.push(text(tag, vr, value));
        }
    }
    Object::from_elements(elements)
}
pub fn build_film_box(
    film_session_uid: &str,
    columns: i64,
    rows: i64,
    p: &FilmBoxPresentation,
) -> Object {
    let mut elements = vec![
        text(
            TAG_IMAGE_DISPLAY_FORMAT,
            Vr::ST,
            &format!("STANDARD\\{columns},{rows}"),
        ),
        sequence_of(
            TAG_REFERENCED_FILM_SESSION_SEQUENCE,
            vec![reference_item(FILM_SESSION_SOP_CLASS_UID, film_session_uid)],
        ),
    ];
    for (tag, vr, value) in [
        (TAG_FILM_ORIENTATION, Vr::CS, &p.film_orientation),
        (TAG_FILM_SIZE_ID, Vr::CS, &p.film_size_id),
        (TAG_MAGNIFICATION_TYPE, Vr::CS, &p.magnification_type),
        (TAG_SMOOTHING_TYPE, Vr::CS, &p.smoothing_type),
        (TAG_BORDER_DENSITY, Vr::CS, &p.border_density),
        (TAG_EMPTY_IMAGE_DENSITY, Vr::CS, &p.empty_image_density),
        (TAG_TRIM, Vr::CS, &p.trim),
        (TAG_CONFIGURATION_INFORMATION, Vr::ST, &p.configuration),
    ] {
        if !value.is_empty() {
            elements.push(text(tag, vr, value));
        }
    }
    if !p.annotation_display_format_id.is_empty() {
        elements.push(text(
            TAG_ANNOTATION_DISPLAY_FORMAT_ID,
            Vr::CS,
            &p.annotation_display_format_id,
        ));
    }
    if !p.presentation_lut_uid.is_empty() {
        elements.push(presentation_lut_reference(&p.presentation_lut_uid));
    }
    if p.min_density_set || p.min_density > 0 {
        elements.push(us(TAG_MIN_DENSITY, p.min_density));
    }
    if p.max_density_set || p.max_density > 0 {
        elements.push(us(TAG_MAX_DENSITY, p.max_density));
    }
    if p.illumination_set || p.illumination > 0 {
        elements.push(us(TAG_ILLUMINATION, p.illumination));
    }
    if p.reflected_ambient_light_set || p.reflected_ambient_light > 0 {
        elements.push(us(TAG_REFLECTED_AMBIENT_LIGHT, p.reflected_ambient_light));
    }
    Object::from_elements(elements)
}
pub fn build_image_box(position: i64, image: Option<&Image>, polarity: &str) -> Result<Object> {
    let color = image.is_some_and(|i| i.color);
    let sequence = if color {
        TAG_BASIC_COLOR_IMAGE_SEQUENCE
    } else {
        TAG_BASIC_GRAYSCALE_IMAGE_SEQUENCE
    };
    let mut elements = vec![us(TAG_IMAGE_BOX_POSITION, position)];
    if !polarity.is_empty() {
        elements.push(text(TAG_POLARITY, Vr::CS, polarity));
    }
    let Some(image) = image else {
        elements.push(sequence_of(sequence, Vec::new()));
        return Ok(Object::from_elements(elements));
    };
    let (mut samples, mut allocated, mut planar) = (1, 8, -1);
    if image.color {
        samples = 3;
        planar = 1;
    } else if image.bits == 12 {
        allocated = 16;
    }
    let vr = if allocated == 16 { Vr::OW } else { Vr::OB };
    let mut item = vec![
        us(TAG_SAMPLES_PER_PIXEL, samples),
        text(TAG_PHOTOMETRIC_INTERPRETATION, Vr::CS, &image.photometric),
        us(TAG_ROWS, image.rows),
        us(TAG_COLUMNS, image.columns),
        us(TAG_BITS_ALLOCATED, allocated),
        us(TAG_BITS_STORED, image.bits),
        us(TAG_HIGH_BIT, image.bits - 1),
        us(TAG_PIXEL_REPRESENTATION, 0),
        raw_element(TAG_PIXEL_DATA, vr, image.pixel_data.clone()),
    ];
    if planar >= 0 {
        item.insert(2, us(TAG_PLANAR_CONFIGURATION, planar));
    }
    if image.pixel_aspect_ratio != [0, 0] && image.pixel_aspect_ratio != [1, 1] {
        item.push(Element {
            header: ElementHeader {
                tag: TAG_PIXEL_ASPECT_RATIO,
                vr: Vr::IS,
                length: 0,
                length_set: false,
            },
            value: Value::Strings(vec![
                image.pixel_aspect_ratio[0].to_string(),
                image.pixel_aspect_ratio[1].to_string(),
            ]),
        });
    }
    elements.push(sequence_of(
        sequence,
        vec![DataSet {
            elements: item,
            item_offset: None,
        }],
    ));
    let ds = Object::from_elements(elements);
    let items = ds.get_sequence(sequence).unwrap_or_default();
    if let Some(first) = items.first() {
        parse_image(first, image.color, (1 << 16) - 1, 1 << 40)?;
    }
    Ok(ds)
}
