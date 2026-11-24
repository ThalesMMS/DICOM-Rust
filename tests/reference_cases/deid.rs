#[allow(unused_imports)]
use super::support::*;
use dicom_rust::attrpath;
use dicom_rust::deid::{
    self, ActionCode, AttributeRequirement, BasicProfileOptions, CallbackError, Options,
    PixelRegion, PrivateTagAllow, PrivateTagPolicy, ProfileAction, ProfileLimits, ProfileOption,
    PseudonymTemplate, Pseudonymizer, Report, SafePrivateProvenance, SafePrivateRegistry,
    SafePrivateRule, UidRemapper,
};
use dicom_rust::object::{
    self, DataSet, Element, ElementHeader, File, Object, ReadResult, Tag, UNDEFINED_LENGTH, Value,
    Vr,
};
use dicom_rust::textfmt::quote;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
fn payload(data: &[u8]) -> String {
    if data.len() <= 64 {
        return hex::encode(data);
    }
    let sum = Sha256::digest(data);
    format!("len={} sha256={}", data.len(), hex::encode(&sum[..8]))
}
fn header(e: &Element) -> String {
    let h = &e.header;
    let length = if h.length_set && h.length != UNDEFINED_LENGTH {
        h.length.to_string()
    } else {
        "undef".to_string()
    };
    format!(
        "({:04X},{:04X}) {} {}",
        h.tag.group, h.tag.element, h.vr, length
    )
}
fn value(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_string(),
        Value::Raw(raw) => format!("raw {}", payload(raw)),
        Value::Fragments {
            offset_table,
            fragments,
        } => {
            let mut parts = vec![format!("offsets {}", payload(offset_table))];
            parts.extend(fragments.iter().map(|f| payload(f)));
            format!("fragments {} [{}]", fragments.len(), parts.join("; "))
        }
        Value::Sequence(items) => format!("items {}", items.len()),
        _ => "kind 1".to_string(),
    }
}
fn dump(b: &mut String, elements: &[Element], indent: &str) {
    let mut sorted: Vec<&Element> = elements.iter().collect();
    sorted.sort_by_key(|e| e.header.tag);
    for e in sorted {
        b.push_str(&format!("{indent}{} {}\n", header(e), value(&e.value)));
        if let Value::Sequence(items) = &e.value {
            for (i, item) in items.iter().enumerate() {
                b.push_str(&format!("{indent}  item {i}\n"));
                dump(b, &item.elements, &format!("{indent}    "));
            }
        }
    }
}
#[derive(Default)]
struct RandomUids(HashMap<String, String>);
impl RandomUids {
    fn normalize(&mut self, uid: &str) -> String {
        let trimmed = uid.trim_end_matches(['\0', ' ']);
        let Some(digits) = trimmed.strip_prefix("2.25.") else {
            return uid.to_string();
        };
        if digits.len() > 39 || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return uid.to_string();
        }
        let Ok(n) = digits.parse::<u128>() else {
            return uid.to_string();
        };
        if (n >> 76) & 15 != 4 {
            return uid.to_string();
        }
        if let Some(p) = self.0.get(trimmed) {
            return p.clone();
        }
        let p = format!("9.9.{}", self.0.len() + 1);
        self.0.insert(trimmed.to_string(), p.clone());
        p
    }
    fn elements(&mut self, elements: &[Element]) -> Vec<Element> {
        let mut out = Vec::with_capacity(elements.len());
        for e in elements {
            let mut e = e.clone();
            match &e.value {
                Value::Sequence(items) => {
                    let items: Vec<DataSet> = items
                        .iter()
                        .map(|item| DataSet {
                            elements: self.elements(&item.elements),
                            item_offset: item.item_offset,
                        })
                        .collect();
                    e.value = Value::Sequence(items);
                }
                Value::Raw(raw) if e.vr() == Vr::UI => {
                    let text = String::from_utf8_lossy(raw).into_owned();
                    let mut parts: Vec<String> = text.split('\\').map(str::to_string).collect();
                    let mut changed = false;
                    for p in parts.iter_mut() {
                        let n = self.normalize(p);
                        if n != *p {
                            *p = n;
                            changed = true;
                        }
                    }
                    if changed {
                        let mut text = parts.join("\\");
                        if text.len() % 2 == 1 {
                            text.push('\0');
                        }
                        e = Element {
                            header: ElementHeader::new(e.tag(), Vr::UI, text.len() as u32),
                            value: Value::Raw(text.into_bytes()),
                        };
                    }
                }
                _ => {}
            }
            out.push(e);
        }
        out
    }
    fn object(&mut self, o: &mut Object) {
        for e in self.elements(o.elements()) {
            o.put(e);
        }
    }
}
fn render_file(mut f: File) -> String {
    let mut r = RandomUids::default();
    r.object(&mut f.dataset);
    r.object(&mut f.meta);
    let out = match object::writer::write_file(&f) {
        Ok(out) => out,
        Err(e) => return format!("write-error={e}\n"),
    };
    let sum = Sha256::digest(&out);
    let mut b = format!(
        "len={} sha256={} random-uids={}\n",
        out.len(),
        hex::encode(sum),
        r.0.len()
    );
    let back = match object::read_file(&out) {
        Ok(back) => back,
        Err((_, e)) => return format!("{b}reread-error={e}\n"),
    };
    b.push_str(&format!(
        "transfer-syntax {}\nmeta\n",
        back.transfer_syntax.uid
    ));
    dump(&mut b, back.meta.elements(), "  ");
    b.push_str("dataset\n");
    dump(&mut b, back.dataset.elements(), "  ");
    b
}
fn bytes(data: &[i64]) -> Vec<u8> {
    data.iter().map(|&b| b as u8).collect()
}
fn remapper(key: &str) -> UidRemapper {
    if key == "random" {
        UidRemapper::new()
    } else {
        UidRemapper::new_keyed(key.as_bytes())
    }
}
fn parse_tag(s: &str) -> Result<Tag, String> {
    let bad = || format!("deid harness: bad tag {}", quote(s));
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let v = u32::from_str_radix(s, 16).map_err(|_| bad())?;
    Ok(Tag::new((v >> 16) as u16, v as u16))
}
fn private_policy(spec: &str) -> Result<Option<PrivateTagPolicy>, String> {
    if spec.is_empty() {
        return Ok(None);
    }
    if spec == "retain" {
        return Ok(Some(deid::retain_private_tags()));
    }
    if spec == "remove" {
        return Ok(Some(deid::remove_private_tags()));
    }
    if let Some(rest) = spec.strip_prefix("allow:") {
        let mut list = Vec::new();
        for entry in rest.split(',') {
            if entry.is_empty() {
                continue;
            }
            let (tag_text, creator) = entry.split_once('=').unwrap_or((entry, ""));
            list.push(PrivateTagAllow {
                tag: parse_tag(tag_text)?,
                creator: creator.to_string(),
            });
        }
        return Ok(Some(deid::allow_private_tags(&list)));
    }
    Err(format!("deid harness: bad private policy {}", quote(spec)))
}
fn render_report(r: &Report) -> String {
    format!(
        "burned-in risk={} value={} regions={}\nvisual risk={} value={}\nedited={} csa scrubbed={} cleared={} unscrubbable={}\n",
        r.burned_in_pixel.risk.as_str(),
        quote(&r.burned_in_pixel.metadata_value),
        r.burned_in_pixel.redacted_regions.len(),
        r.recognizable_visual_features.risk.as_str(),
        quote(&r.recognizable_visual_features.metadata_value),
        r.edited_attributes,
        r.csa.scrubbed,
        r.csa.bytes_cleared,
        r.csa.unscrubbable
    )
}
fn anonymize_result(
    result: ReadResult,
    patient_name: &str,
    patient_id: &str,
    keep_dates: bool,
    policy: &str,
    edits: &str,
    uid_key: &str,
) -> Result<String, String> {
    let f = result.map_err(|(_, e)| e.to_string())?;
    let policy = private_policy(policy)?;
    let script = attrpath::parse_script(edits).map_err(|e| e.to_string())?;
    let opts = Options {
        patient_name: patient_name.to_string(),
        patient_id: patient_id.to_string(),
        keep_dates,
        private_tag_policy: policy,
        edits: script,
        edit_options: attrpath::Options::default(),
        burned_in_pixel_policy: None,
    };
    let mut uids = remapper(uid_key);
    match deid::clone_anonymized_file(&f, &opts, &mut uids) {
        Ok((clone, report)) => Ok(render_report(&report) + &render_file(clone)),
        Err((report, err)) => Ok(format!("{}error={err}\n", render_report(&report))),
    }
}
pub fn anonymize(
    data: Vec<i64>,
    patient_name: String,
    patient_id: String,
    keep_dates: bool,
    policy: String,
    edits: String,
    uid_key: String,
) -> Result<String, String> {
    anonymize_result(
        object::read_file(&bytes(&data)),
        &patient_name,
        &patient_id,
        keep_dates,
        &policy,
        &edits,
        &uid_key,
    )
}
pub fn anonymize_file(
    path: String,
    patient_name: String,
    patient_id: String,
    keep_dates: bool,
    policy: String,
    edits: String,
    uid_key: String,
) -> Result<String, String> {
    anonymize_result(
        object::open_file(&path),
        &patient_name,
        &patient_id,
        keep_dates,
        &policy,
        &edits,
        &uid_key,
    )
}
pub fn remap_hierarchy(data: Vec<i64>, uid_key: String) -> Result<String, String> {
    let f = object::read_file(&bytes(&data)).map_err(|(_, e)| e.to_string())?;
    let clone = deid::clone_with_remapped_hierarchy_uids(&f, &mut remapper(&uid_key))
        .map_err(|e| e.to_string())?;
    Ok(render_file(clone))
}
pub fn assess_risk(data: Vec<i64>) -> Result<String, String> {
    let f = object::read_file(&bytes(&data)).map_err(|(_, e)| e.to_string())?;
    Ok(render_report(&deid::assess_visual_phi_risk(&f.dataset)))
}
pub fn resolve_action(code: String, requirement: i64) -> Result<String, String> {
    let requirement = AttributeRequirement(requirement as u8);
    let code =
        ActionCode::parse(&code).ok_or_else(|| "deid: invalid PS3.15 action code".to_string())?;
    let action = deid::resolve_action_code(code, requirement).map_err(|e| e.to_string())?;
    Ok(format!("{} {}", action.as_str(), requirement.as_str()))
}
fn atoi(s: &str) -> i64 {
    dicom_rust::numtext::parse_int(s).unwrap_or(0)
}
fn profile_options(spec: &str) -> Result<(BasicProfileOptions, UidRemapper, bool), String> {
    let mut opts = BasicProfileOptions::default();
    let mut uids = UidRemapper::new_keyed(b"harness-key");
    let mut invalid_option = false;
    for entry in spec.split('|') {
        if entry.is_empty() {
            continue;
        }
        let (key, val) = entry.split_once('=').unwrap_or((entry, ""));
        let mode = val.to_string();
        match key {
            "options" => {
                for o in val.split(',').filter(|o| !o.is_empty()) {
                    match ProfileOption::parse(o) {
                        Some(o) => opts.selected_options.push(o),
                        None => invalid_option = true,
                    }
                }
            }
            "uids" => uids = remapper(val),
            "require" => opts.require_resolved_conditional = true,
            "resolver" => {
                opts.requirement_resolver = Some(Box::new(move |a| match mode.as_str() {
                    "type1" => Ok(AttributeRequirement::TYPE1),
                    "type2" => Ok(AttributeRequirement::TYPE2),
                    "optional" => Ok(AttributeRequirement::OPTIONAL),
                    "invalid" => Ok(AttributeRequirement(3)),
                    "error" => Err(CallbackError),
                    "panic" => panic!("resolver panic"),
                    _ => Ok(match a.vr {
                        Vr::SQ | Vr::DA | Vr::TM | Vr::DT => AttributeRequirement::TYPE2,
                        Vr::UI | Vr::PN => AttributeRequirement::TYPE1,
                        _ => AttributeRequirement::OPTIONAL,
                    }),
                }))
            }
            "cleaner" => {
                opts.cleaner = Some(Box::new(move |c| {
                    let (tag, vr) = (c.attribute.tag, c.attribute.vr);
                    match mode.as_str() {
                        "error" => Err(CallbackError),
                        "panic" => panic!("cleaner panic"),
                        "badtag" => Ok(raw(Tag::new(0x0009, 0x0001), vr, b"X ")),
                        "badvr" => Ok(raw(tag, if vr == Vr::LO { Vr::SH } else { Vr::LO }, b"X ")),
                        "nil" => Ok(Element {
                            header: ElementHeader {
                                tag,
                                vr,
                                length: 0,
                                length_set: false,
                            },
                            value: Value::Nil,
                        }),
                        _ if vr.is_string_like() => Ok(raw(tag, vr, b"CLEANED ")),
                        _ => Ok(raw(tag, vr, &[0, 0])),
                    }
                }))
            }
            "dummy" => {
                opts.dummy_value_provider = Some(Box::new(move |d| {
                    let (tag, vr) = (d.attribute.tag, d.attribute.vr);
                    match mode.as_str() {
                        "error" => Err(CallbackError),
                        "empty" => Ok(raw(tag, vr, b"")),
                        "badvr" => Ok(raw(tag, Vr::UN, &[1, 0])),
                        _ if vr == Vr::SQ => Ok(Element {
                            header: ElementHeader::new(tag, vr, UNDEFINED_LENGTH),
                            value: Value::Sequence(vec![DataSet::default()]),
                        }),
                        _ if vr.is_string_like() => Ok(raw(tag, vr, b"DUMMY ")),
                        _ => Ok(raw(tag, vr, &[7, 0])),
                    }
                }))
            }
            "shift" => {
                opts.date_shift_policy = Some(Box::new(move |_| {
                    if mode == "error" {
                        return Err(CallbackError);
                    }
                    Ok(atoi(&mode) as i32 as i64)
                }))
            }
            "pixel" => {
                opts.pixel_cleaner = Some(Box::new(move |o| match mode.as_str() {
                    "error" => Err(CallbackError),
                    "mutate" => {
                        o.put(raw(Tag::new(0x0010, 0x0010), Vr::PN, b"X "));
                        Ok(Vec::new())
                    }
                    "pixels" => {
                        o.put(raw(Tag::new(0x7FE0, 0x0010), Vr::OW, &[0, 0, 0, 0]));
                        Ok(vec![PixelRegion {
                            width: 1,
                            height: 1,
                            ..Default::default()
                        }])
                    }
                    _ => Ok(vec![PixelRegion::default(); atoi(&mode).max(0) as usize]),
                }))
            }
            "visual" => {
                opts.visual_features_cleaner = Some(Box::new(move |o| {
                    match mode.as_str() {
                        "error" => return Err(CallbackError),
                        "mutate" => o.put(raw(Tag::new(0x0010, 0x0020), Vr::LO, b"X ")),
                        "overlay" => o.put(raw(Tag::new(0x6000, 0x0022), Vr::LO, b"ov")),
                        _ => {}
                    }
                    Ok(())
                }))
            }
            "safe" => {
                let mut parts: Vec<&str> = val.splitn(3, '@').collect();
                while parts.len() < 3 {
                    parts.push("");
                }
                let mut rules = Vec::new();
                for r in parts[2].split('+').filter(|r| !r.is_empty()) {
                    let mut f: Vec<&str> = r.split('/').collect();
                    while f.len() < 5 {
                        f.push("");
                    }
                    rules.push(SafePrivateRule {
                        tag: parse_tag(f[0])?,
                        creator: f[1].to_string(),
                        vr: Vr::parse(f[2].as_bytes()),
                        vm: atoi(f[3]) as i32 as i64,
                        action: ProfileAction::parse(f[4]),
                    });
                }
                let provenance = SafePrivateProvenance {
                    version: parts[0].to_string(),
                    checksum: parts[1].to_string(),
                };
                opts.safe_private_registry =
                    Some(SafePrivateRegistry::new(provenance, rules).map_err(|e| e.to_string())?);
            }
            "pseudo" => {
                let mut parts: Vec<&str> = val.splitn(3, '@').collect();
                while parts.len() < 3 {
                    parts.push("");
                }
                let template = PseudonymTemplate {
                    patient_name: parts[0].to_string(),
                    patient_id: parts[1].to_string(),
                };
                opts.pseudonymizer = Some(
                    Pseudonymizer::new(&template, parts[2].as_bytes())
                        .map_err(|e| e.to_string())?,
                );
            }
            "limits" => {
                let mut n = [0i64; 7];
                for (i, v) in val.split(',').enumerate() {
                    if i < n.len() {
                        n[i] = atoi(v);
                    }
                }
                let int = |v: i64| v as i32 as i64;
                opts.limits = ProfileLimits {
                    max_depth: int(n[0]),
                    max_elements: int(n[1]),
                    max_items: int(n[2]),
                    max_value_bytes: n[3],
                    max_actions: int(n[4]),
                    max_pixel_masks: int(n[5]),
                    max_report_bytes: int(n[6]),
                };
            }
            _ => return Err(format!("deid harness: bad profile key {}", quote(key))),
        }
    }
    Ok((opts, uids, invalid_option))
}
const INVALID_OPTIONS: &str = "deid: invalid basic profile options";
fn raw(tag: Tag, vr: Vr, data: &[u8]) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, data.len() as u32),
        value: Value::Raw(data.to_vec()),
    }
}
fn profile_result(result: ReadResult, spec: &str) -> Result<String, String> {
    let f = result.map_err(|(_, e)| e.to_string())?;
    let (mut opts, mut uids, invalid) = profile_options(spec)?;
    if invalid {
        return Err(INVALID_OPTIONS.to_string());
    }
    let (clone, report) =
        deid::clone_file_with_basic_profile(&f, &mut opts, &mut uids).map_err(|e| e.to_string())?;
    Ok(format!("{}\n{}", report.json(), render_file(clone)))
}
pub fn profile(data: Vec<i64>, spec: String) -> Result<String, String> {
    profile_result(object::read_file(&bytes(&data)), &spec)
}
pub fn profile_file(path: String, spec: String) -> Result<String, String> {
    profile_result(object::open_file(&path), &spec)
}
pub fn profile_twice(first: Vec<i64>, second: Vec<i64>, spec: String) -> Result<String, String> {
    let (mut opts, mut uids, invalid) = profile_options(&spec)?;
    let mut b = String::new();
    for (i, data) in [first, second].iter().enumerate() {
        let mut f = object::read_file(&bytes(data)).map_err(|(_, e)| e.to_string())?;
        let planned = if invalid {
            Err(INVALID_OPTIONS.to_string())
        } else {
            deid::plan_basic_profile(&f.dataset, &mut opts, &mut uids).map_err(|e| e.to_string())
        };
        let mut plan = match planned {
            Ok(plan) => plan,
            Err(err) => {
                b.push_str(&format!("plan {i} error={err}\n"));
                continue;
            }
        };
        b.push_str(&format!("plan {i} {}\n", plan.report().json()));
        if i == 1 {
            f.dataset
                .put(raw(Tag::new(0x0010, 0x4000), Vr::LT, b"changed "));
            if let Err(err) = plan.apply(&mut f.dataset) {
                b.push_str(&format!("stale error={err}\n"));
            }
            continue;
        }
        if let Err(err) = plan.apply(&mut f.dataset) {
            b.push_str(&format!("apply error={err}\n"));
            continue;
        }
        if let Err(err) = plan.apply(&mut f.dataset) {
            b.push_str(&format!("reapply error={err}\n"));
        }
        b.push_str(&render_file(f));
    }
    Ok(b)
}
pub fn pseudonyms(
    name_template: String,
    id_template: String,
    secret: String,
    identities: String,
) -> Result<String, String> {
    let template = PseudonymTemplate {
        patient_name: name_template,
        patient_id: id_template,
    };
    let mut p = Pseudonymizer::new(&template, secret.as_bytes()).map_err(|e| e.to_string())?;
    let mut b = String::new();
    for identity in identities.split('\n') {
        match p.pseudonym(identity) {
            Ok(x) => b.push_str(&format!(
                "{} {}\n",
                quote(&x.patient_name),
                quote(&x.patient_id)
            )),
            Err(err) => b.push_str(&format!("error={err}\n")),
        }
    }
    Ok(b)
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Anonymize" => {
            let out = anonymize(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
                arg(args, 6)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "AnonymizeFile" => {
            let out = anonymize_file(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
                arg(args, 6)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "RemapHierarchy" => {
            let out = remap_hierarchy(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "AssessRisk" => {
            let out = assess_risk(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ResolveAction" => {
            let out = resolve_action(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Profile" => {
            let out = profile(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ProfileFile" => {
            let out = profile_file(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ProfileTwice" => {
            let out = profile_twice(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Pseudonyms" => {
            let out = pseudonyms(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?, arg(args, 3)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
