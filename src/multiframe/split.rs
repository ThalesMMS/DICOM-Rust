use super::classic::{
    Budget, append_source_reference, conversion_error, required_uid, str_elem, strings_elem,
    transcode_error, transcode_limits, ui,
};
use super::mosaic::{MosaicGeometry, MosaicOptions, mosaic_geometry_of};
use super::uih_grid::{UihGridGeometry, uih_grid_geometry, uih_required_uid};
use super::{Error, Kind, Limits, MR_IMAGE_STORAGE_UID, dot, normalize_limits};
use crate::derivation::Source;
use crate::derivedio;
use crate::object::{Element, ElementHeader, File, Object, Tag, Value, Vr};
use crate::pixel::pixeldata::extract_native_frames;
use crate::pixel::transcode::{self, DecoderRegistry, TranscodeOptions};
const T_SOP_CLASS: Tag = Tag::new(0x0008, 0x0016);
const T_SOP_INSTANCE: Tag = Tag::new(0x0008, 0x0018);
const T_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
const T_SERIES_UID: Tag = Tag::new(0x0020, 0x000E);
const T_INSTANCE_NUMBER: Tag = Tag::new(0x0020, 0x0013);
const T_ROWS: Tag = Tag::new(0x0028, 0x0010);
const T_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const T_IPP: Tag = Tag::new(0x0020, 0x0032);
const T_IOP: Tag = Tag::new(0x0020, 0x0037);
const T_SLICE_LOCATION: Tag = Tag::new(0x0020, 0x1041);
const T_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const T_DERIVATION_DESCRIPTION: Tag = Tag::new(0x0008, 0x2111);
const MOSAIC_DROPPED: [Tag; 4] = [
    Tag::new(0x0028, 0x0008),
    Tag::new(0x0028, 0x0106),
    Tag::new(0x0028, 0x0107),
    T_PIXEL_DATA,
];
pub type UidForSlice<'a> = &'a dyn Fn(i64) -> Result<String, Error>;
pub struct SplitMosaicOptions<'a> {
    pub series_instance_uid: String,
    pub sop_instance_uid_for_slice: Option<UidForSlice<'a>>,
    pub decoder: Option<DecoderRegistry>,
    pub limits: Limits,
    pub keep_private: bool,
    pub geometry: MosaicOptions,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MosaicReport {
    pub geometry: MosaicGeometry,
    pub output_bytes: i64,
}
pub struct SplitUihGridOptions<'a> {
    pub series_instance_uid: String,
    pub sop_instance_uid_for_slice: Option<UidForSlice<'a>>,
    pub decoder: Option<DecoderRegistry>,
    pub limits: Limits,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UihGridReport {
    pub geometry: UihGridGeometry,
    pub output_bytes: i64,
}
fn native_frame(
    src: &File,
    decoder: Option<DecoderRegistry>,
    limits: &Limits,
) -> Result<(Object, crate::pixel::pixeldata::NativeFrames), Error> {
    let (transcoded, _) = transcode::transcode_file(
        src,
        "1.2.840.10008.1.2.1",
        &TranscodeOptions {
            decoder,
            limits: transcode_limits(limits),
            ..TranscodeOptions::default()
        },
    )
    .map_err(|e| conversion_error("transcode", -1, transcode_error(e)))?;
    let frames = extract_native_frames(&transcoded.dataset)
        .map_err(|e| conversion_error("pixel_data", -1, Error::with_text(e.to_string())))?;
    Ok((transcoded.dataset, frames))
}
fn tile_pixels(
    mosaic: &[u8],
    grid: i64,
    rows: i64,
    columns: i64,
    source_columns: i64,
    tile: i64,
    sample_bytes: i64,
) -> Vec<u8> {
    let mut out = vec![0u8; (rows * columns * sample_bytes) as usize];
    let x0 = (tile % grid) * columns;
    let y0 = (tile / grid) * rows;
    let row_bytes = (columns * sample_bytes) as usize;
    for y in 0..rows {
        let start = (((y0 + y) * source_columns + x0) * sample_bytes) as usize;
        let at = y as usize * row_bytes;
        out[at..at + row_bytes].copy_from_slice(&mosaic[start..start + row_bytes]);
    }
    out
}
fn write_output(
    dataset: Object,
    limits: &Limits,
    output_bytes: &mut i64,
    k: i64,
    always_output_stage: bool,
) -> Result<File, Error> {
    let mut file = File {
        preamble: Vec::new(),
        meta: Object::new(),
        dataset,
        transfer_syntax: crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        partial_read: None,
        incomplete_pixel_data: None,
    };
    file.rebuild_file_meta()
        .map_err(|e| conversion_error("file_meta", k, Error::with_text(e.to_string())))?;
    let written = crate::object::writer::write_file(&file).map_err(|e| {
        let stage = if always_output_stage {
            "output"
        } else {
            "write_validation"
        };
        conversion_error(stage, k, Error::with_text(e.to_string()))
    })?;
    if written.len() as i64 > limits.max_output_bytes - *output_bytes {
        return Err(conversion_error(
            "output",
            k,
            Error::wrap(Kind::ResourceLimit, "aggregate output bytes"),
        ));
    }
    *output_bytes += written.len() as i64;
    Ok(file)
}
fn pixel_element(mut pixels: Vec<u8>, wide: bool) -> Element {
    if !pixels.len().is_multiple_of(2) {
        pixels.push(0);
    }
    let vr = if wide { Vr::OW } else { Vr::OB };
    Element {
        header: ElementHeader::new(T_PIXEL_DATA, vr, pixels.len() as u32),
        value: Value::Raw(pixels),
    }
}
pub fn split_mosaic(
    src: &File,
    opts: &SplitMosaicOptions<'_>,
) -> Result<(Vec<File>, MosaicReport), Error> {
    let limits = normalize_limits(opts.limits).map_err(|e| conversion_error("options", -1, e))?;
    let source_sop = required_uid(&src.dataset, T_SOP_INSTANCE)
        .map_err(|e| conversion_error("source", -1, e))?;
    let source_class =
        required_uid(&src.dataset, T_SOP_CLASS).map_err(|e| conversion_error("source", -1, e))?;
    let series = crate::siemens::trim_space(&opts.series_instance_uid).to_string();
    if !crate::object::is_valid_uid(series.as_bytes()) {
        return Err(conversion_error(
            "options",
            -1,
            Error::wrap(Kind::InvalidOptions, "SeriesInstanceUID must be valid"),
        ));
    }
    let Some(uid_for_slice) = opts.sop_instance_uid_for_slice else {
        return Err(conversion_error(
            "options",
            -1,
            Error::wrap(Kind::InvalidOptions, "SOPInstanceUIDForSlice is required"),
        ));
    };
    let g = mosaic_geometry_of(&src.dataset, opts.geometry)
        .map_err(|e| conversion_error("geometry", -1, e))?;
    if g.images > limits.max_frames {
        return Err(conversion_error(
            "geometry",
            -1,
            Error::wrap(Kind::ResourceLimit, format!("{} slices", g.images)),
        ));
    }
    let (transcoded, frames) = native_frame(src, opts.decoder, &limits)?;
    let m = &frames.metadata;
    if frames.data.len() != 1
        || m.samples_per_pixel != 1
        || m.bits_allocated != 8 && m.bits_allocated != 16
    {
        return Err(conversion_error(
            "pixel_data",
            -1,
            Error::wrap(
                Kind::Nonconformant,
                "a mosaic must be one frame of 8- or 16-bit monochrome samples",
            ),
        ));
    }
    let sample_bytes = i64::from(m.bits_allocated / 8);
    let mosaic = &frames.data[0];
    if (mosaic.len() as i64) < g.source_rows * g.source_columns * sample_bytes {
        return Err(conversion_error(
            "pixel_data",
            -1,
            Error::wrap(Kind::Nonconformant, "Pixel Data is shorter than the mosaic"),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut uids = Vec::with_capacity(g.images.max(0) as usize);
    for k in 0..g.images {
        let uid = uid_for_slice(k).map_err(|e| conversion_error("uid", k, e))?;
        let uid = crate::siemens::trim_space(&uid).to_string();
        if !crate::object::is_valid_uid(uid.as_bytes()) || uid == source_sop || seen.contains(&uid)
        {
            return Err(conversion_error(
                "uid",
                k,
                Error::wrap(
                    Kind::InvalidOptions,
                    "generated SOP Instance UID is invalid, repeated or equals the source",
                ),
            ));
        }
        seen.insert(uid.clone());
        uids.push(uid);
    }
    let mut budget = Budget {
        max_elements: limits.max_elements,
        max_depth: limits.max_depth,
        elements: 0,
    };
    let mut outputs = Vec::new();
    let mut output_bytes = 0i64;
    for k in 0..g.images {
        let tile = tile_pixels(
            mosaic,
            g.grid,
            g.rows,
            g.columns,
            g.source_columns,
            g.tile_order[k as usize],
            sample_bytes,
        );
        let ds = build_mosaic_slice(
            &mut budget,
            &transcoded,
            &g,
            k,
            tile,
            m.bits_allocated,
            &series,
            &uids[k as usize],
            &source_class,
            &source_sop,
            opts.keep_private,
        )
        .map_err(|e| conversion_error("slice", k, e))?;
        outputs.push(write_output(ds, &limits, &mut output_bytes, k, false)?);
    }
    Ok((
        outputs,
        MosaicReport {
            geometry: g,
            output_bytes,
        },
    ))
}
#[allow(clippy::too_many_arguments)]
fn build_mosaic_slice(
    budget: &mut Budget,
    source: &Object,
    g: &MosaicGeometry,
    k: i64,
    pixels: Vec<u8>,
    bits: u16,
    series: &str,
    sop: &str,
    source_class: &str,
    source_sop: &str,
    keep_private: bool,
) -> Result<Object, Error> {
    let mut elements = Vec::with_capacity(source.len());
    for e in source.elements() {
        if MOSAIC_DROPPED.contains(&e.tag()) || e.tag().is_private() && !keep_private {
            continue;
        }
        elements.push(budget.clone_element(e, 1)?);
    }
    let mut ds = Object::from_elements(elements);
    ds.set_value_big_endian(false);
    if let Some(image_type) = source.get_strings(T_IMAGE_TYPE) {
        let kept: Vec<String> = image_type
            .into_iter()
            .filter(|v| !crate::siemens::trim_space(v).eq_ignore_ascii_case("MOSAIC"))
            .collect();
        ds.put(strings_elem(T_IMAGE_TYPE, Vr::CS, &kept));
    }
    ds.put(ui(T_SOP_INSTANCE, sop));
    ds.put(ui(T_SERIES_UID, series));
    ds.put(str_elem(T_INSTANCE_NUMBER, Vr::IS, &(k + 1).to_string()));
    ds.put(derivedio::us(T_ROWS, &[g.rows as u16]));
    ds.put(derivedio::us(T_COLUMNS, &[g.columns as u16]));
    let p = g.positions[k as usize];
    ds.put(derivedio::ds(T_IPP, &p));
    ds.put(derivedio::ds(T_SLICE_LOCATION, &[dot(p, g.normal)]));
    ds.put(pixel_element(pixels, bits > 8));
    append_source_reference(
        &mut ds,
        Source {
            sop_class_uid: source_class.to_string(),
            sop_instance_uid: source_sop.to_string(),
            frames: Vec::new(),
            purpose: None,
        },
    )?;
    if !ds.has(T_DERIVATION_DESCRIPTION) {
        ds.put(str_elem(
            T_DERIVATION_DESCRIPTION,
            Vr::ST,
            &format!(
                "Slice {} of {} split from a Siemens mosaic",
                g.tile_order[k as usize] + 1,
                g.images
            ),
        ));
    }
    Ok(ds)
}
pub fn split_uih_grid(
    src: &File,
    opts: &SplitUihGridOptions<'_>,
) -> Result<(Vec<File>, UihGridReport), Error> {
    let limits = normalize_limits(opts.limits).map_err(|e| conversion_error("options", -1, e))?;
    let g = uih_grid_geometry(Some(&src.dataset), &limits)
        .map_err(|e| conversion_error("geometry", -1, e))?;
    let source_sop = uih_required_uid(&src.dataset, T_SOP_INSTANCE)
        .map_err(|e| conversion_error("source", -1, e))?;
    let source_series = uih_required_uid(&src.dataset, T_SERIES_UID)
        .map_err(|e| conversion_error("source", -1, e))?;
    let series = crate::siemens::trim_space(&opts.series_instance_uid).to_string();
    let Some(uid_for_slice) = opts.sop_instance_uid_for_slice.filter(|_| {
        crate::object::is_valid_uid(series.as_bytes())
            && series != source_series
            && series != source_sop
    }) else {
        return Err(conversion_error(
            "options",
            -1,
            Error::bare(Kind::InvalidOptions),
        ));
    };
    let (transcoded, frames) = native_frame(src, opts.decoder, &limits)?;
    let m = &frames.metadata;
    if frames.data.len() != 1
        || m.samples_per_pixel != 1
        || m.bits_allocated != 8 && m.bits_allocated != 16
        || m.photometric_interpretation != "MONOCHROME1"
            && m.photometric_interpretation != "MONOCHROME2"
    {
        return Err(conversion_error(
            "pixel_data",
            -1,
            Error::bare(Kind::Nonconformant),
        ));
    }
    let sample_bytes = i64::from(m.bits_allocated / 8);
    let tile_bytes = g.rows * g.columns * sample_bytes;
    let (nodes, bytes) =
        metadata_cost(&transcoded, &limits).map_err(|e| conversion_error("output", -1, e))?;
    let padded = tile_bytes + tile_bytes % 2;
    if nodes > limits.max_elements / g.images - 32
        || bytes > limits.max_output_bytes / g.images - padded
    {
        return Err(conversion_error(
            "output",
            -1,
            Error::bare(Kind::ResourceLimit),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut uids = Vec::new();
    for k in 0..g.images {
        let uid = uid_for_slice(k).map_err(|e| conversion_error("uid", k, e))?;
        let uid = crate::siemens::trim_space(&uid).to_string();
        if !crate::object::is_valid_uid(uid.as_bytes())
            || uid == source_sop
            || uid == source_series
            || uid == series
            || seen.contains(&uid)
        {
            return Err(conversion_error(
                "uid",
                k,
                Error::bare(Kind::InvalidOptions),
            ));
        }
        seen.insert(uid.clone());
        uids.push(uid);
    }
    let mut budget = Budget {
        max_elements: limits.max_elements,
        max_depth: limits.max_depth,
        elements: 0,
    };
    let mut outputs = Vec::new();
    let mut output_bytes = 0i64;
    for k in 0..g.positions.len() as i64 {
        let pixels = tile_pixels(
            &frames.data[0],
            g.grid_columns,
            g.rows,
            g.columns,
            g.source_columns,
            k,
            sample_bytes,
        );
        let ds = build_uih_slice(
            &mut budget,
            &transcoded,
            &g,
            k,
            pixels,
            m.bits_allocated,
            &series,
            &uids[k as usize],
            &source_sop,
        )
        .map_err(|e| conversion_error("slice", k, e))?;
        outputs.push(write_output(ds, &limits, &mut output_bytes, k, true)?);
    }
    Ok((
        outputs,
        UihGridReport {
            geometry: g,
            output_bytes,
        },
    ))
}
fn metadata_cost(source: &Object, limits: &Limits) -> Result<(i64, i64), Error> {
    fn visit(
        e: &Element,
        depth: i64,
        limits: &Limits,
        nodes: &mut i64,
        bytes: &mut i64,
    ) -> Result<(), Error> {
        if depth > limits.max_depth || *nodes >= limits.max_elements {
            return Err(Error::bare(Kind::ResourceLimit));
        }
        *nodes += 1;
        if let Value::Sequence(items) = &e.value {
            if items.len() as i64 > limits.max_elements - *nodes {
                return Err(Error::bare(Kind::ResourceLimit));
            }
            *nodes += items.len() as i64;
            for item in items {
                for child in &item.elements {
                    visit(child, depth + 1, limits, nodes, bytes)?;
                }
            }
            return Ok(());
        }
        let length = match &e.value {
            Value::Nil => return Err(Error::bare(Kind::InvalidSource)),
            Value::Raw(r) => r.len() as i64,
            Value::Strings(v) => crate::pixel::transcode::strings_len(v),
            Value::Fragments { .. } => return Err(Error::bare(Kind::ResourceLimit)),
            Value::BulkData(_) => return Err(Error::bare(Kind::ResourceLimit)),
            Value::Sequence(_) => 0,
        };
        if length > limits.max_output_bytes - *bytes {
            return Err(Error::bare(Kind::ResourceLimit));
        }
        *bytes += length;
        Ok(())
    }
    let (mut nodes, mut bytes) = (0i64, 0i64);
    for e in source.elements() {
        if MOSAIC_DROPPED.contains(&e.tag()) || e.tag().is_private() {
            continue;
        }
        visit(e, 1, limits, &mut nodes, &mut bytes)?;
    }
    Ok((nodes, bytes))
}
#[allow(clippy::too_many_arguments)]
fn build_uih_slice(
    budget: &mut Budget,
    source: &Object,
    g: &UihGridGeometry,
    k: i64,
    pixels: Vec<u8>,
    bits: u16,
    series: &str,
    sop: &str,
    source_sop: &str,
) -> Result<Object, Error> {
    let mut elements = Vec::with_capacity(source.len());
    for e in source.elements() {
        if MOSAIC_DROPPED.contains(&e.tag()) || e.tag().is_private() {
            continue;
        }
        elements.push(budget.clone_element(e, 1)?);
    }
    let mut ds = Object::from_elements(elements);
    let image_type = source.get_strings(T_IMAGE_TYPE).unwrap_or_default();
    let mut kept = vec!["DERIVED".to_string(), "SECONDARY".to_string()];
    for s in image_type.iter().skip(2) {
        if crate::siemens::trim_space(s) != "VFRAME" {
            kept.push(s.clone());
        }
    }
    ds.put(strings_elem(T_IMAGE_TYPE, Vr::CS, &kept));
    ds.put(ui(T_SOP_INSTANCE, sop));
    ds.put(ui(T_SERIES_UID, series));
    ds.put(str_elem(T_INSTANCE_NUMBER, Vr::IS, &(k + 1).to_string()));
    ds.put(derivedio::us(T_ROWS, &[g.rows as u16]));
    ds.put(derivedio::us(T_COLUMNS, &[g.columns as u16]));
    let (p, o) = (g.positions[k as usize], g.orientations[k as usize]);
    ds.put(derivedio::ds(T_IPP, &p));
    ds.put(derivedio::ds(T_IOP, &o));
    ds.put(derivedio::ds(T_SLICE_LOCATION, &[dot(p, g.normal)]));
    ds.put(pixel_element(pixels, bits == 16));
    append_source_reference(
        &mut ds,
        Source {
            sop_class_uid: MR_IMAGE_STORAGE_UID.to_string(),
            sop_instance_uid: source_sop.to_string(),
            frames: Vec::new(),
            purpose: None,
        },
    )?;
    ds.put(str_elem(
        T_DERIVATION_DESCRIPTION,
        Vr::ST,
        &format!(
            "Slice {} of {} split from a UIH grid image",
            k + 1,
            g.images
        ),
    ));
    Ok(ds)
}
