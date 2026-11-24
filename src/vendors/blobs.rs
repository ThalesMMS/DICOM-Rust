use super::gzip::{Gunzipped, gunzip};
use super::{
    ByteString, Cursor, Error, Kind, Limits, private_value, trim_right_bytes, trim_unicode_space,
};
use crate::nifti::gzip::Terminal;
use crate::object::Object;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeProtocolEntry {
    pub key: ByteString,
    pub value: ByteString,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeProtocolDataBlock {
    pub name: String,
    pub text: ByteString,
    pub xml: bool,
    pub entries: Vec<GeProtocolEntry>,
    pub truncated: bool,
}
impl GeProtocolDataBlock {
    pub fn value(&self, key: &[u8]) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| e.value.as_slice())
    }
}
pub fn ge_protocol_data_block_of(
    dataset: &Object,
    limits: Limits,
) -> Result<GeProtocolDataBlock, Error> {
    let limits = limits.normalized();
    let (raw, _) = private_value(dataset, 0x0025, &["GEMS_SERS_01"], 0x1B, limits)?;
    parse_ge_protocol_data_block(&raw, limits)
}
fn limited_text(stream: &Gunzipped, max: i64) -> Result<bool, String> {
    let len = stream.data.len() as i64;
    if len > max + 1 || (len == max + 1 && !stream.attached) {
        return Ok(false);
    }
    match &stream.terminal {
        Terminal::Eof => Ok(false),
        Terminal::UnexpectedEof if len > 0 => Ok(true),
        other => Err(other.text().to_string()),
    }
}
pub fn parse_ge_protocol_data_block(
    data: &[u8],
    limits: Limits,
) -> Result<GeProtocolDataBlock, Error> {
    let limits = limits.normalized();
    if data.len() as i64 > limits.max_bytes {
        return Err(Error::new(Kind::Limit, format!("{} bytes", data.len())));
    }
    let mut c = Cursor { data, pos: 0 };
    let length = c.uint32()? as usize;
    if length > c.remaining() {
        return Err(Error::new(
            Kind::Invalid,
            format!(
                "Protocol Data Block declares {length} bytes, {} remain",
                c.remaining()
            ),
        ));
    }
    let invalid = |err: &str| Error::new(Kind::Invalid, format!("Protocol Data Block: {err}"));
    let stream = gunzip(&data[4..4 + length], false).map_err(|t| invalid(t.text()))?;
    let truncated = limited_text(&stream, limits.max_decoded_bytes).map_err(|e| invalid(&e))?;
    if stream.data.len() as i64 > limits.max_decoded_bytes {
        return Err(Error::new(
            Kind::Limit,
            format!(
                "Protocol Data Block decodes to more than {} bytes",
                limits.max_decoded_bytes
            ),
        ));
    }
    let mut block = GeProtocolDataBlock {
        name: stream.name,
        text: stream.data,
        truncated,
        ..GeProtocolDataBlock::default()
    };
    let mut lead = block.text.as_slice();
    loop {
        if let Some(rest) = lead.strip_prefix(b"\xEF\xBB\xBF") {
            lead = rest;
        } else if matches!(lead.first(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            lead = &lead[1..];
        } else {
            break;
        }
    }
    if lead.first() == Some(&b'<') {
        block.xml = true;
        return Ok(block);
    }
    for line in block.text.split(|&b| b == b'\n') {
        let line = trim_right_bytes(line, b"\r");
        if trim_unicode_space(line).is_empty() {
            continue;
        }
        if block.entries.len() as i64 == limits.max_entries {
            return Err(Error::new(
                Kind::Limit,
                format!("more than {} entries", limits.max_entries),
            ));
        }
        let (key, value) = match line.iter().position(|&b| b == b' ') {
            Some(i) => (&line[..i], &line[i + 1..]),
            None => (line, &line[line.len()..]),
        };
        let mut value = trim_unicode_space(value);
        if value.len() >= 2 && value[0] == b'"' && value[value.len() - 1] == b'"' {
            value = &value[1..value.len() - 1];
        }
        block.entries.push(GeProtocolEntry {
            key: key.to_vec(),
            value: value.to_vec(),
        });
    }
    Ok(block)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ElscintEntry {
    pub key: ByteString,
    pub values: Vec<ByteString>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ElscintGroup {
    pub name: ByteString,
    pub multiple: bool,
    pub items: Vec<Vec<ElscintEntry>>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ElscintProtocol {
    pub groups: Vec<ElscintGroup>,
    pub text: ByteString,
}
const ELSCINT_MAGIC: u32 = 0xF22D;
const ELSCINT_SINGLE: u32 = 0x0006_981A;
const ELSCINT_MULTIPLE: u32 = 0x0000_0925;
const ELSCINT_GZIP_OFFSET: usize = 0x15F;
pub fn elscint_protocol_of(dataset: &Object, limits: Limits) -> Result<ElscintProtocol, Error> {
    let limits = limits.normalized();
    let (raw, _) = private_value(dataset, 0x01F7, &["ELSCINT1"], 0x26, limits)?;
    parse_elscint_protocol(&raw, limits)
}
pub fn parse_elscint_protocol(data: &[u8], limits: Limits) -> Result<ElscintProtocol, Error> {
    let limits = limits.normalized();
    if data.len() as i64 > limits.max_bytes {
        return Err(Error::new(Kind::Limit, format!("{} bytes", data.len())));
    }
    if data.len() > ELSCINT_GZIP_OFFSET + 3
        && data[ELSCINT_GZIP_OFFSET..ELSCINT_GZIP_OFFSET + 3] == [0x1F, 0x8B, 0x08]
    {
        let invalid = |err: &str| Error::new(Kind::Invalid, format!("ELSCINT1 text: {err}"));
        let stream = gunzip(&data[ELSCINT_GZIP_OFFSET..], true).map_err(|t| invalid(t.text()))?;
        limited_text(&stream, limits.max_decoded_bytes).map_err(|e| invalid(&e))?;
        if stream.data.len() as i64 > limits.max_decoded_bytes {
            return Err(Error::new(
                Kind::Limit,
                format!(
                    "ELSCINT1 text decodes to more than {} bytes",
                    limits.max_decoded_bytes
                ),
            ));
        }
        return Ok(ElscintProtocol {
            groups: Vec::new(),
            text: stream.data,
        });
    }
    let mut c = Cursor { data, pos: 0 };
    let (magic, version, count) = (c.uint32()?, c.uint32()?, c.uint32()?);
    if magic != ELSCINT_MAGIC || version != 1 {
        return Err(Error::new(
            Kind::Invalid,
            format!("ELSCINT1 header {magic:#x} version {version}"),
        ));
    }
    if u64::from(count) * 9 > c.remaining() as u64 {
        return Err(Error::new(
            Kind::Invalid,
            format!("{count} groups do not fit"),
        ));
    }
    let mut budget = limits.max_entries;
    let mut protocol = ElscintProtocol::default();
    for g in 0..count {
        let group =
            elscint_group(&mut c, &mut budget).map_err(|e| e.prefixed(format!("group {g}")))?;
        protocol.groups.push(group);
    }
    Ok(protocol)
}
fn elscint_group(c: &mut Cursor<'_>, budget: &mut i64) -> Result<ElscintGroup, Error> {
    let kind = c.uint32()?;
    let name = c.c_string()?;
    let mut group = ElscintGroup {
        name,
        ..ElscintGroup::default()
    };
    match kind {
        ELSCINT_SINGLE => group.items.push(elscint_entries(c, budget)?),
        ELSCINT_MULTIPLE => {
            group.multiple = true;
            let items = c.uint32()?;
            if u64::from(items) * 4 > c.remaining() as u64 {
                return Err(Error::new(
                    Kind::Invalid,
                    format!("{items} items do not fit"),
                ));
            }
            for i in 0..items {
                let entries =
                    elscint_entries(c, budget).map_err(|e| e.prefixed(format!("item {i}")))?;
                group.items.push(entries);
            }
        }
        _ => {
            return Err(Error::new(Kind::Invalid, format!("group kind {kind:#x}")));
        }
    }
    Ok(group)
}
fn elscint_entries(c: &mut Cursor<'_>, budget: &mut i64) -> Result<Vec<ElscintEntry>, Error> {
    let count = c.uint32()?;
    if u64::from(count) * 5 > c.remaining() as u64 {
        return Err(Error::new(
            Kind::Invalid,
            format!("{count} entries do not fit"),
        ));
    }
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let key = c.c_string()?;
        let values = c.uint32()?;
        if u64::from(values) > c.remaining() as u64 {
            return Err(Error::new(
                Kind::Invalid,
                format!("{values} values do not fit"),
            ));
        }
        *budget -= 1 + i64::from(values);
        if *budget < 0 {
            return Err(Error::new(Kind::Limit, "too many entries"));
        }
        let mut entry = ElscintEntry {
            key,
            values: Vec::with_capacity(values as usize),
        };
        for _ in 0..values {
            entry.values.push(c.c_string()?);
        }
        entries.push(entry);
    }
    Ok(entries)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VeproData {
    pub id: ByteString,
    pub version: ByteString,
    pub user_name: ByteString,
    pub user_address: [ByteString; 5],
    pub recording_date: ByteString,
    pub recording_time: ByteString,
    pub recording_place: ByteString,
    pub recording_source: ByteString,
    pub data_fields: [ByteString; 20],
    pub study_instance_uid: ByteString,
    pub series_uid: ByteString,
    pub modality: ByteString,
}
const VEPRO_SIZE: usize = 1786;
pub fn vepro_data_of(dataset: &Object, limits: Limits) -> Result<VeproData, Error> {
    let limits = limits.normalized();
    let (raw, _) = private_value(
        dataset,
        0x0055,
        &["VEPRO VIF 3.0 DATA", "VEPRO VIM 5.0 DATA"],
        0x20,
        limits,
    )?;
    parse_vepro_data(&raw, limits)
}
pub fn parse_vepro_data(data: &[u8], limits: Limits) -> Result<VeproData, Error> {
    let limits = limits.normalized();
    if data.len() as i64 > limits.max_bytes {
        return Err(Error::new(Kind::Limit, format!("{} bytes", data.len())));
    }
    if data.len() < VEPRO_SIZE {
        return Err(Error::new(
            Kind::Invalid,
            format!("VEPRO data has {} bytes, {VEPRO_SIZE} needed", data.len()),
        ));
    }
    let field = |offset: usize, width: usize| {
        trim_right_bytes(&data[offset..offset + width], b" \x00").to_vec()
    };
    Ok(VeproData {
        id: field(0, 3),
        version: field(3, 3),
        user_name: field(6, 32),
        user_address: std::array::from_fn(|i| field(38 + 32 * i, 32)),
        recording_date: field(198, 8),
        recording_time: field(206, 6),
        recording_place: field(212, 64),
        recording_source: field(276, 64),
        data_fields: std::array::from_fn(|i| field(340 + 64 * i, 64)),
        study_instance_uid: field(1642, 64),
        series_uid: field(1706, 64),
        modality: field(1770, 16),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn elscint_binary() {
        let mut blob = Vec::new();
        for v in [ELSCINT_MAGIC, 1, 1, ELSCINT_SINGLE] {
            blob.extend_from_slice(&v.to_le_bytes());
        }
        blob.extend_from_slice(b"SCAN\0");
        blob.extend_from_slice(&1u32.to_le_bytes());
        blob.extend_from_slice(b"kv\0");
        blob.extend_from_slice(&1u32.to_le_bytes());
        blob.extend_from_slice(b"120\0");
        let p = parse_elscint_protocol(&blob, Limits::default()).unwrap();
        assert_eq!(p.groups[0].name, b"SCAN");
        assert_eq!(p.groups[0].items[0][0].values, vec![b"120".to_vec()]);
        let err = parse_elscint_protocol(&blob[..blob.len() - 1], Limits::default()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "group 0: dicom/vendors: invalid private blob: unterminated string at byte 32"
        );
    }
    #[test]
    fn vepro_short() {
        let err = parse_vepro_data(&[0; 10], Limits::default()).unwrap_err();
        assert!(err.is(Kind::Invalid));
        let d = parse_vepro_data(&[b' '; VEPRO_SIZE], Limits::default()).unwrap();
        assert!(d.id.is_empty());
    }
}
