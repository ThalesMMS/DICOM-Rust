use super::{Error, Kind};
use crate::dictionary::reservations::private_tag;
use crate::object::Object;
use crate::textfmt::quote;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MedComHistory {
    pub version: String,
    pub sections: Vec<MedComSection>,
    pub lines: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MedComSection {
    pub name: String,
    pub records: Vec<Vec<String>>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct MedComLimits {
    pub max_bytes: i64,
    pub max_lines: i64,
}
pub fn medcom_history_of(dataset: &Object, limits: MedComLimits) -> Result<MedComHistory, Error> {
    let tag = match private_tag(dataset, 0x0029, "SIEMENS MEDCOM HEADER", 0x20) {
        Ok(tag) if dataset.has(tag) => tag,
        _ => return Err(Error::new(Kind::InvalidCsa, "no MedCom History")),
    };
    let Some(raw) = dataset.get_raw(tag) else {
        return Err(Error::new(
            Kind::InvalidCsa,
            "MedCom History has no byte value",
        ));
    };
    parse_medcom_history(&raw, limits)
}
pub fn parse_medcom_history(data: &[u8], limits: MedComLimits) -> Result<MedComHistory, Error> {
    let max_bytes = if limits.max_bytes <= 0 {
        16 << 20
    } else {
        limits.max_bytes
    };
    let max_lines = if limits.max_lines <= 0 {
        100_000
    } else {
        limits.max_lines
    } as usize;
    if data.len() as i64 > max_bytes {
        return Err(Error::new(
            Kind::CsaLimit,
            format!("MedCom History of {} bytes", data.len()),
        ));
    }
    let mut data = data;
    if !data.len().is_multiple_of(2) {
        let last = data[data.len() - 1];
        if last != 0 && last != b' ' {
            return Err(Error::new(Kind::InvalidCsa, "MedCom History is not UTF-16"));
        }
        data = &data[..data.len() - 1];
    }
    let units: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let text = String::from_utf16_lossy(&units);
    const VERSION: &str = "MEDCOM HISTORY";
    if !text.starts_with(VERSION) {
        return Err(Error::new(
            Kind::InvalidCsa,
            format!("MedCom History does not start with {}", quote(VERSION)),
        ));
    }
    let mut history = MedComHistory::default();
    for line in text.trim_end_matches(['\0', ' ']).split('\0') {
        if history.lines.len() == max_lines {
            return Err(Error::new(
                Kind::CsaLimit,
                format!("MedCom History has more than {max_lines} strings"),
            ));
        }
        history.lines.push(line.to_string());
        if history.lines.len() == 1 {
            history.version = line.to_string();
        } else if !line.contains("%&") {
            history.sections.push(MedComSection {
                name: line.to_string(),
                records: Vec::new(),
            });
        } else {
            if history.sections.is_empty() {
                history.sections.push(MedComSection::default());
            }
            let section = history.sections.last_mut().expect("section");
            section
                .records
                .push(line.split("%&").map(str::to_string).collect());
        }
    }
    Ok(history)
}
