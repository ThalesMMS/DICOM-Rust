use super::pseudonym::{Pseudonymizer, patient_pseudonym_identity};
use super::safe_private::{SafePrivateRegistry, SafePrivateRule};
use super::tables;
use super::{
    CsaOutcome, CsaScrubReport, Error, Kind, PixelRegion, TAG_BURNED_IN_ANNOTATION, TAG_PATIENT_ID,
    TAG_PATIENT_NAME, TAG_PIXEL_DATA, TAG_RECOGNIZABLE_VISUAL_FEATURES, UidRemapper, clone_file,
    is_csa_header, is_private_creator_tag, pad_text, private_creator_tag_for_data_tag, raw_element,
    rederive_file_meta, scrub_csa_element, single_cs, string_value, string_values, trim_space,
};
use crate::dcmtime;
use crate::object::{
    DataSet, Element, ElementHeader, File, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
};
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_PATIENT_IDENTITY_REMOVED: Tag = Tag::new(0x0012, 0x0062);
const TAG_DEIDENTIFICATION_METHOD: Tag = Tag::new(0x0012, 0x0063);
const TAG_DEIDENTIFICATION_METHOD_CODE_SEQUENCE: Tag = Tag::new(0x0012, 0x0064);
const TAG_LONGITUDINAL_TEMPORAL_INFORMATION: Tag = Tag::new(0x0028, 0x0303);
const TAG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0100);
const TAG_CODING_SCHEME_DESIGNATOR: Tag = Tag::new(0x0008, 0x0102);
const TAG_CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
const TAG_LONG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0119);
const TAG_URN_CODE_VALUE: Tag = Tag::new(0x0008, 0x0120);
const TAG_REFERENCED_SOP_SEQUENCE: Tag = Tag::new(0x0008, 0x1199);
const TAG_CONTENT_VALUE_TYPE: Tag = Tag::new(0x0040, 0xA040);
const TAG_CONCEPT_NAME_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0xA043);
const TAG_STRUCTURED_DATETIME_VALUE: Tag = Tag::new(0x0040, 0xA120);
const TAG_STRUCTURED_DATE_VALUE: Tag = Tag::new(0x0040, 0xA121);
const TAG_STRUCTURED_TIME_VALUE: Tag = Tag::new(0x0040, 0xA122);
const TAG_STRUCTURED_PERSON_NAME_VALUE: Tag = Tag::new(0x0040, 0xA123);
const TAG_STRUCTURED_UID_VALUE: Tag = Tag::new(0x0040, 0xA124);
const TAG_STRUCTURED_CONCEPT_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0xA168);
const TAG_STRUCTURED_TEXT_VALUE: Tag = Tag::new(0x0040, 0xA160);
const TAG_STRUCTURED_MEASURED_VALUE_SEQUENCE: Tag = Tag::new(0x0040, 0xA300);
const TAG_ACQUISITION_CONTEXT_SEQUENCE: Tag = Tag::new(0x0040, 0x0555);
const TAG_SPECIMEN_PREPARATION_CONTENT_SEQUENCE: Tag = Tag::new(0x0040, 0x0612);
const TAG_CONTENT_SEQUENCE: Tag = Tag::new(0x0040, 0xA730);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionCode {
    X,
    Z,
    D,
    C,
    U,
    K,
    ZD,
    XZ,
    XD,
    XZD,
    XZU,
}
impl ActionCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionCode::X => "X",
            ActionCode::Z => "Z",
            ActionCode::D => "D",
            ActionCode::C => "C",
            ActionCode::U => "U",
            ActionCode::K => "K",
            ActionCode::ZD => "Z/D",
            ActionCode::XZ => "X/Z",
            ActionCode::XD => "X/D",
            ActionCode::XZD => "X/Z/D",
            ActionCode::XZU => "X/Z/U*",
        }
    }
    pub fn parse(s: &str) -> Option<ActionCode> {
        [
            ActionCode::X,
            ActionCode::Z,
            ActionCode::D,
            ActionCode::C,
            ActionCode::U,
            ActionCode::K,
            ActionCode::ZD,
            ActionCode::XZ,
            ActionCode::XD,
            ActionCode::XZD,
            ActionCode::XZU,
        ]
        .into_iter()
        .find(|c| c.as_str() == s)
    }
    fn conditional(self) -> bool {
        matches!(
            self,
            ActionCode::ZD | ActionCode::XZ | ActionCode::XD | ActionCode::XZD | ActionCode::XZU
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProfileAction {
    Remove,
    Zero,
    Dummy,
    Clean,
    RemapUid,
    Keep,
}
impl ProfileAction {
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileAction::Remove => "remove",
            ProfileAction::Zero => "zero",
            ProfileAction::Dummy => "dummy",
            ProfileAction::Clean => "clean",
            ProfileAction::RemapUid => "uid-remap",
            ProfileAction::Keep => "keep",
        }
    }
    pub fn parse(s: &str) -> Option<ProfileAction> {
        [
            ProfileAction::Remove,
            ProfileAction::Zero,
            ProfileAction::Dummy,
            ProfileAction::Clean,
            ProfileAction::RemapUid,
            ProfileAction::Keep,
        ]
        .into_iter()
        .find(|a| a.as_str() == s)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttributeRequirement(pub u8);
impl AttributeRequirement {
    #[doc(hidden)]
    pub const OPTIONAL: AttributeRequirement = AttributeRequirement(0);
    #[doc(hidden)]
    pub const TYPE2: AttributeRequirement = AttributeRequirement(1);
    #[doc(hidden)]
    pub const TYPE1: AttributeRequirement = AttributeRequirement(2);
    pub fn as_str(self) -> &'static str {
        match self.0 {
            0 => "optional",
            1 => "type2",
            2 => "type1",
            _ => "invalid",
        }
    }
}
pub fn resolve_action_code(
    code: ActionCode,
    requirement: AttributeRequirement,
) -> Result<ProfileAction, Error> {
    use ActionCode as C;
    use ProfileAction as P;
    if requirement > AttributeRequirement::TYPE1 {
        return Err(Error::bare(Kind::InvalidActionCode));
    }
    let t1 = requirement == AttributeRequirement::TYPE1;
    let t2 = requirement == AttributeRequirement::TYPE2;
    Ok(match code {
        C::X => P::Remove,
        C::Z => P::Zero,
        C::D => P::Dummy,
        C::C => P::Clean,
        C::U => P::RemapUid,
        C::K => P::Keep,
        C::ZD => {
            if t1 {
                P::Dummy
            } else {
                P::Zero
            }
        }
        C::XZ => {
            if t1 || t2 {
                P::Zero
            } else {
                P::Remove
            }
        }
        C::XD => {
            if t1 {
                P::Dummy
            } else {
                P::Remove
            }
        }
        C::XZD => {
            if t1 {
                P::Dummy
            } else if t2 {
                P::Zero
            } else {
                P::Remove
            }
        }
        C::XZU => {
            if t1 {
                P::RemapUid
            } else if t2 {
                P::Zero
            } else {
                P::Remove
            }
        }
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProfileOption {
    CleanPixelData,
    CleanRecognizableFeatures,
    CleanGraphics,
    CleanStructuredContent,
    CleanDescriptors,
    RetainFullDates,
    RetainModifiedDates,
    RetainPatientCharacteristics,
    RetainDeviceIdentity,
    RetainInstitutionIdentity,
    RetainUids,
    RetainSafePrivate,
}
const PROFILE_OPTION_ORDER: [ProfileOption; 12] = [
    ProfileOption::CleanPixelData,
    ProfileOption::CleanRecognizableFeatures,
    ProfileOption::CleanGraphics,
    ProfileOption::CleanStructuredContent,
    ProfileOption::CleanDescriptors,
    ProfileOption::RetainFullDates,
    ProfileOption::RetainModifiedDates,
    ProfileOption::RetainPatientCharacteristics,
    ProfileOption::RetainDeviceIdentity,
    ProfileOption::RetainInstitutionIdentity,
    ProfileOption::RetainUids,
    ProfileOption::RetainSafePrivate,
];
const TABLE_OPTION_COLUMNS: [(ProfileOption, usize); 10] = [
    (ProfileOption::RetainSafePrivate, 1),
    (ProfileOption::RetainUids, 2),
    (ProfileOption::RetainDeviceIdentity, 3),
    (ProfileOption::RetainInstitutionIdentity, 4),
    (ProfileOption::RetainPatientCharacteristics, 5),
    (ProfileOption::RetainFullDates, 6),
    (ProfileOption::RetainModifiedDates, 7),
    (ProfileOption::CleanDescriptors, 8),
    (ProfileOption::CleanStructuredContent, 9),
    (ProfileOption::CleanGraphics, 10),
];
const STRUCTURED_CONTENT_OPTION_COLUMNS: [(ProfileOption, usize); 7] = [
    (ProfileOption::RetainUids, 1),
    (ProfileOption::RetainDeviceIdentity, 2),
    (ProfileOption::RetainInstitutionIdentity, 3),
    (ProfileOption::RetainPatientCharacteristics, 4),
    (ProfileOption::RetainFullDates, 5),
    (ProfileOption::RetainModifiedDates, 6),
    (ProfileOption::CleanDescriptors, 7),
];
impl ProfileOption {
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileOption::CleanPixelData => "clean-pixel-data",
            ProfileOption::CleanRecognizableFeatures => "clean-recognizable-visual-features",
            ProfileOption::CleanGraphics => "clean-graphics",
            ProfileOption::CleanStructuredContent => "clean-structured-content",
            ProfileOption::CleanDescriptors => "clean-descriptors",
            ProfileOption::RetainFullDates => "retain-full-dates",
            ProfileOption::RetainModifiedDates => "retain-modified-dates",
            ProfileOption::RetainPatientCharacteristics => "retain-patient-characteristics",
            ProfileOption::RetainDeviceIdentity => "retain-device-identity",
            ProfileOption::RetainInstitutionIdentity => "retain-institution-identity",
            ProfileOption::RetainUids => "retain-uids",
            ProfileOption::RetainSafePrivate => "retain-safe-private",
        }
    }
    pub fn parse(s: &str) -> Option<ProfileOption> {
        PROFILE_OPTION_ORDER.into_iter().find(|o| o.as_str() == s)
    }
    fn method_code(self) -> (&'static str, &'static str) {
        match self {
            ProfileOption::CleanPixelData => ("113101", "Clean Pixel Data Option"),
            ProfileOption::CleanRecognizableFeatures => {
                ("113102", "Clean Recognizable Visual Features Option")
            }
            ProfileOption::CleanGraphics => ("113103", "Clean Graphics Option"),
            ProfileOption::CleanStructuredContent => ("113104", "Clean Structured Content Option"),
            ProfileOption::CleanDescriptors => ("113105", "Clean Descriptors Option"),
            ProfileOption::RetainFullDates => (
                "113106",
                "Retain Longitudinal Temporal Information Full Dates Option",
            ),
            ProfileOption::RetainModifiedDates => (
                "113107",
                "Retain Longitudinal Temporal Information Modified Dates Option",
            ),
            ProfileOption::RetainPatientCharacteristics => {
                ("113108", "Retain Patient Characteristics Option")
            }
            ProfileOption::RetainDeviceIdentity => ("113109", "Retain Device Identity Option"),
            ProfileOption::RetainUids => ("113110", "Retain UIDs Option"),
            ProfileOption::RetainSafePrivate => ("113111", "Retain Safe Private Option"),
            ProfileOption::RetainInstitutionIdentity => {
                ("113112", "Retain Institution Identity Option")
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfileLimits {
    pub max_depth: i64,
    pub max_elements: i64,
    pub max_items: i64,
    pub max_value_bytes: i64,
    pub max_actions: i64,
    pub max_pixel_masks: i64,
    pub max_report_bytes: i64,
}
impl ProfileLimits {
    pub fn defaults() -> ProfileLimits {
        ProfileLimits {
            max_depth: 64,
            max_elements: 100_000,
            max_items: 100_000,
            max_value_bytes: 2 << 30,
            max_actions: 100_000,
            max_pixel_masks: 10_000,
            max_report_bytes: 8 << 20,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttributePathStep {
    pub sequence_tag: Tag,
    pub item_index: usize,
}
#[derive(Clone, Debug)]
pub struct AttributeContext {
    pub path: Vec<AttributePathStep>,
    pub tag: Tag,
    pub vr: Vr,
    pub sop_class_uid: String,
    pub action_code: ActionCode,
}
#[derive(Clone, Debug)]
pub struct ElementContext {
    pub attribute: AttributeContext,
    pub element: Element,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackError;
impl std::fmt::Display for CallbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("deid: profile callback failed")
    }
}
impl std::error::Error for CallbackError {}
pub type AttributeRequirementResolver =
    Box<dyn Fn(&AttributeContext) -> Result<AttributeRequirement, CallbackError>>;
pub type ElementCleaner = Box<dyn Fn(&ElementContext) -> Result<Element, CallbackError>>;
pub type DummyValueProvider = Box<dyn Fn(&ElementContext) -> Result<Element, CallbackError>>;
pub type DateShiftPolicy = Box<dyn Fn(&Object) -> Result<i64, CallbackError>>;
pub type ProfilePixelCleaner = Box<dyn Fn(&mut Object) -> Result<Vec<PixelRegion>, CallbackError>>;
pub type VisualFeaturesCleaner = Box<dyn Fn(&mut Object) -> Result<(), CallbackError>>;
fn guard<T>(f: impl FnOnce() -> Result<T, CallbackError>) -> Result<T, Error> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        _ => Err(Error::bare(Kind::ProfileCallback)),
    }
}
#[derive(Default)]
pub struct BasicProfileOptions {
    pub selected_options: Vec<ProfileOption>,
    pub requirement_resolver: Option<AttributeRequirementResolver>,
    pub require_resolved_conditional: bool,
    pub cleaner: Option<ElementCleaner>,
    pub dummy_value_provider: Option<DummyValueProvider>,
    pub date_shift_policy: Option<DateShiftPolicy>,
    pub safe_private_registry: Option<SafePrivateRegistry>,
    pub pixel_cleaner: Option<ProfilePixelCleaner>,
    pub visual_features_cleaner: Option<VisualFeaturesCleaner>,
    pub pseudonymizer: Option<Pseudonymizer>,
    pub limits: ProfileLimits,
    pub pixel_redaction: Option<super::pixel_redaction::PixelRedaction>,
    #[cfg(feature = "deid-encrypted")]
    pub encrypted_attributes: Option<super::encrypted::EncryptedAttributesOptions>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileActionRecord {
    pub path: Vec<AttributePathStep>,
    pub tag: Tag,
    pub action: ProfileAction,
    pub action_code: ActionCode,
    pub reason: &'static str,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResidualRisk {
    pub code: &'static str,
    pub tag: Tag,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfileSummary {
    pub removed: i64,
    pub zeroed: i64,
    pub dummied: i64,
    pub cleaned: i64,
    pub uids_remapped: i64,
    pub uids_retained: i64,
    pub uids_unresolved: i64,
    pub uid_collisions: i64,
    pub private_removed: i64,
    pub private_retained: i64,
    pub dates_shifted: i64,
    pub pixel_masks: i64,
    pub patient_pseudonyms: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileReport {
    pub standard_version: String,
    pub table_source: String,
    pub table_checksum: String,
    pub structured_content_source: String,
    pub structured_content_checksum: String,
    pub retired_code_source: String,
    pub retired_code_checksum: String,
    pub safe_private_version: String,
    pub safe_private_checksum: String,
    pub complete: bool,
    pub options: Vec<ProfileOption>,
    pub actions: Vec<ProfileActionRecord>,
    pub residual_risks: Vec<ResidualRisk>,
    pub summary: ProfileSummary,
    pub csa: CsaScrubReport,
    pub pixel_redaction: Option<super::pixel_redaction::PixelRedactionReport>,
    #[cfg(feature = "deid-encrypted")]
    pub encrypted_attributes: Option<super::encrypted::EncryptedAttributesReport>,
}
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn json_tag(t: Tag) -> String {
    format!("{{\"Group\":{},\"Element\":{}}}", t.group, t.element)
}
fn json_path(path: &[AttributePathStep]) -> String {
    let steps: Vec<String> = path
        .iter()
        .map(|s| {
            format!(
                "{{\"sequence_tag\":{},\"item_index\":{}}}",
                json_tag(s.sequence_tag),
                s.item_index
            )
        })
        .collect();
    format!("[{}]", steps.join(","))
}
impl ProfileActionRecord {
    pub fn json(&self) -> String {
        let mut out = String::from("{");
        if !self.path.is_empty() {
            out.push_str(&format!("\"path\":{},", json_path(&self.path)));
        }
        out.push_str(&format!(
            "\"tag\":{},\"action\":{},\"action_code\":{},\"reason\":{}}}",
            json_tag(self.tag),
            json_string(self.action.as_str()),
            json_string(self.action_code.as_str()),
            json_string(self.reason)
        ));
        out
    }
}
impl ResidualRisk {
    pub fn json(&self) -> String {
        format!(
            "{{\"code\":{},\"tag\":{}}}",
            json_string(self.code),
            json_tag(self.tag)
        )
    }
}
impl ProfileReport {
    pub fn json(&self) -> String {
        let mut f: Vec<String> = vec![
            format!(
                "\"standard_version\":{}",
                json_string(&self.standard_version)
            ),
            format!("\"table_source\":{}", json_string(&self.table_source)),
            format!("\"table_checksum\":{}", json_string(&self.table_checksum)),
        ];
        for (name, value) in [
            ("structured_content_source", &self.structured_content_source),
            (
                "structured_content_checksum",
                &self.structured_content_checksum,
            ),
            ("retired_code_source", &self.retired_code_source),
            ("retired_code_checksum", &self.retired_code_checksum),
            ("safe_private_version", &self.safe_private_version),
            ("safe_private_checksum", &self.safe_private_checksum),
        ] {
            if !value.is_empty() {
                f.push(format!("\"{name}\":{}", json_string(value)));
            }
        }
        f.push(format!("\"complete\":{}", self.complete));
        if !self.options.is_empty() {
            let options: Vec<String> = self
                .options
                .iter()
                .map(|o| json_string(o.as_str()))
                .collect();
            f.push(format!("\"options\":[{}]", options.join(",")));
        }
        if !self.actions.is_empty() {
            let actions: Vec<String> = self.actions.iter().map(ProfileActionRecord::json).collect();
            f.push(format!("\"actions\":[{}]", actions.join(",")));
        }
        if !self.residual_risks.is_empty() {
            let risks: Vec<String> = self.residual_risks.iter().map(ResidualRisk::json).collect();
            f.push(format!("\"residual_risks\":[{}]", risks.join(",")));
        }
        let s = &self.summary;
        let mut summary = format!(
            "{{\"removed\":{},\"zeroed\":{},\"dummied\":{},\"cleaned\":{},\"uids_remapped\":{},\"uids_retained\":{},\"uids_unresolved\":{},\"uid_collisions\":{},\"private_removed\":{},\"private_retained\":{},\"dates_shifted\":{},\"pixel_masks\":{}",
            s.removed,
            s.zeroed,
            s.dummied,
            s.cleaned,
            s.uids_remapped,
            s.uids_retained,
            s.uids_unresolved,
            s.uid_collisions,
            s.private_removed,
            s.private_retained,
            s.dates_shifted,
            s.pixel_masks
        );
        if s.patient_pseudonyms != 0 {
            summary.push_str(&format!(",\"patient_pseudonyms\":{}", s.patient_pseudonyms));
        }
        summary.push('}');
        f.push(format!("\"summary\":{summary}"));
        let mut csa = Vec::new();
        if self.csa.scrubbed != 0 {
            csa.push(format!("\"scrubbed\":{}", self.csa.scrubbed));
        }
        if self.csa.bytes_cleared != 0 {
            csa.push(format!("\"bytes_cleared\":{}", self.csa.bytes_cleared));
        }
        if self.csa.unscrubbable != 0 {
            csa.push(format!(
                "\"removed_unscrubbable\":{}",
                self.csa.unscrubbable
            ));
        }
        f.push(format!("\"csa\":{{{}}}", csa.join(",")));
        if let Some(r) = &self.pixel_redaction {
            f.push(format!("\"pixel_redaction\":{}", r.json()));
        }
        #[cfg(feature = "deid-encrypted")]
        if let Some(r) = &self.encrypted_attributes {
            f.push(format!("\"encrypted_attributes\":{}", r.json()));
        }
        format!("{{{}}}", f.join(","))
    }
}
type Rule = [Option<ActionCode>; 11];
type ScRule = [Option<ActionCode>; 8];
struct CompiledTable {
    exact: HashMap<Tag, Rule>,
    curves: Option<Rule>,
    overlay: HashMap<u16, Rule>,
    private: Option<Rule>,
}
fn profile_table() -> &'static CompiledTable {
    use std::sync::OnceLock;
    static TABLE: OnceLock<CompiledTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = CompiledTable {
            exact: HashMap::new(),
            curves: None,
            overlay: HashMap::new(),
            private: None,
        };
        for (pattern, actions) in tables::PROFILE_RULES.iter() {
            match *pattern {
                "(50xx,xxxx)" => t.curves = Some(*actions),
                "(60xx,4000)" => {
                    t.overlay.insert(0x4000, *actions);
                }
                "(60xx,3000)" => {
                    t.overlay.insert(0x3000, *actions);
                }
                "(gggg,eeee) where gggg is odd" => t.private = Some(*actions),
                p => {
                    let g = u16::from_str_radix(&p[1..5], 16).unwrap_or(0);
                    let e = u16::from_str_radix(&p[6..10], 16).unwrap_or(0);
                    t.exact.insert(Tag::new(g, e), *actions);
                }
            }
        }
        t
    })
}
impl CompiledTable {
    fn lookup(&self, tag: Tag) -> Option<Rule> {
        if let Some(rule) = self.exact.get(&tag) {
            return Some(*rule);
        }
        if tag.group.is_multiple_of(2) && tag.group & 0xff00 == 0x5000 && self.curves.is_some() {
            return self.curves;
        }
        if tag.group.is_multiple_of(2)
            && tag.group & 0xff00 == 0x6000
            && let Some(rule) = self.overlay.get(&tag.element)
        {
            return Some(*rule);
        }
        if tag.is_private() && self.private.is_some() {
            return self.private;
        }
        None
    }
}
struct StructuredTable {
    rules: HashMap<(String, String, String), (&'static str, ScRule)>,
    aliases: HashMap<(String, String), (String, String)>,
}
fn structured_table() -> &'static StructuredTable {
    use std::sync::OnceLock;
    static TABLE: OnceLock<StructuredTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut rules = HashMap::new();
        for (cv, cs, vt, actions) in tables::STRUCTURED_CONTENT_RULES.iter() {
            let key = (
                trim_space(cv).to_string(),
                trim_space(cs).to_string(),
                crate::textfmt::to_upper(trim_space(vt)),
            );
            rules.insert(key, (*vt, *actions));
        }
        let mut aliases = HashMap::new();
        for (ov, os, cv, cs) in tables::RETIRED_CODE_ALIASES.iter() {
            aliases.insert(
                (trim_space(ov).to_string(), trim_space(os).to_string()),
                (trim_space(cv).to_string(), trim_space(cs).to_string()),
            );
        }
        StructuredTable { rules, aliases }
    })
}
struct PlanChange {
    tag: Tag,
    element: Option<Element>,
}
pub struct BasicProfilePlan {
    source: DataSet,
    source_big_endian: bool,
    limits: ProfileLimits,
    changes: Vec<PlanChange>,
    report: ProfileReport,
    applied: bool,
    preview_only: bool,
}
impl BasicProfilePlan {
    pub fn report(&self) -> ProfileReport {
        self.report.clone()
    }
    pub fn apply(&mut self, obj: &mut Object) -> Result<ProfileReport, Error> {
        let current = obj.to_data_set();
        validate_budget(&current, &self.limits)?;
        if self.preview_only {
            return Err(Error::bare(Kind::DeferredValueUnavailable));
        }
        if self.applied
            || current != self.source
            || obj.value_big_endian() != self.source_big_endian
        {
            return Err(Error::bare(Kind::StaleProfilePlan));
        }
        for change in &self.changes {
            match &change.element {
                None => {
                    obj.remove(change.tag);
                }
                Some(e) => obj.put(e.clone()),
            }
        }
        self.applied = true;
        Ok(self.report.clone())
    }
}
pub fn apply_basic_profile(
    obj: &mut Object,
    options: &mut BasicProfileOptions,
    uids: &mut UidRemapper,
) -> Result<ProfileReport, Error> {
    let mut plan = plan_basic_profile(obj, options, uids)?;
    plan.apply(obj)
}
pub fn clone_file_with_basic_profile(
    src: &File,
    options: &mut BasicProfileOptions,
    uids: &mut UidRemapper,
) -> Result<(File, ProfileReport), Error> {
    let (limits, _) = normalize_options(options)?;
    validate_budget(&src.dataset.to_data_set(), &limits)?;
    let mut clone = clone_file(src)?;
    let pre_redacted = match &options.pixel_redaction {
        Some(r) => Some(super::pixel_redaction::redact_file_for_profile(
            &mut clone,
            r,
            limits.max_pixel_masks,
        )?),
        None => None,
    };
    let mut plan =
        plan_basic_profile_redacted(&clone.dataset, options, uids, pre_redacted.as_ref())?;
    let report = plan.apply(&mut clone.dataset)?;
    let clone = rederive_file_meta(clone)?;
    Ok((clone, report))
}
fn normalize_options(
    options: &mut BasicProfileOptions,
) -> Result<(ProfileLimits, HashSet<ProfileOption>), Error> {
    if options.limits == ProfileLimits::default() {
        options.limits = ProfileLimits::defaults();
    }
    let l = options.limits;
    if l.max_depth <= 0
        || l.max_elements <= 0
        || l.max_items <= 0
        || l.max_value_bytes <= 0
        || l.max_actions <= 0
        || l.max_pixel_masks <= 0
        || l.max_report_bytes <= 0
    {
        return Err(Error::bare(Kind::InvalidProfileOptions));
    }
    let mut requested = HashSet::new();
    for option in &options.selected_options {
        if !requested.insert(*option) {
            return Err(Error::bare(Kind::InvalidProfileOptions));
        }
    }
    use ProfileOption as O;
    if requested.contains(&O::RetainFullDates) && requested.contains(&O::RetainModifiedDates) {
        return Err(Error::bare(Kind::InvalidProfileOptions));
    }
    if requested.contains(&O::RetainModifiedDates) && options.date_shift_policy.is_none() {
        return Err(Error::bare(Kind::DateShiftPolicyRequired));
    }
    if requested.contains(&O::RetainSafePrivate) && options.safe_private_registry.is_none() {
        return Err(Error::bare(Kind::SafePrivateRegistryRequired));
    }
    if options.pixel_redaction.is_some()
        && (requested.contains(&O::CleanPixelData) || options.pixel_cleaner.is_some())
    {
        return Err(Error::bare(Kind::InvalidProfileOptions));
    }
    super::pixel_redaction::validate_pixel_redaction_options(
        options.pixel_redaction.as_ref(),
        l.max_pixel_masks,
    )?;
    #[cfg(feature = "deid-encrypted")]
    if let Some(encrypted) = &options.encrypted_attributes {
        options.encrypted_attributes = Some(
            super::encrypted::normalize_encrypted_attributes_options(encrypted)?,
        );
    }
    if requested.contains(&O::CleanPixelData) && options.pixel_cleaner.is_none() {
        return Err(Error::bare(Kind::PixelCleanerRequired));
    }
    if requested.contains(&O::CleanRecognizableFeatures)
        && options.visual_features_cleaner.is_none()
    {
        return Err(Error::bare(Kind::VisualCleanerRequired));
    }
    options.selected_options = PROFILE_OPTION_ORDER
        .into_iter()
        .filter(|o| requested.contains(o))
        .collect();
    Ok((options.limits, requested))
}
pub fn plan_basic_profile(
    obj: &Object,
    options: &mut BasicProfileOptions,
    uids: &mut UidRemapper,
) -> Result<BasicProfilePlan, Error> {
    plan_basic_profile_redacted(obj, options, uids, None)
}
pub(crate) fn plan_basic_profile_redacted(
    obj: &Object,
    options: &mut BasicProfileOptions,
    uids: &mut UidRemapper,
    pre_redacted: Option<&crate::pixel::redact::RedactionReport>,
) -> Result<BasicProfilePlan, Error> {
    let (limits, selected) = normalize_options(options)?;
    let mut pseudonymizer = options.pseudonymizer.take();
    let result = plan(
        obj,
        options,
        pseudonymizer.as_mut(),
        uids,
        limits,
        selected,
        pre_redacted,
    );
    options.pseudonymizer = pseudonymizer;
    result
}
fn plan(
    obj: &Object,
    options: &BasicProfileOptions,
    pseudonymizer: Option<&mut Pseudonymizer>,
    uids: &mut UidRemapper,
    limits: ProfileLimits,
    selected: HashSet<ProfileOption>,
    pre_redacted: Option<&crate::pixel::redact::RedactionReport>,
) -> Result<BasicProfilePlan, Error> {
    let source = obj.to_data_set();
    validate_budget(&source, &limits)?;
    let mut state = Planner {
        options,
        limits,
        selected,
        table: profile_table(),
        uids,
        report: ProfileReport {
            standard_version: tables::GENERATED_PROFILE_STANDARD_VERSION.into(),
            table_source: tables::GENERATED_PROFILE_SOURCEURL.into(),
            table_checksum: tables::GENERATED_PROFILE_PROJECTIONSHA256.into(),
            complete: true,
            options: options.selected_options.clone(),
            ..Default::default()
        },
        elements: 0,
        items: 0,
        value_bytes: 0,
        date_shift_days: 0,
        sop_class_uid: String::new(),
        report_bytes: 0,
        pseudonymized: false,
        big_endian: obj.value_big_endian(),
        pixel_redaction: None,
        preview_only: false,
    };
    if state.selected(ProfileOption::CleanStructuredContent) {
        state.report.structured_content_source =
            tables::GENERATED_STRUCTURED_CONTENT_SOURCEURL.into();
        state.report.structured_content_checksum =
            tables::GENERATED_STRUCTURED_CONTENT_PROJECTIONSHA256.into();
        state.report.retired_code_source = tables::GENERATED_RETIRED_CODE_SOURCEURL.into();
        state.report.retired_code_checksum = tables::GENERATED_RETIRED_CODE_PROJECTIONSHA256.into();
    }
    if state.selected(ProfileOption::RetainSafePrivate) {
        let provenance = options
            .safe_private_registry
            .as_ref()
            .map(|r| r.provenance().clone())
            .unwrap_or_default();
        state.report.safe_private_version = provenance.version;
        state.report.safe_private_checksum = provenance.checksum;
    }
    state.initialize_report_budget()?;
    state.set_sop_class_uid(&source);
    if state.sop_class_uid == "1.2.840.10008.1.3.10" {
        return Err(Error::bare(Kind::DicomdirPolicyRequired));
    }
    if let Some(r) = &options.pixel_redaction {
        state.pixel_redaction = Some(super::pixel_redaction::plan_pixel_redaction(
            r,
            obj,
            limits.max_pixel_masks,
        )?);
    }
    state.add_source_residual_risks(obj)?;
    state.add_pixel_redaction_residual_risks()?;
    if state.selected(ProfileOption::RetainModifiedDates) {
        let policy = options.date_shift_policy.as_ref().expect("normalized");
        let clone = Object::from_data_set(source.clone());
        let shift = guard(|| policy(&clone))?;
        if shift == 0 {
            return Err(Error::bare(Kind::UnrepresentableDateShift));
        }
        state.date_shift_days = shift;
    }
    let result = state.transform_data_set(&source, &[], 0)?;
    let mut planned = Object::from_data_set(result);
    planned.set_value_big_endian(obj.value_big_endian());
    if let Some(p) = pseudonymizer {
        pseudonymize(p, obj, &mut planned)?;
        state.report.summary.patient_pseudonyms = 1;
        state.pseudonymized = true;
    }
    state.apply_pixel_redaction(&mut planned, pre_redacted)?;
    if state.selected(ProfileOption::CleanPixelData) {
        if has_deferred_value(&planned, is_pixel_cleaner_tag) {
            return Err(Error::bare(Kind::DeferredValueUnavailable));
        }
        let before = planned.to_data_set();
        let cleaner = options.pixel_cleaner.as_ref().expect("normalized");
        let regions = guard(|| cleaner(&mut planned))?;
        if has_deferred_value(&planned, is_pixel_cleaner_tag) {
            return Err(Error::bare(Kind::DeferredValueUnavailable));
        }
        if regions.len() as i64 > limits.max_pixel_masks {
            return Err(Error::bare(Kind::ProfileResourceLimit));
        }
        if !mutations_allowed(&before, &planned.to_data_set(), is_pixel_cleaner_tag) {
            return Err(Error::bare(Kind::ProfileCallback));
        }
        state.report.summary.pixel_masks = regions.len() as i64;
        planned.put(raw_element(
            TAG_BURNED_IN_ANNOTATION,
            Vr::CS,
            b"NO".to_vec(),
        ));
    }
    if state.selected(ProfileOption::CleanRecognizableFeatures) {
        if has_deferred_value(&planned, is_visual_cleaner_tag) {
            return Err(Error::bare(Kind::DeferredValueUnavailable));
        }
        let before = planned.to_data_set();
        let cleaner = options
            .visual_features_cleaner
            .as_ref()
            .expect("normalized");
        guard(|| cleaner(&mut planned))?;
        if has_deferred_value(&planned, is_visual_cleaner_tag) {
            return Err(Error::bare(Kind::DeferredValueUnavailable));
        }
        if !mutations_allowed(&before, &planned.to_data_set(), is_visual_cleaner_tag) {
            return Err(Error::bare(Kind::ProfileCallback));
        }
        planned.put(raw_element(
            TAG_RECOGNIZABLE_VISUAL_FEATURES,
            Vr::CS,
            b"NO".to_vec(),
        ));
    }
    state.add_required_profile_attributes(&mut planned);
    #[cfg(feature = "deid-encrypted")]
    if let Some(encrypted) = &options.encrypted_attributes {
        state.report.encrypted_attributes = Some(super::encrypted::apply_encrypted_attributes(
            encrypted,
            &limits,
            obj,
            &mut planned,
        )?);
    }
    let result = planned.to_data_set();
    validate_budget(&result, &limits)?;
    let changes = profile_changes(&source, &result);
    state.report.pixel_redaction = state.pixel_redaction.clone();
    if state.report.json().len() as i64 > limits.max_report_bytes {
        return Err(Error::bare(Kind::ProfileResourceLimit));
    }
    Ok(BasicProfilePlan {
        source,
        source_big_endian: obj.value_big_endian(),
        limits,
        changes,
        report: state.report,
        applied: false,
        preview_only: state.preview_only,
    })
}
struct Planner<'a> {
    options: &'a BasicProfileOptions,
    limits: ProfileLimits,
    selected: HashSet<ProfileOption>,
    table: &'static CompiledTable,
    uids: &'a mut UidRemapper,
    report: ProfileReport,
    elements: i64,
    items: i64,
    value_bytes: i64,
    date_shift_days: i64,
    sop_class_uid: String,
    report_bytes: i64,
    pseudonymized: bool,
    big_endian: bool,
    pixel_redaction: Option<super::pixel_redaction::PixelRedactionReport>,
    preview_only: bool,
}
fn limit() -> Error {
    Error::bare(Kind::ProfileResourceLimit)
}
fn find_element(ds: &DataSet, tag: Tag) -> Option<&Element> {
    ds.elements.iter().find(|e| e.tag() == tag)
}
fn data_set_string(ds: &DataSet, tag: Tag) -> Option<String> {
    let value = trim_space(&string_value(find_element(ds, tag)?)).to_string();
    (!value.is_empty()).then_some(value)
}
fn sequence_element(tag: Tag, items: Vec<DataSet>) -> Element {
    Element {
        header: ElementHeader::new(tag, Vr::SQ, UNDEFINED_LENGTH),
        value: Value::Sequence(items),
    }
}
fn append_path(path: &[AttributePathStep], step: AttributePathStep) -> Vec<AttributePathStep> {
    let mut out = path.to_vec();
    out.push(step);
    out
}
fn is_structured_content_item_sequence(tag: Tag) -> bool {
    tag == TAG_CONTENT_SEQUENCE
        || tag == TAG_ACQUISITION_CONTEXT_SEQUENCE
        || tag == TAG_SPECIMEN_PREPARATION_CONTENT_SEQUENCE
}
fn structured_content_value_tag(value_type: &str) -> Option<Tag> {
    Some(
        match crate::textfmt::to_upper(trim_space(value_type)).as_str() {
            "TEXT" => TAG_STRUCTURED_TEXT_VALUE,
            "PNAME" => TAG_STRUCTURED_PERSON_NAME_VALUE,
            "UIDREF" => TAG_STRUCTURED_UID_VALUE,
            "DATE" => TAG_STRUCTURED_DATE_VALUE,
            "TIME" => TAG_STRUCTURED_TIME_VALUE,
            "DATETIME" => TAG_STRUCTURED_DATETIME_VALUE,
            "CODE" => TAG_STRUCTURED_CONCEPT_CODE_SEQUENCE,
            "NUM" => TAG_STRUCTURED_MEASURED_VALUE_SEQUENCE,
            "IMAGE" | "COMPOSITE" | "WAVEFORM" => TAG_REFERENCED_SOP_SEQUENCE,
            _ => return None,
        },
    )
}
fn private_creators_from_elements(elements: &[Element]) -> HashMap<Tag, String> {
    elements
        .iter()
        .filter(|e| is_private_creator_tag(e.tag()))
        .map(|e| (e.tag(), trim_space(&string_value(e)).to_string()))
        .collect()
}
fn value_bytes(element: &Element) -> Option<i64> {
    match &element.value {
        Value::Nil => {
            let h = &element.header;
            if h.length_set && h.length != UNDEFINED_LENGTH {
                Some(h.length as i64)
            } else {
                Some(0)
            }
        }
        Value::Sequence(_) => Some(0),
        Value::Fragments {
            offset_table,
            fragments,
        } => {
            Some(offset_table.len() as i64 + fragments.iter().map(|f| f.len() as i64).sum::<i64>())
        }
        Value::Raw(raw) => Some(raw.len() as i64),
        Value::Strings(values) => {
            let mut n: i64 = values.iter().map(|v| v.len() as i64).sum();
            if !values.is_empty() {
                n += values.len() as i64 - 1;
            }
            Some(n + n % 2)
        }
        _ => Some(0),
    }
}
fn element_vm(element: &Element) -> i64 {
    match &element.value {
        Value::Nil => 0,
        Value::Sequence(_) | Value::Fragments { .. } => 1,
        Value::Raw(raw) => {
            if raw.is_empty() {
                return 0;
            }
            let vr = element.vr();
            if vr.is_string_like() {
                return string_values(element).len() as i64;
            }
            if matches!(
                vr,
                Vr::OB | Vr::OD | Vr::OF | Vr::OL | Vr::OV | Vr::OW | Vr::UN
            ) {
                return 1;
            }
            let width = match vr {
                Vr::US | Vr::SS => 2,
                Vr::UL | Vr::SL | Vr::FL | Vr::AT => 4,
                Vr::UV | Vr::SV | Vr::FD => 8,
                _ => 1,
            };
            if raw.len() % width != 0 {
                0
            } else {
                (raw.len() / width) as i64
            }
        }
        Value::Strings(values) => values.len() as i64,
        _ => 1,
    }
}
fn safe_rule_matches(rule: &SafePrivateRule, element: &Element) -> bool {
    Some(element.vr()) == rule.vr && element_vm(element) == rule.vm
}
fn is_pixel_cleaner_tag(tag: Tag) -> bool {
    tag == Tag::new(0x7FE0, 0x0010)
        || tag == Tag::new(0x7FE0, 0x0008)
        || tag == Tag::new(0x7FE0, 0x0009)
        || tag == Tag::new(0x0088, 0x0200)
}
fn is_visual_cleaner_tag(tag: Tag) -> bool {
    is_pixel_cleaner_tag(tag)
        || (tag.group.is_multiple_of(2) && tag.group & 0xff00 == 0x6000)
        || tag.group == 0x0070
}
fn has_deferred_value(obj: &Object, allowed: fn(Tag) -> bool) -> bool {
    let mut stack: Vec<(DataSet, bool)> = vec![(obj.to_data_set(), false)];
    while let Some((ds, ancestor)) = stack.pop() {
        for element in &ds.elements {
            let here = ancestor || allowed(element.tag());
            if here && matches!(element.value, Value::Nil) {
                return true;
            }
            if let Value::Sequence(items) = &element.value {
                for item in items {
                    stack.push((item.clone(), here));
                }
            }
        }
    }
    false
}
fn mutations_allowed(before: &DataSet, after: &DataSet, allowed: fn(Tag) -> bool) -> bool {
    let mut before_by: HashMap<Tag, Vec<&Element>> = HashMap::new();
    let mut after_by: HashMap<Tag, Vec<&Element>> = HashMap::new();
    for e in &before.elements {
        before_by.entry(e.tag()).or_default().push(e);
    }
    for e in &after.elements {
        after_by.entry(e.tag()).or_default().push(e);
    }
    for (tag, elements) in &before_by {
        let empty = Vec::new();
        let replacements = after_by.get(tag).unwrap_or(&empty);
        if elements.len() != replacements.len() {
            if !allowed(*tag) {
                return false;
            }
            if elements
                .iter()
                .chain(replacements.iter())
                .any(|e| matches!(e.value, Value::Sequence(_)))
            {
                return false;
            }
        }
        for (element, replacement) in elements.iter().zip(replacements.iter()) {
            if element == replacement {
                continue;
            }
            match (&element.value, &replacement.value) {
                (Value::Sequence(b), Value::Sequence(a)) => {
                    if element.vr() != replacement.vr() || b.len() != a.len() {
                        return false;
                    }
                    for (bi, ai) in b.iter().zip(a.iter()) {
                        if !mutations_allowed(bi, ai, allowed) {
                            return false;
                        }
                    }
                }
                (Value::Sequence(_), _) | (_, Value::Sequence(_)) => return false,
                _ => {
                    if !allowed(*tag) {
                        return false;
                    }
                }
            }
        }
    }
    for (tag, elements) in &after_by {
        if !before_by.contains_key(tag) {
            if !allowed(*tag) {
                return false;
            }
            if elements
                .iter()
                .any(|e| matches!(e.value, Value::Sequence(_)))
            {
                return false;
            }
        }
    }
    true
}
fn validate_budget(ds: &DataSet, limits: &ProfileLimits) -> Result<(), Error> {
    let mut stack: Vec<(&DataSet, i64)> = vec![(ds, 0)];
    let (mut elements, mut items, mut bytes) = (0i64, 0i64, 0i64);
    while let Some((current, depth)) = stack.pop() {
        if depth > limits.max_depth
            || current.elements.len() as i64 > limits.max_elements - elements
        {
            return Err(limit());
        }
        elements += current.elements.len() as i64;
        for element in &current.elements {
            let b = value_bytes(element).ok_or_else(limit)?;
            if b > limits.max_value_bytes - bytes {
                return Err(limit());
            }
            bytes += b;
            let Value::Sequence(seq) = &element.value else {
                continue;
            };
            if depth >= limits.max_depth || seq.len() as i64 > limits.max_items - items {
                return Err(limit());
            }
            items += seq.len() as i64;
            for item in seq.iter().rev() {
                stack.push((item, depth + 1));
            }
        }
    }
    Ok(())
}
fn profile_changes(before: &DataSet, after: &DataSet) -> Vec<PlanChange> {
    let before_by: HashMap<Tag, &Element> = before.elements.iter().map(|e| (e.tag(), e)).collect();
    let after_by: HashMap<Tag, &Element> = after.elements.iter().map(|e| (e.tag(), e)).collect();
    let mut changes = Vec::new();
    for (tag, element) in &before_by {
        match after_by.get(tag) {
            None => changes.push(PlanChange {
                tag: *tag,
                element: None,
            }),
            Some(replacement) if *replacement != *element => changes.push(PlanChange {
                tag: *tag,
                element: Some((*replacement).clone()),
            }),
            _ => {}
        }
    }
    for (tag, element) in &after_by {
        if !before_by.contains_key(tag) {
            changes.push(PlanChange {
                tag: *tag,
                element: Some((*element).clone()),
            });
        }
    }
    changes.sort_by_key(|c| c.tag);
    changes
}
fn put_number(out: &mut Vec<u8>, v: u64, width: usize, big_endian: bool) {
    let b = v.to_le_bytes();
    if big_endian {
        out.extend(b[..width].iter().rev());
    } else {
        out.extend(&b[..width]);
    }
}
fn binary_element(tag: Tag, vr: Vr, value: u64, width: usize, big_endian: bool) -> Element {
    let mut raw = Vec::new();
    put_number(&mut raw, value, width, big_endian);
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Raw(raw),
    }
}
fn replacement_is_empty(element: &Element) -> bool {
    match &element.value {
        Value::Nil => true,
        Value::Raw(raw) => raw.is_empty(),
        Value::Strings(values) => {
            values.is_empty() || (values.len() == 1 && trim_space(&values[0]).is_empty())
        }
        _ => false,
    }
}
fn remap_uid_element(element: &Element, uids: &mut UidRemapper) -> Element {
    let values = string_values(element);
    if values.is_empty() {
        return raw_element(element.tag(), Vr::UI, Vec::new());
    }
    let mapped: Vec<String> = values.iter().map(|v| uids.map(trim_space(v))).collect();
    raw_element(element.tag(), Vr::UI, mapped.join("\\").into_bytes())
}
fn built_in_dummy(element: &Element, uids: &mut UidRemapper, big_endian: bool) -> Element {
    let (tag, vr) = (element.tag(), element.vr());
    let number = |width: usize, v: u64| binary_element(tag, vr, v, width, big_endian);
    match vr {
        Vr::UI => return remap_uid_element(element, uids),
        Vr::US | Vr::SS => return number(2, 1),
        Vr::UL | Vr::SL => return number(4, 1),
        Vr::UV | Vr::SV => return number(8, 1),
        Vr::FL => return number(4, 1f32.to_bits() as u64),
        Vr::FD => return number(8, 1f64.to_bits()),
        Vr::AT => {
            let mut raw = Vec::new();
            put_number(&mut raw, 0x0008, 2, big_endian);
            put_number(&mut raw, 0x0008, 2, big_endian);
            return Element {
                header: ElementHeader {
                    tag,
                    vr,
                    length: 0,
                    length_set: false,
                },
                value: Value::Raw(raw),
            };
        }
        Vr::OB | Vr::OW | Vr::OF | Vr::OD | Vr::OL | Vr::OV | Vr::UN => {
            return raw_element(tag, vr, vec![1, 0]);
        }
        _ => {}
    }
    let value = match vr {
        Vr::DA => "19000101",
        Vr::TM => "000000",
        Vr::DT => "19000101000000",
        Vr::AS => "000Y",
        Vr::DS | Vr::IS => "0",
        Vr::PN => "ANONYMIZED",
        Vr::AE | Vr::CS | Vr::SH => "ANON",
        _ => "ANONYMIZED",
    };
    raw_element(tag, vr, value.as_bytes().to_vec())
}
fn add_days(t: dcmtime::ZonedTime, days: i64) -> dcmtime::ZonedTime {
    let (y, m, d) = t.date();
    dcmtime::ZonedTime::from_fields(
        y,
        m,
        d + days,
        t.hour(),
        t.minute(),
        t.second(),
        t.nanosecond(),
        t.offset(),
    )
}
fn shift_temporal_element(element: &Element, days: i64) -> Result<Element, Error> {
    let h = &element.header;
    if matches!(element.value, Value::Nil) && h.length_set && h.length > 0 {
        return Err(Error::bare(Kind::DeferredValueUnavailable));
    }
    let values = string_values(element);
    if values.is_empty() {
        return Ok(raw_element(element.tag(), element.vr(), Vec::new()));
    }
    let mut shifted = vec![String::new(); values.len()];
    for (index, value) in values.iter().enumerate() {
        let value = trim_space(value);
        if value.is_empty() {
            continue;
        }
        shifted[index] = match element.vr() {
            Vr::DA => {
                let mut parsed =
                    dcmtime::parse_date(value).map_err(|_| Error::bare(Kind::ProfileCallback))?;
                parsed.time = add_days(parsed.time, days);
                parsed.dcm()
            }
            Vr::DT => {
                let mut parsed = dcmtime::parse_datetime(value)
                    .map_err(|_| Error::bare(Kind::ProfileCallback))?;
                parsed.time = add_days(parsed.time, days);
                parsed.dcm()
            }
            _ => return Err(Error::bare(Kind::ProfileCallback)),
        };
    }
    if shifted.join("\\") == values.join("\\") {
        return Err(Error::bare(Kind::UnrepresentableDateShift));
    }
    Ok(raw_element(
        element.tag(),
        element.vr(),
        shifted.join("\\").into_bytes(),
    ))
}
fn method_code(code: &str, meaning: &str) -> DataSet {
    DataSet {
        elements: vec![
            raw_element(TAG_CODE_VALUE, Vr::SH, code.as_bytes().to_vec()),
            raw_element(TAG_CODING_SCHEME_DESIGNATOR, Vr::SH, b"DCM".to_vec()),
            raw_element(TAG_CODE_MEANING, Vr::LO, meaning.as_bytes().to_vec()),
        ],
        item_offset: None,
    }
}
impl Planner<'_> {
    fn selected(&self, option: ProfileOption) -> bool {
        self.selected.contains(&option)
    }
    fn set_sop_class_uid(&mut self, source: &DataSet) {
        if let Some(e) = find_element(source, TAG_SOP_CLASS_UID) {
            self.sop_class_uid = trim_space(&string_value(e)).to_string();
        }
    }
    fn initialize_report_budget(&mut self) -> Result<(), Error> {
        let n = self.report.json().len() as i64;
        if n > self.limits.max_report_bytes {
            return Err(limit());
        }
        self.report_bytes = n;
        Ok(())
    }
    fn record(
        &mut self,
        path: &[AttributePathStep],
        tag: Tag,
        code: ActionCode,
        action: ProfileAction,
        reason: &'static str,
    ) -> Result<(), Error> {
        if self.report.actions.len() as i64 >= self.limits.max_actions {
            return Err(limit());
        }
        let record = ProfileActionRecord {
            path: path.to_vec(),
            tag,
            action,
            action_code: code,
            reason,
        };
        let n = record.json().len() as i64;
        if n + 16 > self.limits.max_report_bytes - self.report_bytes {
            return Err(limit());
        }
        self.report_bytes += n + 16;
        self.report.actions.push(record);
        Ok(())
    }
    fn add_residual_risk(&mut self, risk: ResidualRisk) -> Result<(), Error> {
        let n = risk.json().len() as i64;
        if n + 24 > self.limits.max_report_bytes - self.report_bytes {
            return Err(limit());
        }
        self.report_bytes += n + 24;
        self.report.complete = false;
        self.report.residual_risks.push(risk);
        Ok(())
    }
    fn add_pixel_redaction_residual_risks(&mut self) -> Result<(), Error> {
        let Some(r) = self.pixel_redaction.clone() else {
            return Ok(());
        };
        if !r.attested {
            self.add_residual_risk(ResidualRisk {
                code: "pixel-redaction-not-attested",
                tag: TAG_PIXEL_DATA,
            })?;
        }
        if r.frames_covered < r.frames {
            self.add_residual_risk(ResidualRisk {
                code: "pixel-redaction-frames-not-covered",
                tag: TAG_PIXEL_DATA,
            })?;
        }
        Ok(())
    }
    fn apply_pixel_redaction(
        &mut self,
        planned: &mut Object,
        pre_redacted: Option<&crate::pixel::redact::RedactionReport>,
    ) -> Result<(), Error> {
        let options = self.options;
        let (Some(redaction), Some(report)) =
            (&options.pixel_redaction, self.pixel_redaction.as_mut())
        else {
            return Ok(());
        };
        if let Some(done) = pre_redacted {
            report.record(done);
        } else {
            match super::pixel_redaction::redact_planned(
                redaction,
                planned,
                self.limits.max_pixel_masks,
            )? {
                Some(outcome) => report.record(&outcome),
                None => self.preview_only = true,
            }
        }
        let (regions, cleared) = self
            .pixel_redaction
            .as_ref()
            .map_or((0, false), |r| (r.regions, r.annotation_cleared));
        self.report.summary.pixel_masks = regions;
        if cleared {
            planned.put(raw_element(
                TAG_BURNED_IN_ANNOTATION,
                Vr::CS,
                b"NO".to_vec(),
            ));
        }
        Ok(())
    }
    fn add_source_residual_risks(&mut self, source: &Object) -> Result<(), Error> {
        let has_pixels = source.has(TAG_PIXEL_DATA)
            || source.has(Tag::new(0x7FE0, 0x0008))
            || source.has(Tag::new(0x7FE0, 0x0009));
        let cleaned = self.selected(ProfileOption::CleanPixelData)
            || self
                .pixel_redaction
                .as_ref()
                .is_some_and(|r| r.annotation_cleared);
        if !cleaned && has_pixels {
            self.add_residual_risk(ResidualRisk {
                code: "pixel-cleaning-not-attested",
                tag: TAG_PIXEL_DATA,
            })?;
        }
        if !cleaned
            && (has_pixels || source.has(TAG_BURNED_IN_ANNOTATION))
            && single_cs(source, TAG_BURNED_IN_ANNOTATION).as_deref() != Some("NO")
        {
            self.add_residual_risk(ResidualRisk {
                code: "burned-in-annotation-not-cleared",
                tag: TAG_BURNED_IN_ANNOTATION,
            })?;
        }
        if !self.selected(ProfileOption::CleanRecognizableFeatures)
            && (has_pixels || source.has(TAG_RECOGNIZABLE_VISUAL_FEATURES))
            && single_cs(source, TAG_RECOGNIZABLE_VISUAL_FEATURES).as_deref() != Some("NO")
        {
            self.add_residual_risk(ResidualRisk {
                code: "recognizable-visual-features-not-cleared",
                tag: TAG_RECOGNIZABLE_VISUAL_FEATURES,
            })?;
        }
        Ok(())
    }
    fn consume_value(&mut self, element: &Element) -> Result<(), Error> {
        let b = value_bytes(element).ok_or_else(limit)?;
        if b > self.limits.max_value_bytes - self.value_bytes {
            return Err(limit());
        }
        self.value_bytes += b;
        Ok(())
    }
    fn transform_data_set(
        &mut self,
        source: &DataSet,
        path: &[AttributePathStep],
        depth: i64,
    ) -> Result<DataSet, Error> {
        if depth > self.limits.max_depth
            || source.elements.len() as i64 > self.limits.max_elements - self.elements
        {
            return Err(limit());
        }
        if depth == 0 {
            self.set_sop_class_uid(source);
        }
        self.elements += source.elements.len() as i64;
        let creators = private_creators_from_elements(&source.elements);
        let registry = self.options.safe_private_registry.as_ref();
        let mut keep_creators = HashSet::new();
        if self.selected(ProfileOption::RetainSafePrivate) {
            for element in &source.elements {
                let tag = element.tag();
                if !tag.is_private() || tag.element < 0x1000 {
                    continue;
                }
                let creator_tag = private_creator_tag_for_data_tag(tag);
                let creator = creators.get(&creator_tag).map(String::as_str).unwrap_or("");
                if registry
                    .and_then(|r| r.lookup_rule(tag, creator))
                    .is_some_and(|rule| safe_rule_matches(rule, element))
                {
                    keep_creators.insert(creator_tag);
                }
            }
        }
        let mut result = DataSet {
            elements: Vec::with_capacity(source.elements.len()),
            item_offset: source.item_offset,
        };
        for source_element in &source.elements {
            let tag = source_element.tag();
            self.consume_value(source_element)?;
            if tag.group == 0x0004 {
                self.report.summary.removed += 1;
                self.record(
                    path,
                    tag,
                    ActionCode::X,
                    ProfileAction::Remove,
                    "file-set-only-attribute",
                )?;
                continue;
            }
            if is_private_creator_tag(tag) {
                if keep_creators.contains(&tag) {
                    result.elements.push(source_element.clone());
                    self.report.summary.private_retained += 1;
                } else {
                    self.report.summary.private_removed += 1;
                    self.record(
                        path,
                        tag,
                        ActionCode::X,
                        ProfileAction::Remove,
                        "private-creator-unused",
                    )?;
                }
                continue;
            }
            if tag.is_private()
                && tag.element >= 0x1000
                && self.selected(ProfileOption::RetainSafePrivate)
            {
                let creator = creators
                    .get(&private_creator_tag_for_data_tag(tag))
                    .cloned()
                    .unwrap_or_default();
                let rule = registry.and_then(|r| r.lookup_rule(tag, &creator)).cloned();
                if let Some(rule) = rule
                    .as_ref()
                    .filter(|r| safe_rule_matches(r, source_element))
                {
                    if is_csa_header(tag, &creator) {
                        let raw = source_element.raw().map(<[u8]>::to_vec).unwrap_or_default();
                        let (scrubbed, outcome) =
                            scrub_csa_element(source_element, &creator, &raw, &mut self.report.csa);
                        match outcome {
                            CsaOutcome::Scrubbed => {
                                result.elements.push(scrubbed);
                                self.report.summary.private_retained += 1;
                                self.record(
                                    path,
                                    tag,
                                    ActionCode::C,
                                    ProfileAction::Clean,
                                    "safe-private-csa-scrubbed",
                                )?;
                                continue;
                            }
                            CsaOutcome::Unscrubbable => {
                                self.report.summary.private_removed += 1;
                                self.record(
                                    path,
                                    tag,
                                    ActionCode::X,
                                    ProfileAction::Remove,
                                    "safe-private-csa-unscrubbable",
                                )?;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    if rule.action == Some(ProfileAction::Keep) {
                        result.elements.push(source_element.clone());
                        self.report.summary.private_retained += 1;
                        self.record(
                            path,
                            tag,
                            ActionCode::C,
                            ProfileAction::Keep,
                            "safe-private-verified",
                        )?;
                        continue;
                    }
                    let cleaned = self.clean_element(path, source_element, ActionCode::C)?;
                    self.record(
                        path,
                        tag,
                        ActionCode::C,
                        ProfileAction::Clean,
                        "safe-private-verified",
                    )?;
                    result.elements.push(cleaned);
                    self.report.summary.private_retained += 1;
                    continue;
                }
                self.report.summary.private_removed += 1;
                let reason = if rule.is_some() {
                    "safe-private-shape-mismatch"
                } else {
                    "safe-private-unverified"
                };
                self.record(path, tag, ActionCode::X, ProfileAction::Remove, reason)?;
                continue;
            }
            let Some(rule) = self.table.lookup(tag) else {
                if let Value::Sequence(items) = &source_element.value {
                    let transformed = if self.selected(ProfileOption::CleanStructuredContent)
                        && is_structured_content_item_sequence(tag)
                    {
                        self.transform_structured_content_sequence(
                            source_element,
                            items,
                            path,
                            depth,
                        )?
                    } else {
                        self.transform_sequence(source_element, items, path, depth)?
                    };
                    result.elements.push(transformed);
                } else {
                    result.elements.push(source_element.clone());
                }
                continue;
            };
            let code = self.selected_action_code(&rule);
            let (requirement, reason) = self.requirement(path, source_element, code)?;
            let action = resolve_action_code(code, requirement)?;
            if let Some(transformed) =
                self.apply_action(path, source_element, code, action, reason, depth)?
            {
                result.elements.push(transformed);
            }
        }
        Ok(result)
    }
    fn transform_sequence(
        &mut self,
        element: &Element,
        items: &[DataSet],
        path: &[AttributePathStep],
        depth: i64,
    ) -> Result<Element, Error> {
        if depth >= self.limits.max_depth || items.len() as i64 > self.limits.max_items - self.items
        {
            return Err(limit());
        }
        self.items += items.len() as i64;
        let mut out_items = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let item_path = append_path(
                path,
                AttributePathStep {
                    sequence_tag: element.tag(),
                    item_index: index,
                },
            );
            out_items.push(self.transform_data_set(item, &item_path, depth + 1)?);
        }
        let mut out = element.clone();
        out.value = Value::Sequence(out_items);
        out.header.length = UNDEFINED_LENGTH;
        out.header.length_set = true;
        Ok(out)
    }
    fn structured_content_rule(&self, item: &DataSet) -> Option<(&'static str, ScRule)> {
        let value_type = data_set_string(item, TAG_CONTENT_VALUE_TYPE)?;
        let concept = find_element(item, TAG_CONCEPT_NAME_CODE_SEQUENCE)?;
        let Value::Sequence(concepts) = &concept.value else {
            return None;
        };
        if concepts.len() != 1 {
            return None;
        }
        let code_value = [TAG_CODE_VALUE, TAG_LONG_CODE_VALUE, TAG_URN_CODE_VALUE]
            .into_iter()
            .find_map(|t| data_set_string(&concepts[0], t))?;
        let scheme = data_set_string(&concepts[0], TAG_CODING_SCHEME_DESIGNATOR)?;
        let table = structured_table();
        let vt = crate::textfmt::to_upper(&value_type);
        if let Some(rule) = table
            .rules
            .get(&(code_value.clone(), scheme.clone(), vt.clone()))
        {
            return Some(*rule);
        }
        let (cv, cs) = table.aliases.get(&(code_value, scheme))?;
        table.rules.get(&(cv.clone(), cs.clone(), vt)).copied()
    }
    fn selected_structured_code(&self, rule: &ScRule) -> ActionCode {
        let mut code = rule[0];
        for (option, column) in STRUCTURED_CONTENT_OPTION_COLUMNS {
            if self.selected(option) && rule[column].is_some() {
                code = rule[column];
            }
        }
        code.unwrap_or(ActionCode::X)
    }
    fn transform_structured_content_sequence(
        &mut self,
        element: &Element,
        items: &[DataSet],
        path: &[AttributePathStep],
        depth: i64,
    ) -> Result<Element, Error> {
        if depth >= self.limits.max_depth || items.len() as i64 > self.limits.max_items - self.items
        {
            return Err(limit());
        }
        self.items += items.len() as i64;
        let mut out_items = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let item_path = append_path(
                path,
                AttributePathStep {
                    sequence_tag: element.tag(),
                    item_index: index,
                },
            );
            let Some((value_type, rule)) = self.structured_content_rule(item) else {
                return Err(Error::bare(Kind::UnclassifiedStructuredContent));
            };
            let code = self.selected_structured_code(&rule);
            let value_tag = structured_content_value_tag(value_type);
            let value_element = value_tag.and_then(|t| find_element(item, t)).cloned();
            let requirement_element = value_element.clone().unwrap_or(Element {
                header: ElementHeader {
                    tag: TAG_CONTENT_SEQUENCE,
                    vr: Vr::SQ,
                    length: 0,
                    length_set: false,
                },
                value: Value::Nil,
            });
            let (requirement, _) = self.requirement(&item_path, &requirement_element, code)?;
            let action = resolve_action_code(code, requirement)?;
            match action {
                ProfileAction::Remove => {
                    self.consume_data_set_budget(item, depth + 1)?;
                    self.record(
                        &item_path,
                        element.tag(),
                        code,
                        action,
                        "structured-content-table",
                    )?;
                    self.report.summary.removed += 1;
                }
                ProfileAction::Keep => {
                    self.record(
                        &item_path,
                        element.tag(),
                        code,
                        action,
                        "structured-content-table",
                    )?;
                    match &value_element {
                        None => {
                            if crate::textfmt::to_upper(trim_space(value_type)) == "CONTAINER" {
                                out_items.push(self.transform_data_set(
                                    item,
                                    &item_path,
                                    depth + 1,
                                )?);
                                continue;
                            }
                            return Err(Error::bare(Kind::StructuredContentValue));
                        }
                        Some(v) => out_items.push(self.transform_structured_item_keep(
                            item,
                            v,
                            &item_path,
                            depth + 1,
                        )?),
                    }
                }
                ProfileAction::Dummy | ProfileAction::Clean => {
                    let Some(v) = &value_element else {
                        return Err(Error::bare(Kind::StructuredContentValue));
                    };
                    out_items.push(self.transform_structured_item_value(
                        item,
                        v,
                        &item_path,
                        code,
                        action,
                        depth + 1,
                    )?);
                }
                _ => return Err(Error::bare(Kind::InvalidActionCode)),
            }
        }
        let mut out = element.clone();
        out.value = Value::Sequence(out_items);
        out.header.length = UNDEFINED_LENGTH;
        out.header.length_set = true;
        Ok(out)
    }
    fn without_value(item: &DataSet, value_tag: Tag) -> DataSet {
        DataSet {
            elements: item
                .elements
                .iter()
                .filter(|e| e.tag() != value_tag)
                .cloned()
                .collect(),
            item_offset: item.item_offset,
        }
    }
    fn transform_structured_item_value(
        &mut self,
        item: &DataSet,
        value: &Element,
        path: &[AttributePathStep],
        code: ActionCode,
        action: ProfileAction,
        depth: i64,
    ) -> Result<DataSet, Error> {
        let mut transformed =
            self.transform_data_set(&Self::without_value(item, value.tag()), path, depth)?;
        if self.elements >= self.limits.max_elements {
            return Err(limit());
        }
        self.elements += 1;
        self.consume_value(value)?;
        let replacement = if action == ProfileAction::Dummy && value.vr() == Vr::SQ {
            let items = match &value.value {
                Value::Sequence(items) if !items.is_empty() => items.clone(),
                _ => return Err(Error::bare(Kind::StructuredContentValue)),
            };
            let r = self.transform_sequence(value, &items, path, depth)?;
            self.report.summary.dummied += 1;
            self.record(path, value.tag(), code, action, "structured-content-table")?;
            r
        } else {
            match self.apply_action(path, value, code, action, "structured-content-table", depth)? {
                Some(r) => r,
                None => Element {
                    header: ElementHeader {
                        tag: Tag::default(),
                        vr: Vr::UN,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Nil,
                },
            }
        };
        transformed.elements.push(replacement);
        Ok(transformed)
    }
    fn transform_structured_item_keep(
        &mut self,
        item: &DataSet,
        value: &Element,
        path: &[AttributePathStep],
        depth: i64,
    ) -> Result<DataSet, Error> {
        let mut transformed =
            self.transform_data_set(&Self::without_value(item, value.tag()), path, depth)?;
        self.consume_element_budget(value, depth)?;
        transformed.elements.push(value.clone());
        Ok(transformed)
    }
    fn consume_data_set_budget(&mut self, ds: &DataSet, depth: i64) -> Result<(), Error> {
        let mut stack: Vec<(&DataSet, i64)> = vec![(ds, depth)];
        while let Some((current, d)) = stack.pop() {
            if d > self.limits.max_depth
                || current.elements.len() as i64 > self.limits.max_elements - self.elements
            {
                return Err(limit());
            }
            self.elements += current.elements.len() as i64;
            for element in &current.elements {
                self.consume_value(element)?;
                let Value::Sequence(items) = &element.value else {
                    continue;
                };
                if d >= self.limits.max_depth
                    || items.len() as i64 > self.limits.max_items - self.items
                {
                    return Err(limit());
                }
                self.items += items.len() as i64;
                for item in items.iter().rev() {
                    stack.push((item, d + 1));
                }
            }
        }
        Ok(())
    }
    fn consume_element_budget(&mut self, element: &Element, depth: i64) -> Result<(), Error> {
        if self.elements >= self.limits.max_elements {
            return Err(limit());
        }
        self.elements += 1;
        self.consume_value(element)?;
        let Value::Sequence(items) = &element.value else {
            return Ok(());
        };
        if depth >= self.limits.max_depth || items.len() as i64 > self.limits.max_items - self.items
        {
            return Err(limit());
        }
        self.items += items.len() as i64;
        for item in items {
            self.consume_data_set_budget(item, depth + 1)?;
        }
        Ok(())
    }
    fn selected_action_code(&self, rule: &Rule) -> ActionCode {
        let mut code = rule[0];
        for (option, column) in TABLE_OPTION_COLUMNS {
            if self.selected(option) && rule[column].is_some() {
                code = rule[column];
            }
        }
        code.unwrap_or(ActionCode::X)
    }
    fn attribute_context(
        &self,
        path: &[AttributePathStep],
        element: &Element,
        code: ActionCode,
    ) -> AttributeContext {
        AttributeContext {
            path: path.to_vec(),
            tag: element.tag(),
            vr: element.vr(),
            sop_class_uid: self.sop_class_uid.clone(),
            action_code: code,
        }
    }
    fn requirement(
        &self,
        path: &[AttributePathStep],
        element: &Element,
        code: ActionCode,
    ) -> Result<(AttributeRequirement, &'static str), Error> {
        if !code.conditional() {
            return Ok((AttributeRequirement::OPTIONAL, "table"));
        }
        let ctx = self.attribute_context(path, element, code);
        if let Some(resolver) = &self.options.requirement_resolver {
            let requirement = guard(|| resolver(&ctx))?;
            if requirement > AttributeRequirement::TYPE1 {
                return Err(Error::bare(Kind::InvalidProfileOptions));
            }
            return Ok((requirement, "iod-resolver"));
        }
        if self.options.require_resolved_conditional {
            return Err(Error::bare(Kind::InvalidProfileOptions));
        }
        Ok(match code {
            ActionCode::XZ => (AttributeRequirement::TYPE2, "conditional-conservative"),
            _ => (AttributeRequirement::TYPE1, "conditional-conservative"),
        })
    }
    fn apply_action(
        &mut self,
        path: &[AttributePathStep],
        element: &Element,
        code: ActionCode,
        action: ProfileAction,
        reason: &'static str,
        depth: i64,
    ) -> Result<Option<Element>, Error> {
        let tag = element.tag();
        let h = &element.header;
        if matches!(element.value, Value::Nil)
            && h.length_set
            && h.length > 0
            && matches!(action, ProfileAction::Clean | ProfileAction::RemapUid)
        {
            return Err(Error::bare(Kind::DeferredValueUnavailable));
        }
        self.record(path, tag, code, action, reason)?;
        match action {
            ProfileAction::Remove => {
                self.report.summary.removed += 1;
                if tag.is_private() {
                    self.report.summary.private_removed += 1;
                }
                Ok(None)
            }
            ProfileAction::Zero => {
                self.report.summary.zeroed += 1;
                if element.vr() == Vr::SQ {
                    return Ok(Some(sequence_element(tag, Vec::new())));
                }
                Ok(Some(raw_element(tag, element.vr(), Vec::new())))
            }
            ProfileAction::Dummy => {
                self.report.summary.dummied += 1;
                self.dummy_element(path, element, code).map(Some)
            }
            ProfileAction::Clean => {
                if let Value::Sequence(items) = &element.value {
                    let cleaned = if self.selected(ProfileOption::CleanStructuredContent)
                        && is_structured_content_item_sequence(tag)
                    {
                        self.transform_structured_content_sequence(element, items, path, depth)?
                    } else {
                        self.transform_sequence(element, items, path, depth)?
                    };
                    self.report.summary.cleaned += 1;
                    return Ok(Some(cleaned));
                }
                if self.selected(ProfileOption::RetainModifiedDates)
                    && matches!(element.vr(), Vr::DA | Vr::DT | Vr::TM)
                {
                    if element.vr() == Vr::TM {
                        return Ok(Some(element.clone()));
                    }
                    let shifted = shift_temporal_element(element, self.date_shift_days)?;
                    self.report.summary.dates_shifted += 1;
                    return Ok(Some(shifted));
                }
                self.clean_element(path, element, code).map(Some)
            }
            ProfileAction::RemapUid => {
                let (mapped, unresolved) = self.remap_uid_element(element);
                self.report.summary.uids_unresolved += unresolved;
                self.report.summary.uids_remapped += (string_values(element).len() as i64).max(1);
                Ok(Some(mapped))
            }
            ProfileAction::Keep => {
                if element.vr() == Vr::UI {
                    self.report.summary.uids_retained += string_values(element).len() as i64;
                }
                if let Value::Sequence(items) = &element.value {
                    return self
                        .transform_sequence(element, items, path, depth)
                        .map(Some);
                }
                Ok(Some(element.clone()))
            }
        }
    }
    fn remap_uid_element(&mut self, element: &Element) -> (Element, i64) {
        let scope = format!(
            "profile:{:04X}{:04X}",
            element.tag().group,
            element.tag().element
        );
        let values = string_values(element);
        if values.is_empty() {
            let (uid, collisions) = self.uids.map_missing_with_collisions(&scope);
            self.report.summary.uid_collisions += collisions as i64;
            return (raw_element(element.tag(), Vr::UI, uid.into_bytes()), 1);
        }
        let mut unresolved = 0;
        let mut mapped = Vec::with_capacity(values.len());
        for value in &values {
            let original = crate::object::text::normalize_uid(value);
            let (uid, collisions) = if original.is_empty() {
                unresolved += 1;
                self.uids.map_missing_with_collisions(&scope)
            } else {
                self.uids.map_with_collisions(original)
            };
            self.report.summary.uid_collisions += collisions as i64;
            mapped.push(uid);
        }
        (
            raw_element(element.tag(), Vr::UI, mapped.join("\\").into_bytes()),
            unresolved,
        )
    }
    fn clean_element(
        &mut self,
        path: &[AttributePathStep],
        element: &Element,
        code: ActionCode,
    ) -> Result<Element, Error> {
        let Some(cleaner) = &self.options.cleaner else {
            return Err(Error::bare(Kind::CleanerRequired));
        };
        let ctx = ElementContext {
            attribute: self.attribute_context(path, element, code),
            element: element.clone(),
        };
        let replacement = guard(|| cleaner(&ctx))?;
        if replacement.tag() != element.tag()
            || replacement.vr() != element.vr()
            || matches!(replacement.value, Value::Nil)
        {
            return Err(Error::bare(Kind::ProfileCallback));
        }
        self.report.summary.cleaned += 1;
        Ok(replacement)
    }
    fn dummy_element(
        &mut self,
        path: &[AttributePathStep],
        element: &Element,
        code: ActionCode,
    ) -> Result<Element, Error> {
        if element.vr() == Vr::SQ && self.options.dummy_value_provider.is_none() {
            return Err(Error::bare(Kind::DummyValueProviderRequired));
        }
        if let Some(provider) = &self.options.dummy_value_provider {
            let ctx = ElementContext {
                attribute: self.attribute_context(path, element, code),
                element: element.clone(),
            };
            let replacement = guard(|| provider(&ctx))?;
            if replacement.tag() != element.tag()
                || replacement.vr() != element.vr()
                || replacement_is_empty(&replacement)
            {
                return Err(Error::bare(Kind::ProfileCallback));
            }
            return Ok(replacement);
        }
        Ok(built_in_dummy(element, self.uids, self.big_endian))
    }
    fn add_required_profile_attributes(&self, obj: &mut Object) {
        obj.put(raw_element(
            TAG_PATIENT_IDENTITY_REMOVED,
            Vr::CS,
            b"YES".to_vec(),
        ));
        let mut methods = vec!["DICOM Basic Application Confidentiality Profile".to_string()];
        if let Some(r) = &self.pixel_redaction {
            methods.push(if r.attested {
                "Operator-attested pixel region redaction".to_string()
            } else {
                "Unattested pixel region redaction".to_string()
            });
        }
        if self.pseudonymized {
            methods.push("Template pseudonyms for patient name and ID".to_string());
        }
        #[cfg(feature = "deid-encrypted")]
        if self.options.encrypted_attributes.is_some() {
            methods.push(super::encrypted::ENCRYPTED_ATTRIBUTES_METHOD.to_string());
        }
        if methods.len() == 1 {
            obj.put(raw_element(
                TAG_DEIDENTIFICATION_METHOD,
                Vr::LO,
                methods[0].clone().into_bytes(),
            ));
        } else {
            obj.put(Element {
                header: ElementHeader {
                    tag: TAG_DEIDENTIFICATION_METHOD,
                    vr: Vr::LO,
                    length: 0,
                    length_set: false,
                },
                value: Value::Strings(methods),
            });
        }
        let mut codes = vec![method_code(
            "113100",
            "Basic Application Confidentiality Profile",
        )];
        for option in &self.options.selected_options {
            let (code, meaning) = option.method_code();
            codes.push(method_code(code, meaning));
        }
        if self
            .pixel_redaction
            .as_ref()
            .is_some_and(|r| r.annotation_cleared)
        {
            let (code, meaning) = ProfileOption::CleanPixelData.method_code();
            codes.insert(1, method_code(code, meaning));
        }
        obj.put(sequence_element(
            TAG_DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
            codes,
        ));
        let status = if self.selected(ProfileOption::RetainFullDates) {
            "UNMODIFIED"
        } else if self.selected(ProfileOption::RetainModifiedDates) {
            "MODIFIED"
        } else {
            "REMOVED"
        };
        obj.put(raw_element(
            TAG_LONGITUDINAL_TEMPORAL_INFORMATION,
            Vr::CS,
            status.as_bytes().to_vec(),
        ));
    }
}
pub(crate) fn pseudonymize(
    pseudonymizer: &mut Pseudonymizer,
    source: &Object,
    planned: &mut Object,
) -> Result<bool, Error> {
    let pseudonym = pseudonymizer.pseudonym(&patient_pseudonym_identity(source))?;
    if !pseudonym.patient_name.is_empty() {
        planned.put(raw_element(
            TAG_PATIENT_NAME,
            Vr::PN,
            pad_text(&pseudonym.patient_name),
        ));
    }
    if !pseudonym.patient_id.is_empty() {
        planned.put(raw_element(
            TAG_PATIENT_ID,
            Vr::LO,
            pad_text(&pseudonym.patient_id),
        ));
    }
    Ok(true)
}
pub(crate) fn normalize_profile_options(
    options: &mut BasicProfileOptions,
) -> Result<(ProfileLimits, HashSet<ProfileOption>), Error> {
    normalize_options(options)
}
pub(crate) fn validate_profile_budget(ds: &DataSet, limits: &ProfileLimits) -> Result<(), Error> {
    validate_budget(ds, limits)
}
