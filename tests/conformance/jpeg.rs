use crate::support::{self, Outcome, Report};
use dicom_rust::codec::jpeg;
use dicom_rust::codec::jpeglossless::{self, Metadata};
use std::collections::BTreeMap;
use std::path::Path;
fn frame_header(stream: &[u8]) -> Option<(u8, u16, u16, u16, u16)> {
    let mut at = 2;
    while at + 4 <= stream.len() {
        if stream[at] != 0xFF {
            return None;
        }
        if stream[at + 1] == 0xFF {
            at += 1;
            continue;
        }
        let marker = stream[at + 1];
        let length = usize::from(u16::from_be_bytes([stream[at + 2], stream[at + 3]]));
        let sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if sof {
            let s = stream.get(at + 4..at + 10)?;
            return Some((
                marker,
                u16::from(s[0]),
                u16::from_be_bytes([s[1], s[2]]),
                u16::from_be_bytes([s[3], s[4]]),
                u16::from(s[5]),
            ));
        }
        at += 2 + length;
    }
    None
}
fn describe(marker: u8) -> &'static str {
    match marker {
        0xC0 => "baseline",
        0xC1 => "extended sequential",
        0xC2 => "progressive",
        0xC3 => "lossless",
        0xC9 => "extended sequential, arithmetic",
        0xCA => "progressive, arithmetic",
        0xCB => "lossless, arithmetic",
        _ => "another process",
    }
}
fn decode(stream: &[u8]) -> (String, Result<(), String>) {
    let Some((marker, precision, rows, columns, components)) = frame_header(stream) else {
        let metadata = Metadata {
            rows: 1,
            columns: 1,
            samples_per_pixel: 1,
            bits_allocated: 8,
            bits_stored: 8,
            high_bit: 7,
            number_of_frames: 1,
            photometric_interpretation: "MONOCHROME2".into(),
            ..Default::default()
        };
        return (
            "tables only".into(),
            jpeg::decode_frames(&[stream], &metadata, Default::default())
                .map(|_| ())
                .map_err(|e| e.to_string()),
        );
    };
    let allocated = if precision > 8 { 16 } else { 8 };
    let metadata = Metadata {
        rows,
        columns,
        samples_per_pixel: components,
        bits_allocated: allocated,
        bits_stored: precision,
        high_bit: precision.saturating_sub(1),
        pixel_representation: 0,
        planar_configuration: 0,
        planar_configuration_present: components > 1,
        number_of_frames: 1,
        photometric_interpretation: "RGB".into(),
    };
    let what = format!(
        "{}, {precision} bits, {components} components",
        describe(marker)
    );
    let result = if matches!(marker, 0xC3 | 0xCB) {
        jpeglossless::decode_frames(&[stream], &metadata, Default::default())
            .map(|_| ())
            .map_err(|e| e.to_string())
    } else {
        jpeg::decode_frames(&[stream], &metadata, Default::default())
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    (what, result)
}
fn collect(dir: &Path, streams: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        if path.is_dir() {
            collect(&path, streams);
        } else if path.extension().is_some_and(|x| x == "JPG") {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let data = support::read(&path);
            if let Some(first) = streams.get(&name) {
                assert_eq!(first, &data, "{name} differs between directories");
            }
            streams.insert(name, data);
        }
    }
}
#[test]
#[ignore = "needs the conformance cache: run scripts/ci.sh conformance"]
fn itu_t83_jpeg() {
    let Some(dir) = support::set_dir("itu-t83") else {
        return;
    };
    let mut streams = BTreeMap::new();
    collect(&dir, &mut streams);
    let mut report = Report::new("itu-t83 (JPEG)");
    for (name, stream) in &streams {
        let (what, result) = decode(stream);
        let outcome = match result {
            Ok(()) => Outcome::Pass(format!("decoded ({what})")),
            Err(e) => Outcome::Refused(format!("{e} ({what})")),
        };
        report.case(
            name,
            outcome,
            Some("not DICOM Pixel Data: four or more components with different sampling"),
        );
    }
    report.finish();
}
