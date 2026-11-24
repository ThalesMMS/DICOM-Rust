use super::{empty_element, retired};
use crate::object::{Object, Tag, dict, standard_names};
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Present,
    Absent,
    NonEmpty,
    Equals,
    NotEquals,
    Greater,
    NonZero,
    EqualsTag,
    CodeItem,
}
#[derive(Clone, Debug)]
pub(super) struct Clause {
    tag: Tag,
    value_index: usize,
    op: Op,
    values: Vec<String>,
    tags: Vec<Tag>,
    number: f64,
}
impl Clause {
    fn new(tag: Tag, op: Op) -> Clause {
        Clause {
            tag,
            value_index: 0,
            op,
            values: Vec::new(),
            tags: Vec::new(),
            number: 0.0,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub(super) struct Condition {
    clauses: Vec<Clause>,
    any: bool,
    pub(super) forbidden: bool,
}
struct Patterns {
    clause: Regex,
    remark: Regex,
    value: Regex,
    bare_tag: Regex,
    code_item: Regex,
    code_triplet: Regex,
    tag_value: Regex,
    tag_in_text: Regex,
}
const NAME: &str = r"[A-Z0-9][A-Za-z0-9'’ /&\-]*?";
fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        clause: Regex::new(&format!(
            r"^(?:the )?(?:[Vv]alue ([0-9]) of )?(?:the [Vv]alue (?:for|of) )?(?:(?:the )?Attribute )?{NAME} \(([0-9A-Fa-f]{{4}}),([0-9A-Fa-f]{{4}})\)(?: [Vv]alue ([0-9]))?(?: of this [A-Za-z ]+?)?(?: in this (?:Sequence )?Item)? (.+)$"
        ))
        .unwrap(),
        remark: Regex::new(r"[\t\n\f\r ]*\((?:[^()0-9][^()]*)\)").unwrap(),
        value: Regex::new(r#"^(?:"[^"]*"|[A-Z0-9_][A-Z0-9_ .\-]*)$"#).unwrap(),
        bare_tag: Regex::new(&format!(
            r"^(?:either )?(?:the )?{NAME} \([0-9A-Fa-f]{{4}},[0-9A-Fa-f]{{4}}\)$"
        ))
        .unwrap(),
        code_item: Regex::new(&format!(
            r"^(?:the )?{NAME} \(([0-9A-Fa-f]{{4}}),([0-9A-Fa-f]{{4}})\) (?:contains an Item with (?:the )?(?:[Vv]alue|Coded Entry Value|Code Value)|includes an Item with (?:the )?[Vv]alue|is|has an Item with (?:the )?[Vv]alue)$"
        ))
        .unwrap(),
        code_triplet: Regex::new(r#"\(([^,()"]+), ([^,()"]+), "[^"]*"\)[\t\n\f\r ]*$"#).unwrap(),
        tag_value: Regex::new(&format!(r"^{NAME} \(([0-9A-Fa-f]{{4}}),([0-9A-Fa-f]{{4}})\)$")).unwrap(),
        tag_in_text: Regex::new(r"\(([0-9A-Fa-f]{4}),([0-9A-Fa-f]{4})\)").unwrap(),
    })
}
fn submatch(re: &Regex, text: &str) -> Option<Vec<String>> {
    re.captures(text).map(|c| {
        (0..c.len())
            .map(|i| c.get(i).map_or(String::new(), |m| m.as_str().to_string()))
            .collect()
    })
}
fn hex_tag(group: &str, element: &str) -> Tag {
    Tag::new(
        u16::from_str_radix(group, 16).unwrap_or(0),
        u16::from_str_radix(element, 16).unwrap_or(0),
    )
}
const REQUIREMENT_LEAD: [&str; 6] = [
    "Required if: ",
    "Required if ",
    "Required, if ",
    "Required only if ",
    "Required when ",
    "Shall be present if ",
];
const MEASURABLE: [(&str, &str); 7] = [
    (
        "the sop instance was created in response to a frame-level retrieve request",
        "Frame Extraction Sequence (0008,1164) is present",
    ),
    (
        "pixel data is multi-frame cine data",
        "Frame Increment Pointer (0028,0009) is Frame Time (0018,1063) or Frame Time Vector (0018,1065)",
    ),
    (
        "contrast media was used in this image",
        "Contrast/Bolus Agent (0018,0010) or Contrast/Bolus Agent Sequence (0018,0012) is present",
    ),
    (
        "contrast media was used",
        "Contrast/Bolus Agent (0018,0010) or Contrast/Bolus Agent Sequence (0018,0012) is present",
    ),
    (
        "contrast media was applied",
        "Contrast/Bolus Agent (0018,0010) or Contrast/Bolus Agent Sequence (0018,0012) is present",
    ),
    (
        "contrast media were applied",
        "Contrast/Bolus Agent (0018,0010) or Contrast/Bolus Agent Sequence (0018,0012) is present",
    ),
    (
        "lossy compression has been performed on the image",
        "Lossy Image Compression (0028,2110) is 01",
    ),
];
pub(super) fn parse_condition(text: &str) -> Option<Condition> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Condition, bool)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((c, ok)) = cache.lock().unwrap().get(text) {
        return ok.then(|| c.clone());
    }
    let (c, ok) = parse_condition_text(text);
    cache
        .lock()
        .unwrap()
        .insert(text.to_string(), (c.clone(), ok));
    ok.then_some(c)
}
pub(super) fn parse_condition_full(text: &str) -> (Condition, bool) {
    match parse_condition(text) {
        Some(c) => (c, true),
        None => (parse_condition_text(text).0, false),
    }
}
fn parse_condition_text(text: &str) -> (Condition, bool) {
    let p = patterns();
    let lower = text.to_lowercase();
    let mut c = Condition {
        forbidden: lower.contains("shall not be present")
            && !lower.contains("may be present otherwise"),
        ..Condition::default()
    };
    let mut sentence = String::new();
    for lead in REQUIREMENT_LEAD {
        if let Some(rest) = text.strip_prefix(lead) {
            sentence = first_sentence(rest);
            break;
        }
    }
    if sentence.is_empty() {
        return (c, false);
    }
    let key = sentence
        .strip_suffix('.')
        .unwrap_or(&sentence)
        .to_lowercase();
    if let Some((_, rewrite)) = MEASURABLE.iter().find(|(k, _)| *k == key) {
        sentence = rewrite.to_string();
    }
    if let Some(m) = submatch(&p.code_item, &sentence) {
        let code = submatch(&p.code_triplet, text);
        let Some(code) = code else {
            return (c, false);
        };
        if p.code_triplet.find_iter(text).count() != 1 {
            return (c, false);
        }
        let mut cl = Clause::new(hex_tag(&m[1], &m[2]), Op::CodeItem);
        cl.values = vec![code[1].trim().to_string(), code[2].trim().to_string()];
        c.clauses = vec![cl];
        return (c, true);
    }
    let sentence = normalize_sentence(&sentence);
    if let Some(cl) = parse_clause(&sentence) {
        c.clauses = vec![cl];
        return (c, true);
    }
    for sep in [" and ", " or "] {
        if let Some(clauses) = parse_parts(split_parts(&sentence, sep), sep == " or ") {
            c.clauses = clauses;
            c.any = sep == " or ";
            return (c, true);
        }
    }
    (c, false)
}
fn has_suffix_fold_lower(s: &str, tail: &str) -> bool {
    s.to_lowercase().ends_with(tail)
}
fn normalize_sentence(sentence: &str) -> String {
    let p = patterns();
    let mut s = p
        .remark
        .replace_all(sentence, "")
        .replace(", and ", " and ");
    s = s.replace("), Value ", ") Value ");
    s = s.replace(") at the image level ", ") ");
    for tail in [
        ", may be present otherwise",
        "; may be present otherwise",
        ", otherwise may be present",
    ] {
        if has_suffix_fold_lower(&s, tail) {
            s.truncate(s.len() - tail.len());
        }
    }
    protect_attribute_names(&s)
}
pub(super) fn keyword_of(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}
fn std_keyword(tag: Tag) -> Option<&'static str> {
    dict::by_tag(tag)?;
    standard_names(tag).map(|(k, _)| k)
}
fn protect_attribute_names(sentence: &str) -> String {
    let p = patterns();
    let mut sentence = sentence.to_string();
    let matches: Vec<(usize, Tag)> = p
        .tag_in_text
        .captures_iter(&sentence)
        .map(|c| {
            (
                c.get(0).unwrap().start(),
                hex_tag(c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str()),
            )
        })
        .collect();
    for &(start, tag) in matches.iter().rev() {
        let Some(keyword) = std_keyword(tag) else {
            continue;
        };
        let before = sentence[..start].trim_end_matches(' ').to_string();
        let words: Vec<&str> = before.split_whitespace().collect();
        let mut n = words.len().min(16);
        while n >= 2 {
            let name = words[words.len() - n..].join(" ");
            if (name.contains(" and ") || name.contains(" or "))
                && keyword_of(&name).eq_ignore_ascii_case(keyword)
            {
                let protected = name.replace(" and ", " & ").replace(" or ", " / ");
                sentence = format!(
                    "{}{} {}",
                    &before[..before.len() - name.len()],
                    protected,
                    &sentence[start..]
                );
                break;
            }
            n -= 1;
        }
    }
    sentence
}
fn with_attribute_tag(text: &str) -> Option<String> {
    let p = patterns();
    if p.tag_in_text.is_match(text) {
        return None;
    }
    let words: Vec<&str> = text
        .strip_prefix("the ")
        .unwrap_or(text)
        .split_whitespace()
        .collect();
    let mut n = (words.len() as isize - 1).min(10);
    while n >= 2 {
        let name = words[..n as usize].join(" ");
        let first = name.as_bytes()[0];
        if first >= 0x80 || first.is_ascii_lowercase() {
            return None;
        }
        if let Some((tag, _)) = dict::by_keyword(&keyword_of(&name))
            && !retired(tag)
        {
            return Some(format!("{name} {tag} {}", words[n as usize..].join(" ")));
        }
        n -= 1;
    }
    None
}
fn split_parts(sentence: &str, sep: &str) -> Vec<String> {
    let p = patterns();
    let mut parts = Vec::new();
    for part in sentence.split(sep) {
        let pieces: Vec<&str> = part.split(", ").collect();
        let mut list = pieces.len() > 1;
        for piece in &pieces[..pieces.len() - 1] {
            if !p.bare_tag.is_match(piece.trim()) {
                list = false;
            }
        }
        if list {
            parts.extend(pieces.iter().map(|s| s.to_string()));
        } else {
            parts.push(part.to_string());
        }
    }
    parts
}
fn parse_parts(parts: Vec<String>, disjunction: bool) -> Option<Vec<Clause>> {
    let p = patterns();
    if parts.len() < 2 {
        return None;
    }
    let mut shared = String::new();
    if let Some(m) = submatch(&p.clause, parts.last().unwrap().trim()) {
        shared = m[5].clone();
        for (plural, singular) in [
            ("are not present", "is not present"),
            ("are present", "is present"),
            ("are absent", "is absent"),
            ("are not", "is not"),
            ("are ", "is "),
        ] {
            if let Some(rest) = shared.strip_prefix(plural) {
                shared = format!("{singular}{rest}");
                break;
            }
        }
    }
    let mut clauses: Vec<Clause> = Vec::new();
    'parts: for part in parts {
        let part = part.trim();
        if let Some(cl) = parse_clause(part) {
            clauses.push(cl);
            continue;
        }
        if !shared.is_empty()
            && p.bare_tag.is_match(part)
            && let Some(cl) = parse_clause(&format!("{part} {shared}"))
        {
            clauses.push(cl);
            continue;
        }
        if disjunction
            && clauses.last().is_some_and(|c| c.op == Op::Equals)
            && p.value.is_match(part)
        {
            let last = clauses.last_mut().unwrap();
            last.values.push(part.trim_matches('"').to_string());
            continue;
        }
        if let Some(last) = clauses.last() {
            let tag = last.tag;
            for prefix in [
                "the Value is ",
                "the value is ",
                "its Value is ",
                "has a Value of ",
                "has a value of ",
                "has the Value ",
                "has the value ",
                "has Value ",
                "has value ",
                "equals ",
                "is present and has Value ",
                "is present and has a Value of ",
                "is present and equals ",
            ] {
                if let Some(rest) = part.strip_prefix(prefix)
                    && let Some(cl) = with_values(Clause::new(tag, Op::Equals), rest)
                {
                    clauses.push(cl);
                    continue 'parts;
                }
            }
        }
        return None;
    }
    Some(clauses)
}
fn first_sentence(text: &str) -> String {
    let b = text.as_bytes();
    let mut quoted = false;
    for (i, r) in text.char_indices() {
        if r == '"' {
            quoted = !quoted;
        } else if r == '.' && !quoted && (i + 1 == b.len() || b[i + 1] == b' ') {
            return text[..i].trim().to_string();
        }
    }
    text.trim().to_string()
}
fn attribute_name(name: &str) -> bool {
    let name = format!(" {} ", name.trim());
    ![
        " is ", " are ", " and ", " if ", " or ", " has ", " have ", " not ", " was ", " be ",
        " an ", " a ", " when ", " that ", " which ",
    ]
    .iter()
    .any(|w| name.contains(w))
}
fn parse_clause(text: &str) -> Option<Clause> {
    let p = patterns();
    let mut text = text.trim().to_string();
    for lead in ["if ", "either "] {
        if let Some(rest) = text.strip_prefix(lead) {
            text = rest.to_string();
        }
    }
    let mut m = submatch(&p.clause, &text);
    if m.is_none()
        && let Some(tagged) = with_attribute_tag(&text)
    {
        text = tagged;
        m = submatch(&p.clause, &text);
    }
    let m = m?;
    let paren = text.find('(')?;
    if !attribute_name(&text[..paren]) {
        return None;
    }
    let mut cl = Clause::new(hex_tag(&m[2], &m[3]), Op::Present);
    for index in [&m[1], &m[4]] {
        if !index.is_empty() {
            cl.value_index = index.parse().unwrap_or(0);
        }
    }
    let predicate = m[5].trim();
    match predicate {
        "is present" | "is sent" | "is included" | "are present" => {
            cl.op = Op::Present;
            return Some(cl);
        }
        "is not present" | "is absent" | "is not sent" | "is not included" | "not present"
        | "are not present" | "are absent" => {
            cl.op = Op::Absent;
            return Some(cl);
        }
        "is present and has a value"
        | "is present with a value"
        | "has a value"
        | "is present and not empty"
        | "is not empty"
        | "has a Value"
        | "is present and has a Value"
        | "is present with a Value" => {
            cl.op = Op::NonEmpty;
            return Some(cl);
        }
        "has a non-zero value" | "is present and has a non-zero value" | "is non-zero" => {
            cl.op = Op::NonZero;
            return Some(cl);
        }
        _ => {}
    }
    for prefix in ["is ", "equals ", "has a Value of ", "has the Value "] {
        let Some(rest) = predicate.strip_prefix(prefix) else {
            continue;
        };
        let mut tags = Vec::new();
        for part in rest.split(" or ") {
            let part = part.trim();
            let Some(m) = submatch(&p.tag_value, part) else {
                tags.clear();
                break;
            };
            let tag = hex_tag(&m[1], &m[2]);
            let ok = std_keyword(tag).is_some_and(|keyword| {
                keyword_of(&part[..part.rfind('(').unwrap_or(0)]).eq_ignore_ascii_case(keyword)
            });
            if !ok {
                tags.clear();
                break;
            }
            tags.push(tag);
        }
        if !tags.is_empty() {
            cl.op = Op::EqualsTag;
            cl.tags = tags;
            return Some(cl);
        }
    }
    for prefix in [
        "is greater than ",
        "has a value greater than ",
        "has a Value greater than ",
    ] {
        if let Some(rest) = predicate.strip_prefix(prefix) {
            let n = crate::numtext::parse_float(rest).ok()?;
            cl.op = Op::Greater;
            cl.number = n;
            return Some(cl);
        }
    }
    for prefix in [
        "is not ",
        "does not equal ",
        "is other than ",
        "does not have a value of ",
        "does not have a Value of ",
        "Value is not ",
        "value is not ",
        "equals other than ",
    ] {
        if let Some(rest) = predicate.strip_prefix(prefix) {
            cl.op = Op::NotEquals;
            return with_values(cl, rest);
        }
    }
    for prefix in [
        "is present and has a Value of ",
        "is present and has a value of ",
        "is present and has the Value ",
        "is present with a Value of ",
        "is present with a value of ",
        "is present and equals ",
        "is present with Value ",
        "is present and has Value ",
        "has Value ",
        "has a Value of ",
        "has a value of ",
        "has the Value ",
        "has the value ",
        "is equal to ",
        "equals ",
        "= ",
        "Value is ",
        "value is ",
        "Value equals ",
        "includes Value ",
        "includes the Value ",
        "includes ",
        "contains the Value ",
        "is ",
    ] {
        if let Some(rest) = predicate.strip_prefix(prefix) {
            cl.op = Op::Equals;
            return with_values(cl, rest);
        }
    }
    None
}
fn with_values(mut cl: Clause, text: &str) -> Option<Clause> {
    let p = patterns();
    let text = text.trim();
    let text = text.strip_prefix("the Value ").unwrap_or(text).trim();
    let mut values = Vec::new();
    for part in text.replace(", ", " or ").split(" or ") {
        let part = part.trim();
        if !p.value.is_match(part) {
            return None;
        }
        values.push(part.trim_matches('"').to_string());
    }
    if values.is_empty() {
        return None;
    }
    cl.values = values;
    Some(cl)
}
impl Condition {
    pub(super) fn eval(&self, item: &Object, root: &Object) -> bool {
        for cl in &self.clauses {
            let result = cl.eval(item, root);
            if self.any && result {
                return true;
            }
            if !self.any && !result {
                return false;
            }
        }
        !self.any
    }
}
pub(super) fn same(a: &Object, b: &Object) -> bool {
    std::ptr::eq(a, b)
}
impl Clause {
    fn eval(&self, item: &Object, root: &Object) -> bool {
        let holder = if !item.has(self.tag) && !same(root, item) && root.has(self.tag) {
            root
        } else {
            item
        };
        let element = holder.get(self.tag);
        match self.op {
            Op::Present => return element.is_some(),
            Op::Absent => return element.is_none(),
            Op::NonEmpty => {
                return element.is_some_and(|e| !empty_element(holder, e));
            }
            _ => {}
        }
        if element.is_none() {
            return self.op == Op::NotEquals;
        }
        match self.op {
            Op::EqualsTag => {
                let tags = holder.get_tags(self.tag).unwrap_or_default();
                return tags.iter().any(|t| self.tags.contains(t));
            }
            Op::CodeItem => {
                let items = holder.get_sequence(self.tag).unwrap_or_default();
                return items.iter().any(|item| {
                    let value = item
                        .get_string(Tag::new(0x0008, 0x0100))
                        .unwrap_or_default();
                    let scheme = item
                        .get_string(Tag::new(0x0008, 0x0102))
                        .unwrap_or_default();
                    value.trim() == self.values[0] && scheme.trim() == self.values[1]
                });
            }
            _ => {}
        }
        let mut values = element_values(holder, self.tag);
        if self.value_index > 0 {
            if self.value_index > values.len() {
                return self.op == Op::NotEquals;
            }
            values = vec![values[self.value_index - 1].clone()];
        }
        if matches!(self.op, Op::NonZero | Op::Greater) {
            return values.iter().any(|v| {
                crate::numtext::parse_float(v.trim()).is_ok_and(|n| {
                    (self.op == Op::NonZero && n != 0.0)
                        || (self.op == Op::Greater && n > self.number)
                })
            });
        }
        let matched = values.iter().any(|v| {
            self.values
                .iter()
                .any(|want| simple_fold_eq(v.trim(), want))
        });
        if self.op == Op::NotEquals {
            !matched
        } else {
            matched
        }
    }
}
pub(super) fn simple_fold_eq(a: &str, b: &str) -> bool {
    let fold = |c: char| {
        let mut l = c.to_lowercase();
        match (l.next(), l.next()) {
            (Some(x), None) => x,
            _ => c,
        }
    };
    a.chars().count() == b.chars().count()
        && a.chars()
            .zip(b.chars())
            .all(|(c, d)| c == d || fold(c) == fold(d))
}
pub(super) fn element_values(obj: &Object, tag: Tag) -> Vec<String> {
    if let Some(values) = obj.get_strings(tag)
        && !values.is_empty()
    {
        return values;
    }
    if let Ok(numbers) = obj.get_floats(tag) {
        return numbers
            .iter()
            .map(|n| crate::numtext::format_float(*n, 'g', -1))
            .collect();
    }
    Vec::new()
}
