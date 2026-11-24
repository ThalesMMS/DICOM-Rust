use crate::object::Tag;
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrivateEntry {
    pub creator: &'static str,
    pub group: u16,
    pub offset: u8,
    pub vr: &'static str,
    pub keyword: &'static str,
    pub name: &'static str,
    pub vm: &'static str,
    pub retired: bool,
}
impl PrivateEntry {
    pub fn tag(&self) -> Tag {
        Tag::new(self.group, u16::from(self.offset))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateCreatorError;
impl fmt::Display for PrivateCreatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("dicom: invalid private creator reservation")
    }
}
impl std::error::Error for PrivateCreatorError {}
pub fn private_creator_id(value: &str) -> Result<&str, PrivateCreatorError> {
    if value.len() > 64
        || value
            .bytes()
            .any(|b| !(0x20..=0x7e).contains(&b) || b == b'\\')
    {
        return Err(PrivateCreatorError);
    }
    match value.trim_matches(' ') {
        "" => Err(PrivateCreatorError),
        id => Ok(id),
    }
}
pub fn is_private_group(group: u16) -> bool {
    group >= 0x0009 && group != 0xffff && group & 1 != 0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creator_ids() {
        assert_eq!(
            private_creator_id("  SIEMENS CSA HEADER "),
            Ok("SIEMENS CSA HEADER")
        );
        assert_eq!(private_creator_id("A  B"), Ok("A  B"));
        for bad in ["", "   ", "a\\b", "a\0", "é", "\tA", &"x".repeat(65)] {
            assert_eq!(private_creator_id(bad), Err(PrivateCreatorError), "{bad:?}");
        }
        assert_eq!(private_creator_id(&"x".repeat(64)).map(str::len), Ok(64));
    }
    #[test]
    fn private_groups() {
        assert!(is_private_group(0x0009));
        assert!(is_private_group(0xfffd));
        assert!(!is_private_group(0x0007));
        assert!(!is_private_group(0x0010));
        assert!(!is_private_group(0xffff));
    }
}
