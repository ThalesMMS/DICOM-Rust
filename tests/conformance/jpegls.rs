use crate::support::{self, Outcome, Plane, Report};
use dicom_rust::codec::jpeglossless::Metadata;
use dicom_rust::codec::jpegls;
const SUBSAMPLED: &str = "components sub-sampled differently (256x256, 256x64, 128x128): \
    DICOM Pixel Data has one size for all samples, so the decoder refuses the stream";
const EXPLAINED: &[(&str, &str)] = &[("T8SSE0.JLS", SUBSAMPLED), ("T8SSE3.JLS", SUBSAMPLED)];
const CASES: &[(&str, &str, &[&str])] = &[
    ("T87_test-1-2-3-4-5-6", "T8C0E0.JLS", &["TEST8.PPM"]),
    ("T87_test-1-2-3-4-5-6", "T8C1E0.JLS", &["TEST8.PPM"]),
    ("T87_test-1-2-3-4-5-6", "T8C2E0.JLS", &["TEST8.PPM"]),
    ("T87_test-1-2-3-4-5-6", "T8C0E3.JLS", &["TEST8.PPM"]),
    ("T87_test-1-2-3-4-5-6", "T8C1E3.JLS", &["TEST8.PPM"]),
    ("T87_test-1-2-3-4-5-6", "T8C2E3.JLS", &["TEST8.PPM"]),
    (
        "T87_test-7-8",
        "T8SSE0.JLS",
        &["TEST8R.PGM", "TEST8GR4.PGM", "TEST8BS2.PGM"],
    ),
    (
        "T87_test-7-8",
        "T8SSE3.JLS",
        &["TEST8R.PGM", "TEST8GR4.PGM", "TEST8BS2.PGM"],
    ),
    ("T87_test-9-10", "T8NDE0.JLS", &["TEST8BS2.PGM"]),
    ("T87_test-9-10", "T8NDE3.JLS", &["TEST8BS2.PGM"]),
    ("T87_test-11-12", "T16E0.JLS", &["TEST16.PGM"]),
    ("T87_test-11-12", "T16E3.JLS", &["TEST16.PGM"]),
];
fn decode(stream: &[u8]) -> Result<(Vec<Plane>, i64), String> {
    let options = jpegls::DecodeOptions::default();
    let info = jpegls::inspect_codestream(stream, 0, options).map_err(|e| e.to_string())?;
    let bits = info.precision as u16;
    let allocated = if bits > 8 { 16 } else { 8 };
    let metadata = Metadata {
        rows: info.rows as u16,
        columns: info.columns as u16,
        samples_per_pixel: info.components as u16,
        bits_allocated: allocated,
        bits_stored: bits,
        high_bit: bits - 1,
        pixel_representation: 0,
        planar_configuration: 0,
        planar_configuration_present: info.components > 1,
        number_of_frames: 1,
        photometric_interpretation: if info.components == 1 {
            "MONOCHROME2".into()
        } else {
            "RGB".into()
        },
    };
    let codec = if info.near == 0 {
        jpegls::Codec::lossless()
    } else {
        jpegls::Codec::near_lossless()
    };
    let frames = codec
        .decode_frames(&[stream], &metadata, options)
        .map_err(|e| e.to_string())?;
    let planes = support::planes_from_interleaved(
        &frames.data[0],
        frames.columns,
        frames.rows,
        info.components as usize,
        usize::from(allocated / 8),
        false,
        u32::from(allocated),
    );
    Ok((planes, info.near))
}
#[test]
#[ignore = "needs the conformance cache: run scripts/ci.sh conformance"]
fn itu_t87_jpeg_ls() {
    let Some(dir) = support::set_dir("itu-t87") else {
        return;
    };
    let mut report = Report::new("itu-t87 (JPEG-LS)");
    for (folder, stream, sources) in CASES {
        let folder = dir.join(folder);
        let reason = EXPLAINED
            .iter()
            .find(|(name, _)| name == stream)
            .map(|(_, reason)| *reason);
        let mut want = Vec::new();
        for source in *sources {
            want.extend(support::read_pnm(&support::read(&folder.join(source))).expect(source));
        }
        let outcome = match decode(&support::read(&folder.join(stream))) {
            Err(e) => Outcome::Refused(e),
            Ok((got, near)) => {
                let peak = vec![near; want.len()];
                let mse = vec![(near * near) as f64; want.len()];
                support::compare(&got, &want, &peak, &mse)
            }
        };
        report.case(stream, outcome, reason);
    }
    report.finish();
}
