use super::private::{is_private_group, private_creator_id};
use crate::object::{Tag, Vr};
use crate::textfmt::quote;
use std::collections::HashMap;
use std::fmt;
pub const DEFAULT_DIC_MAX_FILE_SIZE: i64 = 16 << 20;
pub const DEFAULT_DIC_MAX_LINE_LENGTH: i64 = 4096;
pub const DEFAULT_DIC_MAX_ENTRIES: i64 = 65536;
pub const DEFAULT_DIC_MAX_REPEATING: i64 = 4096;
pub const DEFAULT_DIC_MAX_SKIPPED: i64 = 1024;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DicOptions {
    pub tolerant: bool,
    pub max_file_size: i64,
    pub max_line_length: i64,
    pub max_entries: i64,
    pub max_repeating: i64,
    pub max_skipped: i64,
}
impl DicOptions {
    fn normalized(mut self) -> Result<DicOptions, DicError> {
        if self.max_file_size < 0
            || self.max_line_length < 0
            || self.max_entries < 0
            || self.max_repeating < 0
            || self.max_skipped < 0
        {
            return Err(DicError::Options);
        }
        for (value, default) in [
            (&mut self.max_file_size, DEFAULT_DIC_MAX_FILE_SIZE),
            (&mut self.max_line_length, DEFAULT_DIC_MAX_LINE_LENGTH),
            (&mut self.max_entries, DEFAULT_DIC_MAX_ENTRIES),
            (&mut self.max_repeating, DEFAULT_DIC_MAX_REPEATING),
            (&mut self.max_skipped, DEFAULT_DIC_MAX_SKIPPED),
        ] {
            if *value == 0 {
                *value = default;
            }
        }
        Ok(self)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DicErrorKind {
    Syntax,
    Vr,
    ResourceLimit,
    Options,
    Other,
}
impl DicErrorKind {
    fn text(self) -> &'static str {
        match self {
            DicErrorKind::Syntax => "dicom: invalid .dic dictionary entry",
            DicErrorKind::Vr => "dicom: unsupported .dic dictionary VR",
            DicErrorKind::ResourceLimit => "dicom: .dic dictionary resource limit exceeded",
            DicErrorKind::Options => "dicom: invalid .dic dictionary options",
            DicErrorKind::Other => "",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DicLineError {
    pub line: i64,
    pub field: &'static str,
    pub reason: String,
    pub kind: DicErrorKind,
}
impl fmt::Display for DicLineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: line {}: {}: {}",
            self.kind.text(),
            self.line,
            self.field,
            self.reason
        )
    }
}
impl std::error::Error for DicLineError {}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DicError {
    Options,
    Line(DicLineError),
    SourceLimit(i64),
    FileLimit { size: i64, limit: i64 },
    NotRegular(String),
    Io(String),
}
impl DicError {
    pub fn kind(&self) -> DicErrorKind {
        match self {
            DicError::Options => DicErrorKind::Options,
            DicError::Line(e) => e.kind,
            DicError::SourceLimit(_) | DicError::FileLimit { .. } => DicErrorKind::ResourceLimit,
            DicError::NotRegular(_) | DicError::Io(_) => DicErrorKind::Other,
        }
    }
}
impl fmt::Display for DicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DicError::Options => f.write_str(DicErrorKind::Options.text()),
            DicError::Line(e) => e.fmt(f),
            DicError::SourceLimit(max) => write!(
                f,
                "{}: source exceeds {max} bytes",
                DicErrorKind::ResourceLimit.text()
            ),
            DicError::FileLimit { size, limit } => write!(
                f,
                "{}: file has {size} bytes, limit {limit}",
                DicErrorKind::ResourceLimit.text()
            ),
            DicError::NotRegular(path) => {
                write!(f, "dicom: .dic dictionary {path} is not a regular file")
            }
            DicError::Io(text) => f.write_str(text),
        }
    }
}
impl std::error::Error for DicError {}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DicReport {
    pub lines: i64,
    pub entries: i64,
    pub overridden: i64,
    pub skipped: Vec<DicLineError>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextualVr {
    Xs,
    Ox,
    Px,
    Lt,
    Up,
}
impl ContextualVr {
    pub fn as_str(self) -> &'static str {
        match self {
            ContextualVr::Xs => "xs",
            ContextualVr::Ox => "ox",
            ContextualVr::Px => "px",
            ContextualVr::Lt => "lt",
            ContextualVr::Up => "up",
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VrSpec {
    pub values: Vec<Vr>,
    pub context: Option<ContextualVr>,
}
impl VrSpec {
    pub fn contextual(context: ContextualVr) -> VrSpec {
        let values = match context {
            ContextualVr::Xs => vec![Vr::US, Vr::SS],
            ContextualVr::Ox | ContextualVr::Px => vec![Vr::OB, Vr::OW],
            ContextualVr::Lt => vec![Vr::US, Vr::SS, Vr::OW],
            ContextualVr::Up => vec![Vr::UL],
        };
        VrSpec {
            values,
            context: Some(context),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DicTagEntry {
    pub tag: Tag,
    pub vr: Option<Vr>,
    pub keyword: String,
    pub name: String,
    pub vm: String,
    pub retired: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
enum Parity {
    #[default]
    Any,
    Even,
    Odd,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
struct Part {
    lo: u16,
    hi: u16,
    mask: u16,
    parity: Parity,
}
impl Part {
    fn exact(self) -> bool {
        self.lo == self.hi
    }
    fn matches(self, v: u16) -> bool {
        if v < self.lo || v > self.hi || v & self.mask != self.lo & self.mask {
            return false;
        }
        match self.parity {
            Parity::Even => v & 1 == 0,
            Parity::Odd => v & 1 == 1,
            Parity::Any => true,
        }
    }
    fn first(self) -> Option<u16> {
        let mut v = self.lo;
        if !self.matches(v) {
            v = v.wrapping_add(1);
        }
        (v >= self.lo && self.matches(v)).then_some(v)
    }
    fn count(self) -> u64 {
        let Some(first) = self.first() else {
            return 0;
        };
        if self.mask != 0 || self.lo == self.hi {
            let wild = (0..16)
                .step_by(4)
                .filter(|shift| {
                    (self.mask >> shift) & 0xf == 0
                        && (self.hi >> shift) & 0xf == 0xf
                        && (self.lo >> shift) & 0xf == 0
                })
                .count();
            let mut n = 1u64 << (4 * wild);
            if self.parity != Parity::Any && self.mask & 0xf == 0 && self.lo != self.hi {
                n /= 2;
            }
            return n;
        }
        let span = u64::from(self.hi - first);
        if self.parity == Parity::Any {
            span + 1
        } else {
            span / 2 + 1
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DicEntry {
    pub line: i64,
    pub tag_field: String,
    pub creator: String,
    pub fixed: bool,
    pub repeating: bool,
    pub vr_token: String,
    pub vr_spec: VrSpec,
    pub version: String,
    pub vm_min: i64,
    pub vm_max: i64,
    pub vm_step: i64,
    pub entry: DicTagEntry,
    group: Part,
    element: Part,
    specificity: u64,
}
fn is_private_data_tag(tag: Tag) -> bool {
    is_private_group(tag.group) && tag.element >= 0x1000
}
impl DicEntry {
    pub fn matches(&self, tag: Tag) -> bool {
        if !self.group.matches(tag.group) {
            return false;
        }
        if self.creator.is_empty() || self.fixed {
            return self.element.matches(tag.element);
        }
        is_private_data_tag(tag) && self.element.matches(tag.element & 0xff)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    creator: String,
    fixed: bool,
    group: Part,
    element: Part,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PrivateKey {
    creator: String,
    fixed: bool,
    group: u16,
    element: u16,
}
#[derive(Debug, Clone, Default)]
pub struct DicDictionary {
    entries: Vec<DicEntry>,
    exact: HashMap<Tag, usize>,
    group_ranges: HashMap<u16, Vec<usize>>,
    wide_ranges: Vec<usize>,
    keywords: HashMap<String, usize>,
    private: HashMap<PrivateKey, usize>,
    fixed_ranges: Vec<usize>,
    offset_ranges: Vec<usize>,
}
pub fn load_dic_file(
    path: impl AsRef<std::path::Path>,
    opts: &DicOptions,
) -> Result<(DicDictionary, DicReport), DicError> {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    let norm = opts.normalized()?;
    let meta = std::fs::metadata(path).map_err(|e| DicError::Io(e.to_string()))?;
    if !meta.is_file() {
        return Err(DicError::NotRegular(path.to_string()));
    }
    let size = meta.len() as i64;
    if size > norm.max_file_size {
        return Err(DicError::FileLimit {
            size,
            limit: norm.max_file_size,
        });
    }
    let data = std::fs::read(path).map_err(|e| DicError::Io(e.to_string()))?;
    parse_dic(&data, opts)
}
struct SizeReader<'a> {
    data: &'a [u8],
    pos: usize,
    read: i64,
    max: i64,
    exceeded: bool,
}
enum ReadEnd {
    Eof,
    Exceeded,
}
impl SizeReader<'_> {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<ReadEnd>) {
        if self.exceeded {
            return (0, Some(ReadEnd::Exceeded));
        }
        if p.is_empty() {
            return (0, None);
        }
        let remaining = self.data.len() - self.pos;
        if self.read == self.max {
            if remaining > 0 {
                self.pos += 1;
                self.exceeded = true;
                return (0, Some(ReadEnd::Exceeded));
            }
            return (0, Some(ReadEnd::Eof));
        }
        let room = (self.max - self.read) as usize;
        let want = p.len().min(room);
        if remaining == 0 {
            return (0, Some(ReadEnd::Eof));
        }
        let n = want.min(remaining);
        p[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        self.read += n as i64;
        (n, None)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanErr {
    Eof,
    Exceeded,
    TooLong,
}
struct Scanner<'a> {
    r: SizeReader<'a>,
    buf: Vec<u8>,
    start: usize,
    end: usize,
    max: usize,
    err: Option<ScanErr>,
}
impl<'a> Scanner<'a> {
    fn set_err(&mut self, e: ScanErr) {
        if self.err.is_none() || self.err == Some(ScanErr::Eof) {
            self.err = Some(e);
        }
    }
    fn scan(&mut self) -> Option<(usize, usize)> {
        loop {
            if self.end > self.start || self.err.is_some() {
                let data = &self.buf[self.start..self.end];
                let at_eof = self.err.is_some();
                let found = match data.iter().position(|b| *b == b'\n') {
                    Some(i) => Some((i + 1, i)),
                    None if at_eof && !data.is_empty() => Some((data.len(), data.len())),
                    None => None,
                };
                if let Some((advance, len)) = found {
                    let mut end = self.start + len;
                    if len > 0 && self.buf[end - 1] == b'\r' {
                        end -= 1;
                    }
                    let token = (self.start, end);
                    self.start += advance;
                    return Some(token);
                }
            }
            if self.err.is_some() {
                self.start = 0;
                self.end = 0;
                return None;
            }
            if self.start > 0 && (self.end == self.buf.len() || self.start > self.buf.len() / 2) {
                self.buf.copy_within(self.start..self.end, 0);
                self.end -= self.start;
                self.start = 0;
            }
            if self.end == self.buf.len() {
                if self.buf.len() >= self.max {
                    self.set_err(ScanErr::TooLong);
                    return None;
                }
                let size = if self.buf.is_empty() {
                    4096
                } else {
                    self.buf.len() * 2
                }
                .min(self.max);
                let mut grown = vec![0u8; size];
                grown[..self.end - self.start].copy_from_slice(&self.buf[self.start..self.end]);
                self.buf = grown;
                self.end -= self.start;
                self.start = 0;
            }
            let end = self.end;
            let (n, e) = self.r.read(&mut self.buf[end..]);
            self.end += n;
            match e {
                Some(ReadEnd::Eof) => self.set_err(ScanErr::Eof),
                Some(ReadEnd::Exceeded) => self.set_err(ScanErr::Exceeded),
                None => {}
            }
        }
    }
}
fn line_error(line: i64, field: &'static str, reason: String) -> DicError {
    DicError::Line(DicLineError {
        line,
        field,
        reason,
        kind: DicErrorKind::ResourceLimit,
    })
}
pub fn parse_dic(input: &[u8], opts: &DicOptions) -> Result<(DicDictionary, DicReport), DicError> {
    let opts = opts.normalized()?;
    let mut report = DicReport::default();
    let max_line = opts.max_line_length;
    let initial = 4096.min(max_line + 2) as usize;
    let mut scanner = Scanner {
        r: SizeReader {
            data: input,
            pos: 0,
            read: 0,
            max: opts.max_file_size,
            exceeded: false,
        },
        buf: vec![0; initial],
        start: 0,
        end: 0,
        max: (max_line + 2) as usize,
        err: None,
    };
    let mut defs: Vec<DicEntry> = Vec::new();
    let mut repeating = 0i64;
    while let Some((start, end)) = scanner.scan() {
        if scanner.r.exceeded {
            return Err(DicError::SourceLimit(opts.max_file_size));
        }
        report.lines += 1;
        let raw = &scanner.buf[start..end];
        if raw.len() as i64 > max_line {
            return Err(line_error(
                report.lines,
                "line",
                format!("longer than {max_line} bytes"),
            ));
        }
        let mut line = raw;
        while let [rest @ .., b' ' | b'\t' | b'\r'] = line {
            line = rest;
        }
        let body = line
            .iter()
            .position(|b| *b != b' ' && *b != b'\t')
            .map_or(&[][..], |i| &line[i..]);
        if body.is_empty() || body[0] == b'#' {
            continue;
        }
        let mut def = match parse_line(line) {
            Ok(def) => def,
            Err(mut e) => {
                e.line = report.lines;
                if !opts.tolerant {
                    return Err(DicError::Line(e));
                }
                if report.skipped.len() as i64 >= opts.max_skipped {
                    return Err(line_error(
                        report.lines,
                        "line",
                        format!("more than {} malformed lines", opts.max_skipped),
                    ));
                }
                report.skipped.push(e);
                continue;
            }
        };
        if defs.len() as i64 >= opts.max_entries {
            return Err(line_error(
                report.lines,
                "line",
                format!("more than {} entries", opts.max_entries),
            ));
        }
        if def.repeating {
            if repeating >= opts.max_repeating {
                return Err(line_error(
                    report.lines,
                    "tag",
                    format!("more than {} repeating entries", opts.max_repeating),
                ));
            }
            repeating += 1;
        }
        def.line = report.lines;
        defs.push(def);
    }
    match scanner.err {
        Some(ScanErr::TooLong) => {
            return Err(line_error(
                report.lines + 1,
                "line",
                format!("longer than {max_line} bytes"),
            ));
        }
        Some(ScanErr::Exceeded) => return Err(DicError::SourceLimit(opts.max_file_size)),
        _ => {}
    }
    report.entries = defs.len() as i64;
    let total = defs.len();
    let d = build(defs);
    report.overridden = (total - d.entries.len()) as i64;
    Ok((d, report))
}
fn better(a: &DicEntry, b: &DicEntry) -> bool {
    if a.specificity != b.specificity {
        return a.specificity < b.specificity;
    }
    a.line > b.line
}
fn build(defs: Vec<DicEntry>) -> DicDictionary {
    let key = |d: &DicEntry| Key {
        creator: d.creator.clone(),
        fixed: d.fixed,
        group: d.group,
        element: d.element,
    };
    let mut latest = HashMap::with_capacity(defs.len());
    for (i, def) in defs.iter().enumerate() {
        latest.insert(key(def), i);
    }
    let mut d = DicDictionary::default();
    for (i, def) in defs.iter().enumerate() {
        if latest[&key(def)] == i {
            d.entries.push(def.clone());
        }
    }
    for (i, def) in d.entries.iter().enumerate() {
        if !def.creator.is_empty() && !def.repeating {
            d.private.insert(
                PrivateKey {
                    creator: def.creator.clone(),
                    fixed: def.fixed,
                    group: def.group.lo,
                    element: def.element.lo,
                },
                i,
            );
        } else if !def.creator.is_empty() && def.fixed {
            d.fixed_ranges.push(i);
        } else if !def.creator.is_empty() {
            d.offset_ranges.push(i);
        } else if !def.repeating {
            d.exact.insert(def.entry.tag, i);
        } else if def.group.exact() {
            d.group_ranges.entry(def.group.lo).or_default().push(i);
        } else {
            d.wide_ranges.push(i);
        }
        if def.creator.is_empty() {
            let keyword = def.entry.keyword.to_lowercase();
            let replace = match d.keywords.get(&keyword) {
                None => true,
                Some(&prev) => {
                    def.repeating == d.entries[prev].repeating || d.entries[prev].repeating
                }
            };
            if replace {
                d.keywords.insert(keyword, i);
            }
        }
    }
    let entries = &d.entries;
    let order = |list: &mut Vec<usize>| {
        list.sort_by(|&a, &b| {
            let (ea, eb) = (&entries[a], &entries[b]);
            ea.specificity
                .cmp(&eb.specificity)
                .then(eb.line.cmp(&ea.line))
        })
    };
    for list in d.group_ranges.values_mut() {
        order(list);
    }
    order(&mut d.wide_ranges);
    order(&mut d.fixed_ranges);
    order(&mut d.offset_ranges);
    d
}
impl DicDictionary {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn entries(&self) -> &[DicEntry] {
        &self.entries
    }
    pub fn has_private_definitions(&self) -> bool {
        !self.private.is_empty() || !self.fixed_ranges.is_empty() || !self.offset_ranges.is_empty()
    }
    fn lookup_absolute(&self, tag: Tag) -> Option<usize> {
        if let Some(&i) = self.exact.get(&tag) {
            return Some(i);
        }
        let mut best: Option<usize> = None;
        let mut pick = |list: Option<&Vec<usize>>| {
            if let Some(&i) = list
                .into_iter()
                .flatten()
                .find(|&&i| self.entries[i].matches(tag))
                && best.is_none_or(|b| better(&self.entries[i], &self.entries[b]))
            {
                best = Some(i);
            }
        };
        pick(self.group_ranges.get(&tag.group));
        pick(Some(&self.wide_ranges));
        best
    }
    pub fn by_tag(&self, tag: Tag) -> Option<DicTagEntry> {
        let i = self.lookup_absolute(tag)?;
        Some(DicTagEntry {
            tag,
            ..self.entries[i].entry.clone()
        })
    }
    pub fn by_keyword(&self, keyword: &str) -> Option<DicTagEntry> {
        let i = self.keywords.get(&keyword.to_lowercase())?;
        Some(self.entries[*i].entry.clone())
    }
    pub fn vr_spec_by_tag(&self, tag: Tag) -> Option<VrSpec> {
        let i = self.lookup_absolute(tag)?;
        let spec = &self.entries[i].vr_spec;
        (!spec.values.is_empty()).then(|| spec.clone())
    }
    pub fn by_private(&self, creator: &str, group: u16, offset: u8) -> Option<DicTagEntry> {
        let creator = private_creator_id(creator).ok()?;
        let i = self
            .private
            .get(&PrivateKey {
                creator: creator.to_string(),
                fixed: false,
                group,
                element: u16::from(offset),
            })
            .copied()
            .or_else(|| {
                self.first_private(&self.offset_ranges, creator, group, u16::from(offset))
            })?;
        Some(DicTagEntry {
            tag: Tag::new(group, u16::from(offset)),
            ..self.entries[i].entry.clone()
        })
    }
    pub fn by_private_tag(&self, creator: &str, tag: Tag) -> Option<DicTagEntry> {
        if !is_private_data_tag(tag) {
            return None;
        }
        let canonical = private_creator_id(creator).ok()?;
        let found = self
            .private
            .get(&PrivateKey {
                creator: canonical.to_string(),
                fixed: true,
                group: tag.group,
                element: tag.element,
            })
            .copied()
            .or_else(|| self.first_private(&self.fixed_ranges, canonical, tag.group, tag.element));
        match found {
            Some(i) => Some(DicTagEntry {
                tag,
                ..self.entries[i].entry.clone()
            }),
            None => self
                .by_private(canonical, tag.group, tag.element as u8)
                .map(|e| DicTagEntry { tag, ..e }),
        }
    }
    fn first_private(
        &self,
        list: &[usize],
        creator: &str,
        group: u16,
        element: u16,
    ) -> Option<usize> {
        list.iter().copied().find(|&i| {
            let def = &self.entries[i];
            def.creator == creator && def.group.matches(group) && def.element.matches(element)
        })
    }
}
fn err(field: &'static str, kind: DicErrorKind, reason: String) -> DicLineError {
    DicLineError {
        line: 0,
        field,
        reason,
        kind,
    }
}
fn syntax(field: &'static str, reason: String) -> DicLineError {
    err(field, DicErrorKind::Syntax, reason)
}
fn dq(s: &str) -> String {
    if s.len() > 64 {
        quote(&format!("{}...", &s[..64]))
    } else {
        quote(s)
    }
}
fn parse_line(line: &[u8]) -> Result<DicEntry, DicLineError> {
    if let Some(i) = line
        .iter()
        .position(|&c| (c < 0x20 && c != b'\t') || c > 0x7e)
    {
        return Err(syntax(
            "line",
            format!(
                "byte 0x{:02X} at column {} is not printable ASCII",
                line[i],
                i + 1
            ),
        ));
    }
    let line = std::str::from_utf8(line).unwrap_or_default();
    let fields: Vec<&str> = line
        .split('\t')
        .map(|f| f.trim_matches(' '))
        .filter(|f| !f.is_empty())
        .collect();
    if fields.len() < 3 || fields.len() > 5 {
        return Err(syntax(
            "line",
            format!(
                "{} tab-separated fields, want Tag, VR, Name[, VM[, Version]]",
                fields.len()
            ),
        ));
    }
    let mut def = DicEntry {
        tag_field: fields[0].to_string(),
        vr_token: fields[1].to_string(),
        version: "DICOM".into(),
        ..DicEntry::default()
    };
    parse_tag(&mut def)?;
    let (vr, spec) = parse_vr(fields[1], &def)?;
    let mut keyword = fields[2];
    let mut retired = false;
    if let Some(rest) = keyword.strip_prefix("RETIRED_")
        && !rest.is_empty()
    {
        keyword = rest;
        retired = true;
    }
    let vm = fields.get(3).copied().unwrap_or("1");
    (def.vm_min, def.vm_max, def.vm_step) = parse_vm(vm)?;
    if let Some(version) = fields.get(4) {
        def.version = version.to_string();
    }
    if def.version.to_lowercase().contains("retired") {
        retired = true;
    }
    def.vr_spec = spec;
    def.entry.vr = vr;
    def.entry.keyword = keyword.to_string();
    def.entry.name = keyword_to_name(keyword);
    def.entry.vm = vm.to_string();
    def.entry.retired = retired;
    Ok(def)
}
fn parse_tag(def: &mut DicEntry) -> Result<(), DicLineError> {
    let s = def.tag_field.clone();
    let b = s.as_bytes();
    if b.len() < 2 || b[0] != b'(' || b[b.len() - 1] != b')' {
        return Err(syntax(
            "tag",
            format!("{} is not enclosed in parentheses", dq(&s)),
        ));
    }
    let body = &s[1..s.len() - 1];
    let Some(comma) = body.find(',') else {
        return Err(syntax("tag", format!("{} has no element part", dq(&s))));
    };
    let group_text = &body[..comma];
    let mut rest = body[comma + 1..].trim_start_matches(' ');
    let mut creator_text = "";
    let mut private = false;
    if let Some(after) = rest.strip_prefix('"') {
        let Some(end) = after.find('"') else {
            return Err(syntax(
                "tag",
                format!("{} has an unterminated private creator", dq(&s)),
            ));
        };
        creator_text = &after[..end];
        private = true;
        rest = after[end + 1..].trim_start_matches(' ');
        let Some(after_comma) = rest.strip_prefix(',') else {
            return Err(syntax(
                "tag",
                format!("{} has no element after the private creator", dq(&s)),
            ));
        };
        rest = after_comma;
    }
    if rest.contains(['(', ')', ',', '"']) || group_text.contains(['(', ')', '"']) {
        return Err(syntax("tag", format!("{} is malformed", dq(&s))));
    }
    let (group, g_width) = parse_part(group_text, true)
        .map_err(|reason| syntax("tag", format!("group of {}: {reason}", dq(&s))))?;
    if g_width != 4 {
        return Err(syntax(
            "tag",
            format!("group of {} must have 4 hex digits", dq(&s)),
        ));
    }
    let (element, e_width) = parse_part(rest, false)
        .map_err(|reason| syntax("tag", format!("element of {}: {reason}", dq(&s))))?;
    def.group = group;
    def.element = element;
    def.repeating = !group.exact() || !element.exact();
    if !private {
        if e_width != 4 {
            return Err(syntax(
                "tag",
                format!("element of {} must have 4 hex digits", dq(&s)),
            ));
        }
    } else {
        let creator = private_creator_id(creator_text).map_err(|_| {
            syntax(
                "tag",
                format!(
                    "private creator {} is not a valid LO identifier",
                    dq(creator_text)
                ),
            )
        })?;
        def.creator = creator.to_string();
        let first = group.first();
        if first.is_none()
            || (!group.exact() && group.parity != Parity::Odd)
            || !first.is_some_and(is_private_group)
        {
            return Err(syntax(
                "tag",
                format!(
                    "{}: private definitions need odd groups from 0009 (use gggg-o-gggg for ranges)",
                    dq(&s)
                ),
            ));
        }
        if element.hi <= 0x00ff {
        } else if element.lo >= 0x1000 {
            def.fixed = true;
        } else {
            return Err(syntax(
                "tag",
                format!(
                    "{}: private element must be an offset 00-FF or a data element 1000-FFFF",
                    dq(&s)
                ),
            ));
        }
    }
    let (Some(g_first), Some(e_first)) = (group.first(), element.first()) else {
        return Err(syntax("tag", format!("{} matches no tag", dq(&s))));
    };
    def.specificity = group.count() * element.count();
    def.entry.tag = Tag::new(g_first, e_first);
    Ok(())
}
fn hex_digit(c: u8) -> Option<u16> {
    (c as char).to_digit(16).map(|d| d as u16)
}
fn dic_hex(s: &str) -> Option<(u16, usize)> {
    if s.len() != 2 && s.len() != 4 {
        return None;
    }
    let mut v = 0u16;
    for c in s.bytes() {
        v = v << 4 | hex_digit(c)?;
    }
    Some((v, s.len()))
}
fn parse_part(text: &str, is_group: bool) -> Result<(Part, usize), String> {
    let text = text.trim_matches(' ');
    let parts: Vec<&str> = text.split('-').collect();
    match parts.len() {
        1 => {
            if text.contains(['x', 'X']) {
                if text.len() != 4 {
                    return Err(format!("pattern {} must have 4 digits", dq(text)));
                }
                let mut p = Part::default();
                for (i, c) in text.bytes().enumerate() {
                    let shift = 12 - 4 * i;
                    if c == b'x' || c == b'X' {
                        p.hi |= 0xf << shift;
                        continue;
                    }
                    let Some(n) = hex_digit(c) else {
                        return Err(format!("{} is not hexadecimal", dq(text)));
                    };
                    p.lo |= n << shift;
                    p.hi |= n << shift;
                    p.mask |= 0xf << shift;
                }
                if is_group {
                    p.parity = Parity::Even;
                }
                return Ok((p, 4));
            }
            let Some((v, width)) = dic_hex(text) else {
                return Err(format!("{} is not 2 or 4 hex digits", dq(text)));
            };
            Ok((
                Part {
                    lo: v,
                    hi: v,
                    ..Part::default()
                },
                width,
            ))
        }
        2 | 3 => {
            let mut p = Part {
                parity: Parity::Even,
                ..Part::default()
            };
            if parts.len() == 3 {
                match parts[1].to_lowercase().as_str() {
                    "o" => p.parity = Parity::Odd,
                    "e" => {}
                    "u" => p.parity = Parity::Any,
                    _ => {
                        return Err(format!(
                            "unknown range restriction {} (want o, e or u)",
                            dq(parts[1])
                        ));
                    }
                }
            }
            let lo = dic_hex(parts[0].trim_matches(' '));
            let hi = dic_hex(parts[parts.len() - 1].trim_matches(' '));
            let (Some((lo, lw)), Some((hi, hw))) = (lo, hi) else {
                return Err(format!(
                    "range {} needs two hex bounds of equal width",
                    dq(text)
                ));
            };
            if lw != hw {
                return Err(format!(
                    "range {} needs two hex bounds of equal width",
                    dq(text)
                ));
            }
            if lo > hi {
                return Err(format!("range {} is descending", dq(text)));
            }
            p.lo = lo;
            p.hi = hi;
            Ok((p, lw))
        }
        _ => Err(format!("{} is malformed", dq(text))),
    }
}
fn parse_vr(token: &str, def: &DicEntry) -> Result<(Option<Vr>, VrSpec), DicLineError> {
    let contextual = match token {
        "xs" => Some((ContextualVr::Xs, Vr::US)),
        "ox" => Some((ContextualVr::Ox, Vr::OW)),
        "px" => Some((ContextualVr::Px, Vr::OW)),
        "lt" => Some((ContextualVr::Lt, Vr::OW)),
        "up" => Some((ContextualVr::Up, Vr::UL)),
        _ => None,
    };
    if let Some((context, fallback)) = contextual {
        return Ok((Some(fallback), VrSpec::contextual(context)));
    }
    if token == "na" {
        if !def.creator.is_empty() || def.group.lo != 0xfffe || def.group.hi != 0xfffe {
            return Err(err(
                "vr",
                DicErrorKind::Vr,
                "\"na\" is only valid for group FFFE items and delimiters".into(),
            ));
        }
        return Ok((None, VrSpec::default()));
    }
    let Some(vr) = Vr::parse(token.as_bytes()) else {
        return Err(err(
            "vr",
            DicErrorKind::Vr,
            format!(
                "{} is not a standard VR or one of xs, ox, px, lt, up, na",
                dq(token)
            ),
        ));
    };
    Ok((
        Some(vr),
        VrSpec {
            values: vec![vr],
            context: None,
        },
    ))
}
fn parse_vm(vm: &str) -> Result<(i64, i64, i64), DicLineError> {
    let bad = || {
        syntax(
            "vm",
            format!("{} is not a VM such as 1, 1-3, 1-n or 2-2n", dq(vm)),
        )
    };
    let num = |s: &str| -> Option<i64> {
        if s.is_empty() || s.len() > 6 || !s.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        s.parse::<i64>().ok().filter(|n| *n > 0)
    };
    let unbounded = |s: &str| -> Option<i64> {
        let s = s.strip_suffix(['n', 'N'])?;
        if s.is_empty() {
            return Some(1);
        }
        num(s)
    };
    let Some((lo, hi)) = vm.split_once('-') else {
        if let Some(n) = num(vm) {
            return Ok((n, n, 1));
        }
        if let Some(n) = unbounded(vm) {
            return Ok((n, -1, 1));
        }
        return Err(bad());
    };
    let min = num(lo).ok_or_else(bad)?;
    if let Some(step) = unbounded(hi) {
        return Ok((min, -1, step));
    }
    match num(hi) {
        Some(max) if max >= min => Ok((min, max, 1)),
        _ => Err(bad()),
    }
}
fn keyword_to_name(keyword: &str) -> String {
    let b = keyword.as_bytes();
    if !b.iter().all(|c| c.is_ascii_alphanumeric()) {
        return keyword.to_string();
    }
    let mut out = String::with_capacity(b.len() + 8);
    for (i, &c) in b.iter().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            let prev = b[i - 1];
            if !prev.is_ascii_uppercase() || b.get(i + 1).is_some_and(|n| n.is_ascii_lowercase()) {
                out.push(' ');
            }
        }
        out.push(c as char);
    }
    out
}
