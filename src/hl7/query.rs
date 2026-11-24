use super::ack::{
    AckTime, Acknowledgment, ERROR_CODE_UNSUPPORTED_MESSAGE_TYPE, new_control_id,
    parse_acknowledgment,
};
use super::adt::{Patient, PatientIdentifier, patient_from};
use super::error::{ApplicationError, Error, ErrorLocation};
use super::{
    Field, FieldExt, HierarchicDesignator, Message, MessageHeader, MessageType, components,
};
pub const PDQ_QUERY_NAME: &str = "IHE PDQ Query";
pub const PIX_QUERY_NAME: &str = "IHE PIX Query";
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryParameter {
    pub field: String,
    pub value: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryOptions {
    pub header: MessageHeader,
    pub tag: String,
    pub limit: i64,
    pub continuation: String,
}
fn new_query(trigger: &str, name: &str, options: &QueryOptions) -> (Message, QueryOptions) {
    let mut options = options.clone();
    let mut header = options.header.clone();
    header.message_type = MessageType {
        code: "QBP".to_owned(),
        trigger: trigger.to_owned(),
        structure: "QBP_Q21".to_owned(),
    };
    if header.control_id.is_empty() {
        header.control_id = new_control_id();
    }
    if header.version.is_empty() {
        header.version = "2.5".to_owned();
    }
    if header.processing_id.is_empty() {
        header.processing_id = "P".to_owned();
    }
    if header.date_time.is_empty() {
        header.date_time = AckTime::now().format();
    }
    if options.tag.is_empty() {
        options.tag = header.control_id.clone();
    }
    let mut message = Message::new(&header);
    let qpd = message.add("QPD");
    qpd.set_value(1, name);
    qpd.set_value(2, &options.tag);
    (message, options)
}
fn add_response_control(message: &mut Message, options: &QueryOptions) {
    let rcp = message.add("RCP");
    rcp.set_value(1, "I");
    if options.limit > 0 {
        rcp.set_field(2, components(&[&options.limit.to_string(), "RD"]));
    }
    if !options.continuation.is_empty() {
        let dsc = message.add("DSC");
        dsc.set_value(1, &options.continuation);
        dsc.set_value(2, "I");
    }
}
pub fn new_pdq_query(parameters: &[QueryParameter], options: &QueryOptions) -> Message {
    let (mut message, options) = new_query("Q22", PDQ_QUERY_NAME, options);
    let field: Field = parameters
        .iter()
        .map(|p| {
            let mut name = p.field.trim().to_owned();
            if !name.starts_with('@') {
                name.insert(0, '@');
            }
            vec![vec![name], vec![p.value.clone()]]
        })
        .collect();
    let last = message.segments.len() - 1;
    message.segments[last].set_field(3, field);
    add_response_control(&mut message, &options);
    message
}
pub fn new_pix_query(
    identifier: &PatientIdentifier,
    domains: &[HierarchicDesignator],
    options: &QueryOptions,
) -> Message {
    let (mut message, options) = new_query("Q23", PIX_QUERY_NAME, options);
    let last = message.segments.len() - 1;
    let qpd = &mut message.segments[last];
    qpd.set_field(3, vec![identifier.repetition()]);
    if !domains.is_empty() {
        let field: Field = domains
            .iter()
            .map(|d| vec![Vec::new(), Vec::new(), Vec::new(), d.component()])
            .collect();
        qpd.set_field(4, field);
    }
    add_response_control(&mut message, &options);
    message
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryResponse {
    pub acknowledgment: Acknowledgment,
    pub tag: String,
    pub status: String,
    pub patients: Vec<Patient>,
    pub continuation: String,
}
pub fn parse_query_response(m: &Message) -> Result<QueryResponse, Error> {
    let header = m.header();
    if header.message_type.code != "RSP" {
        return Err(ApplicationError {
            code: ERROR_CODE_UNSUPPORTED_MESSAGE_TYPE.to_owned(),
            location: ErrorLocation {
                segment: "MSH".to_owned(),
                sequence: 1,
                field: 9,
                component: 0,
            },
            reject: true,
            err: Error::text("not a query response"),
        }
        .into());
    }
    let ack = parse_acknowledgment(m)?;
    let mut response = QueryResponse {
        acknowledgment: ack,
        ..QueryResponse::default()
    };
    if let Some(qak) = m.segment("QAK") {
        response.tag = qak.field(1).string().to_owned();
        response.status = qak.field(2).string().to_owned();
    }
    for pid in m.all("PID") {
        response.patients.push(patient_from(pid));
    }
    if let Some(dsc) = m.segment("DSC") {
        response.continuation = dsc.field(1).string().to_owned();
    }
    Ok(response)
}
