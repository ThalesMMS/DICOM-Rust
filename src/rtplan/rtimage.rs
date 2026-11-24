use super::bounded::{
    ReadOptions, file_read_options, fresh_facade, native_syntax, preflight, strict_object,
};
use super::read::{Reader, item, join};
use super::reflect::semantic_equal;
use super::tags::*;
use super::write::{
    WriteBuilder, WriteOptions, begin_write, finish_write, native_order_facade, put_sequence,
};
use super::*;
use crate::object::{Element, ElementHeader, File, Object, Tag, Value, Vr, is_valid_uid};
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{extract_metadata, extract_native_frames, frame_size};
model! {
    Image "Image" {
        sop_class_uid: String = "SOPClassUID",
        sop_instance_uid: String = "SOPInstanceUID",
        study_instance_uid: String = "StudyInstanceUID",
        series_instance_uid: String = "SeriesInstanceUID",
        frame_of_reference_uid: String = "FrameOfReferenceUID",
        label: String = "Label",
        name: String = "Name",
        description: String = "Description",
        plane: String = "Plane",
        image_type: Vec<String> = "ImageType",
        conversion_type: Attribute<String> = "ConversionType",
        reported_values_origin: Attribute<String> = "ReportedValuesOrigin",
        machine_name: Attribute<String> = "MachineName",
        primary_dosimeter_unit: Attribute<String> = "PrimaryDosimeterUnit",
        receptor_translation: Attribute<Vec<f64>> = "ReceptorTranslation",
        orientation: Attribute<Vec<f64>> = "Orientation",
        image_plane_pixel_spacing: Attribute<Vec<f64>> = "ImagePlanePixelSpacing",
        position: Attribute<Vec<f64>> = "Position",
        receptor_angle: Attribute<f64> = "ReceptorAngle",
        sad: Attribute<f64> = "SAD",
        ssd: Attribute<f64> = "SSD",
        sid: Attribute<f64> = "SID",
        start_cumulative_meterset_weight: Attribute<f64> = "StartCumulativeMetersetWeight",
        end_cumulative_meterset_weight: Attribute<f64> = "EndCumulativeMetersetWeight",
        referenced_plan: Option<Reference> = "ReferencedPlan",
        referenced_beam_number: Attribute<i64> = "ReferencedBeamNumber",
        referenced_fraction_group_number: Attribute<i64> = "ReferencedFractionGroupNumber",
        fraction_number: Attribute<i64> = "FractionNumber",
        exposure: Option<ImageExposure> = "Exposure",
        metadata: Metadata = "Metadata",
        number_of_frames_present: bool = "NumberOfFramesPresent",
        pixels: Bytes = "Pixels",
        warnings: Vec<Problem> = "Warnings",
    }
}
model! {
    ImageExposure "ImageExposure" {
        kvp: Attribute<f64> = "KVP",
        meterset_exposure: Attribute<f64> = "MetersetExposure",
    }
}
fn vector_attribute(r: &mut Reader, o: &Object, t: Tag, p: &str, n: usize) -> Attribute<Vec<f64>> {
    let mut a = Attribute {
        present: o.has(t),
        value: None,
    };
    let v = r.number_list(o, t, p);
    if !v.is_empty() {
        if v.len() != n {
            r.fail(join(p, t), "vector cardinality is invalid");
        }
        a.value = Some(v);
    }
    a
}
fn text_attribute(r: &Reader, o: &Object, t: Tag) -> Attribute<String> {
    let s = r.str(o, t);
    Attribute {
        present: o.has(t),
        value: if s.is_empty() { None } else { Some(s) },
    }
}
pub fn read_image(file: &File, opts: ReadOptions) -> Result<Image, Error> {
    if !file.transfer_syntax.uid.is_empty() && !native_syntax(&file.transfer_syntax) {
        return Err(Error::bare(ErrorKind::UnsupportedFeature));
    }
    let adjusted = file_read_options(file, opts)?;
    read_image_dataset(&file.dataset, adjusted)
}
pub fn read_image_dataset(obj: &Object, opts: ReadOptions) -> Result<Image, Error> {
    let mut b = preflight(obj, &opts.limits, true)?;
    let mut o = fresh_facade(obj, true);
    if o.value_big_endian() {
        b.reserve(b.pixels, 1)?;
        o = native_order_facade(o, false)?;
    }
    let o = &o;
    let sop = o.get_uid(tagSOPClassUID).unwrap_or_default();
    if sop != RT_IMAGE_STORAGE {
        return Err(Error::bare(ErrorKind::UnsupportedSopClass));
    }
    if opts.strict_iod {
        strict_object(o, &b.limits)?;
    }
    let r = &mut Reader::new(b.limits.max_problems);
    let mut img = Image {
        sop_class_uid: sop,
        label: r.required_text(o, ft(0x30020002), ""),
        name: r.str(o, ft(0x30020003)),
        description: r.str(o, ft(0x30020004)),
        plane: r.code(o, ft(0x3002000c)),
        conversion_type: text_attribute(r, o, ft(0x00080064)),
        reported_values_origin: text_attribute(r, o, ft(0x3002000a)),
        machine_name: text_attribute(r, o, ft(0x30020020)),
        primary_dosimeter_unit: text_attribute(r, o, tagPrimaryDosimeterUnit),
        receptor_translation: vector_attribute(r, o, ft(0x3002000d), "", 3),
        orientation: vector_attribute(r, o, ft(0x30020010), "", 6),
        image_plane_pixel_spacing: vector_attribute(r, o, ft(0x30020011), "", 2),
        position: vector_attribute(r, o, ft(0x30020012), "", 2),
        receptor_angle: r.float_attribute(o, ft(0x3002000e), ""),
        sad: r.float_attribute(o, ft(0x30020022), ""),
        ssd: r.float_attribute(o, ft(0x30020024), ""),
        sid: r.float_attribute(o, ft(0x30020026), ""),
        start_cumulative_meterset_weight: r.float_attribute(o, ft(0x300c0008), ""),
        end_cumulative_meterset_weight: r.float_attribute(o, ft(0x300c0009), ""),
        referenced_beam_number: r.int_attribute(o, tagReferencedBeamNumber, ""),
        referenced_fraction_group_number: r.int_attribute(o, ft(0x300c0022), ""),
        fraction_number: r.int_attribute(o, ft(0x30020029), ""),
        number_of_frames_present: o.has(ft(0x00280008)),
        ..Image::default()
    };
    img.sop_instance_uid = o.get_uid(tagSOPInstanceUID).unwrap_or_default();
    img.study_instance_uid = o.get_uid(tagStudyInstanceUID).unwrap_or_default();
    img.series_instance_uid = o.get_uid(tagSeriesInstanceUID).unwrap_or_default();
    img.frame_of_reference_uid = o.get_uid(tagFrameOfReferenceUID).unwrap_or_default();
    for (t, s) in [
        (tagSOPInstanceUID, &img.sop_instance_uid),
        (tagStudyInstanceUID, &img.study_instance_uid),
        (tagSeriesInstanceUID, &img.series_instance_uid),
    ] {
        if !is_valid_uid(s.as_bytes()) {
            r.fail(t.to_string(), "identity UID is missing or invalid");
        }
    }
    if o.has(tagFrameOfReferenceUID) && !is_valid_uid(img.frame_of_reference_uid.as_bytes()) {
        r.fail(
            tagFrameOfReferenceUID.to_string(),
            "frame identity UID is invalid",
        );
    }
    img.image_type = o.get_strings(ft(0x00080008)).unwrap_or_default();
    if img.image_type.len() < 3 || !one_of(&img.image_type[2], &["DRR", "PORTAL"]) {
        return Err(path_error(
            ErrorKind::UnsupportedFeature,
            &ft(0x00080008).to_string(),
            "Image Type is outside DRR/PORTAL profile",
        ));
    }
    let portal = img.image_type[2] == "PORTAL";
    if !one_of(&img.plane, &["NORMAL", "NON_NORMAL"]) {
        r.fail(ft(0x3002000c).to_string(), "image plane is invalid");
    }
    if img.plane == "NON_NORMAL" && !img.orientation.present {
        r.fail(
            ft(0x30020010).to_string(),
            "NON_NORMAL orientation is missing",
        );
    }
    for t in [
        ft(0x00080064),
        ft(0x3002000e),
        ft(0x30020011),
        ft(0x30020012),
        ft(0x30020020),
        tagPrimaryDosimeterUnit,
        ft(0x30020022),
        ft(0x30020026),
    ] {
        if !o.has(t) {
            r.fail(t.to_string(), "Type 2 attribute is missing");
        }
    }
    if portal && !img.reported_values_origin.present {
        r.fail(ft(0x3002000a).to_string(), "PORTAL origin is missing");
    }
    if img
        .reported_values_origin
        .value
        .as_deref()
        .is_some_and(|v| !one_of(v, &["OPERATOR", "PLAN", "ACTUAL"]))
    {
        r.fail(ft(0x3002000a).to_string(), "origin is outside profile");
    }
    if img
        .primary_dosimeter_unit
        .value
        .as_deref()
        .is_some_and(|v| !one_of(v, &["MU", "MINUTE"]))
    {
        r.fail(
            tagPrimaryDosimeterUnit.to_string(),
            "dosimeter unit is outside profile",
        );
    }
    for (a, t) in [
        (&img.sad, ft(0x30020022)),
        (&img.sid, ft(0x30020026)),
        (&img.ssd, ft(0x30020024)),
    ] {
        r.attr_nonnegative(a, t, "", true);
    }
    r.attr_nonnegative(
        &img.start_cumulative_meterset_weight,
        ft(0x300c0008),
        "",
        false,
    );
    r.attr_nonnegative(
        &img.end_cumulative_meterset_weight,
        ft(0x300c0009),
        "",
        false,
    );
    if let (Some(start), Some(end)) = (
        img.start_cumulative_meterset_weight.value,
        img.end_cumulative_meterset_weight.value,
    ) && end < start
    {
        r.fail(
            ft(0x300c0009).to_string(),
            "exposure weight interval decreases",
        );
    }
    if let Some(spacing) = &img.image_plane_pixel_spacing.value {
        for v in spacing {
            if *v <= 0.0 {
                r.fail(ft(0x30020011).to_string(), "spacing must be positive");
            }
        }
    }
    if let Some(v) = &img.orientation.value
        && v.len() == 6
    {
        let (mut u, mut w, mut d) = (0.0, 0.0, 0.0);
        for j in 0..3 {
            u += v[j] * v[j];
            w += v[j + 3] * v[j + 3];
            d += v[j] * v[j + 3];
        }
        if (u - 1.0f64).abs() > 1e-5 || (w - 1.0f64).abs() > 1e-5 || f64::abs(d) > 1e-5 {
            r.fail(
                ft(0x30020010).to_string(),
                "orientation must be orthonormal",
            );
        }
    }
    for t in [
        ft(0x300800a1),
        ft(0x300800a2),
        ft(0x00287fe0),
        ft(0x30020040),
    ] {
        if o.has(t) {
            return Err(path_error(
                ErrorKind::UnsupportedFeature,
                &t.to_string(),
                "advanced device or external pixels are outside profile",
            ));
        }
    }
    let flag = r.code(o, ft(0x300800a3));
    if !flag.is_empty() && flag != "NO" {
        return Err(Error::bare(ErrorKind::UnsupportedFeature));
    }
    let refs = r.sequence(o, ft(0x300c0002));
    if refs.len() > 1 {
        r.fail(
            ft(0x300c0002).to_string(),
            "plan reference cardinality is invalid",
        );
    }
    if refs.len() == 1 {
        let x = Reference {
            sop_class_uid: r.str(&refs[0], tagRefSOPClassUID),
            sop_instance_uid: r.str(&refs[0], tagRefSOPInstanceUID),
        };
        if !one_of(&x.sop_class_uid, &[RT_PLAN_STORAGE, RT_ION_PLAN_STORAGE])
            || !is_valid_uid(x.sop_instance_uid.as_bytes())
        {
            r.fail(
                ft(0x300c0002).to_string(),
                "plan reference identity is invalid",
            );
        }
        img.referenced_plan = Some(x);
    }
    let exposures = r.sequence(o, ft(0x30020030));
    if exposures.len() > 1 {
        return Err(path_error(
            ErrorKind::UnsupportedFeature,
            &ft(0x30020030).to_string(),
            "multiple exposures are outside profile",
        ));
    }
    if exposures.len() == 1 {
        let e = &exposures[0];
        let ep = item("", ft(0x30020030), 0);
        let exposure = ImageExposure {
            kvp: r.float_attribute(e, ft(0x00180060), &ep),
            meterset_exposure: r.float_attribute(e, ft(0x30020032), &ep),
        };
        if portal && (!exposure.kvp.present || !exposure.meterset_exposure.present) {
            r.fail(ep.clone(), "PORTAL Type 2 exposure attributes are missing");
        }
        for t in [
            ft(0x300800a1),
            ft(0x300800a2),
            ft(0x300a00b6),
            ft(0x300a00f4),
            ft(0x300a0107),
            ft(0x300a0420),
        ] {
            if e.has(t) {
                return Err(path_error(
                    ErrorKind::UnsupportedFeature,
                    &join(&ep, t),
                    "exposure accessory is outside profile",
                ));
            }
        }
        if r.int_opt(e, tagNumberOfBlocks, &ep).is_some_and(|n| n != 0) {
            return Err(Error::bare(ErrorKind::UnsupportedFeature));
        }
        r.attr_nonnegative(&exposure.kvp, ft(0x00180060), &ep, false);
        r.attr_nonnegative(&exposure.meterset_exposure, ft(0x30020032), &ep, false);
        img.exposure = Some(exposure);
    }
    let Ok(meta) = extract_metadata(o) else {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "(0028)",
            "pixel metadata is invalid",
        ));
    };
    if meta.number_of_frames != 1
        || meta.samples_per_pixel != 1
        || meta.pixel_representation != 0
        || !one_of(
            &meta.photometric_interpretation,
            &["MONOCHROME1", "MONOCHROME2"],
        )
        || !(meta.bits_allocated == 8 || meta.bits_allocated == 16)
        || meta.bits_stored < 1
        || meta.bits_stored > meta.bits_allocated
        || meta.high_bit != meta.bits_stored - 1
        || meta.planar_configuration_present
    {
        return Err(path_error(
            ErrorKind::UnsupportedFeature,
            "(0028)",
            "pixel layout is outside single-frame unsigned native profile",
        ));
    }
    let n = frame_size(&meta);
    if n <= 0 || n > b.limits.max_pixel_bytes {
        return Err(Error::bare(ErrorKind::ResourceLimit));
    }
    let extent_ok = match o.get(ft(0x7fe00010)).map(|e| &e.value) {
        Some(Value::Raw(raw)) => {
            raw.len() as i64 == n + (n & 1) && !(n & 1 != 0 && raw[raw.len() - 1] != 0)
        }
        _ => false,
    };
    if !extent_ok {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "(7FE0,0010)",
            "exact native frame extent or padding is invalid",
        ));
    }
    if !r.errors.is_empty() {
        return Err(Error::validation(std::mem::take(&mut r.errors)));
    }
    let frames = match extract_native_frames(o) {
        Ok(f) if !f.data.is_empty() => f,
        _ => {
            return Err(path_error(
                ErrorKind::InvalidObject,
                "(7FE0,0010)",
                "native frame extraction failed",
            ));
        }
    };
    img.metadata = meta;
    img.pixels = Bytes(frames.data[0].clone());
    img.warnings = std::mem::take(&mut r.warnings);
    Ok(img)
}
fn text(w: &mut WriteBuilder, o: &mut Object, t: Tag, vr: Vr, a: &Attribute<String>) {
    o.remove(t);
    if a.present {
        w.str(o, t, vr, a.value.as_deref().unwrap_or(""));
    }
}
fn vector(w: &mut WriteBuilder, o: &mut Object, t: Tag, a: &Attribute<Vec<f64>>) {
    o.remove(t);
    if a.present {
        w.numbers(o, t, a.value.as_deref().unwrap_or(&[]));
    }
}
pub fn write_image(img: &Image, opts: &WriteOptions) -> Result<File, Error> {
    if img.sop_class_uid != RT_IMAGE_STORAGE {
        return Err(Error::bare(ErrorKind::UnsupportedSopClass));
    }
    let (mut f, mut w) = begin_write(opts, RT_IMAGE_STORAGE, true, img)?;
    let read_options = ReadOptions {
        limits: w.limits,
        strict_iod: false,
    };
    let before = read_image(&f, read_options)?;
    let o = &mut f.dataset;
    for (t, s) in [
        (tagStudyInstanceUID, &img.study_instance_uid),
        (tagSeriesInstanceUID, &img.series_instance_uid),
        (tagFrameOfReferenceUID, &img.frame_of_reference_uid),
    ] {
        w.optional_text(o, t, Vr::UI, s);
    }
    w.str(o, ft(0x30020002), Vr::SH, &img.label);
    w.optional_text(o, ft(0x30020003), Vr::LO, &img.name);
    w.optional_text(o, ft(0x30020004), Vr::ST, &img.description);
    w.strings(o, ft(0x00080008), Vr::CS, img.image_type.clone());
    w.str(o, ft(0x3002000c), Vr::CS, &img.plane);
    for (t, vr, a) in [
        (ft(0x00080064), Vr::CS, &img.conversion_type),
        (ft(0x3002000a), Vr::CS, &img.reported_values_origin),
        (ft(0x30020020), Vr::SH, &img.machine_name),
        (tagPrimaryDosimeterUnit, Vr::CS, &img.primary_dosimeter_unit),
    ] {
        text(&mut w, o, t, vr, a);
    }
    for (t, a) in [
        (ft(0x3002000d), &img.receptor_translation),
        (ft(0x30020010), &img.orientation),
        (ft(0x30020011), &img.image_plane_pixel_spacing),
        (ft(0x30020012), &img.position),
    ] {
        vector(&mut w, o, t, a);
    }
    for (t, a) in [
        (ft(0x3002000e), &img.receptor_angle),
        (ft(0x30020022), &img.sad),
        (ft(0x30020024), &img.ssd),
        (ft(0x30020026), &img.sid),
        (ft(0x300c0008), &img.start_cumulative_meterset_weight),
        (ft(0x300c0009), &img.end_cumulative_meterset_weight),
    ] {
        w.float(o, t, a);
    }
    w.int(o, tagReferencedBeamNumber, &img.referenced_beam_number);
    w.int(o, ft(0x300c0022), &img.referenced_fraction_group_number);
    w.int(o, ft(0x30020029), &img.fraction_number);
    let old_plan_refs = o.get_sequence(ft(0x300c0002)).unwrap_or_default();
    o.remove(ft(0x300c0002));
    if let Some(plan) = &img.referenced_plan {
        let mut reference = Object::new();
        for p in &old_plan_refs {
            if p.get_uid(tagRefSOPInstanceUID).unwrap_or_default() == plan.sop_instance_uid {
                reference = p.clone();
            }
        }
        w.str(
            &mut reference,
            tagRefSOPClassUID,
            Vr::UI,
            &plan.sop_class_uid,
        );
        w.str(
            &mut reference,
            tagRefSOPInstanceUID,
            Vr::UI,
            &plan.sop_instance_uid,
        );
        put_sequence(o, ft(0x300c0002), &[reference]);
    }
    let mut old_exposure = o.get_sequence(ft(0x30020030)).unwrap_or_default();
    o.remove(ft(0x30020030));
    if let Some(exposure) = &img.exposure {
        let mut e = if old_exposure.len() == 1 {
            old_exposure.remove(0)
        } else {
            Object::new()
        };
        w.float(&mut e, ft(0x00180060), &exposure.kvp);
        w.float(&mut e, ft(0x30020032), &exposure.meterset_exposure);
        put_sequence(o, ft(0x30020030), &[e]);
    }
    let m = &img.metadata;
    for (t, n) in [
        (0x00280010, m.rows),
        (0x00280011, m.columns),
        (0x00280002, m.samples_per_pixel),
        (0x00280100, m.bits_allocated),
        (0x00280101, m.bits_stored),
        (0x00280102, m.high_bit),
        (0x00280103, m.pixel_representation),
    ] {
        w.integer(o, ft(t), i64::from(n));
    }
    w.str(o, ft(0x00280004), Vr::CS, &m.photometric_interpretation);
    o.remove(ft(0x00280006));
    if m.planar_configuration_present {
        w.integer(o, ft(0x00280006), i64::from(m.planar_configuration));
    }
    o.remove(ft(0x00280008));
    if img.number_of_frames_present {
        w.integer(o, ft(0x00280008), m.number_of_frames);
    }
    let mut raw = img.pixels.0.clone();
    if raw.len() & 1 != 0 {
        raw.push(0);
    }
    let vr = if m.bits_allocated == 8 {
        Vr::OB
    } else {
        Vr::OW
    };
    o.put(Element {
        header: ElementHeader {
            tag: ft(0x7fe00010),
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Raw(raw),
    });
    if let Some(e) = w.err.take() {
        return Err(e);
    }
    let actual = read_image(&f, read_options)?;
    if !semantic_equal(img, &actual) {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "",
            "image model cannot be represented by the profile",
        ));
    }
    let changed = !semantic_equal(&before, img);
    if changed {
        f.dataset.remove(ft(0x00880200));
    }
    finish_write(f, w, opts, RT_IMAGE_STORAGE, changed)
}
