use dicom_rust::limits::Limits;
use dicom_rust::net::ul;
use dicom_rust::object::{self, ReadFileOptions, Syntax};
use dicom_rust::pixel::pixeldata::{self, CodecRecovery, PixelData};
use dicom_rust::pixel::transcode::{self, EncoderRegistry, FrameEncoder, TranscodeOptions};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
const FIELD_VALUES: [u32; 8] = [
    0,
    1,
    0x7FFF,
    0xFFFF,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFE,
    0xFFFF_FFFF,
];
fn hammer(name: &str, input: &[u8], stride: usize, f: impl Fn(&[u8])) {
    let run = |what: String, data: &[u8]| {
        if catch_unwind(AssertUnwindSafe(|| f(data))).is_err() {
            panic!("{name}: the reader panicked on the input {what}");
        }
    };
    run("as it is".into(), input);
    for cut in (0..input.len()).step_by(stride) {
        run(format!("cut to {cut} bytes"), &input[..cut]);
    }
    let mut data = input.to_vec();
    for at in (0..input.len()).step_by(stride) {
        for (what, value) in [("flipped", !input[at]), ("zeroed", 0), ("set", 0xFF)] {
            data[at] = value;
            run(format!("with byte {at} {what}"), &data);
        }
        data[at] = input[at];
        for width in [2usize, 4] {
            if at + width > input.len() {
                continue;
            }
            for value in FIELD_VALUES {
                for big in [false, true] {
                    let bytes = if big {
                        value.to_be_bytes()
                    } else {
                        value.to_le_bytes()
                    };
                    let field = if big {
                        &bytes[4 - width..]
                    } else {
                        &bytes[..width]
                    };
                    data[at..at + width].copy_from_slice(field);
                    run(
                        format!("with {width} bytes at {at} set to {value:#x} (big endian: {big})"),
                        &data,
                    );
                }
            }
            data[at..at + width].copy_from_slice(&input[at..at + width]);
        }
    }
}
fn element(group: u16, elem: u16, vr: &str, value: &[u8]) -> Vec<u8> {
    let mut data = value.to_vec();
    if data.len() % 2 == 1 {
        data.push(if vr == "UI" { 0 } else { b' ' });
    }
    let mut out = Vec::new();
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(vr.as_bytes());
    if matches!(vr, "OB" | "OW" | "SQ" | "UN" | "UT") {
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    } else {
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    }
    out.extend_from_slice(&data);
    out
}
fn implicit_element(group: u16, elem: u16, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value);
    out
}
fn item(body: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFE, 0xFF, 0x00, 0xE0];
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}
fn undefined_sequence(group: u16, elem: u16, items: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(b"SQ\0\0");
    out.extend_from_slice(&[0xFF; 4]);
    for body in items {
        out.extend_from_slice(&[0xFE, 0xFF, 0x00, 0xE0, 0xFF, 0xFF, 0xFF, 0xFF]);
        out.extend_from_slice(body);
        out.extend_from_slice(&[0xFE, 0xFF, 0x0D, 0xE0, 0, 0, 0, 0]);
    }
    out.extend_from_slice(&[0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0]);
    out
}
fn encapsulated(fragments: &[&[u8]]) -> Vec<u8> {
    let mut out = vec![
        0xE0, 0x7F, 0x10, 0x00, b'O', b'B', 0, 0, 0xFF, 0xFF, 0xFF, 0xFF,
    ];
    out.extend(item(&[]));
    for f in fragments {
        let mut f = f.to_vec();
        if f.len() % 2 == 1 {
            f.push(0);
        }
        out.extend(item(&f));
    }
    out.extend_from_slice(&[0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0]);
    out
}
fn part10(uid: &str, body: &[u8]) -> Vec<u8> {
    let meta = [
        element(0x0002, 0x0001, "OB", &[0, 1]),
        element(0x0002, 0x0002, "UI", b"1.2.840.10008.5.1.4.1.1.7"),
        element(0x0002, 0x0003, "UI", b"1.2.3.4"),
        element(0x0002, 0x0010, "UI", uid.as_bytes()),
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
fn explicit_body() -> Vec<u8> {
    let inner = element(0x0008, 0x0100, "SH", b"T-1");
    let nested = [
        element(0x0008, 0x1150, "UI", b"1.2.840.10008.5.1.4.1.1.2"),
        undefined_sequence(0x0040, 0xA043, &[&inner]),
    ]
    .concat();
    [
        element(0x0008, 0x0005, "CS", b"ISO_IR 100"),
        element(0x0008, 0x0018, "UI", b"1.2.3.4"),
        element(0x0008, 0x0020, "DA", b"20200102"),
        element(0x0008, 0x1140, "SQ", &item(&nested)),
        undefined_sequence(0x0008, 0x2112, &[&inner, &[]]),
        element(0x0009, 0x0010, "LO", b"CREATOR"),
        element(0x0009, 0x1001, "UN", &[1, 2, 3, 4]),
        element(0x0010, 0x0010, "PN", b"Doe^Jane=\xC4\xD6"),
        element(0x0020, 0x0032, "DS", b"0\\0\\-12.5"),
        element(0x0028, 0x0002, "US", &1u16.to_le_bytes()),
        element(0x0028, 0x0004, "CS", b"MONOCHROME2"),
        element(0x0028, 0x0010, "US", &2u16.to_le_bytes()),
        element(0x0028, 0x0011, "US", &2u16.to_le_bytes()),
        element(0x0028, 0x0100, "US", &8u16.to_le_bytes()),
        element(0x0028, 0x0101, "US", &8u16.to_le_bytes()),
        element(0x0028, 0x0102, "US", &7u16.to_le_bytes()),
        element(0x0028, 0x0103, "US", &0u16.to_le_bytes()),
    ]
    .concat()
}
fn implicit_body() -> Vec<u8> {
    let inner = implicit_element(0x0008, 0x0100, b"T-1 ");
    let mut defined = vec![0x08, 0x00, 0x40, 0x11];
    let it = item(&inner);
    defined.extend_from_slice(&(it.len() as u32).to_le_bytes());
    defined.extend(it);
    let mut undefined = vec![0x08, 0x00, 0x12, 0x21, 0xFF, 0xFF, 0xFF, 0xFF];
    undefined.extend_from_slice(&[0xFE, 0xFF, 0x00, 0xE0, 0xFF, 0xFF, 0xFF, 0xFF]);
    undefined.extend(inner);
    undefined.extend_from_slice(&[0xFE, 0xFF, 0x0D, 0xE0, 0, 0, 0, 0]);
    undefined.extend_from_slice(&[0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0]);
    [
        implicit_element(0x0008, 0x0060, b"OT"),
        defined,
        undefined,
        implicit_element(0x0009, 0x0010, b"CREATOR "),
        implicit_element(0x0009, 0x1001, &[1, 2, 3, 4]),
        implicit_element(0x0010, 0x0010, b"Doe^Jane"),
        implicit_element(0x0028, 0x0010, &2u16.to_le_bytes()),
        implicit_element(0x7FE0, 0x0010, &[0u8; 4]),
    ]
    .concat()
}
fn big_endian_body() -> Vec<u8> {
    let be = |group: u16, elem: u16, vr: &str, value: &[u8]| {
        let mut out = Vec::new();
        out.extend_from_slice(&group.to_be_bytes());
        out.extend_from_slice(&elem.to_be_bytes());
        out.extend_from_slice(vr.as_bytes());
        if matches!(vr, "OB" | "OW" | "SQ" | "UN") {
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(&(value.len() as u32).to_be_bytes());
        } else {
            out.extend_from_slice(&(value.len() as u16).to_be_bytes());
        }
        out.extend_from_slice(value);
        out
    };
    let inner = be(0x0008, 0x0100, "SH", b"T-1 ");
    let mut sq_item = vec![0xFF, 0xFE, 0xE0, 0x00];
    sq_item.extend_from_slice(&(inner.len() as u32).to_be_bytes());
    sq_item.extend(inner);
    [
        be(0x0008, 0x0060, "CS", b"OT"),
        be(0x0008, 0x1140, "SQ", &sq_item),
        be(0x0028, 0x0010, "US", &2u16.to_be_bytes()),
        be(0x0028, 0x0100, "US", &16u16.to_be_bytes()),
        be(0x7FE0, 0x0010, "OW", &[0u8; 8]),
    ]
    .concat()
}
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let n = data.len() as u16;
    let mut out = vec![1];
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&(!n).to_le_bytes());
    out.extend_from_slice(data);
    out
}
fn with_pixels(mut body: Vec<u8>) -> Vec<u8> {
    body.extend(element(0x7FE0, 0x0010, "OW", &[7u8; 4]));
    body
}
const LIMITS: Limits = Limits {
    max_element_bytes: 1 << 20,
    max_pixel_data_bytes: 1 << 20,
    max_data_set_bytes: 0,
    max_inflated_bytes: 1 << 20,
    max_sequence_depth: 16,
    max_elements: 10_000,
    max_items: 1_000,
    max_fragments: 1_000,
    max_rows: 256,
    max_columns: 256,
    max_frames: 8,
    max_frame_bytes: 1 << 20,
    max_pdu_length: 1 << 16,
    max_association_pdu_length: 1 << 16,
    max_associations: 1,
};
fn options(variant: u8) -> ReadFileOptions {
    let mut o = ReadFileOptions {
        limits: LIMITS,
        ..ReadFileOptions::default()
    };
    match variant {
        0 => {}
        1 => {
            o.skip_pixel_data = true;
            o.allow_partial_read = true;
        }
        2 => {
            o.allow_missing_meta_element_group_length = true;
            o.allow_implicit_vr_file_meta = true;
            o.accept_odd_length = true;
            o.recover_container_overruns = true;
            o.recover_implicit_vr_elements = true;
            o.recover_delimiter_anomalies = true;
            o.allow_zlib_deflate = true;
        }
        _ => o.allow_incomplete_pixel_data = true,
    }
    o
}
fn touch(obj: &object::Object, depth: usize) {
    for e in obj.elements() {
        let tag = e.header.tag;
        let _ = obj.get_strings(tag);
        let _ = obj.get_ints(tag);
        let _ = obj.get_floats(tag);
        let _ = obj.get_person_names(tag);
        if depth < 8
            && let Some(items) = obj.get_sequence(tag)
        {
            for item in &items {
                touch(item, depth + 1);
            }
        }
    }
    let _ = obj.character_set();
    let _ = pixeldata::extract_metadata(obj);
    let _ = pixeldata::extract_view(obj);
}
fn read_file(data: &[u8]) {
    for variant in 0..4 {
        match object::read_file_with_options(data, options(variant)) {
            Ok(file) => {
                touch(&file.meta, 0);
                touch(&file.dataset, 0);
            }
            Err((Some(file), _)) => touch(&file.dataset, 0),
            Err((None, _)) => {}
        }
    }
}
fn read_data_set(data: &[u8], syntax: Syntax) {
    for variant in 0..3 {
        let source = Box::new(object::parser::source::MemFile::new(Rc::from(data)));
        if let Ok(obj) = object::read_data_set_with_options(source, syntax, options(variant)) {
            touch(&obj, 0);
        }
    }
}
fn syntax(uid: &str) -> Syntax {
    object::transfer::lookup(uid).expect("a registered transfer syntax")
}
#[test]
fn file_reader_explicit_little_endian() {
    let file = part10("1.2.840.10008.1.2.1", &with_pixels(explicit_body()));
    hammer("file, Explicit VR Little Endian", &file, 1, read_file);
}
#[test]
fn file_reader_implicit_little_endian() {
    let file = part10("1.2.840.10008.1.2", &implicit_body());
    hammer("file, Implicit VR Little Endian", &file, 1, read_file);
}
#[test]
fn file_reader_explicit_big_endian() {
    let file = part10("1.2.840.10008.1.2.2", &big_endian_body());
    hammer("file, Explicit VR Big Endian", &file, 1, read_file);
}
#[test]
fn file_reader_deflated() {
    let file = part10(
        "1.2.840.10008.1.2.1.99",
        &deflate_stored(&with_pixels(explicit_body())),
    );
    hammer(
        "file, Deflated Explicit VR Little Endian",
        &file,
        1,
        read_file,
    );
}
#[test]
fn file_reader_encapsulated() {
    let mut body = explicit_body();
    body.extend(encapsulated(&[&[0xFF, 0x4F, 0xFF, 0x51], &[1, 2, 3, 4]]));
    let file = part10("1.2.840.10008.1.2.4.90", &body);
    hammer("file, encapsulated", &file, 1, read_file);
}
#[test]
fn data_set_reader_in_every_encoding() {
    let mut encapsulated_body = explicit_body();
    encapsulated_body.extend(encapsulated(&[&[1, 2, 3, 4]]));
    let cases: [(&str, Syntax, Vec<u8>); 5] = [
        (
            "data set, Implicit VR Little Endian",
            object::transfer::IMPLICIT_VR_LITTLE_ENDIAN,
            implicit_body(),
        ),
        (
            "data set, Explicit VR Little Endian",
            object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
            with_pixels(explicit_body()),
        ),
        (
            "data set, Explicit VR Big Endian",
            syntax("1.2.840.10008.1.2.2"),
            big_endian_body(),
        ),
        (
            "data set, Deflated",
            syntax("1.2.840.10008.1.2.1.99"),
            deflate_stored(&explicit_body()),
        ),
        (
            "data set, encapsulated",
            syntax("1.2.840.10008.1.2.4.90"),
            encapsulated_body,
        ),
    ];
    for (name, syntax, body) in cases {
        hammer(name, &body, 1, |data| read_data_set(data, syntax));
    }
}
fn pdu(pdu_type: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![pdu_type, 0];
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}
fn ul_item(item_type: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![item_type, 0];
    out.extend_from_slice(&(body.len() as u16).to_be_bytes());
    out.extend_from_slice(body);
    out
}
fn associate(pdu_type: u8) -> Vec<u8> {
    let mut body = vec![0, 1, 0, 0];
    body.extend_from_slice(b"CALLED          ");
    body.extend_from_slice(b"CALLING         ");
    body.extend_from_slice(&[0u8; 32]);
    body.extend(ul_item(0x10, b"1.2.840.10008.3.1.1.1"));
    let mut pc = vec![1, 0, 0, 0];
    if pdu_type == 1 {
        pc.extend(ul_item(0x30, b"1.2.840.10008.1.1"));
        pc.extend(ul_item(0x40, b"1.2.840.10008.1.2"));
        pc.extend(ul_item(0x40, b"1.2.840.10008.1.2.1"));
        body.extend(ul_item(0x20, &pc));
    } else {
        pc.extend(ul_item(0x40, b"1.2.840.10008.1.2"));
        body.extend(ul_item(0x21, &pc));
    }
    let mut user = ul_item(0x51, &16384u32.to_be_bytes());
    user.extend(ul_item(0x52, b"1.2.3.4.5"));
    user.extend(ul_item(0x53, &[0, 1, 0, 1]));
    let uid = b"1.2.840.10008.1.1";
    let mut role = (uid.len() as u16).to_be_bytes().to_vec();
    role.extend_from_slice(uid);
    role.extend_from_slice(&[1, 0]);
    user.extend(ul_item(0x54, &role));
    user.extend(ul_item(0x55, b"IMPL_1"));
    if pdu_type == 1 {
        let mut identity = vec![1, 1, 0, 4];
        identity.extend_from_slice(b"user");
        identity.extend_from_slice(&[0, 0]);
        user.extend(ul_item(0x58, &identity));
    }
    body.extend(ul_item(0x50, &user));
    pdu(pdu_type, &body)
}
fn command_set() -> Vec<u8> {
    let body = [
        implicit_element(0x0000, 0x0002, b"1.2.840.10008.1.1\0"),
        implicit_element(0x0000, 0x0100, &0x0030u16.to_le_bytes()),
        implicit_element(0x0000, 0x0110, &1u16.to_le_bytes()),
        implicit_element(0x0000, 0x0800, &0x0101u16.to_le_bytes()),
    ]
    .concat();
    let mut out = implicit_element(0x0000, 0x0000, &(body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}
fn read_pdus(data: &[u8]) {
    let mut stream: &[u8] = data;
    for _ in 0..16 {
        match ul::read_pdu_with_association_limit(
            &mut stream,
            LIMITS.max_pdu_length,
            LIMITS.max_association_pdu_length,
        ) {
            Ok(pdu) => {
                let _ = ul::encode_pdu(&pdu);
            }
            Err(_) => break,
        }
    }
    if let Some((&pdu_type, body)) = data.split_first() {
        let _ = ul::parse_pdu_body(pdu_type, body);
    }
}
#[test]
fn upper_layer_pdus() {
    let command = command_set();
    let mut pdv = ((command.len() + 2) as u32).to_be_bytes().to_vec();
    pdv.extend_from_slice(&[1, 3]);
    pdv.extend_from_slice(&command);
    let all = [
        ("A-ASSOCIATE-RQ", associate(1)),
        ("A-ASSOCIATE-AC", associate(2)),
        ("A-ASSOCIATE-RJ", pdu(3, &[0, 1, 1, 1])),
        ("P-DATA-TF", pdu(4, &pdv)),
        ("A-RELEASE-RQ", pdu(5, &[0; 4])),
        ("A-RELEASE-RP", pdu(6, &[0; 4])),
        ("A-ABORT", pdu(7, &[0, 0, 2, 0])),
    ];
    for (name, bytes) in &all {
        hammer(name, bytes, 1, read_pdus);
    }
}
#[test]
fn dimse_command_set() {
    use dicom_rust::net::dimse;
    hammer("DIMSE command set", &command_set(), 1, |data| {
        if let Ok(command) = dimse::decode_command_set(data) {
            let _ = dimse::parse_c_echo_response(&command);
            let _ = dimse::parse_c_store_request(&command);
            let _ = dimse::parse_c_find_request(&command);
            let _ = dimse::parse_c_move_request(&command);
            let _ = dimse::parse_c_get_request(&command);
        }
    });
}
fn sample_object() -> object::Object {
    match object::read_file(&part10(
        "1.2.840.10008.1.2.1",
        &with_pixels(explicit_body()),
    )) {
        Ok(file) => file.dataset,
        Err((_, err)) => panic!("{err}"),
    }
}
#[test]
fn dicom_json() {
    use dicom_rust::dicomjson;
    let text = dicomjson::marshal_compact(&sample_object()).expect("marshal");
    hammer("DICOM JSON", &text, 1, |data| {
        let opts = dicomjson::UnmarshalOptions::default();
        if let Ok(obj) = dicomjson::unmarshal(data, &opts) {
            touch(&obj, 0);
        }
        let _ = dicomjson::decode_stream(data, &opts);
    });
}
#[test]
fn dicom_xml() {
    use dicom_rust::dicomxml;
    let text = dicomxml::marshal(Some(&sample_object()), &mut dicomxml::Options::default())
        .expect("marshal");
    hammer("Native DICOM Model XML", &text, 3, |data| {
        if let Ok(obj) = dicomxml::unmarshal(data, &mut dicomxml::UnmarshalOptions::default()) {
            touch(&obj, 0);
        }
    });
}
#[test]
fn dicomdir() {
    use dicom_rust::dicomdir;
    let record = |kind: &str, lower: u32, keys: Vec<u8>| {
        let mut body = element(0x0004, 0x1400, "UL", &0u32.to_le_bytes());
        body.extend(element(0x0004, 0x1410, "US", &0xFFFFu16.to_le_bytes()));
        body.extend(element(0x0004, 0x1420, "UL", &lower.to_le_bytes()));
        body.extend(element(0x0004, 0x1430, "CS", kind.as_bytes()));
        body.extend(keys);
        item(&body)
    };
    let image_keys = [
        element(0x0004, 0x1500, "CS", b"DIR\\IMG1"),
        element(0x0004, 0x1510, "UI", b"1.2.840.10008.5.1.4.1.1.7"),
        element(0x0004, 0x1511, "UI", b"1.2.3.4.5"),
        element(0x0004, 0x1512, "UI", b"1.2.840.10008.1.2.1"),
        element(0x0020, 0x0013, "IS", b"1"),
    ]
    .concat();
    let head = |first: u32| {
        [
            element(0x0004, 0x1130, "CS", b"SET"),
            element(0x0004, 0x1200, "UL", &first.to_le_bytes()),
            element(0x0004, 0x1202, "UL", &first.to_le_bytes()),
            element(0x0004, 0x1212, "US", &0u16.to_le_bytes()),
        ]
        .concat()
    };
    let first = (part10("1.2.840.10008.1.2.1", &head(0)).len() + 12) as u32;
    let patient = record("PATIENT", 0, element(0x0010, 0x0020, "LO", b"ID"));
    let second = first + patient.len() as u32;
    let mut records = record("PATIENT", second, element(0x0010, 0x0020, "LO", b"ID"));
    records.extend(record("IMAGE", 0, image_keys));
    let mut body = head(first);
    body.extend(element(0x0004, 0x1220, "SQ", &records));
    let file = part10("1.2.840.10008.1.2.1", &body);
    let dir = std::env::temp_dir().join(format!("dicom-rust-untrusted-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch folder");
    let path = dir.join("DICOMDIR");
    hammer("DICOMDIR", &file, 4, |data| {
        if std::fs::write(&path, data).is_ok()
            && let Some(path) = path.to_str()
        {
            let _ = dicomdir::parse_file_set(path);
            let _ = dicomdir::referenced_paths(path);
        }
        if let Ok(file) = object::read_file_with_options(data, options(0)) {
            for reference in dicomdir::references(&file.dataset) {
                let _ = dicomdir::resolve(".", &reference);
            }
        }
    });
    let _ = std::fs::remove_dir_all(&dir);
}
fn image_file(
    uid: &str,
    samples: u16,
    bits: u16,
    stored: u16,
    photometric: &str,
    pixel_data: Vec<u8>,
) -> Vec<u8> {
    let us = |v: u16| v.to_le_bytes();
    let mut body = [
        element(0x0028, 0x0002, "US", &us(samples)),
        element(0x0028, 0x0004, "CS", photometric.as_bytes()),
    ]
    .concat();
    if samples > 1 {
        body.extend(element(0x0028, 0x0006, "US", &us(0)));
    }
    body.extend(
        [
            element(0x0028, 0x0008, "IS", b"1"),
            element(0x0028, 0x0010, "US", &us(8)),
            element(0x0028, 0x0011, "US", &us(8)),
            element(0x0028, 0x0100, "US", &us(bits)),
            element(0x0028, 0x0101, "US", &us(stored)),
            element(0x0028, 0x0102, "US", &us(stored - 1)),
            element(0x0028, 0x0103, "US", &us(0)),
        ]
        .concat(),
    );
    body.extend(pixel_data);
    part10(uid, &body)
}
fn native_image(samples: u16, bits: u16, stored: u16, photometric: &str) -> object::File {
    let count = 64 * usize::from(samples) * usize::from(bits / 8);
    let pixels: Vec<u8> = (0..count).map(|i| (i * 7 % 13) as u8).collect();
    let bytes = image_file(
        "1.2.840.10008.1.2.1",
        samples,
        bits,
        stored,
        photometric,
        element(0x7FE0, 0x0010, "OW", &pixels),
    );
    match object::read_file(&bytes) {
        Ok(file) => file,
        Err((_, err)) => panic!("{err}"),
    }
}
fn decode(uid: &str, file_bytes: &[u8]) {
    let Ok(file) = object::read_file_with_options(file_bytes, options(0)) else {
        return;
    };
    let Ok(pixel) = pixeldata::extract_view(&file.dataset) else {
        return;
    };
    for recovery in [
        CodecRecovery::default(),
        CodecRecovery {
            ignore_inconsistent_basic_offset_table: true,
            jpeg_lossless_encoder_quirks: true,
            jpeg_lossless_predictor6_overflow: true,
            rle_segment_overrun: true,
            rle_reversed_byte_segments: true,
            jpeg_sequential_scan_quirks: true,
            jpegls_jai_thresholds: true,
            jpegls_hp_color_transforms: true,
            ..CodecRecovery::default()
        },
    ] {
        let _ =
            pixeldata::decode_frames_with_limits(uid, &pixel, &file.dataset, &recovery, &LIMITS);
    }
}
fn encoders() -> EncoderRegistry {
    use dicom_rust::codec::{jpeg, jpeg2000, jpeglossless, jpegls, jpegxl};
    let mut registry = EncoderRegistry::new();
    let mut add = |encoder: Option<FrameEncoder>| {
        if let Some(encoder) = encoder {
            let _ = registry.register_own(encoder);
        }
    };
    add(Some(FrameEncoder::Rle));
    add(jpeg::Encoder::with_options(jpeg::EncoderOptions {
        quality: 90,
        ..Default::default()
    })
    .ok()
    .map(FrameEncoder::JpegBaseline));
    add(
        jpeg::ExtendedEncoder::with_options(jpeg::ExtendedEncoderOptions {
            quality: 90,
            ..Default::default()
        })
        .ok()
        .map(FrameEncoder::JpegExtended),
    );
    add(Some(FrameEncoder::JpegLossless(
        jpeglossless::Encoder::sv1(),
    )));
    add(jpeglossless::Encoder::process14(1)
        .ok()
        .map(FrameEncoder::JpegLossless));
    add(Some(FrameEncoder::JpegLs(jpegls::Encoder::new())));
    add(jpegls::Encoder::with_options(jpegls::EncoderOptions {
        near: 2,
        allow_lossy: true,
        interleave: 0,
    })
    .ok()
    .map(FrameEncoder::JpegLs));
    for (uid, step) in [
        ("1.2.840.10008.1.2.4.90", 0.0),
        ("1.2.840.10008.1.2.4.91", 0.5),
        ("1.2.840.10008.1.2.4.201", 0.0),
        ("1.2.840.10008.1.2.4.202", 0.0),
        ("1.2.840.10008.1.2.4.203", 0.5),
    ] {
        add(
            jpeg2000::encoder::Encoder::new(&jpeg2000::encoder::Options {
                transfer_syntax_uid: uid.to_string(),
                color_transform: false,
                levels: 1,
                step_size: step,
                allow_lossy: step > 0.0,
            })
            .ok()
            .map(FrameEncoder::Jpeg2000),
        );
    }
    add(Some(FrameEncoder::JpegXlLossless));
    add(jpegxl::dicom::LossyEncoder::new(90)
        .ok()
        .map(FrameEncoder::JpegXlLossy));
    registry
}
#[test]
fn every_codec_decoder() {
    let uids = [
        "1.2.840.10008.1.2.5",
        "1.2.840.10008.1.2.4.50",
        "1.2.840.10008.1.2.4.51",
        "1.2.840.10008.1.2.4.57",
        "1.2.840.10008.1.2.4.70",
        "1.2.840.10008.1.2.4.80",
        "1.2.840.10008.1.2.4.81",
        "1.2.840.10008.1.2.4.90",
        "1.2.840.10008.1.2.4.91",
        "1.2.840.10008.1.2.4.201",
        "1.2.840.10008.1.2.4.202",
        "1.2.840.10008.1.2.4.203",
        "1.2.840.10008.1.2.4.110",
        "1.2.840.10008.1.2.4.112",
    ];
    let sources = [
        native_image(1, 8, 8, "MONOCHROME2"),
        native_image(1, 16, 12, "MONOCHROME2"),
        native_image(3, 8, 8, "RGB"),
    ];
    let mut reached = Vec::new();
    for uid in uids {
        for source in &sources {
            let options = TranscodeOptions {
                allow_lossy: true,
                encoders: Some(encoders()),
                ..TranscodeOptions::default()
            };
            let Ok((encoded, _)) = transcode::transcode_file(source, uid, &options) else {
                continue;
            };
            let Ok(m) = pixeldata::extract_metadata(&encoded.dataset) else {
                continue;
            };
            let Ok(PixelData::Encapsulated { fragments, .. }) =
                pixeldata::extract_view(&encoded.dataset)
            else {
                continue;
            };
            let stream = fragments.concat();
            let wrap = |stream: &[u8]| {
                image_file(
                    uid,
                    m.samples_per_pixel,
                    m.bits_allocated,
                    m.bits_stored,
                    &m.photometric_interpretation,
                    encapsulated(&[stream]),
                )
            };
            let whole = match object::read_file(&wrap(&stream)) {
                Ok(file) => file,
                Err((_, err)) => panic!("{uid}: {err}"),
            };
            let view = pixeldata::extract_view(&whole.dataset).expect("pixel data");
            assert!(
                pixeldata::decode_frames_with_limits(
                    uid,
                    &view,
                    &whole.dataset,
                    &CodecRecovery::default(),
                    &LIMITS
                )
                .is_ok(),
                "{uid}: the encoded image does not decode"
            );
            let stride = stream.len().div_ceil(160).max(1);
            hammer(&format!("codec {uid}"), &stream, stride, |damaged| {
                decode(uid, &wrap(damaged));
            });
            if !reached.contains(&uid) {
                reached.push(uid);
            }
        }
    }
    for uid in [
        "1.2.840.10008.1.2.5",
        "1.2.840.10008.1.2.4.50",
        "1.2.840.10008.1.2.4.70",
        "1.2.840.10008.1.2.4.80",
        "1.2.840.10008.1.2.4.90",
    ] {
        assert!(reached.contains(&uid), "no encoder reached for {uid}");
    }
}
#[test]
fn native_pixel_data() {
    for (samples, bits, stored, photometric) in [
        (1u16, 8u16, 8u16, "MONOCHROME2"),
        (1, 16, 12, "MONOCHROME2"),
        (3, 8, 8, "RGB"),
        (1, 1, 1, "MONOCHROME2"),
    ] {
        let count = (64 * usize::from(samples) * usize::from(bits)).div_ceil(8);
        let pixels = vec![0x5Au8; count + count % 2];
        for uid in ["1.2.840.10008.1.2.1", "1.2.840.10008.1.2.2"] {
            let file = image_file(
                "1.2.840.10008.1.2.1",
                samples,
                bits,
                stored,
                photometric,
                element(0x7FE0, 0x0010, "OW", &pixels),
            );
            hammer(
                &format!("native pixel data, {photometric} {bits} bits"),
                &file[300..],
                1,
                |tail| {
                    let mut damaged = file[..300].to_vec();
                    damaged.extend_from_slice(tail);
                    decode(uid, &damaged);
                },
            );
        }
    }
}
#[test]
fn xml_declaration_with_a_non_utf8_byte_after_a_pseudo_attribute() {
    use dicom_rust::dicomxml;
    const MODEL: &[u8] =
        b"<NativeDicomModel xmlns=\"http://dicom.nema.org/PS3.19/models/NativeDICOM\"></NativeDicomModel>";
    for head in [
        &b"<?xml version=\"1.0\" encoding=\xFF\xFF\xFF\xFF-8\"?>"[..],
        b"<?xml version=\xC3\xA9\"1.0\" encoding=\"UTF-8\"?>",
        b"<?xml version=\xE2\x82\xAC encoding=\xE2\x82\xAC'x'?>",
    ] {
        let xml = [head, MODEL].concat();
        let _ = dicomxml::unmarshal(&xml, &mut dicomxml::UnmarshalOptions::default());
    }
}
