#![allow(dead_code)]
use dicom_rust::object::transfer;
use dicom_rust::object::types::DataSet;
use dicom_rust::object::{Element, ElementHeader, File, Object, Tag, Value, Vr};
pub const EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
pub const IMPLICIT_LE: &str = "1.2.840.10008.1.2";
pub const EXPLICIT_BE: &str = "1.2.840.10008.1.2.2";
pub const DEFLATED: &str = "1.2.840.10008.1.2.1.99";
pub const SECONDARY_CAPTURE: &str = "1.2.840.10008.5.1.4.1.1.7";
pub const CT_IMAGE: &str = "1.2.840.10008.5.1.4.1.1.2";
fn header(tag: Tag, vr: Vr) -> ElementHeader {
    ElementHeader {
        tag,
        vr,
        length: 0,
        length_set: false,
    }
}
pub fn text(group: u16, element: u16, vr: Vr, value: &str) -> Element {
    Element {
        header: header(Tag::new(group, element), vr),
        value: Value::Strings(value.split('\\').map(str::to_string).collect()),
    }
}
pub fn raw(group: u16, element: u16, vr: Vr, bytes: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(Tag::new(group, element), vr, bytes.len() as u32),
        value: Value::Raw(bytes),
    }
}
pub fn us(group: u16, element: u16, values: &[u16]) -> Element {
    raw(
        group,
        element,
        Vr::US,
        values.iter().flat_map(|v| v.to_le_bytes()).collect(),
    )
}
pub fn ul(group: u16, element: u16, values: &[u32]) -> Element {
    raw(
        group,
        element,
        Vr::UL,
        values.iter().flat_map(|v| v.to_le_bytes()).collect(),
    )
}
pub fn ss(group: u16, element: u16, values: &[i16]) -> Element {
    raw(
        group,
        element,
        Vr::SS,
        values.iter().flat_map(|v| v.to_le_bytes()).collect(),
    )
}
pub fn fd(group: u16, element: u16, values: &[f64]) -> Element {
    raw(
        group,
        element,
        Vr::FD,
        values.iter().flat_map(|v| v.to_le_bytes()).collect(),
    )
}
pub fn fl(group: u16, element: u16, values: &[f32]) -> Element {
    raw(
        group,
        element,
        Vr::FL,
        values.iter().flat_map(|v| v.to_le_bytes()).collect(),
    )
}
pub fn sequence(group: u16, element: u16, items: Vec<Vec<Element>>) -> Element {
    Element {
        header: header(Tag::new(group, element), Vr::SQ),
        value: Value::Sequence(
            items
                .into_iter()
                .map(|elements| DataSet {
                    elements,
                    item_offset: None,
                })
                .collect(),
        ),
    }
}
pub fn rich_data_set() -> Object {
    let mut e = vec![
        text(0x0008, 0x0005, Vr::CS, "ISO_IR 100"),
        text(0x0008, 0x0016, Vr::UI, SECONDARY_CAPTURE),
        text(0x0008, 0x0018, Vr::UI, "1.2.826.0.1.3680043.10.1.77"),
        text(0x0008, 0x0020, Vr::DA, "20240131"),
        text(0x0008, 0x0030, Vr::TM, "235959.125"),
        text(0x0008, 0x0060, Vr::CS, "OT"),
        text(0x0008, 0x1030, Vr::LO, "Synthetic study"),
        text(0x0010, 0x0010, Vr::PN, "Muller^Jose"),
        text(0x0010, 0x0020, Vr::LO, "SYN-77"),
        text(0x0010, 0x1010, Vr::AS, "042Y"),
        text(0x0018, 0x0050, Vr::DS, "1.25"),
        text(0x0020, 0x0013, Vr::IS, "7"),
        text(0x0020, 0x0032, Vr::DS, "-12.5\\0\\33.75"),
        text(0x0020, 0x4000, Vr::LT, "A short comment."),
        text(0x0040, 0xA160, Vr::UT, "Unlimited text value"),
        us(0x0028, 0x0010, &[4]),
        us(0x0028, 0x0011, &[4]),
        us(0x0018, 0x1310, &[0, 4, 4, 0]),
        ss(0x0018, 0x9219, &[-3]),
        ul(0x0018, 0x106E, &[0x0102_0304]),
        fd(0x0018, 0x9087, &[0.5, -2.25]),
        fl(0x0018, 0x0013, &[1.5, 0.0, -1.0]),
        raw(0x0028, 0x0009, Vr::AT, vec![0x18, 0x00, 0x63, 0x10]),
        text(0x0009, 0x0010, Vr::LO, "SYNTHETIC CREATOR"),
        raw(0x0009, 0x1001, Vr::OB, vec![1, 2, 3, 4]),
        sequence(
            0x0008,
            0x1140,
            vec![
                vec![
                    text(0x0008, 0x1150, Vr::UI, CT_IMAGE),
                    text(0x0008, 0x1155, Vr::UI, "1.2.826.0.1.3680043.10.1.78"),
                ],
                vec![],
                vec![sequence(
                    0x0040,
                    0xA730,
                    vec![vec![text(0x0040, 0xA010, Vr::CS, "CONTAINS")]],
                )],
            ],
        ),
        raw(
            0x7FE0,
            0x0010,
            Vr::OW,
            (0u16..16).flat_map(|v| (v * 300).to_le_bytes()).collect(),
        ),
    ];
    e.sort_by_key(|e| e.tag());
    Object::from_elements(e)
}
pub fn samples(rows: usize, cols: usize, spp: usize, bits: u32) -> Vec<u16> {
    let max = (1u32 << bits) - 1;
    let mut out = Vec::with_capacity(rows * cols * spp);
    for r in 0..rows {
        for c in 0..cols {
            for s in 0..spp {
                let gradient =
                    (r * 7 + c * 5 + s * 31) as u32 * max / ((rows * 7 + cols * 5 + 62) as u32);
                let checker = if (r / 3 + c / 3) % 2 == 0 { max / 8 } else { 0 };
                out.push(((gradient + checker).min(max)) as u16);
            }
        }
    }
    out
}
pub fn sample_bytes(samples: &[u16], allocated: u32) -> Vec<u8> {
    if allocated == 8 {
        samples.iter().map(|&s| s as u8).collect()
    } else {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }
}
pub fn image(rows: usize, cols: usize, spp: usize, allocated: u32, bits: u32) -> Object {
    let photometric = if spp == 3 { "RGB" } else { "MONOCHROME2" };
    let pixels = sample_bytes(&samples(rows, cols, spp, bits), allocated);
    let mut e = vec![
        text(0x0008, 0x0016, Vr::UI, SECONDARY_CAPTURE),
        text(0x0008, 0x0018, Vr::UI, "1.2.826.0.1.3680043.10.1.2.3"),
        text(0x0008, 0x0060, Vr::CS, "OT"),
        text(0x0010, 0x0010, Vr::PN, "Test^Image"),
        text(0x0010, 0x0020, Vr::LO, "SYN-0001"),
        text(0x0020, 0x000D, Vr::UI, "1.2.826.0.1.3680043.10.1.2"),
        text(0x0020, 0x000E, Vr::UI, "1.2.826.0.1.3680043.10.1.2.1"),
        us(0x0028, 0x0002, &[spp as u16]),
        text(0x0028, 0x0004, Vr::CS, photometric),
        us(0x0028, 0x0010, &[rows as u16]),
        us(0x0028, 0x0011, &[cols as u16]),
        us(0x0028, 0x0100, &[allocated as u16]),
        us(0x0028, 0x0101, &[bits as u16]),
        us(0x0028, 0x0102, &[bits as u16 - 1]),
        us(0x0028, 0x0103, &[0]),
    ];
    if spp == 3 {
        e.push(us(0x0028, 0x0006, &[0]));
    }
    let vr = if allocated == 8 { Vr::OB } else { Vr::OW };
    e.push(raw(0x7FE0, 0x0010, vr, pixels));
    let mut obj = Object::from_elements(e);
    obj.put(text(0x0008, 0x0008, Vr::CS, "DERIVED\\SECONDARY"));
    obj
}
pub fn file(dataset: Object, uid: &str) -> File {
    let mut f = File {
        preamble: Vec::new(),
        meta: Object::new(),
        dataset,
        transfer_syntax: transfer::lookup(uid).expect("known transfer syntax"),
        partial_read: None,
        incomplete_pixel_data: None,
    };
    f.rebuild_file_meta().expect("file meta");
    f
}
pub fn write_and_read(dataset: Object, uid: &str) -> (Vec<u8>, File) {
    let bytes = dicom_rust::object::writer::write_file(&file(dataset, uid)).expect("write");
    let back = dicom_rust::object::read_file(&bytes)
        .map_err(|(_, e)| e)
        .expect("read back");
    (bytes, back)
}
pub fn pixel_bytes(f: &File) -> Vec<u8> {
    f.dataset
        .get_raw(Tag::PIXEL_DATA)
        .expect("native Pixel Data")
}
