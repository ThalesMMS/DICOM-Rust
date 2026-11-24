#![forbid(unsafe_code)]
mod ack;
mod adt;
mod batch;
mod charset;
mod encode;
mod error;
mod parse;
mod query;
mod stream;
#[rustfmt::skip]
#[allow(dead_code)]
mod tables;
mod xml;
pub use ack::{
    AckCode, AckCondition, AckOptions, AckPolicy, AckTime, Acknowledgment, ack_for, new_ack,
    parse_acknowledgment, read_ack_policy,
};
pub use ack::{
    ERROR_CODE_APPLICATION_INTERNAL_ERROR, ERROR_CODE_APPLICATION_RECORD_LOCKED,
    ERROR_CODE_DATA_TYPE, ERROR_CODE_DUPLICATE_KEY_IDENTIFIER, ERROR_CODE_REQUIRED_FIELD_MISSING,
    ERROR_CODE_SEGMENT_SEQUENCE, ERROR_CODE_TABLE_VALUE_NOT_FOUND,
    ERROR_CODE_UNKNOWN_KEY_IDENTIFIER, ERROR_CODE_UNSUPPORTED_EVENT_CODE,
    ERROR_CODE_UNSUPPORTED_MESSAGE_TYPE, ERROR_CODE_UNSUPPORTED_PROCESSING_ID,
    ERROR_CODE_UNSUPPORTED_VERSION_ID,
};
pub use adt::{
    AdtMessage, EVENT_ADD_PERSON, EVENT_ADMIT_PATIENT, EVENT_CHANGE_PATIENT_IDENTIFIER,
    EVENT_MERGE_PATIENT, EVENT_REGISTER_PATIENT, EVENT_UPDATE_PATIENT, EVENT_UPDATE_PERSON,
    Patient, PatientIdentifier, PatientIdentifierChange, PatientLocation, PatientMerge, Visit,
    parse_adt,
};
pub use batch::{Batch, BatchFile, BatchOptions, parse_batch};
pub use charset::CHARACTER_SETS;
pub use error::{
    ApplicationError, BatchError, Error, ErrorLocation, ParseError, Sentinel, StreamError,
};
pub use parse::{ParseOptions, parse, parse_with_options};
pub use query::{
    PDQ_QUERY_NAME, PIX_QUERY_NAME, QueryOptions, QueryParameter, QueryResponse, new_pdq_query,
    new_pix_query, parse_query_response,
};
pub use stream::{Cancel, parse_stream};
pub use xml::{XmlOptions, marshal_xml_message, parse_xml_message};
pub const NULL: &str = "\"\"";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Delimiters {
    pub field: u8,
    pub component: u8,
    pub repetition: u8,
    pub escape: u8,
    pub subcomponent: u8,
    pub truncation: u8,
}
pub const DEFAULT_DELIMITERS: Delimiters = Delimiters {
    field: b'|',
    component: b'^',
    repetition: b'~',
    escape: b'\\',
    subcomponent: b'&',
    truncation: 0,
};
impl Delimiters {
    pub fn encoding_characters(&self) -> String {
        let mut b = vec![self.component, self.repetition];
        if self.escape != 0 {
            b.push(self.escape);
            if self.subcomponent != 0 {
                b.push(self.subcomponent);
                if self.truncation != 0 {
                    b.push(self.truncation);
                }
            }
        }
        b.iter().map(|&c| c as char).collect()
    }
}
pub type Component = Vec<String>;
pub type Repetition = Vec<Component>;
pub type Field = Vec<Repetition>;
pub(crate) trait ComponentExt {
    fn sub(&self, n: usize) -> &str;
}
impl ComponentExt for [String] {
    fn sub(&self, n: usize) -> &str {
        if n < 1 || n > self.len() {
            ""
        } else {
            &self[n - 1]
        }
    }
}
pub(crate) trait RepetitionExt {
    fn component(&self, n: usize) -> &[String];
    fn string(&self, n: usize) -> &str;
}
impl RepetitionExt for [Component] {
    fn component(&self, n: usize) -> &[String] {
        if n < 1 || n > self.len() {
            &[]
        } else {
            &self[n - 1]
        }
    }
    fn string(&self, n: usize) -> &str {
        self.component(n).sub(1)
    }
}
pub trait FieldExt {
    fn first_rep(&self) -> &[Component];
    fn string(&self) -> &str;
    fn is_empty_value(&self) -> bool;
    fn is_null(&self) -> bool;
}
impl FieldExt for [Repetition] {
    fn first_rep(&self) -> &[Component] {
        match self.first() {
            Some(r) => r,
            None => &[],
        }
    }
    fn string(&self) -> &str {
        self.first_rep().string(1)
    }
    fn is_empty_value(&self) -> bool {
        self.iter()
            .all(|r| r.iter().all(|c| c.iter().all(String::is_empty)))
    }
    fn is_null(&self) -> bool {
        self.len() == 1 && self[0].len() == 1 && self[0][0].len() == 1 && self[0][0][0] == NULL
    }
}
pub fn text(value: &str) -> Field {
    vec![vec![vec![value.to_owned()]]]
}
pub fn components(values: &[&str]) -> Field {
    vec![values.iter().map(|v| vec![(*v).to_owned()]).collect()]
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Segment {
    pub name: String,
    pub fields: Vec<Field>,
}
impl Segment {
    pub fn new(name: &str) -> Segment {
        Segment {
            name: name.to_owned(),
            fields: Vec::new(),
        }
    }
    pub fn field(&self, n: usize) -> &[Repetition] {
        if n < 1 || n > self.fields.len() {
            &[]
        } else {
            &self.fields[n - 1]
        }
    }
    pub fn value(&self, n: usize, c: usize, sub: usize) -> &str {
        self.field(n).first_rep().component(c).sub(sub)
    }
    pub fn set_field(&mut self, n: usize, field: Field) {
        if n < 1 {
            return;
        }
        while self.fields.len() < n {
            self.fields.push(Vec::new());
        }
        self.fields[n - 1] = field;
    }
    pub fn set_value(&mut self, n: usize, value: &str) {
        self.set_field(n, text(value));
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub delimiters: Delimiters,
    pub segments: Vec<Segment>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct HierarchicDesignator {
    pub namespace_id: String,
    pub universal_id: String,
    pub universal_id_type: String,
}
impl HierarchicDesignator {
    fn from_repetition(c: &[Component]) -> HierarchicDesignator {
        HierarchicDesignator {
            namespace_id: c.string(1).to_owned(),
            universal_id: c.string(2).to_owned(),
            universal_id_type: c.string(3).to_owned(),
        }
    }
    pub(crate) fn from_component(c: &[String]) -> HierarchicDesignator {
        HierarchicDesignator {
            namespace_id: c.sub(1).to_owned(),
            universal_id: c.sub(2).to_owned(),
            universal_id_type: c.sub(3).to_owned(),
        }
    }
    fn field(&self) -> Field {
        components(&[
            &self.namespace_id,
            &self.universal_id,
            &self.universal_id_type,
        ])
    }
    fn component(&self) -> Component {
        vec![
            self.namespace_id.clone(),
            self.universal_id.clone(),
            self.universal_id_type.clone(),
        ]
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageType {
    pub code: String,
    pub trigger: String,
    pub structure: String,
}
impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.trigger.is_empty() {
            f.write_str(&self.code)
        } else {
            write!(f, "{}^{}", self.code, self.trigger)
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageHeader {
    pub sending_application: HierarchicDesignator,
    pub sending_facility: HierarchicDesignator,
    pub receiving_application: HierarchicDesignator,
    pub receiving_facility: HierarchicDesignator,
    pub date_time: String,
    pub message_type: MessageType,
    pub control_id: String,
    pub processing_id: String,
    pub version: String,
    pub character_set: String,
}
fn header_from(msh: &Segment) -> MessageHeader {
    let message_type = msh.field(9).first_rep();
    MessageHeader {
        sending_application: HierarchicDesignator::from_repetition(msh.field(3).first_rep()),
        sending_facility: HierarchicDesignator::from_repetition(msh.field(4).first_rep()),
        receiving_application: HierarchicDesignator::from_repetition(msh.field(5).first_rep()),
        receiving_facility: HierarchicDesignator::from_repetition(msh.field(6).first_rep()),
        date_time: msh.field(7).string().to_owned(),
        message_type: MessageType {
            code: message_type.string(1).to_owned(),
            trigger: message_type.string(2).to_owned(),
            structure: message_type.string(3).to_owned(),
        },
        control_id: msh.field(10).string().to_owned(),
        processing_id: msh.field(11).string().to_owned(),
        version: msh.field(12).string().to_owned(),
        character_set: msh.field(18).string().to_owned(),
    }
}
impl Message {
    pub fn new(header: &MessageHeader) -> Message {
        let mut msh = Segment::new("MSH");
        msh.set_value(1, &(DEFAULT_DELIMITERS.field as char).to_string());
        msh.set_value(2, &DEFAULT_DELIMITERS.encoding_characters());
        msh.set_field(3, header.sending_application.field());
        msh.set_field(4, header.sending_facility.field());
        msh.set_field(5, header.receiving_application.field());
        msh.set_field(6, header.receiving_facility.field());
        msh.set_value(7, &header.date_time);
        msh.set_field(
            9,
            components(&[
                &header.message_type.code,
                &header.message_type.trigger,
                &header.message_type.structure,
            ]),
        );
        msh.set_value(10, &header.control_id);
        msh.set_value(11, &header.processing_id);
        msh.set_value(12, &header.version);
        if !header.character_set.is_empty() {
            msh.set_value(18, &header.character_set);
        }
        Message {
            delimiters: DEFAULT_DELIMITERS,
            segments: vec![msh],
        }
    }
    pub fn segment(&self, name: &str) -> Option<&Segment> {
        self.segments.iter().find(|s| s.name == name)
    }
    pub fn all(&self, name: &str) -> Vec<&Segment> {
        self.segments.iter().filter(|s| s.name == name).collect()
    }
    pub fn header(&self) -> MessageHeader {
        match self.segment("MSH") {
            Some(msh) => header_from(msh),
            None => MessageHeader::default(),
        }
    }
    pub fn add(&mut self, name: &str) -> &mut Segment {
        self.segments.push(Segment::new(name));
        self.segments.last_mut().expect("just pushed")
    }
}
