use crate::dictionary::reservations::is_private_creator_tag;
use crate::object::{Object, Tag};
use crate::textfmt::to_upper;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Manufacturer {
    #[default]
    Unknown,
    Siemens,
    Ge,
    Philips,
    Canon,
    Fujifilm,
    Agfa,
    Carestream,
    Hologic,
    Elscint,
    UnitedImaging,
    Samsung,
    Esaote,
    Shimadzu,
    Konica,
    Neusoft,
    Bruker,
}
impl Manufacturer {
    pub fn as_str(self) -> &'static str {
        match self {
            Manufacturer::Unknown => "",
            Manufacturer::Siemens => "Siemens",
            Manufacturer::Ge => "GE",
            Manufacturer::Philips => "Philips",
            Manufacturer::Canon => "Canon",
            Manufacturer::Fujifilm => "Fujifilm",
            Manufacturer::Agfa => "Agfa",
            Manufacturer::Carestream => "Carestream",
            Manufacturer::Hologic => "Hologic",
            Manufacturer::Elscint => "Elscint",
            Manufacturer::UnitedImaging => "United Imaging",
            Manufacturer::Samsung => "Samsung",
            Manufacturer::Esaote => "Esaote",
            Manufacturer::Shimadzu => "Shimadzu",
            Manufacturer::Konica => "Konica Minolta",
            Manufacturer::Neusoft => "Neusoft",
            Manufacturer::Bruker => "Bruker",
        }
    }
}
pub const EVIDENCE_MANUFACTURER: &str = "manufacturer";
pub const EVIDENCE_PRIVATE_CREATOR: &str = "private-creator";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManufacturerEvidence {
    pub manufacturer: Manufacturer,
    pub source: &'static str,
    pub tag: Tag,
    pub value: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ManufacturerDetection {
    pub manufacturer: Manufacturer,
    pub evidence: Vec<ManufacturerEvidence>,
    pub conflict: bool,
}
fn contains(s: &str, words: &[&str]) -> bool {
    words.iter().any(|w| s.contains(w))
}
fn manufacturer_of_normalized(s: &str) -> Manufacturer {
    let word = |w: &str| s == w || s.starts_with(&format!("{w} "));
    if contains(s, &["SIEMENS", "ACUSON"]) {
        Manufacturer::Siemens
    } else if word("GE")
        || contains(s, &["GE MEDICAL", "GE HEALTHCARE", "GENERAL ELECTRIC"])
        || s.starts_with("GEMS")
    {
        Manufacturer::Ge
    } else if contains(s, &["PHILIPS", "PICKER", "MARCONI"]) {
        Manufacturer::Philips
    } else if contains(s, &["TOSHIBA", "CANON"]) {
        Manufacturer::Canon
    } else if word("FUJI") || contains(s, &["HITACHI", "FUJIFILM", "ALOKA"]) {
        Manufacturer::Fujifilm
    } else if s.contains("AGFA") {
        Manufacturer::Agfa
    } else if contains(s, &["KODAK", "CARESTREAM"]) {
        Manufacturer::Carestream
    } else if s.contains("HOLOGIC") {
        Manufacturer::Hologic
    } else if s.contains("ELSCINT") {
        Manufacturer::Elscint
    } else if word("UIH") || s.contains("UNITED IMAGING") {
        Manufacturer::UnitedImaging
    } else if s.contains("SAMSUNG") {
        Manufacturer::Samsung
    } else if s.contains("ESAOTE") {
        Manufacturer::Esaote
    } else if s.contains("SHIMADZU") {
        Manufacturer::Shimadzu
    } else if s.contains("KONICA") {
        Manufacturer::Konica
    } else if s.contains("NEUSOFT") {
        Manufacturer::Neusoft
    } else if s.contains("BRUKER") {
        Manufacturer::Bruker
    } else {
        Manufacturer::Unknown
    }
}
const CREATOR_PREFIXES: &[(&str, Manufacturer)] = &[
    ("SIEMENS", Manufacturer::Siemens),
    ("SIENET", Manufacturer::Siemens),
    ("GEMS_", Manufacturer::Ge),
    ("GE_", Manufacturer::Ge),
    ("GEIIS", Manufacturer::Ge),
    ("GEHC", Manufacturer::Ge),
    ("GEMS ", Manufacturer::Ge),
    ("PHILIPS", Manufacturer::Philips),
    ("SPI-P", Manufacturer::Philips),
    ("PMS", Manufacturer::Philips),
    ("TOSHIBA", Manufacturer::Canon),
    ("CANON", Manufacturer::Canon),
    ("PMTF INFORMATION DATA", Manufacturer::Canon),
    ("HITACHI", Manufacturer::Fujifilm),
    ("HMC", Manufacturer::Fujifilm),
    ("FUJI", Manufacturer::Fujifilm),
    ("AGFA", Manufacturer::Agfa),
    ("KODAK", Manufacturer::Carestream),
    ("CARESTREAM", Manufacturer::Carestream),
    ("HOLOGIC", Manufacturer::Hologic),
    ("ELSCINT", Manufacturer::Elscint),
    ("UIH", Manufacturer::UnitedImaging),
    ("SAMSUNG", Manufacturer::Samsung),
];
pub fn manufacturer_of_name(value: &str) -> Manufacturer {
    let replaced = to_upper(value).replace('.', "").replace('_', " ");
    let normalized = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Manufacturer::Unknown;
    }
    manufacturer_of_normalized(&normalized)
}
pub fn manufacturer_of_creator(creator: &str) -> Manufacturer {
    let creator = to_upper(creator.trim());
    CREATOR_PREFIXES
        .iter()
        .find(|(prefix, _)| creator.starts_with(prefix))
        .map_or(Manufacturer::Unknown, |&(_, m)| m)
}
pub fn detect_manufacturer(dataset: &Object) -> ManufacturerDetection {
    let mut detection = ManufacturerDetection::default();
    let tag_manufacturer = Tag::new(0x0008, 0x0070);
    if let Some(value) = dataset.get_string(tag_manufacturer) {
        let m = manufacturer_of_name(&value);
        if m != Manufacturer::Unknown {
            detection.manufacturer = m;
            detection.evidence.push(ManufacturerEvidence {
                manufacturer: m,
                source: EVIDENCE_MANUFACTURER,
                tag: tag_manufacturer,
                value: value.trim().to_string(),
            });
        }
    }
    let mut counts: Vec<(Manufacturer, usize)> = Vec::new();
    for element in dataset.elements() {
        let tag = element.tag();
        if !is_private_creator_tag(tag) {
            continue;
        }
        let creator = dataset.get_string(tag).unwrap_or_default();
        let m = manufacturer_of_creator(&creator);
        if m == Manufacturer::Unknown {
            continue;
        }
        detection.evidence.push(ManufacturerEvidence {
            manufacturer: m,
            source: EVIDENCE_PRIVATE_CREATOR,
            tag,
            value: creator.trim().to_string(),
        });
        match counts.iter_mut().find(|(seen, _)| *seen == m) {
            Some((_, n)) => *n += 1,
            None => counts.push((m, 1)),
        }
    }
    if detection.manufacturer == Manufacturer::Unknown {
        let mut best = 0;
        for &(m, n) in &counts {
            if n > best {
                detection.manufacturer = m;
                best = n;
            }
        }
    }
    detection.conflict = detection
        .evidence
        .iter()
        .any(|e| e.manufacturer != detection.manufacturer);
    detection
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert_eq!(
            manufacturer_of_name("G.E. Medical Systems"),
            Manufacturer::Ge
        );
        assert_eq!(manufacturer_of_name("GE_MEDICAL_SYSTEMS"), Manufacturer::Ge);
        assert_eq!(manufacturer_of_name("GENESIS"), Manufacturer::Unknown);
        assert_eq!(
            manufacturer_of_name("Picker International"),
            Manufacturer::Philips
        );
        assert_eq!(manufacturer_of_name("  "), Manufacturer::Unknown);
        assert_eq!(manufacturer_of_creator(" gems_sers_01 "), Manufacturer::Ge);
        assert_eq!(manufacturer_of_creator("X SIEMENS"), Manufacturer::Unknown);
    }
}
