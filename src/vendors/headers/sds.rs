use super::{
    Budget, format_g32, header_ascii, private_sequence, private_tag_required, private_value,
};
use crate::object::Object;
use crate::vendors::{Error, Kind, Limits};
const CREATOR: &str = "Philips MR Imaging DD 002";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdsValue {
    pub name: String,
    pub kind: u32,
    pub values: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhilipsSdsItem {
    pub name: String,
    pub data_type: String,
    pub boolean: String,
    pub values: Vec<SdsValue>,
}
fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
pub fn decode_philips_sds(raw: &[u8], l: Limits) -> Result<Vec<SdsValue>, Error> {
    decode(raw, &mut Budget::new(l))
}
fn decode(raw: &[u8], b: &mut Budget) -> Result<Vec<SdsValue>, Error> {
    if raw.len() as i64 > b.limits.max_bytes {
        return Err(Error::bare(Kind::Limit));
    }
    if raw.first() == Some(&1) {
        return Err(Error::new(Kind::UnsupportedHeader, "Philips direct record"));
    }
    if raw.len() < 32 {
        return Err(Error::bare(Kind::Invalid));
    }
    if le32(&raw[24..]) != 8 {
        return Err(Error::new(Kind::UnsupportedHeader, "Philips version"));
    }
    let count = i64::from(le32(&raw[28..]));
    if count > b.entries_left() {
        return Err(Error::bare(Kind::Limit));
    }
    if count > ((raw.len() - 32) / 50) as i64 {
        return Err(Error::bare(Kind::Invalid));
    }
    b.take(count, 0, 0)?;
    let count = count as usize;
    let end = 32 + count * 50;
    let mut out = Vec::new();
    for i in 0..count {
        let start = 32 + i * 50;
        let d = &raw[start..start + 50];
        let Some(nul) = d[..34].iter().position(|&c| c == 0) else {
            return Err(Error::new(Kind::Invalid, "Philips field name"));
        };
        let name = header_ascii(&d[..nul])?;
        let kind = le32(&d[34..]);
        let n = u64::from(le32(&d[38..]));
        let offset = u64::from(le32(&d[46..]));
        if name.is_empty() {
            continue;
        }
        if n == 0 {
            return Err(Error::bare(Kind::Invalid));
        }
        if n > b.entries_left() as u64 {
            return Err(Error::bare(Kind::Limit));
        }
        let width: u64 = match kind {
            2 => 81,
            0 | 1 | 4 => 4,
            _ => {
                return Err(Error::new(
                    Kind::UnsupportedHeader,
                    format!("Philips field type {kind}"),
                ));
            }
        };
        let pos = (start + 46) as u64 + offset;
        let len = raw.len() as u64;
        if pos < end as u64 || pos > len || n > (len - pos) / width {
            return Err(Error::new(Kind::Invalid, "Philips value span"));
        }
        b.take(n as i64, 0, name.len() as i64)?;
        let mut field = SdsValue {
            name,
            kind,
            values: Vec::new(),
        };
        for j in 0..n {
            let v = &raw[(pos + j * width) as usize..(pos + (j + 1) * width) as usize];
            let s = match kind {
                0 => {
                    let f = f32::from_bits(le32(v));
                    if !f.is_finite() {
                        return Err(Error::bare(Kind::Invalid));
                    }
                    format_g32(f)
                }
                1 => (le32(v) as i32).to_string(),
                4 => le32(v).to_string(),
                _ => {
                    let Some(k) = v.iter().position(|&c| c == 0) else {
                        return Err(Error::bare(Kind::Invalid));
                    };
                    header_ascii(&v[..k])?
                }
            };
            b.take(0, 0, s.len() as i64)?;
            field.values.push(s);
        }
        out.push(field);
    }
    if out.is_empty() {
        return Err(Error::new(Kind::Invalid, "empty Philips directory"));
    }
    Ok(out)
}
pub fn philips_sds_of(o: &Object, l: Limits) -> Result<Vec<PhilipsSdsItem>, Error> {
    let tag = private_tag_required(o, 0x2005, CREATOR, 0x32)?;
    let mut b = Budget::new(l);
    let items = private_sequence(o, tag, &mut b)?;
    if items.is_empty() {
        return Err(Error::bare(Kind::Invalid));
    }
    let mut out = Vec::new();
    for item in &items {
        let mut texts = [String::new(), String::new(), String::new()];
        for (text, offset) in texts.iter_mut().zip([0x37u8, 0x39, 0x47]) {
            let tag = private_tag_required(item, 0x2005, CREATOR, offset)?;
            let Ok(s) = item.lookup_string(tag) else {
                return Err(Error::new(Kind::Invalid, "Philips item text"));
            };
            b.take(0, 0, s.len() as i64)?;
            *text = s;
        }
        let raw = private_value(item, 0x2005, CREATOR, 0x44, b.limits)?;
        let values = decode(&raw, &mut b)?;
        let [name, data_type, boolean] = texts;
        out.push(PhilipsSdsItem {
            name,
            data_type,
            boolean,
            values,
        });
    }
    Ok(out)
}
pub(super) fn lines(items: &[PhilipsSdsItem], l: Limits) -> Result<Vec<String>, Error> {
    let mut b = Budget::new(l);
    let mut lines = Vec::new();
    for (i, item) in items.iter().enumerate() {
        b.line(
            &mut lines,
            format!(
                "Item {i}: {} / {} / {}",
                item.name, item.data_type, item.boolean
            ),
        )?;
        for v in &item.values {
            b.line(
                &mut lines,
                format!("{} = {}", v.name, v.values.join("\u{5C}")),
            )?;
        }
    }
    Ok(lines)
}
