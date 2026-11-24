use dicom_rust::dicomdir::{self, FileSet, Options, ScanOptions, WriteOptions};
use std::error::Error;
use std::path::PathBuf;
fn sample_file_set() -> Result<PathBuf, Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!("dicom-rust-fileset-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("IMAGES"))?;
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/data/ct_small.dcm");
    std::fs::copy(fixture, root.join("IMAGES").join("IM000001"))?;
    let root_text = root.to_str().ok_or("temporary path is not UTF-8")?;
    let mut set = FileSet::new(root_text, Options::default())?;
    let (scan, err) = set.scan(&ScanOptions::default());
    if let Some(err) = err {
        return Err(err.into());
    }
    println!("scanned: {} added, {} skipped", scan.added, scan.skipped);
    let (report, err) = dicomdir::commit_dicomdir(&set, &WriteOptions::default());
    if let Some(err) = err {
        return Err(err.into());
    }
    println!(
        "wrote DICOMDIR: {} bytes, {} records",
        report.bytes_written, report.records
    );
    Ok(root)
}
fn main() -> Result<(), Box<dyn Error>> {
    let root = match std::env::args().nth(1) {
        Some(folder) => PathBuf::from(folder),
        None => sample_file_set()?,
    };
    let dicomdir_path = root.join("DICOMDIR");
    if !dicomdir_path.is_file() {
        return Err(format!("{} has no DICOMDIR", root.display()).into());
    }
    let set = dicomdir::parse_file_set(dicomdir_path.to_str().ok_or("path is not UTF-8")?)?;
    println!("file set {:?} ({})", set.file_set_id(), set.file_set_uid());
    for record in set.records() {
        println!(
            "{:<6} {:<20} patient {:<14} study {} series {} modality {}",
            record.record_type,
            record.file_id.to_string(),
            record.patient_name,
            record.study_date,
            record.series_number,
            record.modality,
        );
    }
    Ok(())
}
