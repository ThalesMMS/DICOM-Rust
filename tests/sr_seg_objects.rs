#![cfg(feature = "render")]
#[path = "support/build.rs"]
mod build;
use build::*;
use dicom_rust::dcmtime::ZonedTime;
use dicom_rust::object::{self, Object, Tag, writer};
use dicom_rust::render::{SliceGeometry, Vec3};
use dicom_rust::roi::RasterMask;
use dicom_rust::seg;
use dicom_rust::sr::{self, ImageReference};
const STUDY: &str = "1.2.826.0.1.3680043.10.5.1";
const SERIES: &str = "1.2.826.0.1.3680043.10.5.1.1";
fn through_a_file(f: &object::File) -> object::File {
    let bytes = writer::write_file(f).expect("write");
    object::read_file(&bytes)
        .map_err(|(_, e)| e)
        .expect("read back")
}
fn image_ref(n: u32) -> ImageReference {
    ImageReference {
        study_instance_uid: STUDY.into(),
        series_instance_uid: SERIES.into(),
        sop_class_uid: CT_IMAGE.into(),
        sop_instance_uid: format!("{SERIES}.{n}"),
        frames: Vec::new(),
    }
}
#[test]
fn key_object_selection_round_trips() {
    let refs = vec![image_ref(1), image_ref(2), image_ref(3)];
    let mut doc = sr::kos::new_key_object_selection("1.2.826.0.1.3680043.10.5.9", &refs, None);
    doc.content_date = "20240102".into();
    doc.content_time = "030405".into();
    doc.study_instance_uid = STUDY.into();
    doc.series_instance_uid = "1.2.826.0.1.3680043.10.5.2".into();
    let elements = doc.elements().expect("elements");
    let ds = Object::from_elements(elements);
    assert_eq!(
        ds.get_string(Tag::new(0x0008, 0x0060)).as_deref(),
        Some("KO")
    );
    let back = through_a_file(&file(ds, EXPLICIT_LE));
    let read = sr::read_document(Some(&back.dataset))
        .map_err(|e| e.error)
        .expect("read document");
    assert_eq!(read.sop_instance_uid, "1.2.826.0.1.3680043.10.5.9");
    let images = sr::kos::key_object_selection_images(Some(&read));
    let uids: Vec<&str> = images.iter().map(|r| r.sop_instance_uid.as_str()).collect();
    assert_eq!(
        uids,
        refs.iter()
            .map(|r| r.sop_instance_uid.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(read.title, sr::kos::kos_title_of_interest());
    assert!(sr::read_document(None).is_err());
}
#[test]
fn a_measurement_report_round_trips() {
    use sr::measurement_export::{
        MeasurementExportOptions, ViewerMeasurement, from_measurements, long_axis_concept,
        millimeter_unit,
    };
    let opts = MeasurementExportOptions {
        sop_instance_uid: "1.2.826.0.1.3680043.10.5.10".into(),
        study_instance_uid: STUDY.into(),
        series_instance_uid: "1.2.826.0.1.3680043.10.5.3".into(),
        content_date: "20240102".into(),
        content_time: "030405".into(),
        measurements: vec![ViewerMeasurement {
            kind: "length".into(),
            label: "lesion".into(),
            value: 12.5,
            concept_name: long_axis_concept(),
            units: millimeter_unit(),
            source_image: image_ref(1),
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut report = from_measurements(&opts).expect("report");
    report.common.now = Some(ZonedTime::builder(2024, 1, 2).time(3, 4, 5).build());
    let f = sr::measurement_report::write_measurement_report(Some(&report)).expect("write report");
    let back = through_a_file(&f);
    let read =
        sr::measurement_report::read_measurement_report(Some(&back.dataset)).expect("read report");
    assert_eq!(read.sop_instance_uid, "1.2.826.0.1.3680043.10.5.10");
    assert_eq!(read.groups.len(), 1);
    let m = &read.groups[0].measurements;
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].value, 12.5);
    assert_eq!(m[0].units, millimeter_unit());
    assert_eq!(m[0].concept_name, long_axis_concept());
    assert!(from_measurements(&MeasurementExportOptions::default()).is_err());
}
fn slice(z: f64) -> SliceGeometry {
    SliceGeometry {
        origin: Vec3 {
            x: -10.0,
            y: -12.0,
            z,
        },
        row_dir: Vec3 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        col_dir: Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        },
        normal: Vec3 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
        row_spacing: 0.5,
        col_spacing: 0.75,
        rows: 6,
        columns: 5,
    }
}
fn mask(cells: &[(i64, i64)]) -> RasterMask {
    let mut m = RasterMask::new(5, 6);
    for &(x, y) in cells {
        m.set(x, y, true);
    }
    m
}
fn segment(number: i64, label: &str) -> seg::Segment {
    seg::Segment {
        number,
        label: label.into(),
        algorithm_type: "MANUAL".into(),
        segmented_property_category: seg::generic_roi_category_code(),
        segmented_property_type: seg::generic_roi_type_code(),
        recommended_display_cielab: seg::rgb_to_dicom_cielab([255, 0, 0]),
        ..Default::default()
    }
}
#[test]
fn a_binary_segmentation_round_trips() {
    let frames = [
        (1, 0, vec![(0, 0), (1, 0), (4, 5)]),
        (1, 1, vec![(2, 2), (2, 3), (3, 3)]),
        (2, 1, vec![(0, 5), (4, 0)]),
    ];
    let doc = seg::Document {
        sop_instance_uid: "1.2.826.0.1.3680043.10.5.20".into(),
        study_instance_uid: STUDY.into(),
        series_instance_uid: "1.2.826.0.1.3680043.10.5.4".into(),
        frame_of_reference_uid: "1.2.826.0.1.3680043.10.5.5".into(),
        rows: 6,
        columns: 5,
        segmentation_type: "BINARY".into(),
        segments: vec![segment(1, "first"), segment(2, "second")],
        referenced_images: vec![
            seg::ReferencedImage {
                series_instance_uid: SERIES.into(),
                sop_class_uid: CT_IMAGE.into(),
                sop_instance_uid: format!("{SERIES}.1"),
                frames: Vec::new(),
            },
            seg::ReferencedImage {
                series_instance_uid: SERIES.into(),
                sop_class_uid: CT_IMAGE.into(),
                sop_instance_uid: format!("{SERIES}.2"),
                frames: Vec::new(),
            },
        ],
        frames: frames
            .iter()
            .map(|(segment, slice_index, cells)| seg::Frame {
                segment_number: *segment,
                slice_index: *slice_index,
                referenced_sop_instance_uid: format!("{SERIES}.{}", slice_index + 1),
                geometry: slice(*slice_index as f64 * 2.5),
                mask: Some(mask(cells)),
                ..Default::default()
            })
            .collect(),
        content_date: "20240102".into(),
        content_time: "030405".into(),
        ..Default::default()
    };
    let f = seg::write(&doc).expect("write segmentation");
    assert_eq!(
        f.dataset.get_string(Tag::new(0x0008, 0x0060)).as_deref(),
        Some("SEG")
    );
    let back = through_a_file(&f);
    let read = seg::read(&back.dataset).expect("read segmentation");
    assert_eq!(read.segmentation_type, "BINARY");
    assert_eq!(read.segments.len(), 2);
    assert_eq!(read.segments[1].label, "second");
    assert_eq!(
        read.segments[0].recommended_display_cielab,
        seg::rgb_to_dicom_cielab([255, 0, 0])
    );
    assert_eq!(read.frames.len(), 3);
    for (want, got) in frames.iter().zip(&read.frames) {
        assert_eq!(got.segment_number, want.0);
        let m = got.mask.as_ref().expect("mask");
        for y in 0..6 {
            for x in 0..5 {
                assert_eq!(
                    m.get(x, y),
                    want.2.contains(&(x, y)),
                    "segment {} at ({x},{y})",
                    want.0
                );
            }
        }
        assert!((got.geometry.origin.z - want.1 as f64 * 2.5).abs() < 1e-9);
    }
    let rgb = seg::dicom_cielab_to_rgb(seg::rgb_to_dicom_cielab([10, 200, 30]));
    assert!(
        rgb.iter()
            .zip([10, 200, 30])
            .all(|(a, b)| (a - b).abs() <= 2),
        "{rgb:?}"
    );
}
