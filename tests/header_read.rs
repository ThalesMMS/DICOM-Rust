use dicom_rust::object::{ReadFileOptions, open_file_header_with_options, open_file_with_options};
use std::path::{Path, PathBuf};
fn element(group: u16, elem: u16, vr: &str, value: &[u8]) -> Vec<u8> {
    let mut data = value.to_vec();
    if data.len() % 2 == 1 {
        data.push(0);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(vr.as_bytes());
    if matches!(vr, "OB" | "OW" | "SQ" | "UN") {
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    } else {
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    }
    out.extend_from_slice(&data);
    out
}
fn part10(body: &[u8]) -> Vec<u8> {
    let meta = [
        element(0x0002, 0x0001, "OB", &[0, 1]),
        element(0x0002, 0x0002, "UI", b"1.2.840.10008.5.1.4.1.1.7"),
        element(0x0002, 0x0003, "UI", b"1.2.3.4"),
        element(0x0002, 0x0010, "UI", b"1.2.840.10008.1.2.1"),
    ]
    .concat();
    let mut out = vec![0u8; 128];
    out.extend_from_slice(b"DICM");
    out.extend(element(
        0x0002,
        0x0000,
        "UL",
        &(meta.len() as u32).to_le_bytes(),
    ));
    out.extend(meta);
    out.extend_from_slice(body);
    out
}
fn nested_pixel_data() -> Vec<u8> {
    let mut body = element(0x0008, 0x0060, "CS", b"OT");
    body.extend_from_slice(&[0x88, 0x00, 0x00, 0x02]);
    body.extend_from_slice(b"SQ\0\0");
    body.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    body.extend_from_slice(&[0xFE, 0xFF, 0x00, 0xE0, 0xFF, 0xFF, 0xFF, 0xFF]);
    body.extend(element(0x0028, 0x0010, "US", &2u16.to_le_bytes()));
    body.extend(element(0x7FE0, 0x0010, "OB", &[1, 2, 3, 4]));
    body.extend_from_slice(&[0xFE, 0xFF, 0x0D, 0xE0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0]);
    body.extend(element(0x0028, 0x0010, "US", &4u16.to_le_bytes()));
    body.extend(element(0x0028, 0x0011, "US", &4u16.to_le_bytes()));
    body.extend(element(0x7FE0, 0x0010, "OW", &[7u8; 32]));
    body.extend(element(0xFFFC, 0xFFFC, "OB", &[0u8; 6]));
    part10(&body)
}
fn unit_inputs(unit: &str) -> Option<Vec<Vec<u8>>> {
    let root = std::env::var_os("DICOM_RUST_RECORDED_UNITS")?;
    if !Path::new(&root).is_dir() {
        return None;
    }
    let path = Path::new(&root).join(unit).join("cases.jsonl");
    Some(std::fs::read_to_string(&path).map_or_else(|_| Vec::new(), |text| case_inputs(&text)))
}
fn crate_inputs() -> Vec<Vec<u8>> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reference_cases/read_private.jsonl");
    case_inputs(&std::fs::read_to_string(&path).expect("reading reference cases"))
}
fn case_inputs(text: &str) -> Vec<Vec<u8>> {
    let from_hex = |hex: &str| -> Option<Vec<u8>> {
        if hex.len() < 8
            || !hex.len().is_multiple_of(2)
            || !hex.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return None;
        }
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect()
    };
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("{\"args\":[[") {
            let Some(end) = rest.find(']') else { continue };
            let bytes: Option<Vec<u8>> = rest[..end]
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().parse::<u8>().ok())
                .collect();
            if let Some(bytes) = bytes.filter(|b| !b.is_empty()) {
                out.push(bytes);
            }
        } else if let Some(rest) = line
            .strip_prefix("{\"args\":[{\"$hex\":\"")
            .or_else(|| line.strip_prefix("{\"args\":[\""))
        {
            let Some(end) = rest.find('"') else { continue };
            if let Some(bytes) = from_hex(&rest[..end]) {
                out.push(bytes);
            }
        }
    }
    out
}
fn same(path: &Path, what: &str) {
    for opts in [
        ReadFileOptions::default(),
        ReadFileOptions {
            skip_pixel_data: true,
            max_sequence_depth: 64,
            ..ReadFileOptions::default()
        },
    ] {
        let p = path.to_string_lossy();
        let whole = format!("{:?}", open_file_with_options(&*p, opts));
        let header = format!("{:?}", open_file_header_with_options(&p, opts));
        assert_eq!(header, whole, "{what} with {opts:?}");
    }
}
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dicom-rust-header-read-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
#[test]
fn the_header_read_equals_the_whole_read() {
    let dir = scratch();
    let file = dir.join("f.dcm");
    let check = |inputs: Vec<Vec<u8>>, what: &str| {
        let mut checked = 0;
        for bytes in inputs {
            for content in [bytes.clone(), part10(&bytes)] {
                std::fs::write(&file, &content).unwrap();
                same(&file, what);
                checked += 1;
            }
        }
        checked
    };
    let checked = check(crate_inputs(), "reference case");
    assert!(checked >= 40, "only {checked} reference case inputs found");
    let mut recorded = None;
    for unit in ["dicread", "selreader", "valread", "framereader"] {
        if let Some(inputs) = unit_inputs(unit) {
            *recorded.get_or_insert(0) += check(inputs, unit);
        }
    }
    match recorded {
        Some(checked) => assert!(checked > 100, "only {checked} unit inputs found"),
        None => eprintln!("no DICOM_RUST_RECORDED_UNITS directory: recorded inputs skipped"),
    }
    let nested = nested_pixel_data();
    std::fs::write(&file, &nested).unwrap();
    same(&file, "Pixel Data in an undefined-length sequence");
    for cut in [0, 1, 131, 140, nested.len() / 2, nested.len() - 3] {
        std::fs::write(&file, &nested[..cut]).unwrap();
        same(&file, &format!("truncated at {cut}"));
    }
    std::fs::write(&file, &nested).unwrap();
    let writer = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    same(&file, "a file a writer holds open");
    drop(writer);
    same(&dir, "a folder");
    same(&dir.join("missing.dcm"), "a missing file");
    let _ = std::fs::remove_dir_all(&dir);
}
