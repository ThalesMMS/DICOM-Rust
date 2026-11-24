use super::template_metadata::{CodeContext, ContentItemCodeContexts};
use super::{
    CodedEntry, ContentItem, Document, Kind, RELATIONSHIP_CONTAINS, SrError, identifier_string,
    value,
};
use crate::dcmtime::{format_date, format_time};
use crate::numtext::format_float;
use std::collections::HashMap;
use std::fmt::Write as _;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderOptions {
    pub header: Vec<RenderField>,
    pub show_codes: bool,
    pub max_items: i64,
    pub max_points: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderField {
    pub label: String,
    pub value: String,
}
fn field(label: &str, value: String) -> RenderField {
    RenderField {
        label: label.into(),
        value,
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedItem {
    pub path: String,
    pub depth: i64,
    pub relationship: String,
    pub value_type: String,
    pub concept: String,
    pub value: String,
    pub reference_to: String,
    pub continuity: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedDocument {
    pub title: String,
    pub header: Vec<RenderField>,
    pub items: Vec<RenderedItem>,
    pub truncated: i64,
}
pub(crate) type Contexts = HashMap<String, ContentItemCodeContexts>;
fn nil_document() -> SrError {
    SrError::new(Kind::NilDocument, "")
}
fn defaults(mut opts: RenderOptions) -> RenderOptions {
    if opts.max_items <= 0 {
        opts.max_items = 100_000;
    }
    if opts.max_points <= 0 {
        opts.max_points = 32;
    }
    opts
}
pub fn render_items(
    doc: Option<&Document>,
    opts: &RenderOptions,
) -> Result<RenderedDocument, SrError> {
    render_items_with_contexts(doc, opts, None)
}
pub(crate) fn render_items_with_contexts(
    doc: Option<&Document>,
    opts: &RenderOptions,
    contexts: Option<&Contexts>,
) -> Result<RenderedDocument, SrError> {
    let Some(doc) = doc else {
        return Err(nil_document());
    };
    let opts = defaults(opts.clone());
    let mut out = render_header(doc, &opts, contexts);
    let r = Renderer {
        opts: &opts,
        contexts,
    };
    r.items(&mut out, &doc.content, "1.", 0);
    Ok(out)
}
fn render_header(
    doc: &Document,
    opts: &RenderOptions,
    contexts: Option<&Contexts>,
) -> RenderedDocument {
    let mut out = RenderedDocument {
        title: doc.title.code_meaning.clone(),
        ..Default::default()
    };
    if out.title.is_empty() {
        out.title = "Structured Report".into();
    }
    if let Some(contexts) = contexts {
        let ctx = contexts
            .get("1")
            .map(|c| c.concept_name.clone())
            .unwrap_or_default();
        out.title = context_code(&doc.title, &ctx, opts.show_codes);
        if out.title.is_empty() {
            out.title = "Structured Report".into();
        }
    }
    out.header.extend(opts.header.iter().cloned());
    let class = sop_class_name(&doc.sop_class_uid);
    if !class.is_empty() {
        out.header.push(field("Document type", class.into()));
    }
    if !doc.completion_flag.is_empty() {
        out.header
            .push(field("Completion", doc.completion_flag.clone()));
    }
    if !doc.verification_flag.is_empty() {
        out.header
            .push(field("Verification", doc.verification_flag.clone()));
    }
    if !doc.completion_flag_description.is_empty() {
        out.header.push(field(
            "Completion description",
            doc.completion_flag_description.clone(),
        ));
    }
    let r = Renderer {
        opts,
        contexts: None,
    };
    for (i, o) in doc.verifying_observers.iter().enumerate() {
        if i as i64 >= opts.max_items {
            out.header.push(field(
                "Verifying observers omitted",
                (doc.verifying_observers.len() - i).to_string(),
            ));
            break;
        }
        let label = format!("Verifying observer {}", i + 1);
        for f in [
            field(&label, crate::personname::display(&o.name)),
            field(&format!("{label} organization"), o.organization.clone()),
            field(
                &format!("{label} date/time"),
                format_date_time(&o.date_time),
            ),
            field(&format!("{label} identification"), r.code(&o.code)),
        ] {
            if !f.value.is_empty() {
                out.header.push(f);
            }
        }
    }
    for (i, p) in doc.predecessor_documents.iter().enumerate() {
        if i as i64 >= opts.max_items {
            out.header.push(field(
                "Predecessor documents omitted",
                (doc.predecessor_documents.len() - i).to_string(),
            ));
            break;
        }
        let label = format!("Predecessor document {}", i + 1);
        for f in [
            field(&label, reference(&p.sop_class_uid, &p.sop_instance_uid)),
            field(&format!("{label} study UID"), p.study_instance_uid.clone()),
            field(
                &format!("{label} series UID"),
                p.series_instance_uid.clone(),
            ),
        ] {
            if !f.value.is_empty() {
                out.header.push(f);
            }
        }
    }
    if !doc.content_date.is_empty() || !doc.content_time.is_empty() {
        out.header.push(field(
            "Content date/time",
            format!(
                "{} {}",
                format_date(&doc.content_date),
                format_time(&doc.content_time)
            )
            .trim()
            .to_string(),
        ));
    }
    if !doc.sop_instance_uid.is_empty() {
        out.header
            .push(field("SOP Instance UID", doc.sop_instance_uid.clone()));
    }
    out
}
struct Renderer<'a> {
    opts: &'a RenderOptions,
    contexts: Option<&'a Contexts>,
}
impl Renderer<'_> {
    fn items(&self, out: &mut RenderedDocument, items: &[ContentItem], prefix: &str, depth: i64) {
        for (i, item) in items.iter().enumerate() {
            let path = format!("{prefix}{}", i + 1);
            if out.items.len() as i64 >= self.opts.max_items {
                out.truncated += 1;
                if item.referenced_content_item_identifier.is_empty() {
                    out.truncated += count_items(&item.children);
                }
                continue;
            }
            let mut rendered = RenderedItem {
                path: path.clone(),
                depth,
                relationship: item.relationship_type.to_lowercase(),
                value_type: item.value_type.clone(),
                ..Default::default()
            };
            let target = identifier_string(&item.referenced_content_item_identifier);
            if !target.is_empty() {
                rendered.reference_to = target;
                rendered.value_type.clear();
            } else {
                if item.value_type == value::CONTAINER {
                    rendered.continuity = item.continuity_of_content.clone();
                }
                rendered.concept = self.code(&item.concept_name);
                rendered.value = self.value(item);
                if let Some(contexts) = self.contexts {
                    let c = contexts.get(&path).cloned().unwrap_or_default();
                    rendered.concept =
                        context_code(&item.concept_name, &c.concept_name, self.opts.show_codes);
                    rendered.value = self.context_value(item, &c);
                }
            }
            let expand = rendered.reference_to.is_empty();
            out.items.push(rendered);
            if expand {
                self.items(out, &item.children, &format!("{path}."), depth + 1);
            }
        }
    }
    fn code(&self, code: &CodedEntry) -> String {
        let mut meaning = code.code_meaning.trim().to_string();
        if meaning.is_empty() {
            meaning = code.code_value.trim().to_string();
        }
        if self.opts.show_codes && !code.code_value.is_empty() {
            let _ = write!(
                meaning,
                " ({}, {})",
                code.code_value, code.coding_scheme_designator
            );
        }
        meaning
    }
    fn value(&self, item: &ContentItem) -> String {
        match item.value_type.as_str() {
            value::TEXT => item.text.clone(),
            value::CODE => self.code(&item.code),
            value::NUM => {
                let Some(m) = &item.measurement else {
                    if item.numeric_value_qualifier.empty() {
                        return "no value".into();
                    }
                    return self.code(&item.numeric_value_qualifier);
                };
                let units = &m.units;
                let mut unit = units.code_meaning.clone();
                if units.coding_scheme_designator.eq_ignore_ascii_case("UCUM")
                    && !units.code_value.is_empty()
                {
                    unit = units.code_value.clone();
                    if unit == "1" || unit == "{ratio}" {
                        unit.clear();
                    }
                }
                format!("{} {unit}", format_float(m.value, 'g', -1))
                    .trim()
                    .to_string()
            }
            value::DATETIME => format_date_time(&item.date_time),
            value::DATE => format_date(&item.date),
            value::TIME => format_time(&item.time),
            value::UIDREF => item.uid.clone(),
            value::PNAME => crate::personname::display(&item.person_name),
            value::IMAGE => {
                let mut v = reference(&item.image.sop_class_uid, &item.image.sop_instance_uid);
                if !item.image.frames.is_empty() {
                    let frames: Vec<String> =
                        item.image.frames.iter().map(i64::to_string).collect();
                    let _ = write!(v, ", frames {}", frames.join(","));
                }
                v
            }
            value::COMPOSITE => reference(
                &item.composite.sop_class_uid,
                &item.composite.sop_instance_uid,
            ),
            value::WAVEFORM => {
                let mut v = reference(
                    &item.waveform.composite.sop_class_uid,
                    &item.waveform.composite.sop_instance_uid,
                );
                let n = item.waveform.channels.len();
                if n > 0 {
                    let _ = write!(v, ", {n} channels");
                }
                v
            }
            value::SCOORD => {
                let points: Vec<String> = item
                    .spatial
                    .coordinates
                    .iter()
                    .map(|p| format!("({},{})", g(p.x), g(p.y)))
                    .collect();
                format!("{} {}", item.spatial.graphic_type, self.points(&points))
                    .trim()
                    .to_string()
            }
            value::SCOORD3D => {
                let s = &item.spatial3d;
                let points: Vec<String> = s
                    .coordinates
                    .iter()
                    .map(|p| format!("({},{},{})", g(p.x), g(p.y), g(p.z)))
                    .collect();
                let mut v = format!("{} {}", s.graphic_type, self.points(&points))
                    .trim()
                    .to_string();
                if !s.frame_of_reference_uid.is_empty() {
                    let _ = write!(v, " in {}", s.frame_of_reference_uid);
                }
                v
            }
            value::TCOORD => {
                let t = &item.temporal;
                let mut points: Vec<String> =
                    t.sample_positions.iter().map(u32::to_string).collect();
                points.extend(
                    t.time_offsets
                        .iter()
                        .map(|p| format!("{} s", format_float(*p, 'g', -1))),
                );
                points.extend(t.date_times.iter().map(|p| format_date_time(p)));
                format!("{} {}", t.range_type, self.points(&points))
                    .trim()
                    .to_string()
            }
            value::CONTAINER => String::new(),
            other => format!("({other} value not rendered)"),
        }
    }
    fn context_value(&self, item: &ContentItem, c: &ContentItemCodeContexts) -> String {
        let show = self.opts.show_codes;
        match item.value_type.as_str() {
            value::CODE => context_code(&item.code, &c.value, show),
            value::NUM => {
                let Some(m) = &item.measurement else {
                    if item.numeric_value_qualifier.empty() {
                        return "no value".into();
                    }
                    return context_code(
                        &item.numeric_value_qualifier,
                        &c.numeric_value_qualifier,
                        show,
                    );
                };
                let units = &m.units;
                let mut unit = context_code(units, &c.measurement_units, show);
                if !show
                    && units.coding_scheme_designator.eq_ignore_ascii_case("UCUM")
                    && !units.code_value.is_empty()
                {
                    unit = units.code_value.clone();
                    if unit == "1" || unit == "{ratio}" {
                        unit.clear();
                    }
                }
                let mut v = format!("{} {unit}", format_float(m.value, 'g', -1))
                    .trim()
                    .to_string();
                if show && !item.numeric_value_qualifier.empty() {
                    let _ = write!(
                        v,
                        " [qualifier: {}]",
                        context_code(
                            &item.numeric_value_qualifier,
                            &c.numeric_value_qualifier,
                            true
                        )
                    );
                }
                v
            }
            _ => self.value(item),
        }
    }
    fn points(&self, points: &[String]) -> String {
        if points.len() as i64 > self.opts.max_points {
            let shown = &points[..self.opts.max_points as usize];
            return format!("{} ... ({} points)", shown.join(" "), points.len());
        }
        points.join(" ")
    }
}
fn g(v: f64) -> String {
    format_float(v, 'g', -1)
}
fn count_items(items: &[ContentItem]) -> i64 {
    let mut n = items.len() as i64;
    for item in items {
        if item.referenced_content_item_identifier.is_empty() {
            n += count_items(&item.children);
        }
    }
    n
}
pub(crate) fn context_code(code: &CodedEntry, context: &CodeContext, show: bool) -> String {
    let mut v = code.code_value.as_str();
    if !context.long_code_value.is_empty() {
        v = &context.long_code_value;
    }
    if !context.urn_code_value.is_empty() {
        v = &context.urn_code_value;
    }
    let mut meaning = code.code_meaning.trim().to_string();
    if meaning.is_empty() {
        meaning = v.to_string();
    }
    if !show || v.is_empty() {
        return meaning;
    }
    let mut parts = vec![v];
    if !code.coding_scheme_designator.is_empty() {
        parts.push(&code.coding_scheme_designator);
    }
    if !context.coding_scheme_version.is_empty() {
        parts.push(&context.coding_scheme_version);
    }
    format!("{meaning} ({})", parts.join(", "))
}
fn reference(class: &str, instance: &str) -> String {
    let mut name = sop_class_name(class);
    if name.is_empty() {
        name = class;
    }
    format!("{name} {instance}").trim().to_string()
}
fn sop_class_name(class: &str) -> &'static str {
    crate::dictionary::uid::by_uid(class).map_or("", |e| e.name)
}
fn format_date_time(v: &str) -> String {
    let v = v.trim();
    if v.len() <= 8 {
        return format_date(v);
    }
    if !v.is_char_boundary(8) {
        let b = v.as_bytes();
        return format!(
            "{} {}",
            format_date(&String::from_utf8_lossy(&b[..8])),
            format_time(&String::from_utf8_lossy(&b[8..]))
        )
        .trim()
        .to_string();
    }
    format!("{} {}", format_date(&v[..8]), format_time(&v[8..]))
        .trim()
        .to_string()
}
pub fn render_text(doc: Option<&Document>, opts: &RenderOptions) -> Result<String, SrError> {
    Ok(text_of(&render_items(doc, opts)?))
}
pub(crate) fn text_of(r: &RenderedDocument) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}", text_safe(&r.title));
    for f in &r.header {
        let _ = writeln!(out, "{}: {}", text_safe(&f.label), text_safe(&f.value));
    }
    out.push('\n');
    for item in &r.items {
        let indent = "  ".repeat(item.depth.max(0) as usize);
        let mut line = format!("{} {}", item.path, item.relationship);
        if !item.reference_to.is_empty() {
            let _ = write!(line, " -> {}", item.reference_to);
        } else {
            let _ = write!(line, " {}", item.value_type);
            if !item.concept.is_empty() {
                let _ = write!(line, " {}", text_safe(&item.concept));
            }
            if !item.value.is_empty() {
                let v = text_safe(&item.value).replace('\n', &format!("\n{indent}    "));
                let _ = write!(line, ": {v}");
            }
        }
        let _ = writeln!(out, "{indent}{}", line.trim());
    }
    if r.truncated > 0 {
        let _ = writeln!(out, "... {} more content items", r.truncated);
    }
    out
}
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}
pub(crate) fn text_safe(s: &str) -> String {
    let mut b = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\n' || c == '\t' {
            b.push(c);
        } else if c.is_control() || c == '\u{2028}' || c == '\u{2029}' || is_bidi_control(c) {
            let _ = write!(b, "\\x{:02x}", c as u32);
        } else {
            b.push(c);
        }
    }
    b
}
fn html_escape(s: &str) -> String {
    let mut b = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => b.push_str("&lt;"),
            '>' => b.push_str("&gt;"),
            '&' => b.push_str("&amp;"),
            '\'' => b.push_str("&#39;"),
            '"' => b.push_str("&#34;"),
            _ => b.push(c),
        }
    }
    b
}
fn e(s: &str) -> String {
    html_escape(&text_safe(s))
}
const HTML_STYLE: &str = "body{font-family:sans-serif;margin:1.5em;line-height:1.4}table.header td{padding:0 1em 0 0;vertical-align:top}ul{list-style:none;padding-left:1.4em;margin:0}.rel{color:#666;font-size:smaller}.vt{color:#666;font-size:smaller}.concept{font-weight:bold}.value{white-space:pre-wrap}";
pub fn render_html(doc: Option<&Document>, opts: &RenderOptions) -> Result<String, SrError> {
    Ok(html_of(&render_items(doc, opts)?))
}
pub(crate) fn html_of(r: &RenderedDocument) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>{HTML_STYLE}</style>\n</head>\n<body>\n",
        e(&r.title)
    );
    let _ = writeln!(out, "<h1>{}</h1>", e(&r.title));
    if !r.header.is_empty() {
        out.push_str("<table class=\"header\">\n");
        for f in &r.header {
            let _ = writeln!(
                out,
                "<tr><td>{}</td><td>{}</td></tr>",
                e(&f.label),
                e(&f.value)
            );
        }
        out.push_str("</table>\n");
    }
    if !r.items.is_empty() {
        out.push_str("<ul>\n");
    }
    for (i, item) in r.items.iter().enumerate() {
        let _ = write!(
            out,
            "<li id=\"item-{}\"><span class=\"rel\">{}</span> ",
            item.path,
            e(&item.relationship)
        );
        if !item.reference_to.is_empty() {
            let _ = write!(
                out,
                "<a href=\"#item-{}\">{}</a>",
                path_only(&item.reference_to),
                e(&item.reference_to)
            );
        } else {
            let _ = write!(out, "<span class=\"vt\">{}</span>", e(&item.value_type));
            if !item.concept.is_empty() {
                let _ = write!(out, " <span class=\"concept\">{}</span>", e(&item.concept));
            }
            if !item.value.is_empty() {
                let _ = write!(out, ": <span class=\"value\">{}</span>", e(&item.value));
            }
        }
        let next = r.items.get(i + 1).map_or(-1, |n| n.depth);
        if next > item.depth {
            out.push_str("\n<ul>\n");
            continue;
        }
        out.push_str("</li>\n");
        let mut depth = item.depth;
        while depth > next {
            out.push_str("</ul>\n");
            if depth > 0 {
                out.push_str("</li>\n");
            }
            depth -= 1;
        }
    }
    if r.truncated > 0 {
        let _ = writeln!(out, "<p>{} more content items</p>", r.truncated);
    }
    out.push_str("</body>\n</html>\n");
    out
}
fn path_only(path: &str) -> String {
    path.chars()
        .filter(|c| *c == '.' || c.is_ascii_digit())
        .collect()
}
pub const PRESENTATION_SECTION: &str = "section";
pub const PRESENTATION_NARRATIVE: &str = "narrative";
pub const PRESENTATION_ITEM: &str = "item";
pub const PRESENTATION_INLINE: &str = "inline";
pub const PRESENTATION_REFERENCE: &str = "reference";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresentationBlock {
    pub path: String,
    pub kind: String,
    pub relationship: String,
    pub depth: i64,
    pub value_type: String,
    pub concept: String,
    pub value: String,
    pub reference_to: String,
    pub in_narrative: bool,
    pub continuity: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresentationDocument {
    pub title: String,
    pub header: Vec<RenderField>,
    pub blocks: Vec<PresentationBlock>,
    pub continuity: String,
    pub truncated: i64,
}
pub fn render_presentation(
    doc: Option<&Document>,
    opts: &RenderOptions,
) -> Result<PresentationDocument, SrError> {
    render_presentation_with_contexts(doc, opts, None)
}
pub(crate) fn render_presentation_with_contexts(
    doc: Option<&Document>,
    opts: &RenderOptions,
    contexts: Option<&Contexts>,
) -> Result<PresentationDocument, SrError> {
    let Some(doc) = doc else {
        return Err(nil_document());
    };
    let opts = defaults(opts.clone());
    let r = Renderer {
        opts: &opts,
        contexts,
    };
    let mut header = render_header(doc, &opts, contexts);
    for f in header.header.iter_mut().skip(opts.header.len()) {
        if f.label == "Verification" {
            f.label = "Document verification".into();
        }
    }
    if contexts.is_none() {
        let title = r.code(&doc.title);
        if !title.is_empty() {
            header.title = title;
        }
    }
    let mut out = PresentationDocument {
        title: header.title,
        header: header.header,
        continuity: doc.continuity_of_content.clone(),
        ..Default::default()
    };
    struct Frame<'a> {
        items: &'a [ContentItem],
        index: usize,
        prefix: String,
        continuity: String,
    }
    let mut stack = vec![Frame {
        items: &doc.content,
        index: 0,
        prefix: "1.".into(),
        continuity: doc.continuity_of_content.clone(),
    }];
    while let Some(frame) = stack.last_mut() {
        if frame.index == frame.items.len() {
            stack.pop();
            continue;
        }
        let item = &frame.items[frame.index];
        frame.index += 1;
        let path = format!("{}{}", frame.prefix, frame.index);
        let frame_continuity = frame.continuity.clone();
        let depth = stack.len() as i64 - 1;
        let by_reference = !item.referenced_content_item_identifier.is_empty();
        if out.blocks.len() as i64 == opts.max_items {
            out.truncated += 1;
        } else {
            let mut block = PresentationBlock {
                path: path.clone(),
                depth,
                kind: PRESENTATION_ITEM.into(),
                relationship: item.relationship_type.to_lowercase(),
                value_type: item.value_type.clone(),
                in_narrative: frame_continuity == "CONTINUOUS"
                    && item.relationship_type == RELATIONSHIP_CONTAINS
                    && (by_reference || item.value_type != value::CONTAINER),
                ..Default::default()
            };
            if by_reference {
                block.kind = PRESENTATION_REFERENCE.into();
                block.reference_to = identifier_string(&item.referenced_content_item_identifier);
                block.value_type.clear();
            } else if item.value_type == value::CONTAINER {
                block.kind = PRESENTATION_SECTION.into();
                block.continuity = item.continuity_of_content.clone();
                if item.continuity_of_content == "CONTINUOUS" {
                    block.kind = PRESENTATION_NARRATIVE.into();
                }
            } else if block.in_narrative {
                block.kind = PRESENTATION_INLINE.into();
            }
            if !by_reference {
                block.concept = r.code(&item.concept_name);
                block.value = r.value(item);
                if let Some(contexts) = contexts {
                    let c = contexts.get(&path).cloned().unwrap_or_default();
                    block.concept =
                        context_code(&item.concept_name, &c.concept_name, opts.show_codes);
                    block.value = r.context_value(item, &c);
                }
            }
            out.blocks.push(block);
        }
        if !by_reference && !item.children.is_empty() {
            if stack.len() >= 64 {
                return Err(SrError::new(
                    Kind::ResourceLimitExceeded,
                    ": SR presentation depth",
                ));
            }
            let continuity = if item.value_type == value::CONTAINER {
                item.continuity_of_content.clone()
            } else {
                String::new()
            };
            stack.push(Frame {
                items: &item.children,
                index: 0,
                prefix: format!("{path}."),
                continuity,
            });
        }
    }
    Ok(out)
}
pub fn render_presentation_text(
    doc: Option<&Document>,
    opts: &RenderOptions,
) -> Result<String, SrError> {
    Ok(presentation_text_of(&render_presentation(doc, opts)?))
}
pub(crate) fn presentation_text_of(p: &PresentationDocument) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}", text_safe(&p.title));
    for f in &p.header {
        let _ = writeln!(out, "{}: {}", text_safe(&f.label), text_safe(&f.value));
    }
    out.push('\n');
    let mut inline = false;
    for (i, block) in p.blocks.iter().enumerate() {
        let is_inline = block.in_narrative;
        if inline && (!is_inline || i == 0 || block.depth != p.blocks[i - 1].depth) {
            out.push('\n');
            inline = false;
        }
        let indent = "  ".repeat(block.depth.max(0) as usize);
        let mut v = text_safe(&presentation_value(block));
        if is_inline {
            if inline {
                out.push(' ');
            } else {
                out.push_str(&indent);
            }
            out.push_str(&v.replace('\n', &format!("\n{indent}")));
            inline = true;
            continue;
        }
        if (block.kind == PRESENTATION_SECTION || block.kind == PRESENTATION_NARRATIVE)
            && !block.continuity.is_empty()
        {
            let _ = write!(v, " [{}]", text_safe(&block.continuity));
        }
        if !block.relationship.is_empty() && block.relationship != "contains" {
            v = format!("[{}] {v}", text_safe(&block.relationship));
        }
        let _ = writeln!(out, "{indent}{}", v.replace('\n', &format!("\n{indent}")));
    }
    if inline {
        out.push('\n');
    }
    if p.truncated > 0 {
        let _ = writeln!(out, "... {} more content items", p.truncated);
    }
    out
}
fn presentation_value(block: &PresentationBlock) -> String {
    if !block.reference_to.is_empty() {
        return format!("Reference to {}", block.reference_to);
    }
    let mut v = block.concept.clone();
    if !block.value.is_empty() {
        if !v.is_empty() {
            v.push_str(": ");
        }
        v.push_str(&block.value);
    }
    v
}
const PRESENTATION_STYLE: &str = "body{font-family:sans-serif;max-width:70em;margin:2em auto;padding:0 1em;line-height:1.5}table.header td{padding:0 1em 0 0;vertical-align:top}section{margin:1em 0}section h2{font-size:1.2em}.children{margin-left:1.4em}.item{margin:.3em 0}.narrative{line-height:1.7}.rel{color:#666;font-size:smaller}.flow{margin:1em 0}.flow>.item{display:inline}.value{white-space:pre-wrap}.concept{font-weight:bold}";
pub fn render_presentation_html(
    doc: Option<&Document>,
    opts: &RenderOptions,
) -> Result<String, SrError> {
    Ok(presentation_html_of(&render_presentation(doc, opts)?))
}
pub(crate) fn presentation_html_of(p: &PresentationDocument) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title><style>{PRESENTATION_STYLE}</style></head><body>\n",
        e(&p.title)
    );
    let _ = writeln!(out, "<h1 id=\"item-1\">{}</h1>", e(&p.title));
    if !p.header.is_empty() {
        out.push_str("<table class=\"header\">\n");
        for f in &p.header {
            let _ = writeln!(
                out,
                "<tr><td>{}</td><td>{}</td></tr>",
                e(&f.label),
                e(&f.value)
            );
        }
        out.push_str("</table>\n");
    }
    level(&mut out, &p.blocks, 0, 0);
    if p.truncated > 0 {
        let _ = writeln!(out, "<p>{} more content items</p>", p.truncated);
    }
    out.push_str("</body></html>\n");
    out
}
fn level(out: &mut String, blocks: &[PresentationBlock], mut index: usize, depth: i64) -> usize {
    let mut inline = false;
    while index < blocks.len() && blocks[index].depth == depth {
        let block = &blocks[index];
        let is_inline = block.in_narrative;
        if inline && !is_inline {
            out.push_str("</div>\n");
            inline = false;
        }
        let section = block.kind == PRESENTATION_SECTION || block.kind == PRESENTATION_NARRATIVE;
        if section {
            let _ = writeln!(
                out,
                "<section class=\"{}\" id=\"item-{}\">",
                block.kind, block.path
            );
            if !block.relationship.is_empty() && block.relationship != "contains" {
                let _ = writeln!(out, "<span class=\"rel\">{}</span>", e(&block.relationship));
            }
            if !block.concept.is_empty() {
                let _ = writeln!(out, "<h2>{}</h2>", e(&block.concept));
            }
        } else if is_inline {
            if !inline {
                out.push_str("<div class=\"narrative flow\" role=\"paragraph\">");
                inline = true;
            } else {
                out.push(' ');
            }
            block_value(out, block);
        } else {
            block_value(out, block);
            out.push('\n');
        }
        index += 1;
        if index < blocks.len() && blocks[index].depth > depth {
            out.push_str("<div class=\"children\">\n");
            index = level(out, blocks, index, depth + 1);
            out.push_str("</div>\n");
        }
        if section {
            out.push_str("</section>\n");
        } else {
            out.push_str("</div>\n");
        }
    }
    if inline {
        out.push_str("</div>\n");
    }
    index
}
fn block_value(out: &mut String, block: &PresentationBlock) {
    let _ = write!(out, "<div class=\"item\" id=\"item-{}\">", block.path);
    if !block.relationship.is_empty() && block.relationship != "contains" {
        let _ = write!(
            out,
            "<span class=\"rel\">{}</span> ",
            e(&block.relationship)
        );
    }
    if !block.reference_to.is_empty() {
        let _ = write!(
            out,
            "<a href=\"#item-{}\">Reference to {}</a>",
            path_only(&block.reference_to),
            e(&block.reference_to)
        );
    } else {
        if !block.concept.is_empty() {
            let _ = write!(out, "<span class=\"concept\">{}</span>", e(&block.concept));
            if !block.value.is_empty() {
                out.push_str(": ");
            }
        }
        if !block.value.is_empty() {
            let _ = write!(out, "<span class=\"value\">{}</span>", e(&block.value));
        }
    }
}
