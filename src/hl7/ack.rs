use super::error::{ApplicationError, Error, ErrorLocation};
use super::{FieldExt, Message, MessageHeader, MessageType, NULL, Segment, components};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckCode(pub String);
impl AckCode {
    pub const ACCEPT: &'static str = "AA";
    pub const ERROR: &'static str = "AE";
    pub const REJECT: &'static str = "AR";
    pub const COMMIT_ACCEPT: &'static str = "CA";
    #[doc(hidden)]
    pub const COMMIT_ERROR: &'static str = "CE";
    #[doc(hidden)]
    pub const COMMIT_REJECT: &'static str = "CR";
    pub fn new(code: &str) -> AckCode {
        AckCode(code.to_owned())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
pub const ERROR_CODE_SEGMENT_SEQUENCE: &str = "100";
pub const ERROR_CODE_REQUIRED_FIELD_MISSING: &str = "101";
pub const ERROR_CODE_DATA_TYPE: &str = "102";
pub const ERROR_CODE_TABLE_VALUE_NOT_FOUND: &str = "103";
pub const ERROR_CODE_UNSUPPORTED_MESSAGE_TYPE: &str = "200";
pub const ERROR_CODE_UNSUPPORTED_EVENT_CODE: &str = "201";
pub const ERROR_CODE_UNSUPPORTED_PROCESSING_ID: &str = "202";
pub const ERROR_CODE_UNSUPPORTED_VERSION_ID: &str = "203";
pub const ERROR_CODE_UNKNOWN_KEY_IDENTIFIER: &str = "204";
pub const ERROR_CODE_DUPLICATE_KEY_IDENTIFIER: &str = "205";
pub const ERROR_CODE_APPLICATION_RECORD_LOCKED: &str = "206";
pub const ERROR_CODE_APPLICATION_INTERNAL_ERROR: &str = "207";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AckTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}
impl AckTime {
    pub fn now() -> AckTime {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        AckTime {
            year: now.year(),
            month: now.month(),
            day: now.day(),
            hour: now.hour(),
            minute: now.minute(),
            second: now.second(),
        }
    }
    pub fn format(&self) -> String {
        let year = if self.year < 0 {
            format!("-{:04}", -i64::from(self.year))
        } else {
            format!("{:04}", self.year)
        };
        format!(
            "{year}{:02}{:02}{:02}{:02}{:02}",
            self.month, self.day, self.hour, self.minute, self.second
        )
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AckOptions {
    pub text: String,
    pub error_code: String,
    pub error_location: ErrorLocation,
    pub control_id: String,
    pub time: Option<AckTime>,
}
pub fn new_ack(original: &Message, code: &str, options: &AckOptions) -> Message {
    let header = original.header();
    let now = options.time.unwrap_or_else(AckTime::now);
    let control_id = if options.control_id.is_empty() {
        new_control_id()
    } else {
        options.control_id.clone()
    };
    let version = if header.version.is_empty() {
        "2.5.1".to_owned()
    } else {
        header.version.clone()
    };
    let processing = if header.processing_id.is_empty() {
        "P".to_owned()
    } else {
        header.processing_id.clone()
    };
    let mut ack = Message::new(&MessageHeader {
        sending_application: header.receiving_application.clone(),
        sending_facility: header.receiving_facility.clone(),
        receiving_application: header.sending_application.clone(),
        receiving_facility: header.sending_facility.clone(),
        date_time: now.format(),
        message_type: MessageType {
            code: "ACK".to_owned(),
            trigger: header.message_type.trigger.clone(),
            structure: "ACK".to_owned(),
        },
        control_id,
        processing_id: processing,
        version,
        character_set: header.character_set.clone(),
    });
    let msa = ack.add("MSA");
    msa.set_value(1, code);
    msa.set_value(2, &header.control_id);
    if !options.text.is_empty() {
        msa.set_value(3, &options.text);
    }
    if code != AckCode::ACCEPT && code != AckCode::COMMIT_ACCEPT {
        let error_code = if options.error_code.is_empty() {
            ERROR_CODE_APPLICATION_INTERNAL_ERROR.to_owned()
        } else {
            options.error_code.clone()
        };
        let err = ack.add("ERR");
        let l = &options.error_location;
        if !l.segment.is_empty() {
            let sequence = l.sequence.max(1);
            let mut location = vec![l.segment.clone(), sequence.to_string(), l.field.to_string()];
            if l.component > 0 {
                location.push(String::new());
                location.push(l.component.to_string());
            }
            let refs: Vec<&str> = location.iter().map(String::as_str).collect();
            err.set_field(2, components(&refs));
        }
        err.set_field(
            3,
            components(&[&error_code, error_code_text(&error_code), "HL70357"]),
        );
        err.set_value(4, "E");
    }
    ack
}
pub fn ack_for(original: &Message, err: Option<&Error>, options: &AckOptions) -> Message {
    let Some(err) = err else {
        return new_ack(original, AckCode::ACCEPT, options);
    };
    let mut options = options.clone();
    let mut code = AckCode::ERROR;
    if let Some(app) = err.as_application() {
        if app.reject {
            code = AckCode::REJECT;
        }
        if options.error_code.is_empty() {
            options.error_code = app.code.clone();
        }
        if options.error_location.segment.is_empty() {
            options.error_location = app.location.clone();
        }
    }
    if options.error_code.is_empty() {
        options.error_code = ERROR_CODE_APPLICATION_INTERNAL_ERROR.to_owned();
    }
    if options.text.is_empty() {
        options.text = error_code_text(&options.error_code).to_owned();
    }
    new_ack(original, code, &options)
}
pub(crate) fn error_code_text(code: &str) -> &'static str {
    match code {
        ERROR_CODE_SEGMENT_SEQUENCE => "Segment sequence error",
        ERROR_CODE_REQUIRED_FIELD_MISSING => "Required field missing",
        ERROR_CODE_DATA_TYPE => "Data type error",
        ERROR_CODE_TABLE_VALUE_NOT_FOUND => "Table value not found",
        ERROR_CODE_UNSUPPORTED_MESSAGE_TYPE => "Unsupported message type",
        ERROR_CODE_UNSUPPORTED_EVENT_CODE => "Unsupported event code",
        ERROR_CODE_UNSUPPORTED_PROCESSING_ID => "Unsupported processing id",
        ERROR_CODE_UNSUPPORTED_VERSION_ID => "Unsupported version id",
        ERROR_CODE_UNKNOWN_KEY_IDENTIFIER => "Unknown key identifier",
        ERROR_CODE_DUPLICATE_KEY_IDENTIFIER => "Duplicate key identifier",
        ERROR_CODE_APPLICATION_RECORD_LOCKED => "Application record locked",
        _ => "Application internal error",
    }
}
pub(crate) fn new_control_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let state = std::collections::hash_map::RandomState::new();
    let mut out = String::with_capacity(20);
    for round in 0..2u64 {
        let mut h = state.build_hasher();
        h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        h.write_u64(round);
        if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            h.write_u128(d.as_nanos());
        }
        let v = h.finish();
        let bytes = if round == 0 { 8 } else { 2 };
        for b in &v.to_be_bytes()[..bytes] {
            out.push_str(&format!("{b:02x}"));
        }
    }
    out
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Acknowledgment {
    pub code: String,
    pub control_id: String,
    pub text: String,
    pub error_code: String,
}
fn app_error(code: &str, segment: &str, sequence: i64, field: i64, reason: &str) -> Error {
    ApplicationError {
        code: code.to_owned(),
        location: ErrorLocation {
            segment: segment.to_owned(),
            sequence,
            field,
            component: 0,
        },
        reject: false,
        err: Error::text(reason),
    }
    .into()
}
pub fn parse_acknowledgment(m: &Message) -> Result<Acknowledgment, Error> {
    let Some(msa) = m.segment("MSA") else {
        return Err(app_error(
            ERROR_CODE_SEGMENT_SEQUENCE,
            "MSA",
            1,
            0,
            "no MSA segment",
        ));
    };
    let mut ack = Acknowledgment {
        code: msa.field(1).string().to_owned(),
        control_id: msa.field(2).string().to_owned(),
        text: msa.field(3).string().to_owned(),
        error_code: String::new(),
    };
    match ack.code.as_str() {
        "AA" | "AE" | "AR" | "CA" | "CE" | "CR" => {}
        _ => {
            return Err(app_error(
                ERROR_CODE_TABLE_VALUE_NOT_FOUND,
                "MSA",
                1,
                1,
                "unknown acknowledgment code",
            ));
        }
    }
    if let Some(err) = m.segment("ERR") {
        ack.error_code = err.value(3, 1, 1).to_owned();
        if ack.error_code.is_empty() {
            ack.error_code = err.value(1, 4, 1).to_owned();
        }
    }
    Ok(ack)
}
pub type AckCondition = String;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AckPolicy {
    pub accept: AckCondition,
    pub application: AckCondition,
}
pub fn read_ack_policy(message: &Message) -> Result<AckPolicy, Error> {
    let Some(msh) = message.segments.first().filter(|s| s.name == "MSH") else {
        return Err(app_error(
            ERROR_CODE_SEGMENT_SEQUENCE,
            "MSH",
            1,
            0,
            "missing initial MSH segment",
        ));
    };
    let accept = read_ack_condition(msh, 15)?;
    let application = read_ack_condition(msh, 16)?;
    Ok(AckPolicy {
        accept,
        application,
    })
}
fn read_ack_condition(msh: &Segment, number: usize) -> Result<AckCondition, Error> {
    let field = msh.field(number);
    if field.is_empty() {
        return Ok(String::new());
    }
    if field.len() == 1 && field[0].len() == 1 && field[0][0].len() == 1 {
        let condition = field.string();
        match condition {
            "" => return Ok(String::new()),
            c if c == NULL => return Ok(String::new()),
            "AL" | "NE" | "ER" | "SU" => return Ok(condition.to_owned()),
            _ => {}
        }
    }
    Err(app_error(
        ERROR_CODE_TABLE_VALUE_NOT_FOUND,
        "MSH",
        1,
        number as i64,
        "invalid acknowledgment condition",
    ))
}
fn wants(condition: &str, success: bool) -> bool {
    condition == "AL" || (condition == "SU" && success) || (condition == "ER" && !success)
}
impl AckPolicy {
    pub fn enhanced(&self) -> bool {
        !self.accept.is_empty() || !self.application.is_empty()
    }
    pub fn wants(&self, code: &str) -> bool {
        match code {
            "CA" | "CE" | "CR" => wants(&self.accept, code == "CA"),
            "AA" | "AE" | "AR" => !self.enhanced() || wants(&self.application, code == "AA"),
            _ => false,
        }
    }
}
