use dicom_rust::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
use dicom_rust::object::writer::write_file;
use dicom_rust::object::{Element, ElementHeader, File, Object, Tag, Value, Vr};
use std::error::Error;
const CT_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.2";
const STUDY_UID: &str = "2.25.231413466718843014539263813404283946519";
const SERIES_UID: &str = "2.25.231413466718843014539263813404283946520";
const INSTANCE_UID: &str = "2.25.231413466718843014539263813404283946521";
const ROWS: u16 = 64;
const COLUMNS: u16 = 64;
fn phantom() -> Vec<u8> {
    let mut out = Vec::with_capacity(usize::from(ROWS) * usize::from(COLUMNS) * 2);
    let c = f64::from(ROWS) / 2.0;
    for y in 0..ROWS {
        for x in 0..COLUMNS {
            let r = ((f64::from(x) - c).powi(2) + (f64::from(y) - c).powi(2)).sqrt();
            let insert = ((f64::from(x) - c - 8.0).powi(2) + (f64::from(y) - c).powi(2)).sqrt();
            let hu: i32 = if r > 28.0 {
                -1000
            } else if r > 24.0 {
                900
            } else if insert < 5.0 {
                250
            } else {
                0
            };
            out.extend_from_slice(&((hu + 1024) as u16).to_le_bytes());
        }
    }
    out
}
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        std::env::temp_dir()
            .join("ct_small.dcm")
            .display()
            .to_string()
    });
    let mut ds = Object::new();
    let text = |ds: &mut Object, group: u16, element: u16, vr: Vr, value: &str| {
        ds.set_strings(Tag::new(group, element), vr, &[value.to_string()])
    };
    text(&mut ds, 0x0008, 0x0016, Vr::UI, CT_IMAGE_STORAGE)?;
    text(&mut ds, 0x0008, 0x0018, Vr::UI, INSTANCE_UID)?;
    text(&mut ds, 0x0008, 0x0020, Vr::DA, "20240101")?;
    text(&mut ds, 0x0008, 0x0030, Vr::TM, "120000")?;
    text(&mut ds, 0x0008, 0x0050, Vr::SH, "ACC0001")?;
    text(&mut ds, 0x0008, 0x0060, Vr::CS, "CT")?;
    text(&mut ds, 0x0008, 0x1030, Vr::LO, "Phantom")?;
    text(&mut ds, 0x0010, 0x0010, Vr::PN, "PHANTOM^DISC")?;
    text(&mut ds, 0x0010, 0x0020, Vr::LO, "PH0001")?;
    text(&mut ds, 0x0020, 0x000D, Vr::UI, STUDY_UID)?;
    text(&mut ds, 0x0020, 0x000E, Vr::UI, SERIES_UID)?;
    text(&mut ds, 0x0020, 0x0010, Vr::SH, "1")?;
    ds.set_is(Tag::new(0x0020, 0x0011), &[1])?;
    ds.set_is(Tag::new(0x0020, 0x0013), &[1])?;
    ds.set_ds(Tag::new(0x0020, 0x0032), &[-32.0, -32.0, 0.0])?;
    ds.set_ds(Tag::new(0x0020, 0x0037), &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0])?;
    ds.set_us(Tag::new(0x0028, 0x0002), &[1]);
    text(&mut ds, 0x0028, 0x0004, Vr::CS, "MONOCHROME2")?;
    ds.set_us(Tag::new(0x0028, 0x0010), &[ROWS]);
    ds.set_us(Tag::new(0x0028, 0x0011), &[COLUMNS]);
    ds.set_ds(Tag::new(0x0028, 0x0030), &[1.0, 1.0])?;
    ds.set_us(Tag::new(0x0028, 0x0100), &[16]);
    ds.set_us(Tag::new(0x0028, 0x0101), &[12]);
    ds.set_us(Tag::new(0x0028, 0x0102), &[11]);
    ds.set_us(Tag::new(0x0028, 0x0103), &[0]);
    ds.set_ds(Tag::new(0x0028, 0x1050), &[40.0])?;
    ds.set_ds(Tag::new(0x0028, 0x1051), &[400.0])?;
    ds.set_ds(Tag::new(0x0028, 0x1052), &[-1024.0])?;
    ds.set_ds(Tag::new(0x0028, 0x1053), &[1.0])?;
    let pixels = phantom();
    ds.put(Element {
        header: ElementHeader::new(Tag::PIXEL_DATA, Vr::OW, pixels.len() as u32),
        value: Value::Raw(pixels),
    });
    let mut file = File {
        preamble: vec![0; 128],
        meta: Object::new(),
        dataset: ds,
        transfer_syntax: EXPLICIT_VR_LITTLE_ENDIAN,
        partial_read: None,
        incomplete_pixel_data: None,
    };
    file.rebuild_file_meta()?;
    let bytes = write_file(&file)?;
    std::fs::write(&path, &bytes)?;
    println!("wrote {} bytes to {path}", bytes.len());
    Ok(())
}
