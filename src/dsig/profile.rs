use super::profile_catalog::{CURVE_PROFILE_ELEMENTS, OVERLAY_PROFILE_ELEMENTS, PROFILE_MODULES};
use super::stream::excluded;
use super::{
    CERTIFICATE_TYPE_X509, Code, Kind, PublicKey, Signature, TAG_CERTIFICATE_OF_SIGNER,
    TAG_CERTIFICATE_TYPE, TAG_CERTIFIED_TIMESTAMP, TAG_CERTIFIED_TIMESTAMP_TYPE, TAG_CODE_VALUE,
    TAG_CODING_SCHEME_DESIGNATOR, TAG_DATA_ELEMENTS_SIGNED, TAG_DIGITAL_SIGNATURE_PURPOSE_CODE_SEQ,
    TAG_DIGITAL_SIGNATURES_SEQUENCE, TAG_MAC_ALGORITHM, TAG_MAC_ID_NUMBER,
    TAG_MAC_PARAMETERS_SEQUENCE, TAG_SIGNATURE, bytes_value, find, parse_certificate,
    sequence_items, tag, tag_values, uint16_value,
};
use crate::attrpath::{self, Segment};
use crate::dictionary::reservations::private_tag;
use crate::dictionary::uid::{StorageCategory, lookup_storage_sop_class};
use crate::object::{Element, Object, Tag, Value};
use crate::validation::iod::modules;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;
pub const PROFILE_STANDARD_VERSION: &str = "PS3.15 2026d";
pub const PROFILE_SOURCE_SHA256: &str =
    "1eb2a9724822476b3f1f3de23d15ae655a4c7f6e9d32bf16f2d9ef76d7d8437c";
pub const PROFILE_MODULE_SOURCE_SHA256: &str =
    "3ae695b956ba57c490933a49220b515aeaedb5ece86f05c4bf7e94ee0fbcc6ef";
pub const PROFILE_COVERAGE_COMPLETE: &str = "complete";
pub const PROFILE_COVERAGE_NEEDS_CONTEXT: &str = "needs-context";
pub const PROFILE_COVERAGE_PARTIAL: &str = "partial";
const TAG_SOP_CLASS_UID: Tag = tag(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = tag(0x0008, 0x0018);
const TAG_STUDY_INSTANCE_UID: Tag = tag(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = tag(0x0020, 0x000E);
const TAG_INSTANCE_CREATION_DATE: Tag = tag(0x0008, 0x0012);
const TAG_INSTANCE_CREATION_TIME: Tag = tag(0x0008, 0x0013);
const TAG_RAW_DATA_MARKER: Tag = tag(0x0008, 0x9123);
const TAG_VERIFICATION_FLAG: Tag = tag(0x0040, 0xA493);
const TAG_VERIFYING_OBSERVER_SEQ: Tag = tag(0x0040, 0xA073);
const TAG_VERIFICATION_DATE_TIME: Tag = tag(0x0040, 0xA030);
const TAG_OBSERVATION_DATE_TIME: Tag = tag(0x0040, 0xA032);
const TAG_CURRENT_REQUESTED_EVID: Tag = tag(0x0040, 0xA375);
const TAG_PERTINENT_OTHER_EVID: Tag = tag(0x0040, 0xA385);
const TAG_PREDECESSOR_DOCUMENTS: Tag = tag(0x0040, 0xA360);
const TAG_REFERENCED_MAC: Tag = tag(0x0400, 0x0403);
const NO_TAG: Tag = tag(0, 0);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Profile {
    #[default]
    None,
    BaseRsa,
    CreatorRsa,
    AuthorizationRsa,
    SrRsa,
    Unknown(i64),
}
impl Profile {
    pub fn from_number(number: i64) -> Profile {
        match number {
            0 => Profile::None,
            1 => Profile::BaseRsa,
            2 => Profile::CreatorRsa,
            3 => Profile::AuthorizationRsa,
            4 => Profile::SrRsa,
            other => Profile::Unknown(other),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Profile::BaseRsa => "Base RSA",
            Profile::CreatorRsa => "Creator RSA",
            Profile::AuthorizationRsa => "Authorization RSA",
            Profile::SrRsa => "Structured Report RSA",
            Profile::None | Profile::Unknown(_) => "none",
        }
    }
    fn creator_or_authorization(self) -> bool {
        matches!(self, Profile::CreatorRsa | Profile::AuthorizationRsa)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileOptions {
    pub human_verified_paths: Option<Vec<String>>,
    pub max_elements: i64,
    pub max_depth: i64,
    pub max_issues: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProfileIssue {
    pub code: &'static str,
    pub tag: Tag,
    pub path: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileReport {
    pub profile: Profile,
    pub standard_version: &'static str,
    pub source_sha256: &'static str,
    pub module_source_sha256: &'static str,
    pub coverage: &'static str,
    pub required_tags: Vec<Tag>,
    pub issues: Vec<ProfileIssue>,
    pub sr_verification_candidate: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileError {
    parts: Vec<(Kind, String)>,
}
impl ProfileError {
    pub fn is(&self, kind: Kind) -> bool {
        self.parts.iter().any(|(k, _)| *k == kind)
    }
}
impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let texts: Vec<&str> = self.parts.iter().map(|(_, t)| t.as_str()).collect();
        f.write_str(&texts.join("\n"))
    }
}
impl std::error::Error for ProfileError {}
impl From<ProfileError> for super::Error {
    fn from(err: ProfileError) -> super::Error {
        super::Error {
            kind: err.parts.first().map_or(Kind::Other, |(k, _)| *k),
            text: err.to_string(),
            also: err.parts.iter().skip(1).map(|(k, _)| *k).collect(),
        }
    }
}
pub(super) fn sign_precheck(
    elements: &[Element],
    listed_tags: usize,
    opts: &ProfileOptions,
) -> Result<(), ProfileError> {
    let fail = |part: Part| Err(ProfileError { parts: vec![part] });
    if opts.max_elements < 0 || opts.max_depth < 0 || opts.max_issues < 0 {
        return fail(limit_detail("negative bound"));
    }
    let max_elements = match opts.max_elements {
        0 => 100_000,
        n => n,
    };
    let max_depth = match opts.max_depth {
        0 => 64,
        n => n,
    };
    if max_depth > 64 {
        return fail(limit_detail("depth exceeds 64"));
    }
    if opts
        .human_verified_paths
        .as_ref()
        .is_some_and(|p| p.len() > 256)
    {
        return fail(limit_detail("human path count exceeds 256"));
    }
    if listed_tags as i64 > max_elements {
        return fail(limit());
    }
    match bounded_walk(elements, max_elements, max_depth, &mut |_, _| {}) {
        Ok(()) => Ok(()),
        Err(part) => fail(part),
    }
}
type Part = (Kind, String);
fn limit() -> Part {
    (Kind::ProfileLimit, Kind::ProfileLimit.text().to_string())
}
fn limit_detail(detail: &str) -> Part {
    (
        Kind::ProfileLimit,
        format!("{}: {detail}", Kind::ProfileLimit.text()),
    )
}
fn allowed_profile_mac(algorithm: &str) -> bool {
    matches!(
        algorithm,
        "RIPEMD160" | "MD5" | "SHA1" | "SHA256" | "SHA384" | "SHA512"
    )
}
fn has_rsa_key(sig: &Signature) -> bool {
    matches!(
        sig.certificate.as_ref().map(|c| &c.public_key),
        Some(PublicKey::Rsa { .. })
    )
}
fn repeating_profile_tag(t: Tag) -> bool {
    t.group.is_multiple_of(2)
        && ((0x6000..=0x601E).contains(&t.group) && OVERLAY_PROFILE_ELEMENTS.contains(&t.element)
            || (0x5000..=0x501E).contains(&t.group) && CURVE_PROFILE_ELEMENTS.contains(&t.element))
}
fn profile_catalog() -> &'static Result<HashMap<Profile, HashSet<Tag>>, Part> {
    static CATALOG: OnceLock<Result<HashMap<Profile, HashSet<Tag>>, Part>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let incomplete = |detail: String| {
            (
                Kind::ProfileIncomplete,
                format!("{}: {detail}", Kind::ProfileIncomplete.text()),
            )
        };
        if modules::SOURCE_SHA256 != PROFILE_MODULE_SOURCE_SHA256
            || modules::STANDARD_VERSION != "PS3.3 2026d"
        {
            return Err(incomplete(
                "module catalog edition requires explicit profile review".to_string(),
            ));
        }
        let mut out: HashMap<Profile, HashSet<Tag>> = HashMap::new();
        for profile in [
            Profile::CreatorRsa,
            Profile::AuthorizationRsa,
            Profile::SrRsa,
        ] {
            out.insert(profile, HashSet::new());
        }
        for module in PROFILE_MODULES {
            let tags: Vec<u32> = match module.tags {
                Some(tags) => tags.to_vec(),
                None => match modules::attributes(module.name) {
                    Some(attributes) if !attributes.is_empty() => {
                        attributes.iter().map(|a| a.tag).collect()
                    }
                    _ => {
                        return Err(incomplete(format!(
                            "required module catalog unavailable: {} ({})",
                            module.name, module.table
                        )));
                    }
                },
            };
            for profile in module.profiles {
                let set = out.entry(*profile).or_default();
                set.extend(tags.iter().map(|&v| tag((v >> 16) as u16, v as u16)));
            }
        }
        Ok(out)
    })
}
fn profile_string(elements: &[Element], t: Tag, max: usize) -> Option<String> {
    let s = match &find(elements, t)?.value {
        Value::Strings(values) => match values.as_slice() {
            [one] if one.len() <= max => one.clone(),
            _ => return None,
        },
        Value::Raw(raw) if raw.len() <= max => String::from_utf8_lossy(raw).into_owned(),
        _ => return None,
    };
    Some(s.trim_end_matches([' ', '\0']).to_string())
}
fn has_verification_purpose(purposes: &[Code]) -> bool {
    purposes
        .iter()
        .any(|p| p.value == "5" && p.scheme == "ASTM-sigpurpose")
}
fn bounded_walk(
    elements: &[Element],
    max_elements: i64,
    max_depth: i64,
    visit: &mut dyn FnMut(&Element, &str),
) -> Result<(), Part> {
    fn walk(
        elements: &[Element],
        prefix: &str,
        depth: i64,
        count: &mut i64,
        bounds: (i64, i64),
        visit: &mut dyn FnMut(&Element, &str),
    ) -> Result<(), Part> {
        if depth > bounds.1 {
            return Err(limit());
        }
        for el in elements {
            *count += 1;
            if *count > bounds.0 {
                return Err(limit());
            }
            let path = format!("{prefix}{}", el.tag());
            visit(el, &path);
            if let Value::Sequence(items) = &el.value {
                for (i, item) in items.iter().enumerate() {
                    *count += 1;
                    if *count > bounds.0 {
                        return Err(limit());
                    }
                    let nested = format!("{path}[{i}].");
                    walk(&item.elements, &nested, depth + 1, count, bounds, visit)?;
                }
            }
        }
        Ok(())
    }
    let mut count = 0;
    walk(
        elements,
        "",
        0,
        &mut count,
        (max_elements, max_depth),
        visit,
    )
}
struct Evaluation<'a> {
    opts: ProfileOptions,
    report: ProfileReport,
    required: BTreeSet<(u16, u16)>,
    issues: HashSet<ProfileIssue>,
    limit_hit: bool,
    present: HashMap<Tag, &'a Element>,
    signed: HashSet<Tag>,
}
impl<'a> Evaluation<'a> {
    fn issue(&mut self, code: &'static str, tag: Tag, path: impl Into<String>) {
        let issue = ProfileIssue {
            code,
            tag,
            path: path.into(),
        };
        if self.issues.contains(&issue) {
            return;
        }
        if self.issues.len() as i64 >= self.opts.max_issues {
            self.limit_hit = true;
            self.report.coverage = PROFILE_COVERAGE_PARTIAL;
            return;
        }
        self.issues.insert(issue);
    }
    fn require(&mut self, t: Tag, path: &str) {
        self.required.insert((t.group, t.element));
        match self.present.get(&t) {
            None => self.issue("required-absent", t, path),
            Some(el) if excluded(el) => self.issue("required-unsignable", t, path),
            Some(_) if !self.signed.contains(&t) => self.issue("required-not-signed", t, path),
            Some(_) => {}
        }
    }
    fn finish(mut self, err: Option<Part>) -> (ProfileReport, Option<ProfileError>) {
        self.report.required_tags = self
            .required
            .iter()
            .map(|&(group, element)| tag(group, element))
            .collect();
        let mut issues: Vec<ProfileIssue> = self.issues.into_iter().collect();
        issues.sort_by(|a, b| {
            (&a.path, a.tag.group, a.tag.element, a.code).cmp(&(
                &b.path,
                b.tag.group,
                b.tag.element,
                b.code,
            ))
        });
        self.report.issues = issues;
        if err.is_some() || self.limit_hit {
            self.report.coverage = PROFILE_COVERAGE_PARTIAL;
        }
        let mut parts: Vec<Part> = err.into_iter().collect();
        if !self.report.issues.is_empty() {
            parts.push((
                Kind::ProfileViolation,
                format!(
                    "{} ({}): {} attribute-profile issue(s)",
                    Kind::ProfileViolation.text(),
                    self.report.profile.name(),
                    self.report.issues.len()
                ),
            ));
        }
        if self.report.coverage == PROFILE_COVERAGE_NEEDS_CONTEXT {
            parts.push((
                Kind::ProfileIncomplete,
                Kind::ProfileIncomplete.text().to_string(),
            ));
        }
        if self.limit_hit {
            parts.push(limit());
        }
        let error = (!parts.is_empty()).then_some(ProfileError { parts });
        (self.report, error)
    }
    fn require_human_paths(
        &mut self,
        elements: &'a [Element],
        paths: &[String],
    ) -> Result<(), Part> {
        for (n, text) in paths.iter().enumerate() {
            if text.len() > attrpath::MAX_PATH_LENGTH {
                return Err(limit_detail("human path length exceeds 4096"));
            }
            let path = match attrpath::parse(text) {
                Ok(path) if !path.is_empty() && !path.wildcard() && !path.selects_item() => path,
                _ => {
                    self.issue("human-path-invalid", NO_TAG, format!("declaration[{n}]"));
                    continue;
                }
            };
            let mut current = elements;
            let mut root = NO_TAG;
            let mut resolved: Vec<String> = Vec::new();
            let mut found = true;
            for (i, seg) in path.0.iter().enumerate() {
                let Some(t) = segment_tag(current, seg) else {
                    found = false;
                    break;
                };
                if i == 0 {
                    root = t;
                }
                resolved.push(if seg.has_item {
                    format!("{t}[{}]", seg.item)
                } else {
                    t.to_string()
                });
                let Some(el) = find(current, t) else {
                    found = false;
                    break;
                };
                if seg.has_item {
                    match &el.value {
                        Value::Sequence(items) if seg.item < items.len() => {
                            current = &items[seg.item].elements;
                        }
                        _ => {
                            found = false;
                            break;
                        }
                    }
                }
            }
            let canonical = resolved.join(".");
            if found {
                self.require(root, &canonical);
            } else {
                self.issue("human-path-not-found", root, canonical);
            }
        }
        Ok(())
    }
    fn check_sr(&mut self, sig: &'a Signature) -> Result<(), Part> {
        let data_set: &'a [Element] = &sig.data_set;
        let sop = profile_string(data_set, TAG_SOP_CLASS_UID, 64);
        let report_class = sop.as_deref().is_some_and(|sop| {
            lookup_storage_sop_class(sop).is_some_and(|class| {
                class.category == StorageCategory::StructuredReport
                    || sop == "1.2.840.10008.5.1.4.1.1.88.59"
            })
        });
        if !report_class {
            self.issue("sr-or-kos-required", TAG_SOP_CLASS_UID, "");
        }
        if sig.purposes.is_empty() {
            self.issue(
                "sr-purpose-required",
                TAG_DIGITAL_SIGNATURE_PURPOSE_CODE_SEQ,
                "",
            );
        }
        if let Some(observer) = self.present.get(&TAG_VERIFYING_OBSERVER_SEQ)
            && !matches!(observer.value, Value::Sequence(_))
        {
            self.issue("sr-observer-sequence", TAG_VERIFYING_OBSERVER_SEQ, "");
        }
        let observers = sequence_items(data_set, TAG_VERIFYING_OBSERVER_SEQ);
        if observers.len() > 1 {
            self.issue("sr-observer-count", TAG_VERIFYING_OBSERVER_SEQ, "");
        }
        let signable =
            |elements: &[Element], t: Tag| find(elements, t).is_some_and(|el| !excluded(el));
        if profile_string(data_set, TAG_VERIFICATION_FLAG, 16).as_deref() == Some("VERIFIED") {
            match observers {
                [observer] => {
                    if !signable(&observer.elements, TAG_VERIFICATION_DATE_TIME) {
                        self.issue(
                            "sr-verification-datetime-required",
                            TAG_VERIFICATION_DATE_TIME,
                            "(0040,A073)[0].(0040,A030)",
                        );
                    }
                }
                _ => self.issue(
                    "sr-verified-observer-required",
                    TAG_VERIFYING_OBSERVER_SEQ,
                    "",
                ),
            }
            let extra = [
                TAG_SOP_INSTANCE_UID,
                TAG_VERIFICATION_FLAG,
                TAG_VERIFYING_OBSERVER_SEQ,
            ];
            if has_verification_purpose(&sig.purposes) {
                for t in extra {
                    self.require(t, "");
                }
            }
            let candidate_ok = |e: &Evaluation<'a>, candidate: &Signature| {
                if !candidate.path.is_empty()
                    || !has_verification_purpose(&candidate.purposes)
                    || !base_candidate(candidate)
                {
                    return false;
                }
                let covered = |t: Tag| {
                    e.present.get(&t).is_some_and(|el| !excluded(el))
                        && candidate.data_elements_signed.contains(&t)
                };
                e.required.iter().all(|&(g, el)| covered(tag(g, el)))
                    && extra.into_iter().all(covered)
                    && matches!(observers, [o] if signable(&o.elements, TAG_VERIFICATION_DATE_TIME))
            };
            let mut candidate = candidate_ok(self, sig);
            if !candidate {
                candidate = sr_candidates(data_set, self.opts.max_elements)?
                    .iter()
                    .any(|c| candidate_ok(self, c));
            }
            self.report.sr_verification_candidate = candidate;
            if !candidate {
                self.issue(
                    "sr-verification-candidate-required",
                    TAG_DIGITAL_SIGNATURE_PURPOSE_CODE_SEQ,
                    "",
                );
            }
        }
        let mut found: Vec<ProfileIssue> = Vec::new();
        bounded_walk(
            data_set,
            self.opts.max_elements,
            self.opts.max_depth,
            &mut |el, path| {
                if el.tag() != TAG_REFERENCED_MAC {
                    return;
                }
                let items = match &el.value {
                    Value::Sequence(items) if !items.is_empty() => items,
                    _ => {
                        found.push(ProfileIssue {
                            code: "sr-referenced-mac-sequence",
                            tag: TAG_REFERENCED_MAC,
                            path: path.to_string(),
                        });
                        return;
                    }
                };
                for (i, item) in items.iter().enumerate() {
                    let algorithm = profile_string(&item.elements, TAG_MAC_ALGORITHM, 16);
                    if !algorithm.as_deref().is_some_and(allowed_profile_mac) {
                        found.push(ProfileIssue {
                            code: "sr-referenced-mac-algorithm",
                            tag: TAG_MAC_ALGORITHM,
                            path: format!("{path}[{i}].(0400,0015)"),
                        });
                    }
                }
            },
        )?;
        for issue in found {
            self.issue(issue.code, issue.tag, issue.path);
        }
        Ok(())
    }
}
fn segment_tag(elements: &[Element], segment: &Segment) -> Option<Tag> {
    if !segment.private() {
        return Some(segment.tag);
    }
    let object = Object::from_elements(elements.to_vec());
    private_tag(
        &object,
        segment.tag.group,
        &segment.creator,
        segment.tag.element as u8,
    )
    .ok()
}
fn base_candidate(sig: &Signature) -> bool {
    sig.mac_algorithm.len() <= 16
        && allowed_profile_mac(&sig.mac_algorithm)
        && sig.certificate_type == CERTIFICATE_TYPE_X509
        && has_rsa_key(sig)
}
fn sr_candidates(elements: &[Element], max_elements: i64) -> Result<Vec<Signature>, Part> {
    let items = sequence_items(elements, TAG_DIGITAL_SIGNATURES_SEQUENCE);
    let macs = sequence_items(elements, TAG_MAC_PARAMETERS_SEQUENCE);
    if items.len() > 256 || macs.len() > 256 {
        return Err(limit());
    }
    let mut out = Vec::new();
    let mut certificate_bytes = 0;
    for item in items {
        let fields = &item.elements;
        let purpose = sequence_items(fields, TAG_DIGITAL_SIGNATURE_PURPOSE_CODE_SEQ)
            .iter()
            .any(|code| {
                profile_string(&code.elements, TAG_CODE_VALUE, 16).as_deref() == Some("5")
                    && profile_string(&code.elements, TAG_CODING_SCHEME_DESIGNATOR, 16).as_deref()
                        == Some("ASTM-sigpurpose")
            });
        if !purpose {
            continue;
        }
        let cert = bytes_value(fields, TAG_CERTIFICATE_OF_SIGNER);
        if cert.len() > 65536 {
            return Err(limit());
        }
        certificate_bytes += cert.len();
        if certificate_bytes > 1 << 20 {
            return Err(limit());
        }
        let mut s = Signature {
            purposes: vec![Code {
                value: "5".to_string(),
                scheme: "ASTM-sigpurpose".to_string(),
                meaning: String::new(),
            }],
            certificate_type: profile_string(fields, TAG_CERTIFICATE_TYPE, 16).unwrap_or_default(),
            certificate: parse_certificate(&cert),
            certified_timestamp_type: profile_string(fields, TAG_CERTIFIED_TIMESTAMP_TYPE, 16)
                .unwrap_or_default(),
            certified_timestamp: bytes_value(fields, TAG_CERTIFIED_TIMESTAMP),
            ..Signature::default()
        };
        if bytes_value(fields, TAG_SIGNATURE).is_empty() {
            continue;
        }
        let Some(id) = uint16_value(fields, TAG_MAC_ID_NUMBER) else {
            continue;
        };
        if let Some(mac) = macs
            .iter()
            .find(|mac| uint16_value(&mac.elements, TAG_MAC_ID_NUMBER) == Some(id))
        {
            s.mac_algorithm =
                profile_string(&mac.elements, TAG_MAC_ALGORITHM, 16).unwrap_or_default();
            if let Some(at) = find(&mac.elements, TAG_DATA_ELEMENTS_SIGNED) {
                if let Value::Raw(raw) = &at.value
                    && (raw.len() / 4) as i64 > max_elements
                {
                    return Err(limit());
                }
                s.data_elements_signed = tag_values(&mac.elements, TAG_DATA_ELEMENTS_SIGNED);
            }
        }
        out.push(s);
    }
    Ok(out)
}
pub fn check_profile(sig: &Signature, profile: Profile) -> Result<(), ProfileError> {
    match check_profile_with_options(sig, profile, &ProfileOptions::default()).1 {
        None => Ok(()),
        Some(err) => Err(err),
    }
}
pub fn check_profile_with_options(
    sig: &Signature,
    profile: Profile,
    opts: &ProfileOptions,
) -> (ProfileReport, Option<ProfileError>) {
    let mut e = Evaluation {
        opts: opts.clone(),
        report: ProfileReport {
            profile,
            standard_version: PROFILE_STANDARD_VERSION,
            source_sha256: PROFILE_SOURCE_SHA256,
            module_source_sha256: PROFILE_MODULE_SOURCE_SHA256,
            coverage: PROFILE_COVERAGE_COMPLETE,
            ..ProfileReport::default()
        },
        required: BTreeSet::new(),
        issues: HashSet::new(),
        limit_hit: false,
        present: HashMap::new(),
        signed: HashSet::new(),
    };
    if opts.max_elements < 0 || opts.max_depth < 0 || opts.max_issues < 0 {
        return e.finish(Some(limit_detail("negative bound")));
    }
    if e.opts.max_elements == 0 {
        e.opts.max_elements = 100_000;
    }
    if e.opts.max_depth == 0 {
        e.opts.max_depth = 64;
    }
    if e.opts.max_issues == 0 {
        e.opts.max_issues = 256;
    }
    if e.opts.max_depth > 64 {
        return e.finish(Some(limit_detail("depth exceeds 64")));
    }
    if opts
        .human_verified_paths
        .as_ref()
        .is_some_and(|p| p.len() > 256)
    {
        return e.finish(Some(limit_detail("human path count exceeds 256")));
    }
    match profile {
        Profile::None => return e.finish(None),
        Profile::Unknown(number) => {
            return e.finish(Some((
                Kind::Other,
                format!("dicom dsig: unknown profile {number}"),
            )));
        }
        _ => {}
    }
    let max = e.opts.max_elements;
    if sig.data_elements_signed.len() as i64 > max || sig.purposes.len() as i64 > max {
        return e.finish(Some(limit()));
    }
    if let Err(err) = bounded_walk(&sig.data_set, max, e.opts.max_depth, &mut |_, _| {}) {
        return e.finish(Some(err));
    }
    if sig.mac_algorithm.len() > 16 || !allowed_profile_mac(&sig.mac_algorithm) {
        e.issue("base-mac-algorithm", TAG_MAC_ALGORITHM, "");
    }
    if sig.certificate_type != CERTIFICATE_TYPE_X509 {
        e.issue("base-certificate-type", TAG_CERTIFICATE_TYPE, "");
    }
    if sig.certificate.is_none() {
        e.issue("base-certificate-unreadable", TAG_CERTIFICATE_OF_SIGNER, "");
    } else if !has_rsa_key(sig) {
        e.issue("base-key-not-rsa", TAG_CERTIFICATE_OF_SIGNER, "");
    }
    if profile == Profile::BaseRsa {
        return e.finish(None);
    }
    if !sig.path.is_empty() {
        e.issue("main-dataset-required", NO_TAG, "");
    }
    let catalog = match profile_catalog() {
        Ok(catalog) => catalog.get(&profile),
        Err(err) => return e.finish(Some(err.clone())),
    };
    e.signed = sig.data_elements_signed.iter().copied().collect();
    e.present = sig.data_set.iter().map(|el| (el.tag(), el)).collect();
    for t in [
        TAG_SOP_CLASS_UID,
        TAG_STUDY_INSTANCE_UID,
        TAG_SERIES_INSTANCE_UID,
    ] {
        e.require(t, "");
    }
    if profile.creator_or_authorization() {
        e.require(TAG_SOP_INSTANCE_UID, "");
    }
    for el in &sig.data_set {
        let t = el.tag();
        if catalog.is_some_and(|c| c.contains(&t))
            || profile.creator_or_authorization() && repeating_profile_tag(t)
        {
            e.require(t, "");
        }
    }
    if profile == Profile::CreatorRsa {
        for t in [TAG_INSTANCE_CREATION_DATE, TAG_INSTANCE_CREATION_TIME] {
            if e.present.contains_key(&t) {
                e.require(t, "");
            }
        }
    }
    if profile.creator_or_authorization() && e.present.contains_key(&TAG_RAW_DATA_MARKER) {
        for el in &sig.data_set {
            let t = el.tag();
            if t.group % 2 == 1 && t.group >= 0x0009 && t.group != 0xFFFF && t.element != 0 {
                e.require(t, "");
            }
        }
    }
    if profile == Profile::AuthorizationRsa {
        match &opts.human_verified_paths {
            None => {
                e.report.coverage = PROFILE_COVERAGE_NEEDS_CONTEXT;
                e.issue("human-verified-context-required", NO_TAG, "");
            }
            Some(paths) => {
                if let Err(err) = e.require_human_paths(&sig.data_set, paths) {
                    return e.finish(Some(err));
                }
            }
        }
    }
    if profile == Profile::SrRsa {
        for t in [
            TAG_CURRENT_REQUESTED_EVID,
            TAG_PERTINENT_OTHER_EVID,
            TAG_PREDECESSOR_DOCUMENTS,
            TAG_OBSERVATION_DATE_TIME,
        ] {
            if e.present.contains_key(&t) {
                e.require(t, "");
            }
        }
        if let Err(err) = e.check_sr(sig) {
            return e.finish(Some(err));
        }
    }
    e.finish(None)
}
