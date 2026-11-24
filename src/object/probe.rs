#![forbid(unsafe_code)]
use super::dict;
use super::error::{Error, Kind};
use super::parser::source::MemFile;
use super::parser::{Reader, ReaderOptions, TokenKind};
use super::transfer::{self, Syntax};
use super::types::{ElementHeader, Tag, UNDEFINED_LENGTH, Vr};
use std::rc::Rc;
use std::time::{Duration, Instant};
pub const ERR_PROBE_AMBIGUOUS: &str = "dicom: transfer syntax probe is ambiguous";
pub const ERR_PROBE_IMPOSSIBLE: &str = "dicom: transfer syntax probe found no viable candidate";
pub const ERR_PROBE_POLICY: &str = "dicom: invalid transfer syntax probe policy";
pub const DEFAULT_PROBE_BYTES: i64 = 1 << 20;
const DEFAULT_ELEMENTS: i64 = 256;
const DEFAULT_DEPTH: i64 = 16;
const DEFAULT_FRAGMENTS: i64 = 64;
const DEFAULT_CANDIDATES: i64 = 8;
const DEFAULT_MINIMUM_ELEMENTS: i64 = 2;
const DEFAULT_TOKENS: i64 = 1024;
const DEFAULT_DURATION: Duration = Duration::from_secs(1);
const DEFAULT_CANDIDATE_DURATION: Duration = Duration::from_millis(250);
const MINIMUM_DURATION: Duration = Duration::from_micros(1);
const DEFAULT_CONFIDENCE: f64 = 0.75;
const DEFAULT_GAP: f64 = 0.10;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Outcome {
    #[default]
    None,
    Selected,
    Ambiguous,
    Impossible,
}
impl Outcome {
    pub fn text(self) -> &'static str {
        match self {
            Outcome::None => "",
            Outcome::Selected => "selected",
            Outcome::Ambiguous => "ambiguous",
            Outcome::Impossible => "impossible",
        }
    }
}
pub mod reason {
    pub const NONE: &str = "";
    pub const COMPLETE: &str = "complete";
    pub const PREFIX_COHERENT: &str = "prefix-coherent";
    pub const PARSE_ERROR: &str = "parse-error";
    pub const NO_EVIDENCE: &str = "no-evidence";
    pub const CANDIDATE_BUDGET: &str = "candidate-byte-budget";
    pub const SAFETY_BUDGET: &str = "candidate-safety-budget";
    pub const PIXEL_DATA_BOUNDARY: &str = "pixel-data-boundary";
    pub const CANDIDATE_TIMEOUT: &str = "candidate-time-budget";
    pub const SHARED_TIMEOUT: &str = "shared-time-budget";
}
pub mod warning {
    pub const INPUT_TRUNCATED: &str = "input-truncated-to-shared-byte-budget";
    pub const TIME_LIMITED: &str = "shared-time-budget-exhausted";
}
pub mod failure {
    pub const NONE: &str = "";
    pub const MALFORMED: &str = "malformed";
    pub const TRUNCATED: &str = "truncated";
    pub const EVIDENCE_BUDGET: &str = "evidence-budget";
    pub const SAFETY_BUDGET: &str = "safety-budget";
    pub const TIMEOUT: &str = "timeout";
}
pub mod strictness {
    pub const NONE: &str = "";
    pub const RESERVED_BYTES: &str = "nonzero-reserved-bytes";
    pub const ODD_ELEMENT_LENGTH: &str = "odd-element-length";
}
#[derive(Clone, Debug, Default)]
pub struct ProbeOptions {
    pub candidates: Vec<Syntax>,
    pub minimum_confidence: f64,
    pub minimum_confidence_gap: f64,
    pub max_probe_bytes: i64,
    pub max_candidate_bytes: i64,
    pub max_elements: i64,
    pub max_sequence_depth: i64,
    pub max_fragments: i64,
    pub max_candidates: i64,
    pub minimum_elements: i64,
    pub max_tokens: i64,
    pub max_duration: Option<Duration>,
    pub max_candidate_duration: Option<Duration>,
}
#[derive(Clone, Debug)]
pub struct CandidateReport {
    pub syntax: Syntax,
    pub score: i64,
    pub confidence: f64,
    pub reason: &'static str,
    pub failure_class: &'static str,
    pub strictness_violation: &'static str,
    pub bytes_consumed: i64,
    pub tokens: i64,
    pub elements: i64,
    pub known_elements: i64,
    pub private_elements: i64,
    pub ordered_elements: i64,
    pub plausible_lengths: i64,
    pub dictionary_vr_matches: i64,
    pub dictionary_vr_mismatches: i64,
    pub dictionary_vm_matches: i64,
    pub complete: bool,
}
impl CandidateReport {
    fn new(syntax: Syntax) -> CandidateReport {
        CandidateReport {
            syntax,
            score: 0,
            confidence: 0.0,
            reason: reason::NONE,
            failure_class: failure::NONE,
            strictness_violation: strictness::NONE,
            bytes_consumed: 0,
            tokens: 0,
            elements: 0,
            known_elements: 0,
            private_elements: 0,
            ordered_elements: 0,
            plausible_lengths: 0,
            dictionary_vr_matches: 0,
            dictionary_vr_mismatches: 0,
            dictionary_vm_matches: 0,
            complete: false,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct ProbeReport {
    pub outcome: Outcome,
    pub selected: Option<Syntax>,
    pub confidence: f64,
    pub candidates: Vec<CandidateReport>,
    pub warnings: Vec<&'static str>,
}
impl ProbeReport {
    pub fn selected_candidate(&self) -> Option<&CandidateReport> {
        let selected = self.selected.as_ref()?;
        if selected.uid.is_empty() {
            return None;
        }
        self.candidates
            .iter()
            .find(|c| c.syntax.uid == selected.uid)
    }
    pub fn render(&self) -> String {
        let mut b = format!(
            "outcome={} confidence={:.3}",
            self.outcome.text(),
            self.confidence
        );
        if let Some(s) = &self.selected
            && !s.uid.is_empty()
        {
            b.push_str(&format!(" selected={}", s.uid));
        }
        for c in &self.candidates {
            b.push_str(&format!(
                " candidate={} score={} confidence={:.3} reason={} failure={} strictness={} bytes={} tokens={} elements={}",
                c.syntax.uid,
                c.score,
                c.confidence,
                c.reason,
                c.failure_class,
                c.strictness_violation,
                c.bytes_consumed,
                c.tokens,
                c.elements
            ));
        }
        for w in &self.warnings {
            b.push_str(&format!(" warning={w}"));
        }
        b
    }
}
struct Normalized {
    candidates: Vec<Syntax>,
    minimum_confidence: f64,
    minimum_confidence_gap: f64,
    max_probe_bytes: i64,
    max_candidate_bytes: i64,
    max_elements: i64,
    max_sequence_depth: i64,
    max_fragments: i64,
    minimum_elements: i64,
    max_tokens: i64,
    max_duration: Duration,
    max_candidate_duration: Duration,
}
fn or_default(v: i64, d: i64) -> i64 {
    if v == 0 { d } else { v }
}
fn policy(detail: String) -> Error {
    Error::msg(format!("{ERR_PROBE_POLICY}: {detail}"))
}
fn normalize(o: &ProbeOptions) -> Result<Normalized, Error> {
    if o.minimum_confidence < 0.0
        || o.minimum_confidence_gap < 0.0
        || o.max_probe_bytes < 0
        || o.max_candidate_bytes < 0
        || o.max_elements < 0
        || o.max_sequence_depth < 0
        || o.max_fragments < 0
        || o.max_candidates < 0
        || o.minimum_elements < 0
        || o.max_tokens < 0
    {
        return Err(policy("limits and thresholds must not be negative".into()));
    }
    let candidates = if o.candidates.is_empty() {
        vec![
            transfer::IMPLICIT_VR_LITTLE_ENDIAN,
            transfer::EXPLICIT_VR_LITTLE_ENDIAN,
            transfer::lookup("1.2.840.10008.1.2.2").unwrap_or(transfer::EXPLICIT_VR_LITTLE_ENDIAN),
        ]
    } else {
        o.candidates.clone()
    };
    let max_probe_bytes = or_default(o.max_probe_bytes, DEFAULT_PROBE_BYTES);
    let n = Normalized {
        minimum_confidence: if o.minimum_confidence == 0.0 {
            DEFAULT_CONFIDENCE
        } else {
            o.minimum_confidence
        },
        minimum_confidence_gap: if o.minimum_confidence_gap == 0.0 {
            DEFAULT_GAP
        } else {
            o.minimum_confidence_gap
        },
        max_probe_bytes,
        max_candidate_bytes: or_default(o.max_candidate_bytes, max_probe_bytes),
        max_elements: or_default(o.max_elements, DEFAULT_ELEMENTS),
        max_sequence_depth: or_default(o.max_sequence_depth, DEFAULT_DEPTH),
        max_fragments: or_default(o.max_fragments, DEFAULT_FRAGMENTS),
        minimum_elements: or_default(o.minimum_elements, DEFAULT_MINIMUM_ELEMENTS),
        max_tokens: or_default(o.max_tokens, DEFAULT_TOKENS),
        max_duration: o
            .max_duration
            .filter(|d| !d.is_zero())
            .unwrap_or(DEFAULT_DURATION),
        max_candidate_duration: o
            .max_candidate_duration
            .filter(|d| !d.is_zero())
            .unwrap_or(DEFAULT_CANDIDATE_DURATION),
        candidates: Vec::new(),
    };
    let max_candidates = or_default(o.max_candidates, DEFAULT_CANDIDATES);
    if candidates.len() as i64 > max_candidates {
        return Err(policy(format!(
            "{} candidates exceed limit {max_candidates}",
            candidates.len()
        )));
    }
    let mut out = Vec::with_capacity(candidates.len());
    for (i, candidate) in candidates.into_iter().enumerate() {
        let candidate = transfer::lookup(candidate.uid).unwrap_or(candidate);
        if candidate.uid.is_empty() || !candidate.supported {
            return Err(policy(format!(
                "candidate {i} is not a supported native syntax"
            )));
        }
        if candidate.encapsulated || candidate.deflated {
            return Err(policy(format!(
                "candidate {} requires an explicit transfer syntax hint",
                crate::textfmt::quote(candidate.uid)
            )));
        }
        if out.iter().any(|c: &Syntax| c.uid == candidate.uid) {
            return Err(policy(format!(
                "duplicate candidate {}",
                crate::textfmt::quote(candidate.uid)
            )));
        }
        out.push(candidate);
    }
    Ok(Normalized {
        candidates: out,
        ..n
    })
}
fn deadline(duration: Duration) -> Instant {
    let now = Instant::now();
    if duration < MINIMUM_DURATION {
        return now;
    }
    now + duration
}
fn numeric_width(vr: Vr) -> Option<u32> {
    match vr {
        Vr::US | Vr::SS => Some(2),
        Vr::AT | Vr::FL | Vr::SL | Vr::UL => Some(4),
        Vr::FD | Vr::OD | Vr::OV | Vr::SV | Vr::UV => Some(8),
        _ => None,
    }
}
fn plausible_element_length(h: &ElementHeader) -> bool {
    if h.length == UNDEFINED_LENGTH {
        return h.vr == Vr::SQ || h.vr == Vr::UN || h.tag == Tag::PIXEL_DATA;
    }
    h.length.is_multiple_of(numeric_width(h.vr).unwrap_or(1))
}
fn fixed_width_multiplicity(h: &ElementHeader) -> Option<i64> {
    if h.length == UNDEFINED_LENGTH {
        return None;
    }
    let width = numeric_width(h.vr)?;
    if !h.length.is_multiple_of(width) {
        return None;
    }
    Some(i64::from(h.length / width))
}
fn atoi(s: &str) -> Option<i64> {
    crate::numtext::parse_int(s).ok()
}
fn vm_allows_count(vm: &str, count: i64) -> bool {
    if count <= 0 || vm.is_empty() {
        return false;
    }
    let parts: Vec<&str> = vm.split('-').collect();
    let Some(minimum) = atoi(parts[0]) else {
        return false;
    };
    if parts.len() == 1 {
        return count == minimum;
    }
    if parts.len() != 2 || count < minimum {
        return false;
    }
    let upper = parts[1];
    if upper == "n" {
        return true;
    }
    if let Some(m) = upper.strip_suffix('n') {
        return atoi(m).is_some_and(|m| m > 0 && count % m == 0);
    }
    atoi(upper).is_some_and(|max| count <= max)
}
fn has(err: &Error, sentinel: &str) -> bool {
    err.to_string().contains(sentinel)
}
fn classify(err: &Error, at_evidence_boundary: bool) -> &'static str {
    if has(err, "dicom: maximum element count exceeded") || err.is(Kind::MaxTotalBytes) {
        return failure::EVIDENCE_BUDGET;
    }
    if has(err, "dicom: maximum sequence nesting depth exceeded")
        || has(err, "dicom: maximum fragment count exceeded")
        || has(err, "dicom: maximum element value size exceeded")
    {
        return failure::SAFETY_BUDGET;
    }
    if err.is(Kind::Eof) || err.is(Kind::UnexpectedEof) {
        if at_evidence_boundary {
            return failure::EVIDENCE_BUDGET;
        }
        return failure::TRUNCATED;
    }
    failure::MALFORMED
}
fn ratio(n: i64, d: i64) -> f64 {
    if d <= 0 { 0.0 } else { n as f64 / d as f64 }
}
fn candidate_confidence(r: &CandidateReport, input_bytes: usize, parse_err: bool) -> f64 {
    if r.failure_class == failure::SAFETY_BUDGET || r.failure_class == failure::TIMEOUT {
        return 0.0;
    }
    let mut c = 0.0;
    if r.complete {
        c += 0.35;
    } else if r.failure_class == failure::EVIDENCE_BUDGET && r.elements > 0 {
        c += 0.30;
    }
    c += 0.20f64.min(r.elements as f64 * 0.04);
    c += 0.20 * ratio(r.known_elements, r.elements);
    c += 0.10 * ratio(r.plausible_lengths, r.elements);
    c += 0.05 * ratio(r.ordered_elements, r.elements);
    if r.known_elements > 0 {
        c += 0.10 * ratio(r.dictionary_vr_matches, r.known_elements);
        c += 0.05 * ratio(r.dictionary_vm_matches, r.known_elements);
    } else if r.syntax.explicit_vr {
        c += 0.05;
    }
    if input_bytes > 0 {
        c += 0.10 * 1f64.min(r.bytes_consumed as f64 / input_bytes as f64);
    }
    c -= (0.25f64.min(r.dictionary_vr_mismatches as f64 * 0.10))
        .min(ratio(r.dictionary_vr_mismatches, r.known_elements));
    if parse_err && r.failure_class == failure::MALFORMED {
        c -= 0.20;
    } else if parse_err && r.failure_class == failure::TRUNCATED {
        c -= 0.10;
    }
    c.clamp(0.0, 1.0)
}
fn probe_candidate(
    data: &[u8],
    syntax: Syntax,
    opts: &Normalized,
    input_truncated: bool,
    shared_deadline: Instant,
) -> CandidateReport {
    let mut report = CandidateReport::new(syntax);
    let file = MemFile::new(Rc::from(data));
    let mut reader = Reader::new(
        Box::new(file),
        Some((data.len() as i64, 0)),
        syntax,
        ReaderOptions {
            max_total_bytes: data.len() as i64 + 1,
            max_sequence_depth: opts.max_sequence_depth,
            max_elements: opts.max_elements,
            max_fragments: opts.max_fragments,
            strict_reserved_bytes: true,
            accept_odd_length: false,
            skip_pixel_data: true,
            ..ReaderOptions::default()
        },
    );
    reader.set_stop_before_pixel_data(true);
    let candidate_deadline = deadline(opts.max_candidate_duration).min(shared_deadline);
    let mut depth = 0i64;
    let mut last_tags: std::collections::HashMap<i64, Tag> = std::collections::HashMap::new();
    let mut parse_err = false;
    loop {
        if report.tokens >= opts.max_tokens {
            report.reason = reason::CANDIDATE_BUDGET;
            report.failure_class = failure::EVIDENCE_BUDGET;
            break;
        }
        let now = Instant::now();
        if now >= candidate_deadline {
            report.reason = if now >= shared_deadline {
                reason::SHARED_TIMEOUT
            } else {
                reason::CANDIDATE_TIMEOUT
            };
            report.failure_class = failure::TIMEOUT;
            break;
        }
        let token = match reader.next() {
            Ok(token) => token,
            Err(err) if err.is(Kind::Eof) => {
                report.complete = !input_truncated;
                if report.complete {
                    report.reason = reason::COMPLETE;
                } else if report.elements > 0 {
                    report.reason = reason::PREFIX_COHERENT;
                    report.failure_class = failure::EVIDENCE_BUDGET;
                } else {
                    report.reason = reason::CANDIDATE_BUDGET;
                }
                break;
            }
            Err(err) => {
                parse_err = true;
                if has(
                    &err,
                    "dicom: explicit VR long-form reserved bytes must be zero",
                ) {
                    report.strictness_violation = strictness::RESERVED_BYTES;
                } else if has(&err, "dicom: element length must be even") {
                    report.strictness_violation = strictness::ODD_ELEMENT_LENGTH;
                }
                report.failure_class = classify(
                    &err,
                    input_truncated && reader.position() >= data.len() as i64,
                );
                report.reason = match report.failure_class {
                    failure::EVIDENCE_BUDGET => reason::CANDIDATE_BUDGET,
                    failure::SAFETY_BUDGET => reason::SAFETY_BUDGET,
                    _ => reason::PARSE_ERROR,
                };
                break;
            }
        };
        report.tokens += 1;
        match token.kind {
            TokenKind::EndItem | TokenKind::EndSequence => {
                last_tags.remove(&depth);
                if depth > 0 {
                    depth -= 1;
                }
                continue;
            }
            TokenKind::StartItem => {
                depth += 1;
                continue;
            }
            _ => {}
        }
        report.elements += 1;
        let header = token
            .header
            .unwrap_or_else(|| ElementHeader::new(Tag::default(), Vr::UN, 0));
        let tag = header.tag;
        if tag.is_private() {
            report.private_elements += 1;
        }
        if let Some(entry) = dict::by_tag(tag) {
            report.known_elements += 1;
            if !syntax.explicit_vr || entry.vr == header.vr || entry.vr == Vr::UN {
                report.dictionary_vr_matches += 1;
            } else {
                report.dictionary_vr_mismatches += 1;
            }
            if let Some(count) = fixed_width_multiplicity(&header)
                && vm_allows_count(dict::vm(tag).unwrap_or(""), count)
            {
                report.dictionary_vm_matches += 1;
            }
        }
        if plausible_element_length(&header) {
            report.plausible_lengths += 1;
        }
        match last_tags.get(&depth) {
            Some(previous) if *previous > tag => {}
            _ => {
                report.ordered_elements += 1;
                last_tags.insert(depth, tag);
            }
        }
        if tag == Tag::PIXEL_DATA || token.kind == TokenKind::StartPixelSequence {
            report.reason = reason::PIXEL_DATA_BOUNDARY;
            report.failure_class = failure::EVIDENCE_BUDGET;
            break;
        }
        if token.kind == TokenKind::StartSequence {
            depth += 1;
        }
    }
    report.bytes_consumed = reader.position();
    if report.elements == 0 {
        report.reason = reason::NO_EVIDENCE;
        report.confidence = 0.0;
        report.score = 0;
        return report;
    }
    report.confidence = candidate_confidence(&report, data.len(), parse_err);
    report.score = (report.confidence * 1000.0 + 0.5) as i64;
    report
}
pub fn probe_transfer_syntax(data: &[u8], options: &ProbeOptions) -> (ProbeReport, Option<Error>) {
    let opts = match normalize(options) {
        Ok(opts) => opts,
        Err(err) => {
            return (
                ProbeReport {
                    outcome: Outcome::Impossible,
                    ..ProbeReport::default()
                },
                Some(err),
            );
        }
    };
    let shared_limit = (data.len() as i64).min(opts.max_probe_bytes) as usize;
    let shared = &data[..shared_limit];
    let shared_truncated = data.len() > shared_limit;
    let mut report = ProbeReport::default();
    if shared_truncated {
        report.warnings.push(warning::INPUT_TRUNCATED);
    }
    let shared_deadline = deadline(opts.max_duration);
    for &syntax in &opts.candidates {
        let candidate_limit = (shared.len() as i64).min(opts.max_candidate_bytes) as usize;
        let candidate_truncated = shared_truncated || shared.len() > candidate_limit;
        if Instant::now() >= shared_deadline {
            if !report.warnings.contains(&warning::TIME_LIMITED) {
                report.warnings.push(warning::TIME_LIMITED);
            }
            let mut c = CandidateReport::new(syntax);
            c.reason = reason::SHARED_TIMEOUT;
            c.failure_class = failure::TIMEOUT;
            report.candidates.push(c);
            continue;
        }
        report.candidates.push(probe_candidate(
            &shared[..candidate_limit],
            syntax,
            &opts,
            candidate_truncated,
            shared_deadline,
        ));
    }
    report.candidates.sort_by(|l, r| {
        r.score
            .cmp(&l.score)
            .then_with(|| l.syntax.uid.cmp(r.syntax.uid))
    });
    if report
        .candidates
        .iter()
        .any(|c| c.failure_class == failure::TIMEOUT)
    {
        report.outcome = Outcome::Ambiguous;
        if let Some(c) = report.candidates.first() {
            report.confidence = c.confidence;
        }
        return (report, Some(Error::msg(ERR_PROBE_AMBIGUOUS)));
    }
    let Some(winner) = report.candidates.first() else {
        report.outcome = Outcome::Impossible;
        return (report, Some(Error::msg(ERR_PROBE_IMPOSSIBLE)));
    };
    if winner.elements == 0 || winner.confidence == 0.0 {
        report.outcome = Outcome::Impossible;
        return (report, Some(Error::msg(ERR_PROBE_IMPOSSIBLE)));
    }
    let second = report.candidates.get(1).map_or(0.0, |c| c.confidence);
    let (elements, confidence, syntax) = (winner.elements, winner.confidence, winner.syntax);
    if elements < opts.minimum_elements
        || confidence < opts.minimum_confidence
        || confidence - second < opts.minimum_confidence_gap
    {
        report.outcome = Outcome::Ambiguous;
        report.confidence = confidence;
        return (report, Some(Error::msg(ERR_PROBE_AMBIGUOUS)));
    }
    report.outcome = Outcome::Selected;
    report.selected = Some(syntax);
    report.confidence = confidence;
    (report, None)
}
