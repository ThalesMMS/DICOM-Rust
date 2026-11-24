use dicom_rust::codec::jpegxl;
use std::path::Path;
use std::time::{Duration, Instant};
const BOUND: Duration = Duration::from_millis(500);
fn slow_inputs() -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/slow");
    let mut out = Vec::new();
    for target in ["codec_jpegxl", "transcode"] {
        let dir = root.join(target);
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|e| e.expect("directory entry").path())
            .collect();
        names.sort();
        for path in names {
            let data = std::fs::read(&path).expect("slow input");
            out.push((format!("{target}/{}", path.display()), data));
        }
    }
    out
}
fn fragments(data: &[u8]) -> Vec<&[u8]> {
    let frames = usize::from(data[8] % 4 + 1);
    let payload = &data[10..];
    let step = payload.len().div_ceil(frames).max(1);
    payload.chunks(step).collect()
}
#[test]
fn small_inputs_declaring_huge_icc_profiles_are_refused_quickly() {
    let inputs = slow_inputs();
    assert_eq!(inputs.len(), 7, "the seven inputs of fuzz/slow");
    for (name, data) in &inputs {
        let start = Instant::now();
        let err = jpegxl::decode(fragments(data)[0]).expect_err(name);
        let took = start.elapsed();
        assert!(err.is_limit(), "{name}: {err}");
        assert!(took < BOUND, "{name}: {took:?}");
    }
}
#[test]
fn every_fragment_of_the_slow_inputs_decodes_quickly() {
    for (name, data) in &slow_inputs() {
        let start = Instant::now();
        for fragment in fragments(data) {
            let _ = jpegxl::decode(fragment);
        }
        let took = start.elapsed();
        assert!(took < BOUND, "{name}: {took:?}");
    }
}
