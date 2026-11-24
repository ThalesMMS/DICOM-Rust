use super::payload::{
    CDA_NAMESPACE, PayloadLimits, cda_identifier_attributes, inspect_payload, xml_decoder,
    xml_error_text,
};
use super::{Error, Kind, MIME_TYPE_CDA, Result, compare_cda_identifiers};
use crate::dicomxml::xmlwire::{Attr, Name, Tok, XmlError};
fn invalid() -> Error {
    Error::sentinel(Kind::InvalidDocument)
}
fn too_large() -> Error {
    Error::sentinel(Kind::TooLarge)
}
fn unsupported(what: &str) -> Error {
    Error::wrap(Kind::CdaNarrativeUnsupported, what)
}
pub(super) fn cda_instance_identifier(data: &[u8]) -> Result<String> {
    let mut decoder = xml_decoder(data);
    let mut depth = 0;
    let (mut has_root, mut in_document, mut has_identifier) = (false, false, false);
    let mut namespace = String::new();
    let mut identifier = String::new();
    loop {
        let token = match decoder.token() {
            Ok(t) => t,
            Err(XmlError::Eof) => break,
            Err(e) => {
                return Err(Error::wrap(
                    Kind::InvalidDocument,
                    &format!("CDA is not XML: {}", xml_error_text(&e)),
                ));
            }
        };
        match token {
            Tok::Start(name, attrs) => {
                depth += 1;
                if depth == 1 {
                    has_root = true;
                    in_document = name.local == "ClinicalDocument";
                    namespace = name.space.clone();
                }
                if in_document && depth == 2 && name.local == "id" && name.space == namespace {
                    if has_identifier {
                        return Err(invalid());
                    }
                    has_identifier = true;
                    let (root, extension) = cda_identifier_attributes(&attrs)?;
                    identifier = root.trim().to_string();
                    if !identifier.is_empty() && !extension.is_empty() {
                        identifier.push('^');
                        identifier.push_str(&extension);
                    }
                }
            }
            Tok::End(_) => depth -= 1,
            _ => {}
        }
    }
    if !has_root {
        return Err(Error::wrap(
            Kind::InvalidDocument,
            "CDA has no root element",
        ));
    }
    Ok(identifier)
}
#[derive(Clone, Debug)]
enum Part {
    Node(usize),
    Text(String),
}
#[derive(Clone, Debug)]
struct Node {
    name: Name,
    attrs: Vec<Attr>,
    parts: Vec<Part>,
}
struct Tree {
    nodes: Vec<Node>,
}
impl Tree {
    fn is(&self, n: usize, name: &str) -> bool {
        let node = &self.nodes[n];
        node.name.space == CDA_NAMESPACE && node.name.local == name
    }
    fn attribute(&self, n: usize, name: &str) -> String {
        self.nodes[n]
            .attrs
            .iter()
            .find(|a| a.name.space.is_empty() && a.name.local == name)
            .map(|a| a.value.clone())
            .unwrap_or_default()
    }
    fn child_nodes(&self, n: usize) -> impl Iterator<Item = usize> + '_ {
        self.nodes[n].parts.iter().filter_map(|p| match p {
            Part::Node(c) => Some(*c),
            Part::Text(_) => None,
        })
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CdaNarrativeLimits {
    pub max_bytes: i64,
    pub max_depth: i64,
    pub max_tokens: i64,
    pub max_items: i64,
    pub max_text_bytes: i64,
}
pub fn default_cda_narrative_limits() -> CdaNarrativeLimits {
    CdaNarrativeLimits {
        max_bytes: 16 << 20,
        max_depth: 64,
        max_tokens: 100_000,
        max_items: 10_000,
        max_text_bytes: 2 << 20,
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CdaNarrative {
    pub title: String,
    pub hl7_instance_identifier: String,
    pub plain_text: String,
    pub blocks: Vec<CdaNarrativeBlock>,
    pub diagnostics: Vec<CdaNarrativeDiagnostic>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CdaNarrativeBlock {
    pub kind: &'static str,
    pub depth: i64,
    pub text: String,
    pub ordered: bool,
    pub number: i64,
    pub cells: Option<Vec<String>>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CdaNarrativeDiagnostic {
    pub code: &'static str,
    pub count: i64,
}
pub const CDA_NARRATIVE_SECTION: &str = "section";
pub const CDA_NARRATIVE_PARAGRAPH: &str = "paragraph";
pub const CDA_NARRATIVE_LIST: &str = "list";
pub const CDA_NARRATIVE_LIST_ITEM: &str = "list-item";
pub const CDA_NARRATIVE_TABLE: &str = "table";
pub const CDA_NARRATIVE_CAPTION: &str = "caption";
pub const CDA_NARRATIVE_TABLE_ROW: &str = "table-row";
fn normalize_cda_narrative_limits(mut l: CdaNarrativeLimits) -> Result<CdaNarrativeLimits> {
    let d = default_cda_narrative_limits();
    if l.max_bytes == 0 {
        l.max_bytes = d.max_bytes;
    }
    if l.max_depth == 0 {
        l.max_depth = d.max_depth;
    }
    if l.max_tokens == 0 {
        l.max_tokens = d.max_tokens;
    }
    if l.max_items == 0 {
        l.max_items = d.max_items;
    }
    if l.max_text_bytes == 0 {
        l.max_text_bytes = d.max_text_bytes;
    }
    if l.max_bytes < 1
        || l.max_bytes > 256 << 20
        || l.max_depth < 1
        || l.max_depth > 64
        || l.max_tokens < 1
        || l.max_tokens > 1_000_000
        || l.max_items < 1
        || l.max_items > 1_000_000
        || l.max_text_bytes < 1
        || l.max_text_bytes > 64 << 20
    {
        return Err(invalid());
    }
    Ok(l)
}
pub fn parse_cda_narrative(data: &[u8], limits: CdaNarrativeLimits) -> Result<CdaNarrative> {
    parse(data, "", limits)
}
pub fn parse_encapsulated_cda_narrative(
    data: &[u8],
    declared_identifier: &str,
    limits: CdaNarrativeLimits,
) -> Result<CdaNarrative> {
    parse(data, declared_identifier, limits)
}
fn parse(data: &[u8], declared: &str, limits: CdaNarrativeLimits) -> Result<CdaNarrative> {
    let limits = normalize_cda_narrative_limits(limits)?;
    let info = inspect_payload(
        MIME_TYPE_CDA,
        data,
        PayloadLimits {
            max_bytes: limits.max_bytes,
            max_depth: limits.max_depth,
            max_records: limits.max_tokens,
        },
    )?;
    compare_cda_identifiers(declared, &info.hl7_instance_identifier)?;
    let (tree, root) = read_xml(data, &limits, false)?;
    let Some(root) = root else {
        return Err(invalid());
    };
    let mut p = Parser {
        tree: &tree,
        limits,
        out: CdaNarrative {
            hl7_instance_identifier: info.hl7_instance_identifier.clone(),
            ..CdaNarrative::default()
        },
        items: 0,
        text_bytes: 0,
        readable: false,
    };
    p.charge_text(&info.hl7_instance_identifier)?;
    let (mut titles, mut bodies) = (0, 0);
    for node in tree.child_nodes(root) {
        if tree.is(node, "title") {
            titles += 1;
            if titles > 1 {
                return Err(invalid());
            }
            p.audit(node)?;
            let title = p.inline_text(&tree.nodes[node].parts)?;
            p.charge_text(&title)?;
            p.out.title = title;
        } else if tree.is(node, "component") {
            for child in tree.child_nodes(node) {
                if tree.is(child, "nonXMLBody") {
                    return Err(unsupported("nonXML body"));
                }
                if tree.is(child, "structuredBody") {
                    bodies += 1;
                    if bodies > 1 {
                        return Err(invalid());
                    }
                    p.sections(child, 0)?;
                }
            }
        }
    }
    if !p.readable {
        return Err(Error::sentinel(Kind::CdaNarrativeAbsent));
    }
    p.format()?;
    Ok(p.out)
}
fn read_xml(
    data: &[u8],
    limits: &CdaNarrativeLimits,
    reject_duplicate_attrs: bool,
) -> Result<(Tree, Option<usize>)> {
    let mut decoder = xml_decoder(data);
    let mut tree = Tree { nodes: Vec::new() };
    let mut root = None;
    let mut stack: Vec<usize> = Vec::new();
    let mut tokens: i64 = 0;
    loop {
        let token = match decoder.token() {
            Ok(t) => t,
            Err(XmlError::Eof) => return Ok((tree, root)),
            Err(_) => return Err(invalid()),
        };
        if tokens >= limits.max_tokens {
            return Err(too_large());
        }
        match token {
            Tok::Start(name, attrs) => {
                if stack.len() as i64 >= limits.max_depth {
                    return Err(too_large());
                }
                if reject_duplicate_attrs && attrs.len() > 1 {
                    let mut seen: Vec<&Name> = Vec::new();
                    for a in &attrs {
                        if seen.contains(&&a.name) {
                            return Err(invalid());
                        }
                        seen.push(&a.name);
                    }
                }
                let index = tree.nodes.len();
                tree.nodes.push(Node {
                    name,
                    attrs,
                    parts: Vec::new(),
                });
                match stack.last() {
                    None => root = Some(index),
                    Some(&parent) => tree.nodes[parent].parts.push(Part::Node(index)),
                }
                stack.push(index);
            }
            Tok::End(_) => {
                stack.pop();
            }
            Tok::CharData(text) => {
                if let Some(&top) = stack.last() {
                    tree.nodes[top]
                        .parts
                        .push(Part::Text(String::from_utf8_lossy(&text).into_owned()));
                }
            }
            Tok::Directive(_) => return Err(invalid()),
            Tok::ProcInst(target, _) => {
                if target != "xml" {
                    return Err(unsupported("XML processing instruction"));
                }
                if tokens != 0 {
                    return Err(invalid());
                }
            }
            Tok::Comment(_) => {}
        }
        tokens += 1;
    }
}
struct Parser<'a> {
    tree: &'a Tree,
    limits: CdaNarrativeLimits,
    out: CdaNarrative,
    items: i64,
    text_bytes: i64,
    readable: bool,
}
struct TextBuilder {
    limit: i64,
    text: String,
    last: char,
}
fn is_bidi_control(r: char) -> bool {
    matches!(r, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}
impl TextBuilder {
    fn append(&mut self, value: &str) -> Result<()> {
        if value.len() as i64 > self.limit - self.text.len() as i64 {
            return Err(too_large());
        }
        self.text.push_str(value);
        Ok(())
    }
    fn write(&mut self, value: &str) -> Result<()> {
        for r in value.chars() {
            let (part, r) =
                if is_bidi_control(r) || r.is_control() && r != '\n' && r != '\r' && r != '\t' {
                    (format!("\\x{:02x}", r as u32), r)
                } else if r.is_whitespace() {
                    if self.last == ' ' || self.last == '\n' || self.text.is_empty() {
                        continue;
                    }
                    (" ".to_string(), ' ')
                } else {
                    (r.to_string(), r)
                };
            self.append(&part)?;
            self.last = r;
        }
        Ok(())
    }
    fn line_break(&mut self) -> Result<()> {
        self.append("\n")?;
        self.last = '\n';
        Ok(())
    }
    fn value(&self) -> String {
        let lines: Vec<&str> = self.text.split('\n').map(str::trim).collect();
        lines.join("\n").trim().to_string()
    }
}
impl Parser<'_> {
    fn text_builder(&self) -> TextBuilder {
        TextBuilder {
            limit: self.limits.max_text_bytes,
            text: String::new(),
            last: '\0',
        }
    }
    fn charge_text(&mut self, text: &str) -> Result<()> {
        if text.len() as i64 > self.limits.max_text_bytes - self.text_bytes {
            return Err(too_large());
        }
        self.text_bytes += text.len() as i64;
        Ok(())
    }
    fn block(&mut self, block: CdaNarrativeBlock) -> Result<()> {
        let cells = block.cells.as_deref().unwrap_or_default();
        let items = 1 + cells.len() as i64;
        if items > self.limits.max_items - self.items {
            return Err(too_large());
        }
        self.charge_text(&block.text)?;
        for cell in cells {
            self.charge_text(cell)?;
            self.readable = self.readable || !cell.trim().is_empty();
        }
        self.items += items;
        if block.kind != CDA_NARRATIVE_SECTION
            && block.kind != CDA_NARRATIVE_CAPTION
            && !block.text.trim().is_empty()
        {
            self.readable = true;
        }
        self.out.blocks.push(block);
        Ok(())
    }
    fn diagnostic(&mut self, code: &'static str) {
        for d in &mut self.out.diagnostics {
            if d.code == code {
                d.count += 1;
                return;
            }
        }
        self.out
            .diagnostics
            .push(CdaNarrativeDiagnostic { code, count: 1 });
    }
    fn simple(kind: &'static str, depth: i64, text: String) -> CdaNarrativeBlock {
        CdaNarrativeBlock {
            kind,
            depth,
            text,
            ..CdaNarrativeBlock::default()
        }
    }
    fn sections(&mut self, container: usize, depth: i64) -> Result<()> {
        let tree = self.tree;
        for part in tree.child_nodes(container) {
            if !tree.is(part, "component") {
                continue;
            }
            for child in tree.child_nodes(part) {
                if tree.is(child, "section") {
                    self.section(child, depth)?;
                }
            }
        }
        Ok(())
    }
    fn section(&mut self, node: usize, depth: i64) -> Result<()> {
        let tree = self.tree;
        let mut title = String::new();
        let (mut titles, mut texts) = (0, 0);
        for part in tree.child_nodes(node) {
            if tree.is(part, "title") {
                titles += 1;
                if titles > 1 {
                    return Err(invalid());
                }
                self.audit(part)?;
                title = self.inline_text(&tree.nodes[part].parts)?;
            }
        }
        self.block(Self::simple(CDA_NARRATIVE_SECTION, depth, title))?;
        for part in tree.child_nodes(node) {
            if tree.is(part, "text") {
                texts += 1;
                if texts > 1 {
                    return Err(invalid());
                }
                self.audit(part)?;
                self.flow(&tree.nodes[part].parts, depth + 1)?;
            } else if tree.is(part, "component") {
                for child in tree.child_nodes(part) {
                    if tree.is(child, "section") {
                        self.section(child, depth + 1)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn flow(&mut self, parts: &[Part], depth: i64) -> Result<()> {
        let tree = self.tree;
        let mut text = self.text_builder();
        for part in parts {
            let child = match part {
                Part::Text(s) => {
                    text.write(s)?;
                    continue;
                }
                Part::Node(c) => *c,
            };
            if tree.is(child, "paragraph")
                || tree.is(child, "caption")
                || tree.is(child, "list")
                || tree.is(child, "table")
            {
                let value = text.value();
                if !value.is_empty() {
                    self.block(Self::simple(CDA_NARRATIVE_PARAGRAPH, depth, value))?;
                }
                text = self.text_builder();
                if tree.is(child, "list") {
                    self.list(child, depth)?;
                } else if tree.is(child, "table") {
                    self.table(child, depth)?;
                } else {
                    let kind = if tree.is(child, "caption") {
                        CDA_NARRATIVE_CAPTION
                    } else {
                        CDA_NARRATIVE_PARAGRAPH
                    };
                    let value = self.inline_text(&tree.nodes[child].parts)?;
                    if !value.is_empty() {
                        self.block(Self::simple(kind, depth, value))?;
                    }
                }
            } else {
                self.inline(child, &mut text)?;
            }
        }
        let value = text.value();
        if !value.is_empty() {
            self.block(Self::simple(CDA_NARRATIVE_PARAGRAPH, depth, value))?;
        }
        Ok(())
    }
    fn list(&mut self, node: usize, depth: i64) -> Result<()> {
        let tree = self.tree;
        let list_type = tree.attribute(node, "listType");
        if !list_type.is_empty() && list_type != "ordered" && list_type != "unordered" {
            return Err(unsupported("list type"));
        }
        let ordered = list_type == "ordered";
        self.block(CdaNarrativeBlock {
            kind: CDA_NARRATIVE_LIST,
            depth,
            ordered,
            ..CdaNarrativeBlock::default()
        })?;
        let mut number = 0;
        for part in &tree.nodes[node].parts {
            let child = match part {
                Part::Text(s) => {
                    if !s.trim().is_empty() {
                        return Err(invalid());
                    }
                    continue;
                }
                Part::Node(c) => *c,
            };
            if tree.is(child, "caption") {
                let value = self.inline_text(&tree.nodes[child].parts)?;
                self.block(Self::simple(CDA_NARRATIVE_CAPTION, depth + 1, value))?;
            } else if tree.is(child, "item") {
                number += 1;
                self.list_item(child, depth + 1, ordered, number)?;
            } else {
                self.diagnostic("unsupported-element-omitted");
            }
        }
        Ok(())
    }
    fn list_item(&mut self, node: usize, depth: i64, ordered: bool, number: i64) -> Result<()> {
        let tree = self.tree;
        let parts = &tree.nodes[node].parts;
        let cut = parts
            .iter()
            .position(|p| {
                matches!(p, Part::Node(c) if tree.is(*c, "paragraph") || tree.is(*c, "list") || tree.is(*c, "table"))
            })
            .unwrap_or(parts.len());
        let value = self.inline_text(&parts[..cut])?;
        self.block(CdaNarrativeBlock {
            kind: CDA_NARRATIVE_LIST_ITEM,
            depth,
            ordered,
            number,
            text: value,
            cells: None,
        })?;
        if cut < parts.len() {
            return self.flow(&parts[cut..], depth + 1);
        }
        Ok(())
    }
    fn table(&mut self, node: usize, depth: i64) -> Result<()> {
        self.block(Self::simple(CDA_NARRATIVE_TABLE, depth, String::new()))?;
        self.table_parts(node, depth + 1)
    }
    fn table_parts(&mut self, node: usize, depth: i64) -> Result<()> {
        let tree = self.tree;
        for part in &tree.nodes[node].parts {
            let child = match part {
                Part::Text(s) => {
                    if !s.trim().is_empty() {
                        return Err(invalid());
                    }
                    continue;
                }
                Part::Node(c) => *c,
            };
            if tree.is(child, "caption") {
                let value = self.inline_text(&tree.nodes[child].parts)?;
                self.block(Self::simple(CDA_NARRATIVE_CAPTION, depth, value))?;
            } else if tree.is(child, "thead") || tree.is(child, "tbody") || tree.is(child, "tfoot")
            {
                self.table_parts(child, depth)?;
            } else if tree.is(child, "tr") {
                let mut cells: Option<Vec<String>> = None;
                let mut row_bytes: i64 = 0;
                for cell in &tree.nodes[child].parts {
                    let cell = match cell {
                        Part::Text(s) => {
                            if !s.trim().is_empty() {
                                return Err(invalid());
                            }
                            continue;
                        }
                        Part::Node(c) => *c,
                    };
                    if !tree.is(cell, "th") && !tree.is(cell, "td") {
                        return Err(unsupported("table cell"));
                    }
                    let count = cells.as_ref().map_or(0, Vec::len) as i64;
                    if 1 + count >= self.limits.max_items - self.items {
                        return Err(too_large());
                    }
                    for name in ["rowspan", "colspan"] {
                        let value = tree.attribute(cell, name);
                        if !value.is_empty() && value != "1" {
                            return Err(unsupported("table span"));
                        }
                    }
                    let value = self.inline_text(&tree.nodes[cell].parts)?;
                    if value.len() as i64 > self.limits.max_text_bytes - self.text_bytes - row_bytes
                    {
                        return Err(too_large());
                    }
                    row_bytes += value.len() as i64;
                    cells.get_or_insert_with(Vec::new).push(value);
                }
                self.block(CdaNarrativeBlock {
                    kind: CDA_NARRATIVE_TABLE_ROW,
                    depth,
                    cells,
                    ..CdaNarrativeBlock::default()
                })?;
            } else if tree.is(child, "col") || tree.is(child, "colgroup") {
                self.diagnostic("table-layout-ignored");
            } else {
                self.diagnostic("unsupported-element-omitted");
            }
        }
        Ok(())
    }
    fn inline_text(&mut self, parts: &[Part]) -> Result<String> {
        let mut text = self.text_builder();
        self.inline_contents(parts, &mut text)?;
        Ok(text.value())
    }
    fn inline_contents(&mut self, parts: &[Part], text: &mut TextBuilder) -> Result<()> {
        for part in parts {
            match part {
                Part::Text(s) => text.write(s)?,
                Part::Node(c) => self.inline(*c, text)?,
            }
        }
        Ok(())
    }
    fn inline(&mut self, node: usize, text: &mut TextBuilder) -> Result<()> {
        let tree = self.tree;
        let n = &tree.nodes[node];
        if n.name.space != CDA_NAMESPACE {
            self.diagnostic("unsupported-element-omitted");
            return Ok(());
        }
        match n.name.local.as_str() {
            "content" | "caption" => match tree.attribute(node, "revised").as_str() {
                "delete" => {
                    self.diagnostic("deleted-content-suppressed");
                    Ok(())
                }
                "insert" => {
                    let inner = self.inline_text(&n.parts)?;
                    if inner.is_empty() {
                        return Ok(());
                    }
                    text.write("[inserted: ")?;
                    text.write(&inner)?;
                    text.write("]")
                }
                "" => self.inline_contents(&n.parts, text),
                _ => Err(unsupported("revision marker")),
            },
            "br" => text.line_break(),
            local @ ("sub" | "sup" | "footnote") => {
                let inner = self.inline_text(&n.parts)?;
                if inner.is_empty() {
                    return Ok(());
                }
                let (prefix, suffix) = match local {
                    "sup" => ("^(", ")"),
                    "footnote" => (" [footnote: ", "]"),
                    _ => ("_(", ")"),
                };
                text.write(prefix)?;
                text.write(&inner)?;
                text.write(suffix)
            }
            "linkHtml" => {
                self.diagnostic("local-link-inert");
                self.inline_contents(&n.parts, text)
            }
            "footnoteRef" => {
                self.diagnostic("footnote-reference-unresolved");
                Ok(())
            }
            "renderMultiMedia" | "script" | "iframe" | "object" | "img" => {
                Err(unsupported("multimedia or active content"))
            }
            "paragraph" => {
                text.line_break()?;
                self.inline_contents(&n.parts, text)?;
                text.line_break()
            }
            "list" | "table" => Err(unsupported("nested block in inline content")),
            _ => {
                self.diagnostic("unsupported-element-omitted");
                Ok(())
            }
        }
    }
    fn audit(&mut self, node: usize) -> Result<()> {
        let tree = self.tree;
        let n = &tree.nodes[node];
        if matches!(
            n.name.local.as_str(),
            "renderMultiMedia"
                | "script"
                | "iframe"
                | "object"
                | "img"
                | "style"
                | "video"
                | "audio"
                | "reference"
        ) {
            return Err(unsupported("multimedia or active content"));
        }
        if !tree.attribute(node, "styleCode").is_empty() {
            self.diagnostic("style-hints-ignored");
        }
        for a in &n.attrs {
            if matches!(
                a.name.local.as_str(),
                "href" | "src" | "background" | "codeBase"
            ) && (!tree.is(node, "linkHtml")
                || a.name.local != "href"
                || !a.value.starts_with('#'))
            {
                return Err(unsupported("external resource"));
            }
        }
        for child in tree.child_nodes(node) {
            self.audit(child)?;
        }
        Ok(())
    }
    fn format(&mut self) -> Result<()> {
        let mut text = String::new();
        for block in &self.out.blocks {
            let mut value = block.text.clone();
            match block.kind {
                CDA_NARRATIVE_LIST | CDA_NARRATIVE_TABLE => continue,
                CDA_NARRATIVE_LIST_ITEM => {
                    let marker = if block.ordered {
                        format!("{}.", block.number)
                    } else {
                        "•".to_string()
                    };
                    value = format!("{marker} {value}").trim().to_string();
                }
                CDA_NARRATIVE_TABLE_ROW => {
                    let cells: Vec<String> = block
                        .cells
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .map(|cell| {
                            if cell.contains(['|', '\n', '"', '\\']) {
                                crate::textfmt::quote(cell)
                            } else {
                                cell.clone()
                            }
                        })
                        .collect();
                    value = cells.join(" | ");
                }
                _ => {}
            }
            if value.is_empty() {
                continue;
            }
            let indent = "  ".repeat(block.depth.max(0) as usize);
            for line in value.split('\n') {
                if (indent.len() + line.len() + 1) as i64
                    > self.limits.max_text_bytes - text.len() as i64
                {
                    return Err(too_large());
                }
                text.push_str(&indent);
                text.push_str(line);
                text.push('\n');
            }
        }
        self.out.plain_text = text.trim().to_string();
        Ok(())
    }
}
pub const CDA_PROFILE_BASE_HEADER: &str = "base";
pub const CDA_PROFILE_CCD_HEADER: &str = "ccd";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CdaValidationOptions {
    pub profile: String,
    pub limits: PayloadLimits,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CdaValidationFinding {
    pub code: &'static str,
    pub path: String,
    pub count: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CdaValidationReport {
    pub profile: String,
    pub findings: Vec<CdaValidationFinding>,
}
impl CdaValidationReport {
    pub fn has_errors(&self) -> bool {
        !self.findings.is_empty()
    }
    fn add(&mut self, code: &'static str, path: &str) {
        for f in &mut self.findings {
            if f.code == code && f.path == path {
                f.count += 1;
                return;
            }
        }
        self.findings.push(CdaValidationFinding {
            code,
            path: path.to_string(),
            count: 1,
        });
    }
}
const HEADER_PATH: &str = "/ClinicalDocument";
const BODY_PATH: &str = "/ClinicalDocument/component/structuredBody";
const LINK_PATH: &str = "/ClinicalDocument/component/structuredBody//section/text//linkHtml/@href";
const FOOTNOTE_PATH: &str =
    "/ClinicalDocument/component/structuredBody//section/text//footnoteRef/@IDREF";
const ENTRY_PATH: &str =
    "/ClinicalDocument/component/structuredBody//section/entry//reference/@value";
pub fn validate_cda(data: &[u8], opts: &CdaValidationOptions) -> Result<CdaValidationReport> {
    let profile = if opts.profile.is_empty() {
        CDA_PROFILE_BASE_HEADER
    } else {
        opts.profile.as_str()
    };
    if profile != CDA_PROFILE_BASE_HEADER && profile != CDA_PROFILE_CCD_HEADER {
        return Err(Error::sentinel(Kind::UnsupportedCdaValidationProfile));
    }
    let limits = normalize_cda_narrative_limits(CdaNarrativeLimits {
        max_bytes: opts.limits.max_bytes,
        max_depth: opts.limits.max_depth,
        max_tokens: opts.limits.max_records,
        ..CdaNarrativeLimits::default()
    })?;
    inspect_payload(
        MIME_TYPE_CDA,
        data,
        PayloadLimits {
            max_bytes: limits.max_bytes,
            max_depth: limits.max_depth,
            max_records: limits.max_tokens,
        },
    )?;
    let (tree, root) = read_xml(data, &limits, true)?;
    let mut report = CdaValidationReport {
        profile: profile.to_string(),
        findings: Vec::new(),
    };
    let Some(root) = root else {
        return Err(invalid());
    };
    if let Some(body) = validate_header(&mut report, &tree, root) {
        let mut g = Graph {
            tree: &tree,
            report: &mut report,
            by_id: std::collections::HashMap::new(),
            anchors: vec![Anchor::default()],
            references: Vec::new(),
        };
        g.sections(body);
        g.validate_references();
    }
    Ok(report)
}
fn children(tree: &Tree, node: usize) -> std::collections::HashMap<String, Vec<usize>> {
    let mut out: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for child in tree.child_nodes(node) {
        let name = &tree.nodes[child].name;
        if name.space == CDA_NAMESPACE {
            out.entry(name.local.clone()).or_default().push(child);
        }
    }
    out
}
fn validate_header(report: &mut CdaValidationReport, tree: &Tree, root: usize) -> Option<usize> {
    let header = children(tree, root);
    let count = |name: &str| header.get(name).map_or(0, Vec::len);
    for name in [
        "code",
        "effectiveTime",
        "confidentialityCode",
        "custodian",
        "component",
    ] {
        if count(name) != 1 {
            report.add("header-cardinality", &format!("{HEADER_PATH}/{name}"));
        }
    }
    for name in ["recordTarget", "author"] {
        if count(name) == 0 {
            report.add("header-cardinality", &format!("{HEADER_PATH}/{name}"));
        }
    }
    for name in ["typeId", "title", "setId", "versionNumber"] {
        if count(name) > 1 {
            report.add("header-cardinality", &format!("{HEADER_PATH}/{name}"));
        }
    }
    for (name, expected) in [("classCode", "DOCCLIN"), ("moodCode", "EVN")] {
        let value = tree.attribute(root, name);
        if !value.is_empty() && value != expected {
            report.add("header-fixed-value", &format!("{HEADER_PATH}/@{name}"));
        }
    }
    if let Some(ids) = header.get("typeId")
        && ids.len() == 1
        && (tree.attribute(ids[0], "root") != "2.16.840.1.113883.1.3"
            || tree.attribute(ids[0], "extension") != "POCD_HD000040")
    {
        report.add("header-fixed-value", &format!("{HEADER_PATH}/typeId"));
    }
    if (count("setId") == 0) != (count("versionNumber") == 0) {
        report.add("revision-pair", HEADER_PATH);
    }
    if report.profile == CDA_PROFILE_CCD_HEADER {
        const CCD_ROOT: &str = "2.16.840.1.113883.10.20.22.1.2";
        let declared = header.get("templateId").is_some_and(|templates| {
            templates.iter().any(|&t| {
                tree.attribute(t, "root") == CCD_ROOT && tree.attribute(t, "nullFlavor").is_empty()
            })
        });
        if !declared {
            report.add("ccd-template", &format!("{HEADER_PATH}/templateId"));
        }
        if let Some(codes) = header.get("code")
            && codes.len() == 1
        {
            let code = codes[0];
            if tree.attribute(code, "code") != "34133-9"
                || tree.attribute(code, "codeSystem") != "2.16.840.1.113883.6.1"
                || !tree.attribute(code, "nullFlavor").is_empty()
            {
                report.add("ccd-document-code", &format!("{HEADER_PATH}/code"));
            }
        }
    }
    if count("component") != 1 {
        return None;
    }
    let body = children(tree, header["component"][0]);
    let structured = body.get("structuredBody").map_or(0, Vec::len);
    let non_xml = body.get("nonXMLBody").map_or(0, Vec::len);
    if structured + non_xml != 1 {
        report.add("body-choice", &format!("{HEADER_PATH}/component"));
        return None;
    }
    if structured == 1 {
        return Some(body["structuredBody"][0]);
    }
    None
}
#[derive(Clone, Copy, Default)]
struct Anchor {
    section: usize,
    footnote: bool,
    narrative: bool,
}
struct Reference {
    from: usize,
    target: String,
    path: &'static str,
    section: Option<usize>,
}
struct Graph<'a> {
    tree: &'a Tree,
    report: &'a mut CdaValidationReport,
    by_id: std::collections::HashMap<String, i64>,
    anchors: Vec<Anchor>,
    references: Vec<Reference>,
}
impl Graph<'_> {
    fn sections(&mut self, parent: usize) {
        let tree = self.tree;
        for part in tree.child_nodes(parent) {
            if !tree.is(part, "component") {
                continue;
            }
            for child in tree.child_nodes(part) {
                if tree.is(child, "section") {
                    self.section(child);
                }
            }
        }
    }
    fn section(&mut self, section: usize) {
        let tree = self.tree;
        let owner = self.add_anchor(section, section, false);
        for part in tree.child_nodes(section) {
            if tree.is(part, "text") {
                self.narrative(part, section, owner);
            } else if tree.is(part, "entry") {
                self.entry(part, section, false);
            }
        }
        self.sections(section);
    }
    fn add_anchor(&mut self, node: usize, section: usize, narrative: bool) -> usize {
        let id = self.tree.attribute(node, "ID");
        if id.is_empty() {
            return 0;
        }
        if self.by_id.get(&id).copied().unwrap_or(0) != 0 {
            self.report
                .add("duplicate-narrative-id", &format!("{BODY_PATH}//@ID"));
            self.by_id.insert(id, -1);
            return 0;
        }
        let index = self.anchors.len();
        self.by_id.insert(id, index as i64);
        self.anchors.push(Anchor {
            section,
            footnote: self.tree.is(node, "footnote"),
            narrative,
        });
        index
    }
    fn narrative(&mut self, node: usize, section: usize, mut owner: usize) {
        let tree = self.tree;
        let anchor = self.add_anchor(node, section, true);
        if anchor != 0 {
            owner = anchor;
        }
        if tree.is(node, "linkHtml") {
            if let Some(target) = tree.attribute(node, "href").strip_prefix('#') {
                self.references.push(Reference {
                    from: owner,
                    target: target.to_string(),
                    path: LINK_PATH,
                    section: None,
                });
            }
        } else if tree.is(node, "footnoteRef") {
            self.references.push(Reference {
                from: owner,
                target: tree.attribute(node, "IDREF"),
                path: FOOTNOTE_PATH,
                section: None,
            });
        }
        for child in tree.child_nodes(node) {
            if tree.nodes[child].name.space == CDA_NAMESPACE {
                self.narrative(child, section, owner);
            }
        }
    }
    fn entry(&mut self, node: usize, section: usize, text_reference: bool) {
        let tree = self.tree;
        if tree.is(node, "observationMedia") || tree.is(node, "regionOfInterest") {
            self.add_anchor(node, section, false);
        }
        if text_reference
            && tree.is(node, "reference")
            && let Some(target) = tree.attribute(node, "value").strip_prefix('#')
        {
            self.references.push(Reference {
                from: 0,
                target: target.to_string(),
                path: ENTRY_PATH,
                section: Some(section),
            });
        }
        let child_reference = tree.is(node, "originalText") || tree.is(node, "text");
        for child in tree.child_nodes(node) {
            if tree.nodes[child].name.space == CDA_NAMESPACE {
                self.entry(child, section, child_reference);
            }
        }
    }
    fn validate_references(&mut self) {
        let n = self.anchors.len();
        let mut edges: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut incoming = vec![0i64; n];
        for r in &self.references {
            let to = self.by_id.get(&r.target).copied().unwrap_or(0);
            if to == 0 {
                self.report.add("dangling-narrative-reference", r.path);
            } else if to < 0 {
                self.report.add("ambiguous-narrative-reference", r.path);
            } else if r.path == FOOTNOTE_PATH && !self.anchors[to as usize].footnote {
                self.report.add("wrong-narrative-target", r.path);
            } else if let Some(section) = r.section
                && (!self.anchors[to as usize].narrative
                    || self.anchors[to as usize].section != section)
            {
                self.report.add("wrong-narrative-target", r.path);
            } else if r.from != 0 {
                edges[r.from].push(to as usize);
                incoming[to as usize] += 1;
            }
        }
        let mut queue: Vec<usize> = (0..n).filter(|&i| incoming[i] == 0).collect();
        let mut head = 0;
        while head < queue.len() {
            for &to in &edges[queue[head]] {
                incoming[to] -= 1;
                if incoming[to] == 0 {
                    queue.push(to);
                }
            }
            head += 1;
        }
        if queue.len() != n {
            self.report.add("cyclic-narrative-reference", BODY_PATH);
        }
    }
}
