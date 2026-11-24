use crate::harness::{Runner, Work};
use crate::inputs::{self, Inputs};
use crate::part10::{self, CtPlace};
use dicom_rust::dicomdir::{self, FileId, FileSet, Options, WriteOptions};
use std::path::{Path, PathBuf};
pub const INSTANCES: usize = 1000;
const VERSION: &str = "dicomdir-v1";
pub const NAMES: &[&str] = &[
    "dicomdir/read/referenced_paths_1000",
    "dicomdir/read/parse_file_set_1000",
    "dicomdir/index/add_1000",
    "dicomdir/write/1000",
];
fn ids() -> Vec<(CtPlace, Vec<String>)> {
    let mut out = Vec::with_capacity(INSTANCES);
    for p in 0..10u64 {
        for s in 0..2u64 {
            let study = p * 10 + s + 1;
            for se in 0..5u64 {
                let series = study * 10 + se + 1;
                for i in 0..10u64 {
                    let place = CtPlace {
                        patient: p + 1,
                        study,
                        series,
                        instance: i,
                    };
                    let id = vec![
                        "DICOM".to_string(),
                        format!("P{:04}", p + 1),
                        format!("S{:04}", s + 1),
                        format!("SE{:04}", se + 1),
                        format!("I{:04}", i + 1),
                    ];
                    out.push((place, id));
                }
            }
        }
    }
    out
}
pub fn root(inp: &Inputs) -> Result<PathBuf, String> {
    let root = inputs::cache_dir().join(VERSION);
    if root.join("DONE").is_file() && root.join("DICOMDIR").is_file() {
        return Ok(root);
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let ct = inp.ct_slice();
    let mut crop = Vec::with_capacity(64 * 64 * 2);
    for y in 0..64 {
        for x in 0..64 {
            crop.extend_from_slice(&ct.pixels[(200 + y) * 512 + 200 + x].to_le_bytes());
        }
    }
    let all = ids();
    for (place, id) in &all {
        let path: PathBuf = id.iter().fold(root.clone(), |p, c| p.join(c));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let bytes = part10::ct_native_file(*place, 64, 64, place.instance as f64, &crop);
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    let set = index(&root)?;
    let (_, err) = dicomdir::commit_dicomdir(&set, &WriteOptions::default());
    if let Some(e) = err {
        return Err(format!("commit DICOMDIR: {e}"));
    }
    std::fs::write(root.join("DONE"), b"").map_err(|e| e.to_string())?;
    Ok(root)
}
fn index(root: &Path) -> Result<FileSet, String> {
    let mut set =
        FileSet::new(&root.to_string_lossy(), Options::default()).map_err(|e| e.to_string())?;
    for (_, id) in ids() {
        set.add(&FileId(id)).map_err(|e| e.to_string())?;
    }
    Ok(set)
}
pub fn run(r: &mut Runner, inp: &Inputs) {
    if !r.any_selected(NAMES) {
        return;
    }
    let root = match root(inp) {
        Ok(p) => p,
        Err(e) => panic!("DICOMDIR file set: {e}"),
    };
    let dicomdir = root.join("DICOMDIR").to_string_lossy().into_owned();
    let n = Work::Items(INSTANCES as u64);
    r.bench("dicomdir/read/referenced_paths_1000", n, || {
        dicomdir::referenced_paths(&dicomdir).expect("referenced paths")
    });
    r.bench("dicomdir/read/parse_file_set_1000", n, || {
        dicomdir::parse_file_set(&dicomdir).expect("parse file set")
    });
    r.bench("dicomdir/index/add_1000", n, || {
        index(&root).expect("index")
    });
    let set = index(&root).expect("index");
    r.bench("dicomdir/write/1000", n, || {
        let mut out = Vec::new();
        dicomdir::write_dicomdir(&mut out, &set, &WriteOptions::default()).expect("write");
        out
    });
}
pub fn check(inp: &Inputs) -> Result<(), String> {
    let root = root(inp)?;
    let dicomdir = root.join("DICOMDIR").to_string_lossy().into_owned();
    let paths = dicomdir::referenced_paths(&dicomdir).map_err(|e| e.to_string())?;
    if paths.len() != INSTANCES {
        return Err(format!(
            "DICOMDIR references {} files, want {INSTANCES}",
            paths.len()
        ));
    }
    let set = dicomdir::parse_file_set(&dicomdir).map_err(|e| e.to_string())?;
    if set.records().len() != INSTANCES {
        return Err(format!(
            "file set has {} records, want {INSTANCES}",
            set.records().len()
        ));
    }
    Ok(())
}
