use dicom_rust::object::open_file;
use dicom_rust::pixel::frame_reader::decode_file_frames_view;
use dicom_rust::pixel::guard::{GuardError, GuardOptions, guard_file};
use std::path::{Path, PathBuf};
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}
#[test]
fn every_decodable_fixture_passes_the_guard() {
    let Some(root) = std::env::var_os("DICOM_RUST_FIXTURES").map(PathBuf::from) else {
        eprintln!("DICOM_RUST_FIXTURES is not set: skipped");
        return;
    };
    let mut files = Vec::new();
    walk(&root, &mut files);
    let (mut decoded, mut refused, mut trailing) = (0, Vec::new(), Vec::new());
    for path in files {
        if std::fs::metadata(&path).map_or(true, |m| m.len() > 256 << 20) {
            continue;
        }
        let Ok(file) = open_file(&path) else {
            continue;
        };
        if decode_file_frames_view(&file, None).is_err() {
            continue;
        }
        decoded += 1;
        match guard_file(&file, &GuardOptions::default()) {
            Ok(()) => {}
            Err(GuardError::NativeLength { actual, expected }) if actual > expected => {
                let recovering = GuardOptions {
                    allow_trailing: true,
                    ..GuardOptions::default()
                };
                if let Err(e) = guard_file(&file, &recovering) {
                    refused.push(format!("{}: {}", path.display(), e.details()));
                }
                trailing.push(path.display().to_string());
            }
            Err(e) => refused.push(format!("{}: {}", path.display(), e.details())),
        }
    }
    eprintln!("{decoded} decodable fixtures checked");
    eprintln!(
        "refused for trailing bytes (by the guard's rule):\n{}",
        trailing.join("\n")
    );
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}
