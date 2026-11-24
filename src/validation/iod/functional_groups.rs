use super::modules::ConstraintSource;
use super::{
    Error, Finding, Kind, Options, Report, STANDARD_VERSION, Validator, iod_for_sop_class,
};
use crate::object::geometry::functional_group_source as fgs;
use crate::object::{DataSet, Element, ElementHeader, Object, Tag, Value, Vr};
use crate::validation::Severity;
use std::collections::{HashMap, HashSet};
pub const INVALID_FUNCTIONAL_GROUP: Kind = "invalid-functional-group";
pub const MISSING_FUNCTIONAL_GROUP: Kind = "missing-functional-group";
pub const FUNCTIONAL_GROUP_CONFLICT: Kind = "functional-group-conflict";
pub const MANUAL_FUNCTIONAL_GROUP: Kind = "manual-functional-group";
pub const MAX_FUNCTIONAL_GROUP_METADATA_BYTES: i64 = 16 << 20;
pub(super) const FG_SHARED: Tag = Tag::new(0x5200, 0x9229);
pub(super) const FG_PER_FRAME: Tag = Tag::new(0x5200, 0x9230);
const FG_FRAME_CONTENT: Tag = Tag::new(0x0020, 0x9111);
fn fg_tag(n: u32) -> Tag {
    Tag::new((n >> 16) as u16, n as u16)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FunctionalGroupOptions {
    pub big_endian: bool,
    pub iod: String,
    pub max_frames: i64,
    pub max_elements: i64,
    pub max_items: i64,
    pub max_depth: i64,
    pub max_operations: i64,
    pub max_findings: i64,
}
pub fn default_options() -> FunctionalGroupOptions {
    FunctionalGroupOptions {
        max_frames: 4096,
        max_elements: 100000,
        max_items: 100000,
        max_depth: 16,
        max_operations: 1000000,
        max_findings: 1000,
        ..FunctionalGroupOptions::default()
    }
}
fn normalize(mut o: FunctionalGroupOptions) -> Result<FunctionalGroupOptions, Error> {
    let d = default_options();
    let hard = [65536, 1000000, 1000000, 64, 4000000, 10000];
    let defaults = [
        d.max_frames,
        d.max_elements,
        d.max_items,
        d.max_depth,
        d.max_operations,
        d.max_findings,
    ];
    let fields = [
        &mut o.max_frames,
        &mut o.max_elements,
        &mut o.max_items,
        &mut o.max_depth,
        &mut o.max_operations,
        &mut o.max_findings,
    ];
    for (i, p) in fields.into_iter().enumerate() {
        if *p < 0 || *p > hard[i] {
            return Err(Error::FunctionalGroupOptions);
        }
        if *p == 0 {
            *p = defaults[i];
        }
    }
    Ok(o)
}
struct FgState {
    seen: HashSet<String>,
    projections: Vec<Object>,
    opts: FunctionalGroupOptions,
    report: Report,
    elements: i64,
    items: i64,
    operations: i64,
    primitive_bytes: i64,
    ambiguous: bool,
}
pub fn validate_functional_groups(
    obj: &Object,
    opts: &FunctionalGroupOptions,
) -> (Report, Option<Error>) {
    let normalized = match normalize(opts.clone()) {
        Ok(o) => o,
        Err(e) => return (Report::default(), Some(e)),
    };
    if obj.len() as i64 > normalized.max_elements {
        return (Report::default(), Some(Error::FunctionalGroupLimit));
    }
    validate_fg_data_set(&obj.to_data_set(), normalized, Some(obj))
}
pub fn validate_functional_group_data_set(
    ds: &DataSet,
    opts: &FunctionalGroupOptions,
) -> (Report, Option<Error>) {
    validate_fg_data_set(ds, opts.clone(), None)
}
fn validate_fg_data_set(
    ds: &DataSet,
    opts: FunctionalGroupOptions,
    facade: Option<&Object>,
) -> (Report, Option<Error>) {
    let opts = match normalize(opts) {
        Ok(o) => o,
        Err(e) => return (Report::default(), Some(e)),
    };
    let mut s = FgState {
        seen: HashSet::new(),
        projections: Vec::new(),
        report: Report {
            iod: opts.iod.clone(),
            standard_version: STANDARD_VERSION.to_string(),
            ..Report::default()
        },
        opts,
        elements: 0,
        items: 0,
        operations: 0,
        primitive_bytes: 0,
        ambiguous: false,
    };
    macro_rules! fail {
        ($e:expr) => {
            return (s.report, Some($e))
        };
    }
    if let Err(e) = s.scan(ds, 0, "") {
        fail!(e);
    }
    if s.ambiguous {
        return (s.report, None);
    }
    let owned;
    let obj: &Object = match facade {
        Some(f) => f,
        None => {
            let mut o = Object::from_elements(ds.elements.clone());
            if s.opts.big_endian {
                o.set_value_big_endian(true);
            }
            owned = o;
            &owned
        }
    };
    if s.opts.iod.is_empty() {
        let u = fg_text(obj, 0x00080016);
        s.opts.iod = iod_for_sop_class(&u).unwrap_or_default();
    }
    s.report.iod = s.opts.iod.clone();
    let Some(mut rules) = fg_rules(&s.opts.iod) else {
        fail!(Error::UnknownIod("functional groups profile".into()));
    };
    let iod = s.opts.iod.clone();
    let nof = Tag::new(0x0028, 0x0008);
    let n = match fg_integer(obj, nof) {
        Some(n) if n >= 1 => n,
        _ => {
            s.add(
                INVALID_FUNCTIONAL_GROUP,
                &nof.to_string(),
                nof,
                "number-of-frames",
                "C.7.6.6.1.1",
                "Number of Frames must be one positive IS value",
            );
            return (s.report, None);
        }
    };
    if n > s.opts.max_frames {
        fail!(Error::FunctionalGroupLimit);
    }
    let shared = match fg_raw_sequence(obj, FG_SHARED) {
        Some(sh) if sh.len() == 1 => sh,
        _ => {
            s.add(
                INVALID_FUNCTIONAL_GROUP,
                &FG_SHARED.to_string(),
                FG_SHARED,
                "shared-cardinality",
                "C.7.6.16",
                "Shared Functional Groups must have exactly one item",
            );
            return (s.report, None);
        }
    };
    let frames_raw = fg_raw_sequence(obj, FG_PER_FRAME);
    if obj.has(FG_PER_FRAME) && frames_raw.as_ref().is_none_or(|f| f.len() as i64 != n) {
        s.add(
            INVALID_FUNCTIONAL_GROUP,
            &FG_PER_FRAME.to_string(),
            FG_PER_FRAME,
            "per-frame-cardinality",
            "C.7.6.16",
            "Per-Frame item count must match Number of Frames",
        );
        return (s.report, None);
    }
    let frames = frames_raw.unwrap_or_default();
    if frames.len() as i64 > s.opts.max_frames {
        fail!(Error::FunctionalGroupLimit);
    }
    let mut shared_cost = shared[0].elements.len() as i64 + 1 + rules.len() as i64;
    for e in &shared[0].elements {
        if let Value::Sequence(items) = &e.value {
            shared_cost += items.len() as i64;
        }
    }
    if n > s.opts.max_operations / shared_cost {
        fail!(Error::FunctionalGroupLimit);
    }
    if let Err(e) = s.bind_instance_conditions(obj, &mut rules, n) {
        fail!(e);
    }
    for conflict in obj.functional_group_conflicts() {
        for frame in conflict.frames {
            s.add(
                FUNCTIONAL_GROUP_CONFLICT,
                &fg_path(frame, conflict.macro_tag),
                conflict.macro_tag,
                "shared-per-frame-conflict",
                "C.7.6.16.1.1",
                "Macro occurs in Shared and Per-Frame; existing resolver reports a conflict",
            );
        }
    }
    let allowed: HashSet<Tag> = rules.iter().map(|r| r.tag).collect();
    let mut known = HashSet::new();
    for profile in ["Enhanced CT Image", "Enhanced MR Image", "Segmentation"] {
        for r in fg_rules(profile).unwrap_or_default() {
            known.insert(r.tag);
        }
    }
    let mut groups = vec![shared[0].clone()];
    groups.extend(frames.iter().cloned());
    for (i, group) in groups.iter().enumerate() {
        for e in &group.elements {
            if known.contains(&e.tag()) && !allowed.contains(&e.tag()) {
                let path = if i > 0 {
                    fg_path(i as i64 - 1, e.tag())
                } else {
                    format!("{FG_SHARED}[1]/{}", e.tag())
                };
                s.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &path,
                    e.tag(),
                    "macro-profile-location",
                    fg_profile_section(&iod),
                    "Known macro is outside this IOD's permitted functional group profile",
                );
            }
        }
    }
    let shared_obj = obj.get_sequence(FG_SHARED).unwrap_or_default();
    if shared_obj.first().is_some_and(|o| o.has(FG_FRAME_CONTENT)) {
        s.add(
            INVALID_FUNCTIONAL_GROUP,
            &format!("{FG_SHARED}[1]/{FG_FRAME_CONTENT}"),
            FG_FRAME_CONTENT,
            "frame-content-location",
            fg_profile_section(&iod),
            "Frame Content shall not be Shared",
        );
    }
    let big = obj.value_big_endian();
    s.projections = (0..n as usize)
        .map(|frame| {
            let mut elements = vec![Element {
                header: ElementHeader {
                    tag: FG_SHARED,
                    vr: Vr::SQ,
                    length: 0,
                    length_set: false,
                },
                value: Value::Sequence(shared.clone()),
            }];
            if !frames.is_empty() {
                elements.push(Element {
                    header: ElementHeader {
                        tag: FG_PER_FRAME,
                        vr: Vr::SQ,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Sequence(vec![frames[frame].clone()]),
                });
            }
            let mut o = Object::from_elements(elements);
            o.set_value_big_endian(big);
            o
        })
        .collect();
    for frame in 0..n {
        for r in &rules {
            if let Err(e) = s.work() {
                fail!(e);
            }
            let projection = &s.projections[frame as usize];
            let (items, mut source, resolve_err) = projection.functional_group_items(0, r.tag);
            let conflict = resolve_err.is_some();
            let mut path = fg_path(frame, r.tag);
            if source == fgs::SHARED {
                path = format!("{FG_SHARED}[1]/{}", r.tag);
            }
            let mut present = !items.is_empty();
            if !present {
                (present, source) = fg_macro_present(projection, r.tag);
                if source == fgs::SHARED {
                    path = format!("{FG_SHARED}[1]/{}", r.tag);
                }
            }
            let (required, known) = r.required.eval(obj, frame);
            if !present && known && required {
                s.add(
                    MISSING_FUNCTIONAL_GROUP,
                    &path,
                    r.tag,
                    "required-macro",
                    fg_profile_section(&iod),
                    "Required macro is absent for this frame",
                );
            }
            if !present && !known {
                s.add(
                    MANUAL_FUNCTIONAL_GROUP,
                    &path,
                    r.tag,
                    "manual-macro-condition",
                    fg_profile_section(&iod),
                    "Macro presence needs context not established by this dataset",
                );
            }
            if present && !conflict {
                let (minimum, maximum, count_known) = fg_macro_cardinality(obj, r);
                let count = items.len() as i64;
                if count < minimum || maximum > 0 && count > maximum {
                    s.add(
                        INVALID_FUNCTIONAL_GROUP,
                        &path,
                        r.tag,
                        "macro-cardinality",
                        fg_cardinality_section(r),
                        "Macro item count is outside its normative range",
                    );
                } else if !count_known && count > 1 {
                    s.add(
                        MANUAL_FUNCTIONAL_GROUP,
                        &path,
                        r.tag,
                        "manual-macro-cardinality",
                        fg_cardinality_section(r),
                        "Macro maximum needs a Multi-energy CT Acquisition condition not established by this dataset",
                    );
                }
            }
            if present && r.forbidden.is_some_and(|f| f(obj)) {
                s.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &path,
                    r.tag,
                    "macro-forbidden",
                    fg_profile_section(&iod),
                    "Macro is forbidden under the dataset condition",
                );
            }
        }
        if iod == "Segmentation" {
            s.check_seg_geometry(obj, frame);
        }
    }
    if let Err(e) = s.dimensions(obj, n) {
        fail!(e);
    }
    let mut v = Validator {
        root: obj,
        report: std::mem::take(&mut s.report),
        max: s.opts.max_findings,
        seen: HashSet::new(),
        inferred: false,
        check_values: true,
        external: None,
    };
    v.check_functional_group_values("Enhanced Functional Groups");
    (v.report, None)
}
impl FgState {
    fn work(&mut self) -> Result<(), Error> {
        self.operations += 1;
        if self.operations > self.opts.max_operations {
            return Err(Error::FunctionalGroupLimit);
        }
        Ok(())
    }
    fn add(&mut self, k: Kind, path: &str, tag: Tag, rule: &str, section: &str, msg: &str) {
        let key = format!("{k} {path} {rule}");
        if self.seen.contains(&key) {
            return;
        }
        if self.report.findings.len() as i64 >= self.opts.max_findings {
            self.report.truncated += 1;
            return;
        }
        self.seen.insert(key);
        let severity = if k == MANUAL_FUNCTIONAL_GROUP {
            Severity::Info
        } else {
            Severity::Error
        };
        self.report.findings.push(Finding {
            kind: k,
            severity,
            module: "Enhanced Functional Groups".into(),
            path: path.to_string(),
            tag,
            typ: String::new(),
            condition: String::new(),
            rule: rule.to_string(),
            source: Some(fg_source(section, tag)),
            message: msg.to_string(),
        });
    }
    fn scan(&mut self, ds: &DataSet, depth: i64, path: &str) -> Result<(), Error> {
        self.work()?;
        if depth > self.opts.max_depth
            || ds.elements.len() as i64 > self.opts.max_elements - self.elements
        {
            return Err(Error::FunctionalGroupLimit);
        }
        let mut seen = HashSet::new();
        self.elements += ds.elements.len() as i64;
        for e in &ds.elements {
            self.work()?;
            let p = format!("{path}{}", e.tag());
            if !seen.insert(e.tag()) {
                self.ambiguous = true;
                self.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &p,
                    e.tag(),
                    "duplicate-tag",
                    "PS3.5 7.1",
                    "Duplicate raw tag is ambiguous; no last-wins validation",
                );
            }
            if e.tag() != fg_tag(0x7fe00010)
                && e.tag() != fg_tag(0x7fe00008)
                && e.tag() != fg_tag(0x7fe00009)
            {
                let (size, supported, within) =
                    fg_primitive_size(&e.value, super::MAX_CONSTRAINT_VALUE_BYTES);
                if !supported {
                    return Err(Error::FunctionalGroupOptions);
                }
                if !within || size > MAX_FUNCTIONAL_GROUP_METADATA_BYTES - self.primitive_bytes {
                    return Err(Error::FunctionalGroupLimit);
                }
                self.primitive_bytes += size;
            }
            let Value::Sequence(items) = &e.value else {
                if e.vr() == Vr::SQ {
                    self.ambiguous = true;
                    self.add(
                        MANUAL_FUNCTIONAL_GROUP,
                        &p,
                        e.tag(),
                        "sequence-unavailable",
                        "PS3.5 7.1",
                        "Sequence items are not materialized; structural validation is incomplete",
                    );
                }
                continue;
            };
            if e.vr() != Vr::SQ {
                self.ambiguous = true;
                self.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &p,
                    e.tag(),
                    "sequence-vr",
                    "PS3.5 7.1",
                    "Sequence value requires SQ VR",
                );
            }
            if depth == 0 && e.tag() == FG_PER_FRAME && items.len() as i64 > self.opts.max_frames {
                return Err(Error::FunctionalGroupLimit);
            }
            if items.len() as i64 > self.opts.max_items - self.items {
                return Err(Error::FunctionalGroupLimit);
            }
            self.items += items.len() as i64;
            for (i, item) in items.iter().enumerate() {
                self.scan(item, depth + 1, &format!("{p}[{}]/", i + 1))?;
            }
        }
        Ok(())
    }
    fn bind_instance_conditions(
        &mut self,
        obj: &Object,
        rules: &mut [FgRule],
        frames: i64,
    ) -> Result<(), Error> {
        let (original, original_known) = fg_original(obj, 0);
        for r in rules.iter_mut() {
            let tag = r.tag;
            if tag != fg_tag(0x00189117) && tag != fg_tag(0x00189107) {
                continue;
            }
            let (mut required, mut known) = (false, true);
            if !original_known {
                known = false;
            } else if original {
                let (mac, attribute, wanted) = if tag == fg_tag(0x00189107) {
                    (fg_tag(0x00189006), fg_tag(0x00189027), "SLAB")
                } else {
                    (fg_tag(0x00189226), fg_tag(0x00089209), "DIFFUSION")
                };
                for frame in 0..frames {
                    self.work()?;
                    let v = fg_frame_text(obj, frame, mac, attribute);
                    if v == wanted {
                        required = true;
                    }
                    if v.is_empty() {
                        known = false;
                    }
                }
                if required {
                    known = true;
                }
            }
            r.required = Required::Fixed(required, known);
        }
        Ok(())
    }
    fn check_seg_geometry(&mut self, obj: &Object, frame: i64) {
        if !obj.has(fg_tag(0x00200052)) {
            return;
        }
        let projection = &self.projections[frame as usize];
        let has = |tag: Tag| fg_macro_present(projection, tag).0;
        let measures = has(fg_tag(0x00289110));
        let patient = has(fg_tag(0x00209113)) && has(fg_tag(0x00209116));
        let slide = has(fg_tag(0x0048021A));
        let orientation = obj.get(fg_tag(0x00480102));
        let origin = fg_raw_sequence(obj, fg_tag(0x00480008));
        let implicit_slide = fg_text(obj, 0x00209311) == "TILED_FULL"
            && orientation.is_some_and(|o| o.vr() == Vr::DS && !fg_indexed_empty(o))
            && origin.is_some_and(|o| o.len() == 1);
        if !measures || !patient && !slide && !implicit_slide {
            self.add(
                MISSING_FUNCTIONAL_GROUP,
                &fg_path(frame, fg_tag(0x00289110)),
                fg_tag(0x00289110),
                "seg-frame-of-reference-geometry",
                "A.51.5",
                "Frame of Reference requires Pixel Measures and patient position/orientation or the slide geometry branch",
            );
        }
    }
    fn dimensions(&mut self, obj: &Object, frames: i64) -> Result<(), Error> {
        let tag = fg_tag(0x00209222);
        let Some(raw) = fg_raw_sequence(obj, tag) else {
            return Ok(());
        };
        if raw.is_empty() {
            self.add(
                INVALID_FUNCTIONAL_GROUP,
                &tag.to_string(),
                tag,
                "dimension-index-cardinality",
                "C.7.6.17",
                "Present Dimension Index Sequence requires one or more items",
            );
            return Ok(());
        }
        if raw.len() as i64 > self.opts.max_operations / (frames + 1) {
            return Err(Error::FunctionalGroupLimit);
        }
        let dimensions = obj.get_sequence(tag).unwrap_or_default();
        let div = fg_tag(0x00209157);
        for frame in 0..frames {
            let (holder, _, _) =
                self.projections[frame as usize].functional_group_item(0, FG_FRAME_CONTENT);
            if holder.is_some_and(|h| !h.has(div)) {
                self.add(
                    MISSING_FUNCTIONAL_GROUP,
                    &format!("{}[1]/{div}", fg_path(frame, FG_FRAME_CONTENT)),
                    div,
                    "dimension-index-values-required",
                    "C.7.6.17",
                    "Present Frame Content requires Dimension Index Values when dimensions are explicit",
                );
            }
        }
        let catalog = fg_rules(&self.opts.iod).unwrap_or_default();
        for (i, d) in dimensions.iter().enumerate() {
            self.work()?;
            let path = format!("{tag}[{}]/", i + 1);
            let dip = fg_tag(0x00209165);
            let Some(pointer) = fg_one_at(d, dip) else {
                self.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &format!("{path}{dip}"),
                    dip,
                    "dimension-pointer-vm",
                    "C.7.6.17",
                    "Dimension Index Pointer must be AT VM1",
                );
                continue;
            };
            if pointer == FG_FRAME_CONTENT || pointer == div {
                self.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &format!("{path}{pointer}"),
                    pointer,
                    "dimension-pointer-forbidden",
                    "C.7.6.17",
                    "Dimension Index Pointer shall not index Frame Content or Dimension Index Values",
                );
                continue;
            }
            let fgp = fg_tag(0x00209167);
            let group = fg_one_at(d, fgp);
            if d.has(fgp) && group.is_none() {
                self.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &format!("{path}{fgp}"),
                    fgp,
                    "functional-group-pointer-vm",
                    "C.7.6.17",
                    "Functional Group Pointer must be AT VM1",
                );
                continue;
            }
            if pointer.group % 2 != 0 || group.is_some_and(|g| g.group % 2 != 0) {
                self.add(
                    MANUAL_FUNCTIONAL_GROUP,
                    &format!("{path}{pointer}"),
                    pointer,
                    "private-dimension-pointer",
                    "C.7.6.17",
                    "Private dimension semantics and creator resolution need a separate profile",
                );
                continue;
            }
            if fg_pointer_parent(pointer) && group.is_none() {
                self.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &format!("{path}{pointer}"),
                    pointer,
                    "functional-group-pointer-required",
                    "C.7.6.17",
                    "Indexed profile attribute requires Functional Group Pointer",
                );
                continue;
            }
            let macro_pointer = catalog.iter().any(|r| r.tag == pointer);
            if macro_pointer && group.is_some() {
                self.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &format!("{path}{pointer}"),
                    pointer,
                    "macro-dimension-pointer-location",
                    "C.7.6.17",
                    "Functional Group Pointer shall be absent when the indexed attribute is itself a functional group",
                );
                continue;
            }
            let mut ordinals = Ordinals::default();
            for frame in 0..frames {
                self.work()?;
                let item_holder;
                let holder: &Object = if let Some(g) = group {
                    let (item, _, _) = self.projections[frame as usize].functional_group_item(0, g);
                    match item {
                        Some(item) => {
                            item_holder = item;
                            &item_holder
                        }
                        None => {
                            self.check_dimension_ordinal(frame, i, true, &mut ordinals);
                            self.add(
                                MANUAL_FUNCTIONAL_GROUP,
                                &fg_path(frame, g),
                                g,
                                "dimension-group-absent",
                                "C.7.6.17",
                                "Indexed values may be absent; consistency of the missing-value ordinal remains manual",
                            );
                            continue;
                        }
                    }
                } else {
                    obj
                };
                let mut present = holder.has(pointer);
                if macro_pointer {
                    let (items, _, _) =
                        self.projections[frame as usize].functional_group_items(0, pointer);
                    present = !items.is_empty();
                }
                if present && holder.get(pointer).is_some_and(fg_indexed_empty) {
                    present = false;
                }
                if !present {
                    self.check_dimension_ordinal(frame, i, true, &mut ordinals);
                    self.add(
                        MANUAL_FUNCTIONAL_GROUP,
                        &fg_path(frame, pointer),
                        pointer,
                        "dimension-pointer-absent",
                        "C.7.6.17",
                        "Indexed values may be absent; consistency of the missing-value ordinal remains manual",
                    );
                } else {
                    self.check_dimension_ordinal(frame, i, false, &mut ordinals);
                }
            }
        }
        Ok(())
    }
    fn check_dimension_ordinal(
        &mut self,
        frame: i64,
        dimension: usize,
        missing: bool,
        ordinals: &mut Ordinals,
    ) {
        let (holder, _, _) =
            self.projections[frame as usize].functional_group_item(0, FG_FRAME_CONTENT);
        let Some(holder) = holder else {
            return;
        };
        let tag = fg_tag(0x00209157);
        let Some(element) = holder.get(tag) else {
            self.add(
                MISSING_FUNCTIONAL_GROUP,
                &format!("{}[1]/{tag}", fg_path(frame, FG_FRAME_CONTENT)),
                tag,
                "dimension-index-values-required",
                "C.7.6.17",
                "Present Frame Content requires Dimension Index Values when dimensions are explicit",
            );
            return;
        };
        if element.vr() != Vr::UL
            || !fg_primitive_within(element, super::MAX_CONSTRAINT_VALUE_BYTES)
        {
            self.add(
                INVALID_FUNCTIONAL_GROUP,
                &fg_path(frame, tag),
                tag,
                "dimension-ordinal-encoding",
                "C.7.6.17",
                "Dimension index ordinals require bounded UL values",
            );
            return;
        }
        let Ok(values) = holder.get_ints(tag) else {
            return;
        };
        if dimension >= values.len() {
            return;
        }
        let ordinal = values[dimension];
        if ordinal <= 0 {
            self.add(
                INVALID_FUNCTIONAL_GROUP,
                &fg_path(frame, tag),
                tag,
                "dimension-ordinal-positive",
                "C.7.6.17",
                "Dimension index ordinals start at one",
            );
            return;
        }
        if let Some(&was_missing) = ordinals.states.get(&ordinal)
            && was_missing != missing
        {
            self.add(
                INVALID_FUNCTIONAL_GROUP,
                &fg_path(frame, tag),
                tag,
                "dimension-missing-ordinal-collision",
                "C.7.6.17",
                "Missing indexed values require an ordinal different from present indexed values",
            );
        }
        if missing {
            if ordinals.missing_ordinal != 0 && ordinals.missing_ordinal != ordinal {
                self.add(
                    INVALID_FUNCTIONAL_GROUP,
                    &fg_path(frame, tag),
                    tag,
                    "dimension-missing-ordinal-consistency",
                    "C.7.6.17",
                    "Missing indexed values must use the same missing-value ordinal",
                );
            } else {
                ordinals.missing_ordinal = ordinal;
            }
        }
        ordinals.states.insert(ordinal, missing);
    }
}
#[derive(Default)]
struct Ordinals {
    states: HashMap<i64, bool>,
    missing_ordinal: i64,
}
pub fn validate_with_functional_groups(
    obj: &Object,
    opts: &Options,
    fg: &FunctionalGroupOptions,
) -> (Report, Option<Error>) {
    let mut fg = fg.clone();
    if fg.iod.is_empty() {
        fg.iod = opts.iod.clone();
    }
    let (structural, err) = validate_functional_groups(obj, &fg);
    if err.is_some() {
        return (structural, err);
    }
    let (mut legacy, err) = super::validate(obj, opts);
    if err.is_some() {
        return (legacy, err);
    }
    let max = if opts.max_findings <= 0 {
        1000
    } else {
        opts.max_findings
    };
    for f in structural.findings {
        let duplicate = legacy
            .findings
            .iter()
            .any(|old| old.path == f.path && old.kind == f.kind && old.rule == f.rule);
        if duplicate {
            continue;
        }
        if legacy.findings.len() as i64 >= max {
            legacy.truncated += 1;
        } else {
            legacy.findings.push(f);
        }
    }
    legacy.truncated += structural.truncated;
    (legacy, None)
}
pub(super) fn fg_raw_sequence(obj: &Object, tag: Tag) -> Option<Vec<DataSet>> {
    let e = obj.get(tag)?;
    if e.vr() != Vr::SQ {
        return None;
    }
    match &e.value {
        Value::Sequence(items) => Some(items.clone()),
        _ => None,
    }
}
pub(super) fn fg_integer(obj: &Object, tag: Tag) -> Option<i64> {
    let e = obj.get(tag)?;
    if e.vr() != Vr::IS || !fg_primitive_within(e, 12) {
        return None;
    }
    let values = super::super::string_values(e);
    if values.len() != 1 {
        return None;
    }
    let v = values[0].trim();
    let digits = v.strip_prefix(['+', '-']).unwrap_or(v);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    v.parse().ok()
}
fn fg_path(frame: i64, mac: Tag) -> String {
    format!("{FG_PER_FRAME}[{}]/{mac}", frame + 1)
}
fn fg_macro_present(o: &Object, tag: Tag) -> (bool, &'static str) {
    for (location, source) in [(FG_PER_FRAME, fgs::PER_FRAME), (FG_SHARED, fgs::SHARED)] {
        let Some(items) = fg_raw_sequence(o, location) else {
            continue;
        };
        if items.len() != 1 {
            continue;
        }
        if items[0].elements.iter().any(|e| e.tag() == tag) {
            return (true, source);
        }
    }
    (false, fgs::NONE)
}
pub(super) fn fg_primitive_within(e: &Element, max: i64) -> bool {
    let (_, supported, within) = fg_primitive_size(&e.value, max);
    supported && within
}
fn fg_primitive_size(value: &Value, max: i64) -> (i64, bool, bool) {
    match value {
        Value::Nil | Value::Sequence(_) | Value::BulkData(_) => (0, true, true),
        Value::Raw(raw) => (raw.len() as i64, true, raw.len() as i64 <= max),
        Value::Strings(v) => {
            if v.len() > 100000 {
                return (0, true, false);
            }
            let mut size = 0i64;
            for (i, s) in v.iter().enumerate() {
                if s.len() as i64 > max - size {
                    return (0, true, false);
                }
                size += s.len() as i64;
                if i > 0 {
                    if size == max {
                        return (0, true, false);
                    }
                    size += 1;
                }
            }
            (size, true, true)
        }
        Value::Fragments { .. } => (0, false, false),
    }
}
fn fg_one_at(obj: &Object, tag: Tag) -> Option<Tag> {
    let e = obj.get(tag)?;
    if e.vr() != Vr::AT {
        return None;
    }
    if let Value::Raw(raw) = &e.value
        && raw.len() != 4
    {
        return None;
    }
    let tags = obj.get_tags(tag)?;
    if tags.len() != 1 {
        return None;
    }
    Some(tags[0])
}
fn fg_pointer_parent(pointer: Tag) -> bool {
    matches!(
        pointer.key(),
        0x00209056
            | 0x00209057
            | 0x00209128
            | 0x00200032
            | 0x00200037
            | 0x00280030
            | 0x00180050
            | 0x00180088
            | 0x0062000B
    )
}
fn fg_indexed_empty(e: &Element) -> bool {
    match &e.value {
        Value::Sequence(items) => items.is_empty(),
        Value::BulkData(_) => false,
        v => {
            let (size, supported, _) = fg_primitive_size(v, super::MAX_CONSTRAINT_VALUE_BYTES);
            supported && size == 0
        }
    }
}
#[derive(Clone, Copy)]
enum Required {
    Mandatory,
    Optional,
    Manual,
    Fn(fn(&Object, i64) -> (bool, bool)),
    Fixed(bool, bool),
}
impl Required {
    fn eval(self, obj: &Object, frame: i64) -> (bool, bool) {
        match self {
            Required::Mandatory => (true, true),
            Required::Optional => (false, true),
            Required::Manual => (false, false),
            Required::Fn(f) => f(obj, frame),
            Required::Fixed(a, b) => (a, b),
        }
    }
}
#[derive(Clone, Copy)]
struct FgRule {
    tag: Tag,
    section: &'static str,
    required: Required,
    forbidden: Option<fn(&Object) -> bool>,
}
fn fg_text(o: &Object, n: u32) -> String {
    match o.get(fg_tag(n)) {
        Some(e) if fg_primitive_within(e, 256) => super::super::string_values(e)
            .into_iter()
            .next()
            .unwrap_or_default(),
        _ => String::new(),
    }
}
fn fg_and_original(o: &Object, tag: u32, allowed: fn(&str) -> bool) -> (bool, bool) {
    let (original, known) = fg_original(o, 0);
    if known && !original {
        return (false, true);
    }
    let v = fg_text(o, tag);
    (original && allowed(&v), known && !v.is_empty())
}
fn fg_cardiac(o: &Object, _: i64) -> (bool, bool) {
    fg_and_original(o, 0x00189037, |v| v != "NONE")
}
fn fg_respiratory(o: &Object, _: i64) -> (bool, bool) {
    fg_and_original(o, 0x00189170, |v| {
        v != "NONE" && v != "REALTIME" && v != "BREATH_HOLD"
    })
}
fn fg_kspace(o: &Object, _: i64) -> (bool, bool) {
    fg_and_original(o, 0x00189032, |v| v == "RECTILINEAR")
}
fn fg_velocity(o: &Object, _: i64) -> (bool, bool) {
    fg_and_original(o, 0x00189014, |v| v == "YES")
}
fn fg_image_third(o: &Object, wanted: &str) -> (bool, bool) {
    let Some(e) = o.get(fg_tag(0x00080008)) else {
        return (false, false);
    };
    if !fg_primitive_within(e, 256) {
        return (false, false);
    }
    let v = super::super::string_values(e);
    if v.len() < 3 {
        return (false, false);
    }
    (v[2] == wanted, true)
}
fn fg_metabolite(o: &Object, _: i64) -> (bool, bool) {
    fg_image_third(o, "METABOLITE_MAP")
}
fn fg_asl(o: &Object, _: i64) -> (bool, bool) {
    fg_image_third(o, "ASL")
}
fn fg_original(o: &Object, _: i64) -> (bool, bool) {
    let v = fg_text(o, 0x00080008);
    (v == "ORIGINAL" || v == "MIXED", !v.is_empty())
}
fn fg_mono(o: &Object, _: i64) -> (bool, bool) {
    let v = fg_text(o, 0x00280004);
    (v == "MONOCHROME2", !v.is_empty())
}
fn fg_contrast(o: &Object, _: i64) -> (bool, bool) {
    (o.has(fg_tag(0x00180012)), true)
}
fn fg_derived(o: &Object, frame: i64) -> (bool, bool) {
    if fg_text(o, 0x00080008) == "DERIVED" {
        return (true, true);
    }
    let mut value = fg_frame_text(o, frame, fg_tag(0x00189329), fg_tag(0x00089007));
    if value.is_empty() {
        value = fg_frame_text(o, frame, fg_tag(0x00189226), fg_tag(0x00089007));
    }
    match value.as_str() {
        "DERIVED" => (true, true),
        "ORIGINAL" => (false, true),
        _ => (false, false),
    }
}
fn fg_segmentation(o: &Object, _: i64) -> (bool, bool) {
    let v = fg_text(o, 0x00620001);
    (
        fg_text(o, 0x00209311) != "TILED_FULL" && v != "LABELMAP",
        v == "BINARY" || v == "FRACTIONAL" || v == "LABELMAP",
    )
}
fn fg_seg_derivation(o: &Object, _: i64) -> (bool, bool) {
    if !o.has(fg_tag(0x00200052)) {
        return (true, true);
    }
    (false, false)
}
fn fg_ct_reconstruction(o: &Object, frame: i64) -> (bool, bool) {
    let (original, known) = fg_original(o, frame);
    if known && !original {
        return (false, true);
    }
    let v = fg_frame_text(o, frame, fg_tag(0x00189301), fg_tag(0x00189302));
    (original && v != "CONSTANT_ANGLE", known && !v.is_empty())
}
fn fg_ct_rwvm(o: &Object, _: i64) -> (bool, bool) {
    let v = fg_text(o, 0x00189361);
    (
        v == "YES",
        v == "YES" || v == "NO" || !o.has(fg_tag(0x00189361)),
    )
}
fn fg_not_mono(o: &Object) -> bool {
    let v = fg_text(o, 0x00280004);
    !v.is_empty() && v != "MONOCHROME2"
}
fn fg_rules(iod: &str) -> Option<Vec<FgRule>> {
    let mut rules = Vec::new();
    let mut add = |tag: u32, section: &'static str, required: Required| {
        rules.push(FgRule {
            tag: fg_tag(tag),
            section,
            required,
            forbidden: None,
        })
    };
    if iod == "Segmentation" {
        for tag in [0x00289110, 0x00209113, 0x00209116, 0x0048021A] {
            add(tag, "A.51.5", Required::Manual);
        }
        add(0x00089124, "C.7.6.16.2.6", Required::Fn(fg_seg_derivation));
        add(0x00209111, "C.7.6.16.2.2", Required::Optional);
        add(0x0062000A, "C.8.20.3.1", Required::Fn(fg_segmentation));
        return Some(rules);
    }
    if iod != "Enhanced CT Image" && iod != "Enhanced MR Image" {
        return None;
    }
    for tag in [0x00289110, 0x00209111, 0x00209113, 0x00209116, 0x00209071] {
        add(tag, "C.7.6.16.2", Required::Mandatory);
    }
    add(0x00081140, "C.7.6.16.2.5", Required::Manual);
    add(0x00089124, "C.7.6.16.2.6", Required::Fn(fg_derived));
    add(0x00189118, "C.7.6.16.2.7", Required::Fn(fg_cardiac));
    add(0x00209253, "C.7.6.16.2.17", Required::Fn(fg_respiratory));
    add(0x00189341, "C.7.6.16.2.12", Required::Fn(fg_contrast));
    add(0x00289132, "C.7.6.16.2.10", Required::Optional);
    add(0x00409096, "C.7.6.16.2.11", Required::Optional);
    add(0x00209310, "C.7.6.16.2.23", Required::Optional);
    if iod == "Enhanced CT Image" {
        for tag in [0x00189477, 0x00189329, 0x00289145] {
            add(tag, "A.38.1.4", Required::Mandatory);
        }
        for tag in [
            0x00189301, 0x00189304, 0x00189308, 0x00189326, 0x00189312, 0x00189321, 0x00189325,
        ] {
            add(tag, "A.38.1.4", Required::Fn(fg_original));
        }
        add(0x00189314, "A.38.1.4", Required::Fn(fg_ct_reconstruction));
        add(0x00189360, "A.38.1.4", Required::Manual);
        for r in rules.iter_mut() {
            if r.tag == fg_tag(0x00409096) {
                r.required = Required::Fn(fg_ct_rwvm);
            }
        }
    } else {
        add(0x00189226, "C.8.13.5.1", Required::Mandatory);
        add(0x00289145, "C.7.6.16.2.9", Required::Fn(fg_mono));
        for tag in [
            0x00189112, 0x00189114, 0x00189115, 0x00189006, 0x00189042, 0x00189049, 0x00189119,
        ] {
            add(tag, "A.36.2.4", Required::Fn(fg_original));
        }
        add(0x00189125, "A.36.2.4", Required::Fn(fg_kspace));
        add(0x00189197, "A.36.2.4", Required::Fn(fg_velocity));
        add(0x00189152, "A.36.2.4", Required::Fn(fg_metabolite));
        add(0x00189251, "A.36.2.4", Required::Fn(fg_asl));
        for tag in [0x00189117, 0x00189107] {
            add(tag, "A.36.2.4", Required::Manual);
        }
        for r in rules.iter_mut() {
            if r.tag == fg_tag(0x00289132) || r.tag == fg_tag(0x00409096) {
                r.forbidden = Some(fg_not_mono);
            }
        }
    }
    Some(rules)
}
fn fg_profile_section(iod: &str) -> &'static str {
    match iod {
        "Enhanced CT Image" => "A.38.1.4",
        "Enhanced MR Image" => "A.36.2.4",
        _ => "A.51.5",
    }
}
const CARDINALITY_SECTIONS: [(u32, &str); 25] = [
    (0x00189477, "C.7.6.16.2.18"),
    (0x00189329, "C.8.15.3.1"),
    (0x00289145, "C.8.15.3.10"),
    (0x00189301, "C.8.15.3.2"),
    (0x00189304, "C.8.15.3.3"),
    (0x00189308, "C.8.15.3.4"),
    (0x00189326, "C.8.15.3.5"),
    (0x00189312, "C.8.15.3.6"),
    (0x00189314, "C.8.15.3.7"),
    (0x00189321, "C.8.15.3.8"),
    (0x00189325, "C.8.15.3.9"),
    (0x00189360, "C.8.15.3.11"),
    (0x00189112, "C.8.13.5.2"),
    (0x00189125, "C.8.13.5.3"),
    (0x00189114, "C.8.13.5.4"),
    (0x00189115, "C.8.13.5.5"),
    (0x00189006, "C.8.13.5.6"),
    (0x00189042, "C.8.13.5.7"),
    (0x00189049, "C.8.13.5.8"),
    (0x00189117, "C.8.13.5.9"),
    (0x00189119, "C.8.13.5.10"),
    (0x00189107, "C.8.13.5.11"),
    (0x00189152, "C.8.13.5.12"),
    (0x00189197, "C.8.13.5.13"),
    (0x00189251, "C.8.13.5.14"),
];
const CARDINALITY_HASHES: [(&str, &str); 30] = [
    (
        "C.8.15.3.1",
        "f9e29df26e1ff74aca9b544a6a8a83d0b9aedec978bdd62403d270754f2a7e7b",
    ),
    (
        "C.8.13.5.2",
        "f26a42ae5cfb73aeff9a7a2f11ed9d045cbe54c9bdf3d8d27cdfb5b2f262a4f2",
    ),
    (
        "C.8.13.5.3",
        "54d3b9abdd094193464bdcc38ffa33f305786c3bdf09fd22484bb912892219ea",
    ),
    (
        "C.8.13.5.4",
        "3f8b3ae2dfbc604f42784246a369775157ebb838ddadcc22a364f3cb7f450b94",
    ),
    (
        "C.8.13.5.5",
        "34e1bf79893b334bfaf44353cccf69a5eba6b01a5ecccf7269271aebd8d34e46",
    ),
    (
        "C.8.13.5.6",
        "74baff2f1529840c0dbc3f10f35841870eaa0b2da6aa6020ac3c3ee16f8bd0d6",
    ),
    (
        "C.8.13.5.7",
        "5cc57ad7e0c93c997e39162dcac4d2304180a8f6f1439ee3e249ac995cd90c78",
    ),
    (
        "C.8.13.5.8",
        "ef81eee2b5de9715abf6eed80e3217d528491f468d94cb74a363c33fcaea3d3a",
    ),
    (
        "C.8.13.5.9",
        "83775287aa2048d9a4c79fcd8d3dbe4356904459caac9b7fd53998190892c8bf",
    ),
    (
        "C.8.13.5.10",
        "93dc807e43fe020cdce25059844c4a1b516f46820430ade3fbd05e57f5aaad12",
    ),
    (
        "C.8.13.5.11",
        "aa507de516fd717fb8d7c7cb7a663863486feb2f35b5aac5657caa56a332547b",
    ),
    (
        "C.8.13.5.12",
        "6dd5cbd4e196d3d8190f407d71ef8ff02a6f4521047997255546bba1721bbd8e",
    ),
    (
        "C.8.13.5.13",
        "a60eda173a2aaa9693a533033e28ef0ec48693d81ea5a869e1ccb765b543088a",
    ),
    (
        "C.8.13.5.14",
        "82bb387e66ede168188b5f9181b09cb6eb1dc00e78780ea8117586e3253a9d8d",
    ),
    (
        "C.8.13.5.15",
        "39a7fe871435d5beb815c2be289b83c28ecb2944ff43baa4ccd826947b2b47e6",
    ),
    (
        "C.8.15.3.2",
        "9474c63861a3ec1830d1012b9646f1d5d663e117bb01ebf6fe781abc103a7995",
    ),
    (
        "C.8.15.3.3",
        "e79013da45b6084e31055b596a3ecaffaf9e38502f93ed2e253096fb4b7ecc73",
    ),
    (
        "C.8.15.3.4",
        "b38ea58688480ac308ac2e470cde858fa8a85f76ad311b48c2f22b47397b3907",
    ),
    (
        "C.8.15.3.5",
        "be6f2da6340ae0959b40026129e9c389786b0f94b8d1539d1c0ff64a08f6f3d1",
    ),
    (
        "C.8.15.3.6",
        "b15af979f763dedcbbfd8b0ae6f86dd52f56c59ca717521045af0112df180fa4",
    ),
    (
        "C.8.15.3.7",
        "7ab02fd74cc3d2e98a336ff0ff58df8c6f5effe12043bc987ebab7faf1e5ff1e",
    ),
    (
        "C.8.15.3.8",
        "8b23a859c3c41cc9498621bdb1e1ae275355cdafeaa490993f07edb291e858ac",
    ),
    (
        "C.8.15.3.9",
        "6b7040dca3ca2525dcbbf3b32b79ea12f2ff84c51d3c178865a06aa3e193e6f4",
    ),
    (
        "C.8.15.3.10",
        "82dc06ccc539c875c53316c635ae7cfa0a4e8bae68cfb53f9be2fd41275ac45f",
    ),
    (
        "C.8.15.3.11",
        "cf9c147ed6e28946b22f7e07475c9e8ee08f9bfe201da11aa1e538c5221c9500",
    ),
    (
        "C.8.15.3.12",
        "b6b519e7fea18119f7f2f12dda560067ba3613408e0ac6ee1a1f2c6df49b900b",
    ),
    (
        "C.8.15.3.13",
        "342a6e0c518655ee836be25f15ee34e28f766b6b3a29ca390386ef8a15e20d7d",
    ),
    (
        "C.8.15.3.14",
        "97833caac605e471cda4901c3e21e2fe0f5746f72ebb1294d55f16d0e09b19a9",
    ),
    (
        "C.8.15.3.15",
        "445b6db53cd9d38ed905b4fca768881909aec7c8b7dd371eef0bd1ff64de222c",
    ),
    ("", ""),
];
fn cardinality_hash(section: &str) -> &'static str {
    CARDINALITY_HASHES
        .iter()
        .find(|(k, _)| !k.is_empty() && *k == section)
        .map_or("", |(_, v)| v)
}
fn fg_source(section: &str, tag: Tag) -> ConstraintSource {
    let mut sha = match section {
        "A.38.1.4" => "9c38e8348e573f8d8691d5dddfc4479fab2ee64bf83151458fcd8340cf11063d",
        "A.36.2.4" => "689cfd725cd9d894e74c88d034dc36be89cae36f59e58d4d61ede69226018d4c",
        "A.51.5" => "6159d1a48a22d3b904642897046617fdac319ef9ce4622074f8958e38ad8e050",
        "C.7.6.17" => "5ad3dbabec7f5495ed8c856518020c6349481b27e097f50ea9ed8a7e39bf9b7c",
        _ => "ad70f442400a53643a86a42344ba0d8f93e1e66f33ab6b20791a12f998524634",
    };
    let h = cardinality_hash(section);
    if !h.is_empty() {
        sha = h;
    }
    if section.starts_with("C.7.6.16.2") {
        sha = "7e6c176242d323b9295862bbade84ea42f4eb4b5f8a6076c93c0ea47410a83fe";
    }
    if section.starts_with("C.8.13.5") && h.is_empty() {
        sha = "293c7993d416bd2b7e8f0ad6d07806fdd17846f59974dd31ee3c5e6f9b0d0bfc";
    }
    if section.starts_with("C.8.20.3") {
        sha = "f32b3b524f19b78f213ebee2c9e930a079dd9ad4bf725fae672efcc4d678cd8c";
    }
    if section == "PS3.5 7.1" {
        return ConstraintSource {
            table: "Data Element structure",
            table_id: "7.1",
            index: -1,
            tag: tag.key(),
            section: "7.1",
            edition: "PS3.5 2026d",
            sha256: "7646dc7a3a0603657bf5c8c08b2fb513ec3ca3d2274f3959bc2ba431e01718d5",
            ..ConstraintSource::default()
        };
    }
    ConstraintSource {
        table: "Functional Group structural profile",
        table_id: leak(section),
        index: -1,
        tag: tag.key(),
        section: leak(section),
        edition: "PS3.3 2026d",
        sha256: sha,
        ..ConstraintSource::default()
    }
}
fn leak(s: &str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static SET: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let set = SET.get_or_init(|| Mutex::new(HashSet::new()));
    let mut guard = set.lock().unwrap();
    if let Some(v) = guard.get(s) {
        return v;
    }
    let v: &'static str = Box::leak(s.to_string().into_boxed_str());
    guard.insert(v);
    v
}
fn fg_frame_text(o: &Object, frame: i64, mac: Tag, attribute: Tag) -> String {
    let read = |ds: &DataSet| -> String {
        for e in &ds.elements {
            if e.tag() != mac {
                continue;
            }
            let Value::Sequence(items) = &e.value else {
                return String::new();
            };
            if items.len() != 1 {
                return String::new();
            }
            for v in &items[0].elements {
                if v.tag() == attribute && fg_primitive_within(v, 256) {
                    return super::super::engine::element_string_value(v);
                }
            }
        }
        String::new()
    };
    if let Some(per) = fg_raw_sequence(o, FG_PER_FRAME)
        && frame >= 0
        && (frame as usize) < per.len()
    {
        let v = read(&per[frame as usize]);
        if !v.is_empty() {
            return v;
        }
    }
    if let Some(shared) = fg_raw_sequence(o, FG_SHARED)
        && shared.len() == 1
    {
        return read(&shared[0]);
    }
    String::new()
}
fn fg_macro_cardinality(o: &Object, r: &FgRule) -> (i64, i64, bool) {
    match r.tag.key() {
        0x00081140 | 0x00089124 | 0x00189107 => (0, 0, true),
        0x00409096 | 0x00189341 | 0x00189042 | 0x00189197 | 0x00189251 | 0x00189360 => (1, 0, true),
        0x00189304 | 0x00189312 | 0x00189321 | 0x00189325 => {
            let v = fg_text(o, 0x00189361);
            if v == "YES" {
                return (1, 0, true);
            }
            if v == "NO" || !o.has(fg_tag(0x00189361)) {
                return (1, 1, true);
            }
            (1, 0, false)
        }
        _ => (1, 1, true),
    }
}
fn fg_cardinality_section(r: &FgRule) -> &'static str {
    CARDINALITY_SECTIONS
        .iter()
        .find(|(t, _)| *t == r.tag.key())
        .map_or(r.section, |(_, s)| s)
}
