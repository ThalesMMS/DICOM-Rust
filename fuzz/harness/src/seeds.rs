use crate::{
    HTJ2K_UIDS, IMAGE_HEADER, JPEG_LOSSLESS_UIDS, JPEG_UIDS, JPEG2000_UIDS, JPEGLS_UIDS,
    JPEGXL_UIDS, PHOTOMETRICS, RLE_UIDS,
};
use dicom_rust::object::{self, ReadFileOptions};
use dicom_rust::pixel::pixeldata::{self, PixelData};
use dicom_rust::pixel::transcode::{self, EncoderRegistry, FrameEncoder, TranscodeOptions};
pub fn element(group: u16, elem: u16, vr: &str, value: &[u8]) -> Vec<u8> {
    let mut data = value.to_vec();
    if data.len() % 2 == 1 {
        data.push(if matches!(vr, "UI" | "OB" | "UN") {
            0
        } else {
            b' '
        });
    }
    let mut out = Vec::with_capacity(data.len() + 12);
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(vr.as_bytes());
    if matches!(
        vr,
        "OB" | "OW" | "OF" | "OD" | "OL" | "SQ" | "UN" | "UT" | "UC" | "UR"
    ) {
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    } else {
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    }
    out.extend_from_slice(&data);
    out
}
pub fn implicit_element(group: u16, elem: u16, value: &[u8]) -> Vec<u8> {
    let mut data = value.to_vec();
    if data.len() % 2 == 1 {
        data.push(b' ');
    }
    let mut out = Vec::with_capacity(data.len() + 8);
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}
fn item(body: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFE, 0xFF, 0x00, 0xE0];
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}
fn undefined_sequence(group: u16, elem: u16, items: &[Vec<u8>]) -> Vec<u8> {
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
pub fn encapsulated_pixel_data(fragments: &[&[u8]]) -> Vec<u8> {
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
pub fn part10(uid: &str, body: &[u8]) -> Vec<u8> {
    part10_of("1.2.840.10008.5.1.4.1.1.7", uid, body)
}
fn part10_of(sop_class: &str, uid: &str, body: &[u8]) -> Vec<u8> {
    let meta = [
        element(0x0002, 0x0001, "OB", &[0, 1]),
        element(0x0002, 0x0002, "UI", sop_class.as_bytes()),
        element(0x0002, 0x0003, "UI", b"2.25.90000000000001"),
        element(0x0002, 0x0010, "UI", uid.as_bytes()),
        element(0x0002, 0x0012, "UI", b"2.25.90000000000002"),
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
    let inner = [
        element(0x0008, 0x0100, "SH", b"T-1"),
        element(0x0008, 0x0104, "LO", b"Meaning"),
    ]
    .concat();
    let nested = [
        element(0x0008, 0x1150, "UI", b"1.2.840.10008.5.1.4.1.1.2"),
        undefined_sequence(0x0040, 0xA043, std::slice::from_ref(&inner)),
    ]
    .concat();
    [
        element(0x0008, 0x0005, "CS", b"ISO_IR 100"),
        element(0x0008, 0x0016, "UI", b"1.2.840.10008.5.1.4.1.1.7"),
        element(0x0008, 0x0018, "UI", b"2.25.90000000000001"),
        element(0x0008, 0x0020, "DA", b"20200102"),
        element(0x0008, 0x0030, "TM", b"101112.5"),
        element(0x0008, 0x0060, "CS", b"OT"),
        {
            let mut sq = element(0x0008, 0x1140, "SQ", &item(&nested));
            sq.extend(undefined_sequence(0x0008, 0x2112, &[inner, Vec::new()]));
            sq
        },
        element(0x0009, 0x0010, "LO", b"FUZZ CREATOR"),
        element(0x0009, 0x1001, "UN", &[1, 2, 3, 4]),
        element(0x0010, 0x0010, "PN", b"Doe^Jane=\xC4\xD6^X"),
        element(0x0010, 0x0020, "LO", b"ID-1"),
        element(0x0018, 0x0050, "DS", b"1.5\\2"),
        element(0x0020, 0x0013, "IS", b"7"),
        element(0x0020, 0x0032, "DS", b"0\\0\\-12.5"),
        element(0x0028, 0x0002, "US", &1u16.to_le_bytes()),
        element(0x0028, 0x0004, "CS", b"MONOCHROME2"),
        element(0x0028, 0x0010, "US", &4u16.to_le_bytes()),
        element(0x0028, 0x0011, "US", &4u16.to_le_bytes()),
        element(0x0028, 0x0100, "US", &8u16.to_le_bytes()),
        element(0x0028, 0x0101, "US", &8u16.to_le_bytes()),
        element(0x0028, 0x0102, "US", &7u16.to_le_bytes()),
        element(0x0028, 0x0103, "US", &0u16.to_le_bytes()),
        element(0x0028, 0x1052, "DS", b"-1024"),
        element(0x0040, 0xA30A, "DS", b"1e3"),
    ]
    .concat()
}
fn implicit_body() -> Vec<u8> {
    let inner = implicit_element(0x0008, 0x0100, b"T-1 ");
    let mut sq = vec![0x08, 0x00, 0x40, 0x11];
    let it = item(&inner);
    sq.extend_from_slice(&(it.len() as u32).to_le_bytes());
    sq.extend(it);
    let mut undefined = vec![0x08, 0x00, 0x12, 0x21, 0xFF, 0xFF, 0xFF, 0xFF];
    undefined.extend_from_slice(&[0xFE, 0xFF, 0x00, 0xE0, 0xFF, 0xFF, 0xFF, 0xFF]);
    undefined.extend(inner);
    undefined.extend_from_slice(&[0xFE, 0xFF, 0x0D, 0xE0, 0, 0, 0, 0]);
    undefined.extend_from_slice(&[0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0]);
    [
        implicit_element(0x0008, 0x0005, b"ISO_IR 100"),
        implicit_element(0x0008, 0x0016, b"1.2.840.10008.5.1.4.1.1.7\0"),
        implicit_element(0x0008, 0x0060, b"OT"),
        sq,
        undefined,
        implicit_element(0x0009, 0x0010, b"FUZZ CREATOR"),
        implicit_element(0x0009, 0x1001, &[1, 2, 3, 4]),
        implicit_element(0x0010, 0x0010, b"Doe^Jane"),
        implicit_element(0x0028, 0x0010, &4u16.to_le_bytes()),
        implicit_element(0x0028, 0x0011, &4u16.to_le_bytes()),
        implicit_element(0x7FE0, 0x0010, &[0u8; 16]),
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
    let mut sq_item = vec![0xFF, 0xFE, 0xE0, 0x00];
    let inner = be(0x0008, 0x0100, "SH", b"T-1 ");
    sq_item.extend_from_slice(&(inner.len() as u32).to_be_bytes());
    sq_item.extend(inner);
    [
        be(0x0008, 0x0060, "CS", b"OT"),
        be(0x0008, 0x1140, "SQ", &sq_item),
        be(0x0010, 0x0010, "PN", b"Doe^Jane"),
        be(0x0028, 0x0010, "US", &4u16.to_be_bytes()),
        be(0x0028, 0x0011, "US", &4u16.to_be_bytes()),
        be(0x0028, 0x0100, "US", &16u16.to_be_bytes()),
        be(0x7FE0, 0x0010, "OW", &[0u8; 32]),
    ]
    .concat()
}
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunks = data.chunks(0xFFFF).peekable();
    if chunks.peek().is_none() {
        return vec![1, 0, 0, 0xFF, 0xFF];
    }
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        let n = chunk.len() as u16;
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(!n).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}
fn with_native_pixels(mut body: Vec<u8>) -> Vec<u8> {
    body.extend(element(0x7FE0, 0x0010, "OW", &[7u8; 16]));
    body
}
fn with_encapsulated_pixels(mut body: Vec<u8>) -> Vec<u8> {
    body.extend(encapsulated_pixel_data(&[
        &[0xFF, 0x4F, 0xFF, 0x51],
        &[1, 2, 3, 4],
    ]));
    body
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
    if pdu_type == 1 {
        let mut pc = vec![1, 0, 0, 0];
        pc.extend(ul_item(0x30, b"1.2.840.10008.1.1"));
        pc.extend(ul_item(0x40, b"1.2.840.10008.1.2"));
        pc.extend(ul_item(0x40, b"1.2.840.10008.1.2.1"));
        body.extend(ul_item(0x20, &pc));
    } else {
        let mut pc = vec![1, 0, 0, 0];
        pc.extend(ul_item(0x40, b"1.2.840.10008.1.2"));
        body.extend(ul_item(0x21, &pc));
    }
    let mut user = ul_item(0x51, &16384u32.to_be_bytes());
    user.extend(ul_item(0x52, b"2.25.90000000000003"));
    user.extend(ul_item(0x53, &[0, 1, 0, 1]));
    {
        let uid = b"1.2.840.10008.1.1";
        let mut role = (uid.len() as u16).to_be_bytes().to_vec();
        role.extend_from_slice(uid);
        role.extend_from_slice(&[1, 0]);
        user.extend(ul_item(0x54, &role));
    }
    user.extend(ul_item(0x55, b"FUZZ_1"));
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
fn ul_seeds() -> Vec<Vec<u8>> {
    let command = command_set();
    let mut pdv = ((command.len() + 2) as u32).to_be_bytes().to_vec();
    pdv.extend_from_slice(&[1, 3]);
    pdv.extend_from_slice(&command);
    let all = [
        associate(1),
        associate(2),
        pdu(3, &[0, 1, 1, 1]),
        pdu(4, &pdv),
        pdu(5, &[0; 4]),
        pdu(6, &[0; 4]),
        pdu(7, &[0, 0, 2, 0]),
    ];
    let mut out: Vec<Vec<u8>> = all.to_vec();
    out.push(all.concat());
    out
}
fn dicomdir_file() -> Vec<u8> {
    let record = |kind: &str, keys: Vec<u8>, lower: u32, file: bool| -> Vec<u8> {
        let mut body = element(0x0004, 0x1400, "UL", &0u32.to_le_bytes());
        body.extend(element(0x0004, 0x1410, "US", &0xFFFFu16.to_le_bytes()));
        body.extend(element(0x0004, 0x1420, "UL", &lower.to_le_bytes()));
        body.extend(element(0x0004, 0x1430, "CS", kind.as_bytes()));
        if file {
            body.extend(element(0x0004, 0x1500, "CS", b"DIR\\IMG0001"));
            body.extend(element(0x0004, 0x1510, "UI", b"1.2.840.10008.5.1.4.1.1.7"));
            body.extend(element(0x0004, 0x1511, "UI", b"2.25.900000000000010"));
            body.extend(element(0x0004, 0x1512, "UI", b"1.2.840.10008.1.2.1"));
        }
        body.extend(keys);
        item(&body)
    };
    let keys = [
        [
            element(0x0010, 0x0010, "PN", b"Doe^Jane"),
            element(0x0010, 0x0020, "LO", b"ID-1"),
        ]
        .concat(),
        [
            element(0x0008, 0x0020, "DA", b"20200102"),
            element(0x0008, 0x0030, "TM", b"101112"),
            element(0x0008, 0x0050, "SH", b"ACC1"),
            element(0x0020, 0x000D, "UI", b"2.25.900000000000011"),
            element(0x0020, 0x0010, "SH", b"1"),
        ]
        .concat(),
        [
            element(0x0008, 0x0060, "CS", b"OT"),
            element(0x0020, 0x000E, "UI", b"2.25.900000000000012"),
            element(0x0020, 0x0011, "IS", b"1"),
        ]
        .concat(),
        element(0x0020, 0x0013, "IS", b"1"),
    ];
    let kinds = ["PATIENT", "STUDY", "SERIES", "IMAGE"];
    let sizes: Vec<usize> = kinds
        .iter()
        .zip(&keys)
        .enumerate()
        .map(|(i, (k, keys))| record(k, keys.clone(), 0, i == 3).len())
        .collect();
    let head = |first: u32, last: u32| -> Vec<u8> {
        [
            element(0x0004, 0x1130, "CS", b"FUZZSET"),
            element(0x0004, 0x1200, "UL", &first.to_le_bytes()),
            element(0x0004, 0x1202, "UL", &last.to_le_bytes()),
            element(0x0004, 0x1212, "US", &0u16.to_le_bytes()),
        ]
        .concat()
    };
    let prefix = part10_of("1.2.840.10008.1.3.10", "1.2.840.10008.1.2.1", &head(0, 0)).len() + 12;
    let mut offsets = Vec::new();
    let mut at = prefix;
    for size in &sizes {
        offsets.push(at as u32);
        at += size;
    }
    let mut records = Vec::new();
    for (i, (kind, keys)) in kinds.iter().zip(&keys).enumerate() {
        let lower = offsets.get(i + 1).copied().unwrap_or(0);
        records.extend(record(kind, keys.clone(), lower, i == 3));
    }
    let mut body = head(offsets[0], offsets[0]);
    body.extend(element(0x0004, 0x1220, "SQ", &records));
    part10_of("1.2.840.10008.1.3.10", "1.2.840.10008.1.2.1", &body)
}
#[allow(clippy::too_many_arguments)]
pub fn image_header(
    syntax: u8,
    rows: u16,
    columns: u16,
    samples: u16,
    bits_allocated: u16,
    bits_stored: u16,
    signed: bool,
    planar: u16,
    photometric: &str,
) -> Option<[u8; IMAGE_HEADER]> {
    let samples = match samples {
        1 => 0,
        3 => 1,
        4 => 3,
        _ => return None,
    };
    let bits = match bits_allocated {
        8 => 0,
        16 => 1,
        1 => 2,
        32 => 3,
        _ => return None,
    };
    let photometric = PHOTOMETRICS.iter().position(|p| *p == photometric)? as u8;
    if !(1..=256).contains(&rows) || !(1..=256).contains(&columns) || bits_stored == 0 {
        return None;
    }
    Some([
        syntax,
        (rows - 1) as u8,
        (columns - 1) as u8,
        samples,
        bits,
        (bits_stored - 1) as u8,
        u8::from(signed) | (planar as u8 & 1) << 1,
        photometric,
        0,
        0,
    ])
}
struct Native {
    samples: u16,
    bits_allocated: u16,
    bits_stored: u16,
    photometric: &'static str,
}
const NATIVES: [Native; 3] = [
    Native {
        samples: 1,
        bits_allocated: 8,
        bits_stored: 8,
        photometric: "MONOCHROME2",
    },
    Native {
        samples: 1,
        bits_allocated: 16,
        bits_stored: 12,
        photometric: "MONOCHROME2",
    },
    Native {
        samples: 3,
        bits_allocated: 8,
        bits_stored: 8,
        photometric: "RGB",
    },
];
const SIDE: u16 = 16;
fn native_pixels(n: &Native) -> Vec<u8> {
    let count = usize::from(SIDE) * usize::from(SIDE) * usize::from(n.samples);
    let mut out = Vec::new();
    for i in 0..count {
        let v = ((i * 37 + i / 16 * 11) % (1 << n.bits_stored.min(12))) as u16;
        if n.bits_allocated == 8 {
            out.push(v as u8);
        } else {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}
fn native_file(n: &Native) -> Vec<u8> {
    let header = image_header(
        0,
        SIDE,
        SIDE,
        n.samples,
        n.bits_allocated,
        n.bits_stored,
        false,
        0,
        n.photometric,
    )
    .unwrap_or_default();
    let mut input = header.to_vec();
    input.extend(native_pixels(n));
    crate::ImageInput::parse(&input)
        .map(|i| i.file("1.2.840.10008.1.2.1", false))
        .unwrap_or_default()
}
pub fn encoders() -> EncoderRegistry {
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
fn codec_seeds(uids: &[&str]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for n in &NATIVES {
        let Ok(file) = object::read_file_with_options(&native_file(n), ReadFileOptions::default())
        else {
            continue;
        };
        for (index, uid) in uids.iter().enumerate() {
            let options = TranscodeOptions {
                allow_lossy: true,
                encoders: Some(encoders()),
                ..TranscodeOptions::default()
            };
            let Ok((encoded, _)) = transcode::transcode_file(&file, uid, &options) else {
                continue;
            };
            let ds = &encoded.dataset;
            let Ok(m) = pixeldata::extract_metadata(ds) else {
                continue;
            };
            let Ok(PixelData::Encapsulated { fragments, .. }) = pixeldata::extract_view(ds) else {
                continue;
            };
            let Some(header) = image_header(
                index as u8,
                m.rows,
                m.columns,
                m.samples_per_pixel,
                m.bits_allocated,
                m.bits_stored,
                m.pixel_representation == 1,
                m.planar_configuration,
                &m.photometric_interpretation,
            ) else {
                continue;
            };
            let mut input = header.to_vec();
            input.extend(fragments.concat());
            out.push(input);
        }
    }
    out
}
fn native_seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for n in &NATIVES {
        for syntax in 0..3u8 {
            if let Some(header) = image_header(
                syntax,
                SIDE,
                SIDE,
                n.samples,
                n.bits_allocated,
                n.bits_stored,
                false,
                0,
                n.photometric,
            ) {
                let mut input = header.to_vec();
                input.extend(native_pixels(n));
                out.push(input);
            }
        }
    }
    out
}
fn json_xml_seeds(xml: bool) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let bodies = [with_native_pixels(explicit_body()), explicit_body()];
    for body in bodies {
        let Ok(file) = object::read_file_with_options(
            &part10("1.2.840.10008.1.2.1", &body),
            ReadFileOptions::default(),
        ) else {
            continue;
        };
        if xml {
            let mut options = dicom_rust::dicomxml::Options::default();
            if let Ok(text) = dicom_rust::dicomxml::marshal(Some(&file.dataset), &mut options) {
                out.push(text);
            }
        } else {
            if let Ok(text) = dicom_rust::dicomjson::marshal_compact(&file.dataset) {
                let mut array = b"[".to_vec();
                array.extend_from_slice(&text);
                array.push(b']');
                out.push(array);
                out.push(text);
            }
            if let Ok(text) = dicom_rust::dicomjson::marshal_pretty(&file.dataset) {
                out.push(text);
            }
        }
    }
    if xml {
        out.push(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><NativeDicomModel xmlns=\"http://dicom.nema.org/PS3.19/models/NativeDICOM\"><DicomAttribute tag=\"00100010\" vr=\"PN\" keyword=\"PatientName\"><PersonName number=\"1\"><Alphabetic><FamilyName>Doe</FamilyName></Alphabetic></PersonName></DicomAttribute><DicomAttribute tag=\"00081140\" vr=\"SQ\" keyword=\"ReferencedImageSequence\"><Item number=\"1\"><DicomAttribute tag=\"00081150\" vr=\"UI\" keyword=\"ReferencedSOPClassUID\"><Value number=\"1\">1.2.3</Value></DicomAttribute></Item></DicomAttribute><DicomAttribute tag=\"7FE00010\" vr=\"OB\" keyword=\"PixelData\"><InlineBinary>AAECAw==</InlineBinary></DicomAttribute></NativeDicomModel>"
                .to_vec(),
        );
    } else {
        out.push(
            br#"{"00100010":{"vr":"PN","Value":[{"Alphabetic":"Doe^Jane"}]},"00081140":{"vr":"SQ","Value":[{"00081150":{"vr":"UI","Value":["1.2.3"]}}]},"00280010":{"vr":"US","Value":[4]},"00200032":{"vr":"DS","Value":[0,"1.5",-2e1]},"7FE00010":{"vr":"OB","InlineBinary":"AAECAw=="},"00420011":{"vr":"OB","BulkDataURI":"http://example.test/x"}}"#
                .to_vec(),
        );
    }
    out
}
pub fn seeds(name: &str) -> Vec<Vec<u8>> {
    let explicit = "1.2.840.10008.1.2.1";
    match name {
        "file_reader" => vec![
            part10(explicit, &with_native_pixels(explicit_body())),
            part10("1.2.840.10008.1.2", &implicit_body()),
            part10("1.2.840.10008.1.2.2", &big_endian_body()),
            part10(
                "1.2.840.10008.1.2.1.99",
                &deflate_stored(&with_native_pixels(explicit_body())),
            ),
            part10(
                "1.2.840.10008.1.2.4.90",
                &with_encapsulated_pixels(explicit_body()),
            ),
            part10(
                "1.2.840.10008.1.2.5",
                &with_encapsulated_pixels(explicit_body()),
            ),
            with_native_pixels(explicit_body()),
            implicit_body(),
        ],
        "dataset_implicit_le" => vec![implicit_body()],
        "dataset_explicit_le" => vec![with_native_pixels(explicit_body()), explicit_body()],
        "dataset_explicit_be" => vec![big_endian_body()],
        "dataset_deflated" => vec![
            deflate_stored(&with_native_pixels(explicit_body())),
            deflate_stored(&explicit_body()),
        ],
        "dataset_encapsulated" => vec![with_encapsulated_pixels(explicit_body())],
        "ul_pdu" => ul_seeds(),
        "dimse_command" => vec![command_set()],
        "dicomdir" => vec![dicomdir_file()],
        "dicomjson" => json_xml_seeds(false),
        "dicomxml" => json_xml_seeds(true),
        "pixel_native" => native_seeds(),
        "codec_rle" => codec_seeds(RLE_UIDS),
        "codec_jpeg" => codec_seeds(JPEG_UIDS),
        "codec_jpeg_lossless" => codec_seeds(JPEG_LOSSLESS_UIDS),
        "codec_jpegls" => codec_seeds(JPEGLS_UIDS),
        "codec_jpeg2000" => codec_seeds(JPEG2000_UIDS),
        "codec_htj2k" => codec_seeds(HTJ2K_UIDS),
        "codec_jpegxl" => codec_seeds(JPEGXL_UIDS),
        "file_deferred" => seeds("file_reader"),
        "frame_reader" => frame_reader_seeds(),
        "probe" => vec![
            explicit_body(),
            with_native_pixels(explicit_body()),
            implicit_body(),
            big_endian_body(),
            with_encapsulated_pixels(explicit_body()),
        ],
        "encapsulated_layout" => encapsulated_layout_seeds(),
        "vendor_headers" => vendor_header_seeds(),
        "dicomweb_response" => dicomweb_response_seeds(),
        "transcode" => transcode_seeds(),
        _ => Vec::new(),
    }
}
fn frame_reader_seeds() -> Vec<Vec<u8>> {
    let mut out = vec![part10(
        "1.2.840.10008.1.2.1",
        &with_native_pixels(explicit_body()),
    )];
    for uids in [RLE_UIDS, JPEG_UIDS, JPEGLS_UIDS, JPEG2000_UIDS] {
        for input in codec_seeds(uids) {
            if let Some(image) = crate::ImageInput::parse(&input) {
                out.push(image.file(uids[usize::from(image.syntax) % uids.len()], true));
            }
        }
    }
    out
}
fn encapsulated_layout_seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for (format, uids) in [(0u8, JPEG_UIDS), (1, JPEGLS_UIDS), (2, JPEG2000_UIDS)] {
        for input in codec_seeds(uids) {
            let stream = &input[IMAGE_HEADER..];
            let Ok(len) = u16::try_from(stream.len()) else {
                continue;
            };
            let mut seed = vec![format, 0, 0];
            seed.extend_from_slice(&len.to_le_bytes());
            seed.extend_from_slice(stream);
            out.push(seed.clone());
            let mut two = vec![format | 1 << 2, 8, 0];
            two.extend_from_slice(&0u32.to_le_bytes());
            two.extend_from_slice(&u32::from(len).to_le_bytes());
            for _ in 0..2 {
                two.extend_from_slice(&len.to_le_bytes());
                two.extend_from_slice(stream);
            }
            out.push(two);
        }
    }
    out
}
fn vendor_header_seeds() -> Vec<Vec<u8>> {
    use dicom_rust::siemens::{CsaElement, encode_csa};
    let elements = [
        CsaElement {
            name: "SliceNormalVector".into(),
            vm: 3,
            vr: "FD".into(),
            syngo_dt: 4,
            items: 3,
            values: vec![b"0.0".to_vec(), b"0.0".to_vec(), b"1.0".to_vec()],
        },
        CsaElement {
            name: "NumberOfImagesInMosaic".into(),
            vm: 1,
            vr: "US".into(),
            syngo_dt: 10,
            items: 1,
            values: vec![b"16".to_vec()],
        },
    ];
    let mut out: Vec<Vec<u8>> = encode_csa(&elements).into_iter().collect();
    out.push(
        b"### ASCCONV BEGIN ###\nsKSpace.lBaseResolution\t = \t256\nsSliceArray.asSlice[0].dThickness = 3.0\nsSliceArray.lSize = 2\n### ASCCONV END ###\n"
            .to_vec(),
    );
    out
}
fn dicomweb_response_seeds() -> Vec<Vec<u8>> {
    let mut out = json_xml_seeds(false);
    out.push(
        b"Content-Type: application/dicom; transfer-syntax=1.2.840.10008.1.2.1\r\nContent-Length: 12\r\n\r\n"
            .to_vec(),
    );
    out.push(b"multipart/related; type=\"application/dicom\"; boundary=abc".to_vec());
    out.push(
        br#"{"00081190":{"vr":"UR","Value":["http://example.test/studies/1"]},"00081199":{"vr":"SQ","Value":[{"00081150":{"vr":"UI","Value":["1.2.3"]},"00081155":{"vr":"UI","Value":["1.2.3.4"]}}]}}"#
            .to_vec(),
    );
    out
}
fn transcode_seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut base = 0u8;
    for uids in [
        RLE_UIDS,
        JPEG_UIDS,
        JPEG_LOSSLESS_UIDS,
        JPEGLS_UIDS,
        JPEG2000_UIDS,
        HTJ2K_UIDS,
        JPEGXL_UIDS,
    ] {
        for mut seed in codec_seeds(uids) {
            seed[0] = seed[0].wrapping_add(base);
            out.push(seed);
        }
        base = base.wrapping_add(uids.len() as u8);
    }
    out
}
