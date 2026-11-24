#[path = "support/build.rs"]
mod build;
use build::*;
use dicom_rust::deid::{self, BasicProfileOptions, ProfileOption, UidRemapper};
use dicom_rust::object::{self, File, Object, Tag, Vr, writer};
const STUDY: &str = "1.2.826.0.1.3680043.10.3.1";
fn identified(number: u32) -> File {
    let mut ds: Object = image(4, 4, 1, 8, 8);
    for e in [
        text(0x0008, 0x0018, Vr::UI, &format!("{STUDY}.1.{number}")),
        text(0x0008, 0x0020, Vr::DA, "20240102"),
        text(0x0008, 0x0030, Vr::TM, "101500"),
        text(0x0008, 0x0050, Vr::SH, "ACC-SYN-9"),
        text(0x0008, 0x0080, Vr::LO, "Example Hospital"),
        text(0x0008, 0x0090, Vr::PN, "Referrer^Ref"),
        text(0x0008, 0x1030, Vr::LO, "Synthetic study"),
        text(0x0010, 0x0010, Vr::PN, "Doe^Jane"),
        text(0x0010, 0x0020, Vr::LO, "SYN-1234"),
        text(0x0010, 0x0030, Vr::DA, "19700101"),
        text(0x0010, 0x0040, Vr::CS, "F"),
        text(0x0010, 0x1000, Vr::LO, "OTHER-ID"),
        text(0x0010, 0x4000, Vr::LT, "Patient comment"),
        text(0x0020, 0x000D, Vr::UI, STUDY),
        text(0x0020, 0x000E, Vr::UI, &format!("{STUDY}.1")),
        text(0x0020, 0x0010, Vr::SH, "STUDY1"),
        text(0x0029, 0x0010, Vr::LO, "SYNTHETIC PRIVATE"),
        text(0x0029, 0x1001, Vr::LO, "private identifying text"),
        sequence(
            0x0008,
            0x1140,
            vec![vec![
                text(0x0008, 0x1150, Vr::UI, SECONDARY_CAPTURE),
                text(0x0008, 0x1155, Vr::UI, &format!("{STUDY}.1.99")),
            ]],
        ),
    ] {
        ds.put(e);
    }
    file(ds, EXPLICIT_LE)
}
fn s(f: &File, group: u16, element: u16) -> Option<String> {
    f.dataset.get_string(Tag::new(group, element))
}
fn profile(src: &File, options: Vec<ProfileOption>, key: &[u8]) -> File {
    let mut opts = BasicProfileOptions {
        selected_options: options,
        ..BasicProfileOptions::default()
    };
    let (out, report) =
        deid::clone_file_with_basic_profile(src, &mut opts, &mut UidRemapper::new_keyed(key))
            .expect("basic profile");
    assert!(!report.actions.is_empty());
    out
}
#[test]
fn basic_profile_removes_identity() {
    let src = identified(1);
    let before = writer::write_file(&src).unwrap();
    let out = profile(&src, Vec::new(), b"test key");
    assert_eq!(writer::write_file(&src).unwrap(), before);
    for (g, e) in [
        (0x0010, 0x0010),
        (0x0010, 0x0020),
        (0x0008, 0x0050),
        (0x0008, 0x0090),
        (0x0020, 0x0010),
    ] {
        let v = s(&out, g, e).unwrap_or_default();
        assert!(
            v.trim().is_empty() || v != s(&src, g, e).unwrap(),
            "({g:04X},{e:04X}) kept: {v:?}"
        );
    }
    for (g, e) in [
        (0x0010, 0x0030),
        (0x0010, 0x1000),
        (0x0010, 0x4000),
        (0x0008, 0x0080),
    ] {
        let v = s(&out, g, e).unwrap_or_default();
        assert_ne!(Some(v.clone()), s(&src, g, e), "({g:04X},{e:04X}) kept");
    }
    for (g, e) in [
        (0x0010, 0x1000),
        (0x0010, 0x4000),
        (0x0029, 0x1001),
        (0x0029, 0x0010),
    ] {
        assert!(
            !out.dataset.has(Tag::new(g, e)),
            "({g:04X},{e:04X}) not removed"
        );
    }
    assert_eq!(
        s(&out, 0x0012, 0x0062).as_deref(),
        Some("YES"),
        "Patient Identity Removed"
    );
    assert!(
        out.dataset.has(Tag::new(0x0012, 0x0064)),
        "De-identification Method Code Sequence"
    );
    let sop = s(&out, 0x0008, 0x0018).unwrap();
    assert_ne!(sop, s(&src, 0x0008, 0x0018).unwrap());
    assert!(object::is_valid_uid(sop.as_bytes()), "{sop}");
    assert_eq!(
        out.meta.get_string(Tag::new(0x0002, 0x0003)).as_deref(),
        Some(sop.as_str())
    );
    assert_ne!(s(&out, 0x0020, 0x000D).unwrap(), STUDY);
    if let Some(referenced) = out.dataset.get_sequence(Tag::new(0x0008, 0x1140)) {
        assert_ne!(
            referenced[0]
                .get_string(Tag::new(0x0008, 0x1155))
                .unwrap_or_default(),
            format!("{STUDY}.1.99")
        );
    }
    assert_eq!(pixel_bytes(&out), pixel_bytes(&src));
    assert_eq!(out.dataset.get_int(Tag::new(0x0028, 0x0010)).unwrap(), 4);
    let bytes = writer::write_file(&out).unwrap();
    let back = object::read_file(&bytes).map_err(|(_, e)| e).unwrap();
    assert_eq!(back.dataset.get_string(Tag::new(0x0008, 0x0018)), Some(sop));
}
#[test]
fn uid_mapping_is_keyed_and_consistent() {
    let (a, b) = (identified(1), identified(2));
    let mut uids = UidRemapper::new_keyed(b"same key");
    let mut opts = BasicProfileOptions::default();
    let (a1, _) = deid::clone_file_with_basic_profile(&a, &mut opts, &mut uids).unwrap();
    let (b1, _) = deid::clone_file_with_basic_profile(&b, &mut opts, &mut uids).unwrap();
    assert_eq!(s(&a1, 0x0020, 0x000D), s(&b1, 0x0020, 0x000D));
    assert_ne!(s(&a1, 0x0008, 0x0018), s(&b1, 0x0008, 0x0018));
    let again = profile(&a, Vec::new(), b"same key");
    assert_eq!(s(&again, 0x0008, 0x0018), s(&a1, 0x0008, 0x0018));
    let other = profile(&a, Vec::new(), b"other key");
    assert_ne!(s(&other, 0x0008, 0x0018), s(&a1, 0x0008, 0x0018));
}
#[test]
fn options_retain_what_they_name() {
    let src = identified(1);
    let out = profile(
        &src,
        vec![
            ProfileOption::RetainUids,
            ProfileOption::RetainFullDates,
            ProfileOption::RetainInstitutionIdentity,
        ],
        b"k",
    );
    assert_eq!(s(&out, 0x0008, 0x0018), s(&src, 0x0008, 0x0018));
    assert_eq!(s(&out, 0x0020, 0x000D).as_deref(), Some(STUDY));
    assert_eq!(s(&out, 0x0008, 0x0020).as_deref(), Some("20240102"));
    assert_eq!(s(&out, 0x0008, 0x0080).as_deref(), Some("Example Hospital"));
    assert_ne!(s(&out, 0x0010, 0x0010).as_deref(), Some("Doe^Jane"));
    assert_eq!(
        ProfileOption::parse("retain-uids"),
        Some(ProfileOption::RetainUids)
    );
    assert_eq!(ProfileOption::RetainFullDates.as_str(), "retain-full-dates");
    assert_eq!(ProfileOption::parse("keep-everything"), None);
}
#[test]
fn the_simple_anonymizer_replaces_names() {
    let src = identified(1);
    let opts = deid::Options {
        patient_name: "ANON^ONE".into(),
        patient_id: "ANON-1".into(),
        ..Default::default()
    };
    let (out, _) = deid::clone_anonymized_file(&src, &opts, &mut UidRemapper::new_keyed(b"k"))
        .map_err(|(_, e)| e)
        .expect("anonymize");
    assert_eq!(s(&out, 0x0010, 0x0010).as_deref(), Some("ANON^ONE"));
    assert_eq!(s(&out, 0x0010, 0x0020).as_deref(), Some("ANON-1"));
    assert!(s(&out, 0x0029, 0x1001).is_none(), "private tags removed");
    assert_ne!(s(&out, 0x0008, 0x0018), s(&src, 0x0008, 0x0018));
}
