#![forbid(unsafe_code)]
pub mod fuzzy;
use std::fmt;
const SEGMENT_SEP: &str = "^";
const GROUP_SEP: &str = "=";
const SPEC_URL: &str =
    "https://dicom.nema.org/medical/dicom/current/output/html/part05.html#sect_6.2";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PnGroup {
    Alphabetic,
    Ideographic,
    Phonetic,
}
impl PnGroup {
    const ALL: [PnGroup; 3] = [PnGroup::Alphabetic, PnGroup::Ideographic, PnGroup::Phonetic];
}
impl fmt::Display for PnGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PnGroup::Alphabetic => "Alphabetic",
            PnGroup::Ideographic => "Ideographic",
            PnGroup::Phonetic => "Phonetic",
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonNameError {
    TooManyGroups {
        found: usize,
    },
    TooManyGroupSegments {
        group: PnGroup,
        found: usize,
    },
    NullSepLevelInvalid {
        max_allowed: u64,
        found: u64,
    },
    FormatGroup {
        group: PnGroup,
        source: Box<PersonNameError>,
    },
}
impl PersonNameError {
    #[doc(hidden)]
    pub fn is_parse_error(&self) -> bool {
        self.is_group_count() || self.is_group_segment_count()
    }
    pub fn is_group_count(&self) -> bool {
        match self {
            Self::TooManyGroups { .. } => true,
            Self::FormatGroup { source, .. } => source.is_group_count(),
            _ => false,
        }
    }
    pub fn is_group_segment_count(&self) -> bool {
        match self {
            Self::TooManyGroupSegments { .. } => true,
            Self::FormatGroup { source, .. } => source.is_group_segment_count(),
            _ => false,
        }
    }
    pub fn is_null_sep_level_invalid(&self) -> bool {
        match self {
            Self::NullSepLevelInvalid { .. } => true,
            Self::FormatGroup { source, .. } => source.is_null_sep_level_invalid(),
            _ => false,
        }
    }
}
impl fmt::Display for PersonNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const PARSE: &str = "error parsing PN value";
        match self {
            Self::TooManyGroups { found } => write!(
                f,
                "{PARSE}: no more than 3 groups with '[Alphabetic]{GROUP_SEP}[Ideographic]\
                 {GROUP_SEP}[Phonetic]' format are allowed: value contains {found} groups. \
                 see 'PN' entry in official dicom spec: {SPEC_URL}"
            ),
            Self::TooManyGroupSegments { group, found } => write!(
                f,
                "{PARSE}: no more than 5 segments with '[Last]{s}[First]{s}[Middle]{s}[Prefix]\
                 {s}[Suffix]' format are allowed: value group {group} contains {found} \
                 segments. see 'PN' entry in official dicom spec: {SPEC_URL}",
                s = SEGMENT_SEP
            ),
            Self::NullSepLevelInvalid { max_allowed, found } => write!(
                f,
                "TrailingNullLevel exceeded maximum: cannot be greater than {max_allowed}, \
                 got {found}"
            ),
            Self::FormatGroup { group, source } => {
                write!(f, "error formatting group {group}: {source}")
            }
        }
    }
}
impl std::error::Error for PersonNameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FormatGroup { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct GroupTrailingNullLevel(pub u64);
impl GroupTrailingNullLevel {
    pub const NONE: Self = Self(0);
    pub const GIVEN: Self = Self(1);
    pub const MIDDLE: Self = Self(2);
    pub const PREFIX: Self = Self(3);
    pub const ALL: Self = Self(4);
    fn validate(self) -> Result<(), PersonNameError> {
        if self <= Self::ALL {
            Ok(())
        } else {
            Err(PersonNameError::NullSepLevelInvalid {
                max_allowed: Self::ALL.0,
                found: self.0,
            })
        }
    }
}
impl fmt::Display for GroupTrailingNullLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Self::NONE => "NONE",
            Self::GIVEN => "GivenName",
            Self::MIDDLE => "MiddleName",
            Self::PREFIX => "NamePrefix",
            Self::ALL => "ALL",
            _ => "[INVALID]",
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct InfoTrailingNullLevel(pub u64);
impl InfoTrailingNullLevel {
    pub const NONE: Self = Self(0);
    pub const IDEOGRAPHIC: Self = Self(1);
    pub const ALL: Self = Self(2);
    fn validate(self) -> Result<(), PersonNameError> {
        if self <= Self::ALL {
            Ok(())
        } else {
            Err(PersonNameError::NullSepLevelInvalid {
                max_allowed: Self::ALL.0,
                found: self.0,
            })
        }
    }
}
impl fmt::Display for InfoTrailingNullLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Self::NONE => "NONE",
            Self::IDEOGRAPHIC => "Ideographic",
            Self::ALL => "ALL",
            _ => "[INVALID]",
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct GroupInfo {
    pub family_name: String,
    pub given_name: String,
    pub middle_name: String,
    pub name_prefix: String,
    pub name_suffix: String,
    pub trailing_null_level: GroupTrailingNullLevel,
}
impl GroupInfo {
    fn segments(&self) -> [&str; 5] {
        [
            &self.family_name,
            &self.given_name,
            &self.middle_name,
            &self.name_prefix,
            &self.name_suffix,
        ]
    }
    pub fn dcm(&self) -> Result<String, PersonNameError> {
        self.trailing_null_level.validate()?;
        Ok(render_with_seps(
            &self.segments(),
            SEGMENT_SEP,
            self.trailing_null_level.0,
        ))
    }
    pub fn is_empty(&self) -> bool {
        self.segments().iter().all(|s| s.is_empty())
    }
    pub fn display(&self) -> String {
        display_segments(self.segments())
    }
    fn parse(group_string: &str, group: PnGroup) -> Result<Self, PersonNameError> {
        let segments: Vec<&str> = group_string.split(SEGMENT_SEP).collect();
        if segments.len() > 5 {
            return Err(PersonNameError::TooManyGroupSegments {
                group,
                found: segments.len(),
            });
        }
        let mut info = GroupInfo::default();
        let mut null_level = GroupTrailingNullLevel::NONE;
        for (i, value) in segments.into_iter().enumerate() {
            null_level = if value.is_empty() {
                GroupTrailingNullLevel(i as u64)
            } else {
                GroupTrailingNullLevel::NONE
            };
            let slot = match i {
                0 => &mut info.family_name,
                1 => &mut info.given_name,
                2 => &mut info.middle_name,
                3 => &mut info.name_prefix,
                _ => &mut info.name_suffix,
            };
            *slot = value.to_owned();
        }
        if group_string.ends_with(SEGMENT_SEP) {
            info.trailing_null_level = null_level;
        }
        Ok(info)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Info {
    pub alphabetic: GroupInfo,
    pub ideographic: GroupInfo,
    pub phonetic: GroupInfo,
    pub trailing_null_level: InfoTrailingNullLevel,
}
impl Info {
    pub fn parse(value: &str) -> Result<Self, PersonNameError> {
        let groups: Vec<&str> = value.split(GROUP_SEP).collect();
        if groups.len() > 3 {
            return Err(PersonNameError::TooManyGroups {
                found: groups.len(),
            });
        }
        let mut info = Info::default();
        let mut null_level = InfoTrailingNullLevel::NONE;
        for (i, (group_string, group)) in groups.into_iter().zip(PnGroup::ALL).enumerate() {
            null_level = if group_string.is_empty() {
                InfoTrailingNullLevel(i as u64)
            } else {
                InfoTrailingNullLevel::NONE
            };
            *info.group_mut(group) = GroupInfo::parse(group_string, group)?;
        }
        info.trailing_null_level = null_level;
        Ok(info)
    }
    pub fn group(&self, group: PnGroup) -> &GroupInfo {
        match group {
            PnGroup::Alphabetic => &self.alphabetic,
            PnGroup::Ideographic => &self.ideographic,
            PnGroup::Phonetic => &self.phonetic,
        }
    }
    fn group_mut(&mut self, group: PnGroup) -> &mut GroupInfo {
        match group {
            PnGroup::Alphabetic => &mut self.alphabetic,
            PnGroup::Ideographic => &mut self.ideographic,
            PnGroup::Phonetic => &mut self.phonetic,
        }
    }
    pub fn with_format(
        &self,
        info_level: InfoTrailingNullLevel,
        alphabetic: GroupTrailingNullLevel,
        ideographic: GroupTrailingNullLevel,
        phonetic: GroupTrailingNullLevel,
    ) -> Info {
        let mut info = self.clone();
        info.trailing_null_level = info_level;
        info.alphabetic.trailing_null_level = alphabetic;
        info.ideographic.trailing_null_level = ideographic;
        info.phonetic.trailing_null_level = phonetic;
        info
    }
    pub fn with_trailing_nulls(&self) -> Info {
        self.with_format(
            InfoTrailingNullLevel::ALL,
            GroupTrailingNullLevel::ALL,
            GroupTrailingNullLevel::ALL,
            GroupTrailingNullLevel::ALL,
        )
    }
    pub fn without_trailing_nulls(&self) -> Info {
        self.with_format(
            InfoTrailingNullLevel::NONE,
            GroupTrailingNullLevel::NONE,
            GroupTrailingNullLevel::NONE,
            GroupTrailingNullLevel::NONE,
        )
    }
    pub fn without_empty_groups(&self) -> Info {
        let mut info = self.clone();
        info.trailing_null_level = InfoTrailingNullLevel::NONE;
        for group in PnGroup::ALL {
            let g = info.group_mut(group);
            if g.is_empty() {
                g.trailing_null_level = GroupTrailingNullLevel::NONE;
            }
        }
        info
    }
    pub fn dcm(&self) -> Result<String, PersonNameError> {
        self.trailing_null_level.validate()?;
        let mut groups = Vec::with_capacity(3);
        for group in PnGroup::ALL {
            let rendered =
                self.group(group)
                    .dcm()
                    .map_err(|source| PersonNameError::FormatGroup {
                        group,
                        source: Box::new(source),
                    })?;
            groups.push(rendered);
        }
        let groups: Vec<&str> = groups.iter().map(String::as_str).collect();
        Ok(render_with_seps(
            &groups,
            GROUP_SEP,
            self.trailing_null_level.0,
        ))
    }
    pub fn is_empty(&self) -> bool {
        PnGroup::ALL.iter().all(|&g| self.group(g).is_empty())
    }
    pub fn display(&self) -> String {
        PnGroup::ALL
            .iter()
            .map(|&g| self.group(g).display())
            .find(|d| !d.is_empty())
            .unwrap_or_default()
    }
}
pub fn display(value: &str) -> String {
    let value = value.replace('\0', "");
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    match Info::parse(value) {
        Ok(info) => info.display(),
        Err(_) => display_segments(value.replace(GROUP_SEP, SEGMENT_SEP).split(SEGMENT_SEP)),
    }
}
fn display_segments<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
    segments
        .into_iter()
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}
fn render_with_seps(sections: &[&str], separator: &str, null_sep_level: u64) -> String {
    let Some(mut last) = sections.len().checked_sub(1) else {
        return String::new();
    };
    if last as u64 > null_sep_level {
        last = null_sep_level as usize;
    }
    if let Some(i) = (last + 1..sections.len())
        .rev()
        .find(|&i| !sections[i].is_empty())
    {
        last = i;
    }
    sections[..=last].join(separator)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_with_seps_cases() {
        let five = ["", "", "", "", ""];
        assert_eq!(render_with_seps(&[], "^", 0), "");
        assert_eq!(render_with_seps(&five, "^", 0), "");
        assert_eq!(render_with_seps(&five, "^", 3), "^^^");
        let f = ["family", "", "middle", "", ""];
        assert_eq!(render_with_seps(&f, "^", 0), "family^^middle");
        assert_eq!(render_with_seps(&f, "^", 4), "family^^middle^^");
        assert_eq!(render_with_seps(&["a", "", "c"], "=", 1), "a==c");
        assert_eq!(render_with_seps(&["a", "", "c"], "::", 0), "a::::c");
        assert_eq!(render_with_seps(&["a", "", ""], "^", 100), "a^^");
    }
    #[test]
    fn parse_round_trip() {
        for raw in [
            "CROUCH^BARTEMIUS^'BARTY'^MR^JR",
            "CROUCH^BARTEMIUS^^^",
            "^^'BARTY'^",
            "Potter^Harry^^^==",
            "=山田^太郎",
            "",
        ] {
            assert_eq!(Info::parse(raw).unwrap().dcm().unwrap(), raw);
        }
        let info = Info::parse("CROUCH^BARTEMIUS^^^").unwrap();
        assert_eq!(
            info.alphabetic.trailing_null_level,
            GroupTrailingNullLevel::ALL
        );
    }
    #[test]
    fn errors() {
        let err = Info::parse("Malfoy^Draco^^^^").unwrap_err();
        assert!(err.is_parse_error() && err.is_group_segment_count());
        assert_eq!(
            err.to_string(),
            "error parsing PN value: no more than 5 segments with \
             '[Last]^[First]^[Middle]^[Prefix]^[Suffix]' format are allowed: value group \
             Alphabetic contains 6 segments. see 'PN' entry in official dicom spec: \
             https://dicom.nema.org/medical/dicom/current/output/html/part05.html#sect_6.2"
        );
        let group = GroupInfo {
            trailing_null_level: GroupTrailingNullLevel(5),
            ..Default::default()
        };
        assert_eq!(
            group.dcm().unwrap_err().to_string(),
            "TrailingNullLevel exceeded maximum: cannot be greater than 4, got 5"
        );
    }
    #[test]
    fn display_cases() {
        assert_eq!(display(" DOE ^ JANE ^^ Dr ^ PhD "), "DOE JANE Dr PhD");
        assert_eq!(display("DOE^JANE=山田^太郎"), "DOE JANE");
        assert_eq!(display("==YAMADA^TARO"), "YAMADA TARO");
        assert_eq!(display("\x00DOE^JANE \x00"), "DOE JANE");
        assert_eq!(display(" \x00 "), "");
        assert_eq!(display("a=b=c=d"), "a b c d");
    }
}
