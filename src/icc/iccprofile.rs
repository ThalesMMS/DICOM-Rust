use super::build::{fixed, xyz};
use super::pad4;
pub fn srgb() -> Vec<u8> {
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", mluc("dcm-rust sRGB v4")),
        (b"cprt", mluc("Public domain")),
        (b"wtpt", xyz(0.9642, 1.0, 0.8249)),
        (b"rXYZ", xyz(0.4360747, 0.2225045, 0.0139322)),
        (b"gXYZ", xyz(0.3850649, 0.7168786, 0.0971045)),
        (b"bXYZ", xyz(0.1430804, 0.0606169, 0.7141733)),
        (b"rTRC", srgb_curve()),
        (b"gTRC", srgb_curve()),
        (b"bTRC", srgb_curve()),
    ];
    let table_end = 128 + 4 + tags.len() * 12;
    let mut offset = table_end;
    for (_, value) in tags.iter_mut() {
        value.resize(pad4(value.len()), 0);
        offset += value.len();
    }
    let mut profile = vec![0u8; offset];
    let put32 = |p: &mut [u8], at: usize, v: u32| p[at..at + 4].copy_from_slice(&v.to_be_bytes());
    let len = profile.len() as u32;
    put32(&mut profile, 0, len);
    profile[4..8].copy_from_slice(b"vps ");
    put32(&mut profile, 8, 0x04300000);
    profile[12..16].copy_from_slice(b"mntr");
    profile[16..20].copy_from_slice(b"RGB ");
    profile[20..24].copy_from_slice(b"XYZ ");
    profile[24..26].copy_from_slice(&2026u16.to_be_bytes());
    profile[26..28].copy_from_slice(&1u16.to_be_bytes());
    profile[28..30].copy_from_slice(&1u16.to_be_bytes());
    profile[36..40].copy_from_slice(b"acsp");
    profile[40..44].copy_from_slice(b"APPL");
    put32(&mut profile, 68, fixed(0.9642));
    put32(&mut profile, 72, fixed(1.0));
    put32(&mut profile, 76, fixed(0.8249));
    profile[80..84].copy_from_slice(b"vps ");
    put32(&mut profile, 128, tags.len() as u32);
    let mut data_offset = table_end;
    for (index, (signature, value)) in tags.iter().enumerate() {
        let record = 132 + index * 12;
        profile[record..record + 4].copy_from_slice(*signature);
        put32(&mut profile, record + 4, data_offset as u32);
        put32(&mut profile, record + 8, value.len() as u32);
        profile[data_offset..data_offset + value.len()].copy_from_slice(value);
        data_offset += value.len();
    }
    profile
}
fn mluc(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut out = vec![0u8; 28 + 2 * bytes.len()];
    out[0..4].copy_from_slice(b"mluc");
    out[8..12].copy_from_slice(&1u32.to_be_bytes());
    out[12..16].copy_from_slice(&12u32.to_be_bytes());
    out[16..20].copy_from_slice(b"enUS");
    out[20..24].copy_from_slice(&((2 * bytes.len()) as u32).to_be_bytes());
    out[24..28].copy_from_slice(&28u32.to_be_bytes());
    for (i, &c) in bytes.iter().enumerate() {
        out[28 + 2 * i + 1] = c;
    }
    out
}
fn srgb_curve() -> Vec<u8> {
    let values = [
        2.4,
        0.9478672985781991,
        0.052132701421800945,
        0.07739938080495357,
        0.04045,
        0.0,
        0.0,
    ];
    let mut out = vec![0u8; 12 + values.len() * 4];
    out[0..4].copy_from_slice(b"para");
    out[8..10].copy_from_slice(&4u16.to_be_bytes());
    for (i, &v) in values.iter().enumerate() {
        out[12 + i * 4..16 + i * 4].copy_from_slice(&fixed(v).to_be_bytes());
    }
    out
}
