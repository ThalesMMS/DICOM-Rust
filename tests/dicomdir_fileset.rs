#[path = "support/build.rs"]
mod build;
use build::*;
use dicom_rust::dicomdir::{self, FileId, FileSet, Options, Query, WriteOptions};
use dicom_rust::object::{self, Tag, Vr, writer};
use std::path::{Path, PathBuf};
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("dicom-rust-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        Scratch(dir)
    }
    fn root(&self) -> &str {
        self.0.to_str().expect("UTF-8 temp path")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn instance(root: &Path, id: &[&str], patient: (&str, &str), study: u32, series: u32, number: u32) {
    let mut ds = image(4, 4, 1, 8, 8);
    let uid = |parts: &[u32]| {
        let tail: Vec<String> = parts.iter().map(u32::to_string).collect();
        format!("1.2.826.0.1.3680043.10.2.{}", tail.join("."))
    };
    ds.put(text(0x0008, 0x0018, Vr::UI, &uid(&[study, series, number])));
    ds.put(text(0x0008, 0x0020, Vr::DA, "20240102"));
    ds.put(text(0x0008, 0x0030, Vr::TM, "101500"));
    ds.put(text(0x0008, 0x0050, Vr::SH, &format!("ACC{study}")));
    ds.put(text(
        0x0008,
        0x0060,
        Vr::CS,
        if series == 1 { "OT" } else { "CT" },
    ));
    ds.put(text(0x0008, 0x1030, Vr::LO, "Synthetic"));
    ds.put(text(0x0010, 0x0010, Vr::PN, patient.0));
    ds.put(text(0x0010, 0x0020, Vr::LO, patient.1));
    ds.put(text(0x0020, 0x000D, Vr::UI, &uid(&[study])));
    ds.put(text(0x0020, 0x000E, Vr::UI, &uid(&[study, series])));
    ds.put(text(0x0020, 0x0010, Vr::SH, &format!("S{study}")));
    ds.put(text(0x0020, 0x0011, Vr::IS, &series.to_string()));
    ds.put(text(0x0020, 0x0013, Vr::IS, &number.to_string()));
    let bytes = writer::write_file(&file(ds, EXPLICIT_LE)).expect("write instance");
    let mut path = root.to_path_buf();
    for c in &id[..id.len() - 1] {
        path.push(c);
    }
    std::fs::create_dir_all(&path).unwrap();
    path.push(id[id.len() - 1]);
    std::fs::write(path, bytes).unwrap();
}
fn id(parts: &[&str]) -> FileId {
    dicomdir::new_file_id(parts).expect("valid file ID")
}
type FileSpec = (
    &'static [&'static str],
    (&'static str, &'static str),
    u32,
    u32,
    u32,
);
const FILES: &[FileSpec] = &[
    (&["IMG", "P1", "F1"], ("Alpha^One", "SYN-1"), 1, 1, 1),
    (&["IMG", "P1", "F2"], ("Alpha^One", "SYN-1"), 1, 1, 2),
    (&["IMG", "P1", "F3"], ("Alpha^One", "SYN-1"), 1, 2, 1),
    (&["IMG", "P2", "F1"], ("Beta^Two", "SYN-2"), 2, 1, 1),
];
fn populated(scratch: &Scratch) -> FileSet {
    for (parts, patient, study, series, number) in FILES {
        instance(&scratch.0, parts, *patient, *study, *series, *number);
    }
    let mut set = FileSet::new(
        scratch.root(),
        Options {
            file_set_id: "SYNSET".into(),
            ..Default::default()
        },
    )
    .expect("file-set");
    for (parts, ..) in FILES {
        set.add(&id(parts))
            .unwrap_or_else(|e| panic!("add {parts:?}: {e}"));
    }
    set
}
#[test]
fn a_committed_dicomdir_parses_back() {
    let scratch = Scratch::new("dicomdir-commit");
    let set = populated(&scratch);
    assert_eq!(set.file_set_id(), "SYNSET");
    let uid = set.file_set_uid().to_string();
    assert!(object::is_valid_uid(uid.as_bytes()), "{uid}");
    let stats = set.statistics();
    assert_eq!(
        (stats.patients, stats.studies, stats.series, stats.instances),
        (2, 2, 3, 4)
    );
    let (report, err) = set.validate();
    assert!(err.is_none(), "{err:?} {report:?}");
    let (report, err) = dicomdir::commit_dicomdir(&set, &WriteOptions::default());
    assert!(err.is_none(), "{err:?}");
    assert!(report.bytes_written > 0);
    let path = scratch.0.join("DICOMDIR");
    let dir = object::open_file(path.to_str().unwrap())
        .map_err(|(_, e)| e)
        .expect("DICOMDIR");
    assert_eq!(
        dir.dataset.get_string(Tag::new(0x0004, 0x1130)).as_deref(),
        Some("SYNSET"),
        "File-set ID"
    );
    assert_eq!(
        dir.meta.get_string(Tag::new(0x0002, 0x0002)).as_deref(),
        Some("1.2.840.10008.1.3.10")
    );
    let back = dicomdir::parse_file_set(path.to_str().unwrap()).expect("parse");
    assert_eq!(back.file_set_uid(), uid);
    let mut want: Vec<String> = FILES.iter().map(|(p, ..)| p.join("\\")).collect();
    let mut got: Vec<String> = back
        .records()
        .iter()
        .map(|r| r.file_id.components().join("\\"))
        .collect();
    want.sort();
    got.sort();
    assert_eq!(got, want);
    for r in back.records() {
        assert_eq!(r.sop_class_uid, SECONDARY_CAPTURE);
        assert_eq!(r.transfer_syntax_uid, EXPLICIT_LE);
        assert!(!r.patient_id.is_empty() && !r.study_instance_uid.is_empty());
    }
    assert_eq!(back.statistics().instances, 4);
    let refs = dicomdir::references(&dir.dataset);
    assert_eq!(refs.len(), 4);
    for r in &refs {
        let (mut opened, path) =
            dicomdir::open_referenced_file(scratch.root(), r).expect("referenced file");
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut opened, &mut bytes).unwrap();
        let f = object::read_file(&bytes).map_err(|(_, e)| e).expect(&path);
        assert_eq!(
            f.dataset.get_string(Tag::new(0x0008, 0x0016)).as_deref(),
            Some(SECONDARY_CAPTURE)
        );
    }
    let paths = dicomdir::referenced_paths(path.to_str().unwrap()).expect("paths");
    assert_eq!(paths.len(), 4);
    assert!(paths.iter().all(|p| Path::new(p).is_file()), "{paths:?}");
}
#[test]
fn queries_select_and_removals_publish() {
    let scratch = Scratch::new("dicomdir-query");
    let mut set = populated(&scratch);
    let by_patient = set.query(&Query {
        patient_id: "SYN-1".into(),
        ..Default::default()
    });
    assert_eq!(by_patient.len(), 3);
    let by_modality = set.query(&Query {
        modality: "CT".into(),
        ..Default::default()
    });
    assert_eq!(by_modality.len(), 1);
    assert_eq!(by_modality[0].file_id.components(), ["IMG", "P1", "F3"]);
    assert!(set.remove(&id(&["IMG", "P2", "F1"])));
    assert!(!set.remove(&id(&["IMG", "P2", "F1"])), "second removal");
    let (_, err) = dicomdir::commit_dicomdir(&set, &WriteOptions::default());
    assert!(err.is_none(), "{err:?}");
    let back = dicomdir::parse_file_set(scratch.0.join("DICOMDIR").to_str().unwrap()).unwrap();
    let stats = back.statistics();
    assert_eq!((stats.patients, stats.instances), (1, 3));
    let mut buf = Vec::new();
    dicomdir::write_dicomdir(&mut buf, &set, &WriteOptions::default()).expect("write");
    assert_eq!(buf, std::fs::read(scratch.0.join("DICOMDIR")).unwrap());
}
#[test]
fn bad_file_ids_and_missing_files_are_refused() {
    assert!(dicomdir::new_file_id(&["lowercase"]).is_err());
    assert!(dicomdir::new_file_id(&["NINECHARS"]).is_err());
    assert!(dicomdir::new_file_id::<&str>(&[]).is_err());
    assert!(dicomdir::parse_file_id("IMG\\P1\\F1").is_ok());
    let scratch = Scratch::new("dicomdir-missing");
    let mut set = FileSet::new(scratch.root(), Options::default()).expect("file-set");
    assert!(set.add(&id(&["NOFILE"])).is_err());
    assert!(dicomdir::parse_file_set(scratch.0.join("DICOMDIR").to_str().unwrap()).is_err());
}
