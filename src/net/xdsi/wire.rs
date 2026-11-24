use super::Error;
use crate::dicomxml::xmlwire::{self, Decoder, Tok, XmlError};
use crate::net::dicomweb::mimeheader::{Header, canonical_header_key, parse_media_type};
use crate::net::dicomweb::server::media::format_media_type;
use crate::net::dicomweb::server::spool::random_boundary;
use crate::net::dicomweb::server::stow::MultipartReader;
use std::io::Read;
pub(super) const NS_SOAP: &str = "http://www.w3.org/2003/05/soap-envelope";
pub(super) const NS_WSA: &str = "http://www.w3.org/2005/08/addressing";
pub(super) const NS_XDS: &str = "urn:ihe:iti:xds-b:2007";
pub(super) const NS_RAD: &str = "urn:ihe:rad:xdsi-b:2009";
pub(super) const NS_RIM: &str = "urn:oasis:names:tc:ebxml-regrep:xsd:rim:3.0";
pub(super) const NS_LCM: &str = "urn:oasis:names:tc:ebxml-regrep:xsd:lcm:3.0";
pub(super) const NS_RS: &str = "urn:oasis:names:tc:ebxml-regrep:xsd:rs:3.0";
pub(super) const NS_XOP: &str = "http://www.w3.org/2004/08/xop/include";
pub(super) const ACTION_PUBLISH: &str = "urn:ihe:iti:2007:ProvideAndRegisterDocumentSet-b";
pub(super) const ACTION_PUBLISH_RESPONSE: &str =
    "urn:ihe:iti:2007:ProvideAndRegisterDocumentSet-bResponse";
pub(super) const ACTION_RETRIEVE: &str = "urn:ihe:rad:2009:RetrieveImagingDocumentSet";
pub(super) const ACTION_RETRIEVE_RESPONSE: &str = "urn:ihe:iti:2007:RetrieveDocumentSetResponse";
pub(super) const STATUS_SUCCESS: &str =
    "urn:oasis:names:tc:ebxml-regrep:ResponseStatusType:Success";
pub(super) const STATUS_PARTIAL: &str = "urn:ihe:iti:2007:ResponseStatusType:PartialSuccess";
pub(super) const STATUS_FAILURE: &str =
    "urn:oasis:names:tc:ebxml-regrep:ResponseStatusType:Failure";
const ANONYMOUS: &str = "http://www.w3.org/2005/08/addressing/anonymous";
pub(super) const MAX_SOAP_BYTES: i64 = 2 << 20;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct QName {
    pub space: String,
    pub local: String,
}
impl QName {
    pub fn is(&self, space: &str, local: &str) -> bool {
        self.space == space && self.local == local
    }
}
#[derive(Clone, Debug, Default)]
pub(super) struct Node {
    pub name: QName,
    pub attrs: Vec<(QName, String)>,
    pub text: String,
    pub children: Vec<Node>,
}
pub(super) fn element(ns: &str, local: &str, text: &str, attrs: &[(&str, &str)]) -> Node {
    Node {
        name: QName {
            space: ns.into(),
            local: local.into(),
        },
        attrs: attrs
            .iter()
            .map(|(k, v)| {
                (
                    QName {
                        space: String::new(),
                        local: (*k).into(),
                    },
                    (*v).into(),
                )
            })
            .collect(),
        text: text.into(),
        children: Vec::new(),
    }
}
impl Node {
    pub fn with(mut self, children: Vec<Node>) -> Node {
        self.children = children;
        self
    }
    pub(crate) fn children_named<'a>(
        &'a self,
        ns: &'a str,
        local: &'a str,
    ) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name.is(ns, local))
    }
    pub fn one(&self, ns: &str, local: &str) -> Result<&Node, Error> {
        let mut found = self.children.iter().filter(|c| c.name.is(ns, local));
        match (found.next(), found.next()) {
            (Some(node), None) => Ok(node),
            _ => Err(Error::Protocol),
        }
    }
    pub fn attr(&self, local: &str) -> &str {
        self.attrs
            .iter()
            .find(|(name, _)| name.space.is_empty() && name.local == local)
            .map_or("", |(_, v)| v.as_str())
    }
}
fn escape(out: &mut String, s: &str, newline: bool) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("&#34;"),
            '\'' => out.push_str("&#39;"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' => out.push_str("&#x9;"),
            '\n' if newline => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            c if !xmlwire::is_in_character_range(c as u32) => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
}
fn is_xml_name(s: &str) -> bool {
    let mut chars = s.chars();
    let first = |c: char| c == ':' || c == '_' || c.is_alphabetic();
    match chars.next() {
        Some(c) if first(c) => {
            chars.all(|c| first(c) || c == '.' || c == '-' || c == '\u{b7}' || c.is_alphanumeric())
        }
        _ => false,
    }
}
#[derive(Default)]
struct Printer {
    out: String,
    attr_prefix: Vec<(String, String)>,
    prefixes: Vec<Vec<String>>,
    seq: u64,
}
impl Printer {
    fn create_attr_prefix(&mut self, url: &str) -> String {
        if let Some((_, prefix)) = self.attr_prefix.iter().find(|(u, _)| u == url) {
            return prefix.clone();
        }
        if url == xmlwire::XML_URL {
            return "xml".into();
        }
        let trimmed = url.trim_end_matches('/');
        let mut prefix = match trimmed.rfind('/') {
            Some(i) => &trimmed[i + 1..],
            None => trimmed,
        }
        .to_string();
        if prefix.is_empty() || !is_xml_name(&prefix) || prefix.contains(':') {
            prefix = "_".into();
        }
        if prefix.len() >= 3 && prefix.as_bytes()[..3].eq_ignore_ascii_case(b"xml") {
            prefix = format!("_{prefix}");
        }
        let taken = |p: &str, list: &[(String, String)]| list.iter().any(|(_, q)| q == p);
        if taken(&prefix, &self.attr_prefix) {
            self.seq += 1;
            loop {
                let id = format!("{prefix}_{}", self.seq);
                if !taken(&id, &self.attr_prefix) {
                    prefix = id;
                    break;
                }
                self.seq += 1;
            }
        }
        self.attr_prefix.push((url.to_string(), prefix.clone()));
        self.out.push_str("xmlns:");
        self.out.push_str(&prefix);
        self.out.push_str("=\"");
        escape(&mut self.out, url, true);
        self.out.push_str("\" ");
        if let Some(open) = self.prefixes.last_mut() {
            open.push(prefix.clone());
        }
        prefix
    }
    fn node(&mut self, n: &Node) {
        self.prefixes.push(Vec::new());
        self.out.push('<');
        self.out.push_str(&n.name.local);
        if !n.name.space.is_empty() {
            self.out.push_str(" xmlns=\"");
            escape(&mut self.out, &n.name.space, true);
            self.out.push('"');
        }
        for (name, value) in &n.attrs {
            if name.local.is_empty() {
                continue;
            }
            self.out.push(' ');
            if !name.space.is_empty() {
                let prefix = self.create_attr_prefix(&name.space);
                self.out.push_str(&prefix);
                self.out.push(':');
            }
            self.out.push_str(&name.local);
            self.out.push_str("=\"");
            escape(&mut self.out, value, true);
            self.out.push('"');
        }
        self.out.push('>');
        if !n.text.is_empty() {
            escape(&mut self.out, &n.text, false);
        }
        for child in &n.children {
            self.node(child);
        }
        self.out.push_str("</");
        self.out.push_str(&n.name.local);
        self.out.push('>');
        for prefix in self.prefixes.pop().unwrap_or_default() {
            self.attr_prefix.retain(|(_, p)| *p != prefix);
        }
    }
}
pub(super) fn marshal(root: &Node) -> Vec<u8> {
    let mut p = Printer::default();
    p.node(root);
    p.out.into_bytes()
}
fn no_charset_reader(label: &str, _: &[u8]) -> Result<Vec<u8>, String> {
    Err(format!(
        "xml: encoding {} declared but no charset reader is set",
        crate::textfmt::quote(label)
    ))
}
pub(super) fn parse_xml(data: &[u8]) -> Result<Node, Error> {
    if data.len() as i64 > MAX_SOAP_BYTES {
        return Err(Error::Limit);
    }
    let mut decoder = Decoder::new(data.to_vec(), None, no_charset_reader);
    let mut root: Option<Node> = None;
    let mut stack: Vec<(Node, Vec<u8>)> = Vec::new();
    let mut elements = 0;
    loop {
        let token = match decoder.token() {
            Ok(t) => t,
            Err(XmlError::Eof) => break,
            Err(_) => return Err(Error::Protocol),
        };
        match token {
            Tok::Start(name, attrs) => {
                elements += 1;
                if elements > 20000 || stack.len() >= 24 || attrs.len() > 24 {
                    return Err(Error::Limit);
                }
                let mut seen: Vec<&xmlwire::Name> = Vec::with_capacity(attrs.len());
                for a in &attrs {
                    if seen.contains(&&a.name) {
                        return Err(Error::Protocol);
                    }
                    seen.push(&a.name);
                }
                if stack.is_empty() && root.is_some() {
                    return Err(Error::Protocol);
                }
                let node = Node {
                    name: QName {
                        space: name.space,
                        local: name.local,
                    },
                    attrs: attrs
                        .into_iter()
                        .map(|a| {
                            (
                                QName {
                                    space: a.name.space,
                                    local: a.name.local,
                                },
                                a.value,
                            )
                        })
                        .collect(),
                    ..Node::default()
                };
                stack.push((node, Vec::new()));
            }
            Tok::End(_) => {
                let Some((mut node, text)) = stack.pop() else {
                    return Err(Error::Protocol);
                };
                node.text = String::from_utf8_lossy(&text).into_owned();
                match stack.last_mut() {
                    Some((parent, _)) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Tok::CharData(data) => match stack.last_mut() {
                Some((_, text)) => text.extend_from_slice(&data),
                None => {
                    let text = String::from_utf8_lossy(&data);
                    if !super::trim_unicode_space(&text).is_empty() {
                        return Err(Error::Protocol);
                    }
                }
            },
            Tok::Directive(_) => return Err(Error::Protocol),
            Tok::ProcInst(target, _) => {
                if target != "xml" || root.is_some() || !stack.is_empty() {
                    return Err(Error::Protocol);
                }
            }
            Tok::Comment(_) => {}
        }
    }
    match root {
        Some(root) if stack.is_empty() => Ok(root),
        _ => Err(Error::Protocol),
    }
}
pub(super) fn new_message_id() -> String {
    format!("urn:uuid:{}", uuid::Uuid::new_v4().hyphenated())
}
pub(super) fn envelope(action: &str, to: &str, message_id: &str, body: Node) -> Node {
    let must_understand = (
        QName {
            space: NS_SOAP.into(),
            local: "mustUnderstand".into(),
        },
        "1".to_string(),
    );
    let mut action_node = element(NS_WSA, "Action", action, &[]);
    action_node.attrs = vec![must_understand.clone()];
    let mut to_node = element(NS_WSA, "To", to, &[]);
    to_node.attrs = vec![must_understand];
    let reply =
        element(NS_WSA, "ReplyTo", "", &[]).with(vec![element(NS_WSA, "Address", ANONYMOUS, &[])]);
    let header = element(NS_SOAP, "Header", "", &[]).with(vec![
        action_node,
        element(NS_WSA, "MessageID", message_id, &[]),
        reply,
        to_node,
    ]);
    let body_node = element(NS_SOAP, "Body", "", &[]).with(vec![body]);
    element(NS_SOAP, "Envelope", "", &[]).with(vec![header, body_node])
}
pub(super) fn response_body(root: Node, action: &str, request_id: &str) -> Result<Node, Error> {
    if !root.name.is(NS_SOAP, "Envelope") || root.children.len() != 2 {
        return Err(Error::Protocol);
    }
    let header = root.one(NS_SOAP, "Header")?;
    for child in &header.children {
        let known = child.name.space == NS_WSA
            && matches!(
                child.name.local.as_str(),
                "Action" | "RelatesTo" | "MessageID" | "To"
            );
        for (name, value) in &child.attrs {
            if name.is(NS_SOAP, "mustUnderstand") && (value == "1" || value == "true") && !known {
                return Err(Error::Protocol);
            }
        }
    }
    let action_node = header.one(NS_WSA, "Action").map_err(|_| Error::Protocol)?;
    if !action_node.children.is_empty() || super::trim_unicode_space(&action_node.text) != action {
        return Err(Error::Protocol);
    }
    let relates = header
        .one(NS_WSA, "RelatesTo")
        .map_err(|_| Error::Protocol)?;
    if !relates.children.is_empty() || super::trim_unicode_space(&relates.text) != request_id {
        return Err(Error::Protocol);
    }
    let body = root.one(NS_SOAP, "Body").map_err(|_| Error::Protocol)?;
    if body.children.len() != 1 || body.children[0].name.is(NS_SOAP, "Fault") {
        return Err(Error::Protocol);
    }
    let index = root
        .children
        .iter()
        .position(|c| c.name.is(NS_SOAP, "Body"))
        .unwrap_or(0);
    let mut root = root;
    let mut body = root.children.swap_remove(index);
    Ok(body.children.swap_remove(0))
}
#[derive(Clone, Debug)]
pub(super) struct Attachment {
    pub id: String,
    pub media_type: String,
    pub data: Vec<u8>,
}
fn part_header(content_type: &str, id: &str) -> String {
    format!(
        "Content-Id: <{id}>\r\nContent-Transfer-Encoding: binary\r\nContent-Type: {content_type}\r\n\r\n"
    )
}
pub(super) fn encode_mtom(
    root: &Node,
    action: &str,
    attachments: &[Attachment],
) -> Result<(Vec<u8>, String), Error> {
    let soap = marshal(root);
    if soap.len() as i64 > MAX_SOAP_BYTES {
        return Err(Error::Limit);
    }
    let boundary = random_boundary();
    let mut body = Vec::with_capacity(
        soap.len()
            + attachments
                .iter()
                .map(|a| a.data.len() + 256)
                .sum::<usize>()
            + 512,
    );
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    let root_type = format_media_type(
        "application/xop+xml",
        &[("charset", "UTF-8"), ("type", "application/soap+xml")],
    );
    body.extend_from_slice(part_header(&root_type, "root@xdsi").as_bytes());
    body.extend_from_slice(&soap);
    for a in attachments {
        body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
        body.extend_from_slice(part_header(&a.media_type, &a.id).as_bytes());
        body.extend_from_slice(&a.data);
    }
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let start_info = format_media_type("application/soap+xml", &[("action", action)]);
    let content_type = format_media_type(
        "multipart/related",
        &[
            ("boundary", &boundary),
            ("type", "application/xop+xml"),
            ("start", "<root@xdsi>"),
            ("start-info", &start_info),
        ],
    );
    Ok((body, content_type))
}
pub(super) fn read_mtom(
    content_type: &str,
    body: &mut dyn Read,
    limits: super::Limits,
    max_parts: usize,
) -> Result<(Node, Vec<Attachment>), Error> {
    let media = parse_media_type(content_type).map_err(|_| Error::Protocol)?;
    if media.media_type != "multipart/related"
        || media.param("type") != "application/xop+xml"
        || media.param("boundary").is_empty()
        || media.param("start").is_empty()
    {
        return Err(Error::Protocol);
    }
    match parse_media_type(media.param("start-info")) {
        Ok(info) if info.media_type == "application/soap+xml" => {}
        _ => return Err(Error::Protocol),
    }
    let mut limited = body.take((limits.total_bytes + 1) as u64);
    let mut reader = MultipartReader::new(&mut limited, media.param("boundary"));
    let mut parts: Vec<Attachment> = Vec::new();
    let mut total: i64 = 0;
    let start = media.param("start");
    let root_id = start.strip_prefix('<').unwrap_or(start);
    let root_id = root_id.strip_suffix('>').unwrap_or(root_id).to_string();
    let content_id_key = canonical_header_key("Content-ID");
    for i in 0.. {
        let header: Header = match reader.next_part() {
            Ok(Some(h)) => h,
            Ok(None) => break,
            Err(_) => return Err(Error::Protocol),
        };
        if i >= max_parts {
            return Err(Error::Limit);
        }
        let id = header.get(&content_id_key);
        if id.len() < 3 || !id.starts_with('<') || !id.ends_with('>') || id.len() > 200 {
            return Err(Error::Protocol);
        }
        let id = id[1..id.len() - 1].to_string();
        if parts.iter().any(|p| p.id == id) {
            return Err(Error::Protocol);
        }
        let encoding = header.get("Content-Transfer-Encoding");
        if encoding != "binary" && encoding != "8bit" {
            return Err(Error::Protocol);
        }
        let media = parse_media_type(header.get("Content-Type")).map_err(|_| Error::Protocol)?;
        let mut ceiling = limits.document_bytes;
        if id == root_id {
            ceiling = MAX_SOAP_BYTES;
            if media.media_type != "application/xop+xml"
                || media.param("type") != "application/soap+xml"
            {
                return Err(Error::Protocol);
            }
        }
        let mut data = Vec::new();
        let mut buf = vec![0u8; 32 << 10];
        let cap = (ceiling + 1) as usize;
        while data.len() < cap {
            let want = buf.len().min(cap - data.len());
            match reader.read_part(&mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => data.extend_from_slice(&buf[..n]),
                Err(e) => return Err(Error::Other(e.to_string())),
            }
        }
        if data.len() as i64 > ceiling {
            return Err(Error::Limit);
        }
        total += data.len() as i64;
        if total > limits.total_bytes {
            return Err(Error::Limit);
        }
        parts.push(Attachment {
            id,
            media_type: media.media_type,
            data,
        });
    }
    let Some(index) = parts.iter().position(|p| p.id == root_id) else {
        return Err(Error::Protocol);
    };
    let soap = parts.remove(index);
    let root = parse_xml(&soap.data)?;
    Ok((root, parts))
}
