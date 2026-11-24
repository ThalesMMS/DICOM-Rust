#![forbid(unsafe_code)]
use std::fmt;
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, PartialOrd, Ord)]
pub struct Tag {
    pub group: u16,
    pub element: u16,
}
impl Tag {
    pub const fn new(group: u16, element: u16) -> Tag {
        Tag { group, element }
    }
    pub const PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
    pub const ITEM: Tag = Tag::new(0xFFFE, 0xE000);
    pub const ITEM_DELIMITATION: Tag = Tag::new(0xFFFE, 0xE00D);
    pub const SEQUENCE_DELIMITATION: Tag = Tag::new(0xFFFE, 0xE0DD);
    pub fn is_zero(self) -> bool {
        self == Tag::default()
    }
    pub fn is_private(self) -> bool {
        self.group % 2 == 1
    }
    pub fn is_group_length(self) -> bool {
        self.element == 0
    }
    pub fn is_sequence_delimiting(self) -> bool {
        self.group == 0xFFFE
    }
    pub fn is_item(self) -> bool {
        self == Tag::ITEM
    }
    pub fn is_item_delimitation(self) -> bool {
        self == Tag::ITEM_DELIMITATION
    }
    pub fn is_sequence_delimitation(self) -> bool {
        self == Tag::SEQUENCE_DELIMITATION
    }
    pub fn key(self) -> u32 {
        (self.group as u32) << 16 | self.element as u32
    }
}
impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:04X},{:04X})", self.group, self.element)
    }
}
macro_rules! vrs {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum Vr { $($(#[$doc])* $name),* }
        impl Vr {
            pub fn as_str(self) -> &'static str {
                match self { $(Vr::$name => stringify!($name)),* }
            }
            pub fn parse(bytes: &[u8]) -> Option<Vr> {
                match bytes { $(b if b == stringify!($name).as_bytes() => Some(Vr::$name),)* _ => None }
            }
        }
    };
}
vrs!(
    AE, AS, AT, CS, DA, DS, DT, FL, FD, IS, LO, LT, OB, OD, OF, OL, OV, OW, PN, SH, SL, SQ, SS, SV,
    ST, TM, UC, UI, UL, UN, UR, US, UT, UV,
);
impl Vr {
    pub fn uses_long_explicit_length(self) -> bool {
        matches!(
            self,
            Vr::OB
                | Vr::OD
                | Vr::OF
                | Vr::OL
                | Vr::OV
                | Vr::OW
                | Vr::SQ
                | Vr::SV
                | Vr::UC
                | Vr::UR
                | Vr::UT
                | Vr::UN
                | Vr::UV
        )
    }
    pub fn is_string_like(self) -> bool {
        matches!(
            self,
            Vr::AE
                | Vr::AS
                | Vr::CS
                | Vr::DA
                | Vr::DS
                | Vr::DT
                | Vr::IS
                | Vr::LO
                | Vr::LT
                | Vr::PN
                | Vr::SH
                | Vr::ST
                | Vr::TM
                | Vr::UC
                | Vr::UI
                | Vr::UR
                | Vr::UT
        )
    }
    pub(crate) fn byte_order_width(self) -> usize {
        match self {
            Vr::AT | Vr::OW | Vr::SS | Vr::US => 2,
            Vr::FL | Vr::OF | Vr::OL | Vr::SL | Vr::UL => 4,
            Vr::FD | Vr::OD | Vr::OV | Vr::SV | Vr::UV => 8,
            _ => 1,
        }
    }
}
impl fmt::Display for Vr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
pub type Length = u32;
pub const UNDEFINED_LENGTH: Length = 0xFFFF_FFFF;
pub fn length_string(length: Length) -> String {
    if length == UNDEFINED_LENGTH {
        "UNDEFINED".to_string()
    } else {
        length.to_string()
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ElementHeader {
    pub tag: Tag,
    pub vr: Vr,
    pub length: Length,
    pub length_set: bool,
}
impl ElementHeader {
    pub fn new(tag: Tag, vr: Vr, length: Length) -> ElementHeader {
        ElementHeader {
            tag,
            vr,
            length,
            length_set: true,
        }
    }
    pub fn is_undefined(&self) -> bool {
        self.length == UNDEFINED_LENGTH
    }
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Value {
    Nil,
    Raw(Vec<u8>),
    Sequence(Vec<DataSet>),
    Strings(Vec<String>),
    Fragments {
        offset_table: Vec<u8>,
        fragments: Vec<Vec<u8>>,
    },
    BulkData(String),
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Element {
    pub header: ElementHeader,
    pub value: Value,
}
impl Element {
    pub fn tag(&self) -> Tag {
        self.header.tag
    }
    pub fn vr(&self) -> Vr {
        self.header.vr
    }
    pub fn raw(&self) -> Option<&[u8]> {
        match &self.value {
            Value::Raw(b) => Some(b),
            _ => None,
        }
    }
}
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DataSet {
    pub elements: Vec<Element>,
    pub item_offset: Option<i64>,
}
