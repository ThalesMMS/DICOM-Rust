use super::Metadata;
use std::borrow::Cow;
use std::fmt;
use std::io::Read;
const LAYOUT: &str = "dicom: invalid encapsulated frame layout";
const FRAME_COUNT: &str = "dicom: encapsulated frame count mismatch";
const RESOURCE_LIMIT: &str = "dicom: encapsulated frame resource limit exceeded";
const SOURCE: &str = "dicom: encapsulated fragment source failed";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Layout,
    FrameCount,
    ResourceLimit,
    Source,
    Canceled,
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kinds: [Option<ErrorKind>; 2],
    text: String,
}
impl Error {
    fn bare(kind: ErrorKind) -> Error {
        let text = match kind {
            ErrorKind::Layout => LAYOUT,
            ErrorKind::FrameCount => FRAME_COUNT,
            ErrorKind::ResourceLimit => RESOURCE_LIMIT,
            ErrorKind::Source => SOURCE,
            ErrorKind::Canceled => "context canceled",
            ErrorKind::Other => "",
        };
        Error {
            kinds: [Some(kind), None],
            text: text.to_owned(),
        }
    }
    fn layout() -> Error {
        Error::bare(ErrorKind::Layout)
    }
    fn frame_count() -> Error {
        Error::bare(ErrorKind::FrameCount)
    }
    fn resource() -> Error {
        Error::bare(ErrorKind::ResourceLimit)
    }
    fn layout_detail(detail: impl fmt::Display) -> Error {
        Error {
            kinds: [Some(ErrorKind::Layout), None],
            text: format!("{LAYOUT}: {detail}"),
        }
    }
    fn layout_frame_count(detail: Option<&str>) -> Error {
        let text = match detail {
            Some(d) => format!("{LAYOUT}: {FRAME_COUNT}: {d}"),
            None => format!("{LAYOUT}: {FRAME_COUNT}"),
        };
        Error {
            kinds: [Some(ErrorKind::Layout), Some(ErrorKind::FrameCount)],
            text,
        }
    }
    fn source_detail(detail: impl fmt::Display) -> Error {
        Error {
            kinds: [Some(ErrorKind::Source), None],
            text: format!("{SOURCE}: {detail}"),
        }
    }
    pub fn other(text: impl Into<String>) -> Error {
        Error {
            kinds: [Some(ErrorKind::Other), None],
            text: text.into(),
        }
    }
    pub fn kind(&self) -> ErrorKind {
        self.kinds[0].unwrap_or(ErrorKind::Other)
    }
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kinds.contains(&Some(kind))
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format(pub u8);
impl Format {
    pub const JPEG: Format = Format(1);
    pub const JPEGLS: Format = Format(2);
    pub const JPEG2000: Format = Format(3);
    fn valid(self) -> bool {
        self == Format::JPEG || self == Format::JPEGLS || self == Format::JPEG2000
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_frames: i64,
    pub max_fragments: i64,
    pub max_fragments_per_frame: i64,
    pub max_bytes: u64,
    pub max_frame_bytes: u64,
}
impl Limits {
    pub fn defaults() -> Limits {
        Limits {
            max_frames: 100_000,
            max_fragments: 100_000,
            max_fragments_per_frame: 100_000,
            max_bytes: 512 << 20,
            max_frame_bytes: 512 << 20,
        }
    }
    fn normalized(self) -> Result<Limits, Error> {
        if self.max_frames < 0 || self.max_fragments < 0 || self.max_fragments_per_frame < 0 {
            return Err(Error::resource());
        }
        let d = Limits::defaults();
        let mut l = self;
        if l.max_frames == 0 {
            l.max_frames = d.max_frames;
        }
        if l.max_fragments == 0 {
            l.max_fragments = d.max_fragments;
        }
        if l.max_fragments_per_frame == 0 {
            l.max_fragments_per_frame = d.max_fragments_per_frame;
        }
        if l.max_bytes == 0 {
            l.max_bytes = d.max_bytes;
        }
        if l.max_frame_bytes == 0 {
            l.max_frame_bytes = d.max_frame_bytes;
        }
        Ok(l)
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tables {
    pub basic: Vec<u8>,
    pub extended: Vec<u64>,
    pub lengths: Vec<u64>,
    pub extended_present: bool,
    pub lengths_present: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableValue<'a> {
    Raw(&'a [u8]),
    Uint64(&'a [u64]),
    Unsupported(&'a str),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableAttribute<'a> {
    pub vr_ov: bool,
    pub value: TableValue<'a>,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct EotAttributes<'a> {
    pub extended: Option<TableAttribute<'a>>,
    pub lengths: Option<TableAttribute<'a>>,
}
const TAG_EOT: &str = "(7FE0,0001)";
const TAG_EOT_LENGTHS: &str = "(7FE0,0002)";
const MAX_INT: u64 = i64::MAX as u64;
pub fn read_tables(
    basic: &[u8],
    extended: Option<TableAttribute<'_>>,
    lengths: Option<TableAttribute<'_>>,
    frames: i64,
) -> Result<Tables, Error> {
    let (extended, extended_present) = uint64_offset_table(extended, TAG_EOT, frames)?;
    let (lengths, lengths_present) = uint64_offset_table(lengths, TAG_EOT_LENGTHS, frames)?;
    Ok(Tables {
        basic: basic.to_vec(),
        extended,
        lengths,
        extended_present,
        lengths_present,
    })
}
pub fn from_fragments<'a>(
    basic: &[u8],
    fragments: &'a [&'a [u8]],
    eot: EotAttributes<'_>,
    frames: i64,
    format: Format,
    limits: Limits,
    recovery: Recovery,
) -> Result<Plan<'a>, Error> {
    let EotAttributes { extended, lengths } = eot;
    let l = limits.normalized()?;
    if frames <= 0 {
        return Err(Error::layout());
    }
    if frames > l.max_frames || fragments.len() as i64 > l.max_fragments {
        return Err(Error::resource());
    }
    let tables = read_tables(basic, extended, lengths, frames)?;
    Plan::new(
        Box::new(Memory::new(fragments)),
        &tables,
        frames,
        format,
        l,
        recovery,
    )
}
fn uint64_offset_table(
    attribute: Option<TableAttribute<'_>>,
    tag: &str,
    expected: i64,
) -> Result<(Vec<u64>, bool), Error> {
    let Some(attribute) = attribute else {
        return Ok((Vec::new(), false));
    };
    if !attribute.vr_ov {
        return Err(Error::layout_detail("EOT attribute must use OV"));
    }
    match attribute.value {
        TableValue::Raw(value) => {
            if expected > i64::MAX / 8 || value.len() as i64 != expected.wrapping_mul(8) {
                return Err(Error::layout_detail(format!(
                    "{tag} length={} frames={expected}",
                    value.len()
                )));
            }
            let entries = value
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| u64::from_le_bytes(*c))
                .collect();
            Ok((entries, true))
        }
        TableValue::Uint64(value) => {
            if value.len() as i64 != expected {
                return Err(Error::layout_detail(format!(
                    "{tag} entries={} frames={expected}",
                    value.len()
                )));
            }
            Ok((value.to_vec(), true))
        }
        TableValue::Unsupported(type_name) => Err(Error::layout_detail(format!(
            "{tag} has unsupported value type {type_name}"
        ))),
    }
}
fn next_fragment_item_offset(current: u64, fragment_length: u64) -> Result<u64, Error> {
    fragment_length
        .checked_add(fragment_length & 1)
        .and_then(|padded| current.checked_add(8)?.checked_add(padded))
        .ok_or_else(|| Error {
            kinds: [Some(ErrorKind::ResourceLimit), None],
            text: format!("{RESOURCE_LIMIT}: fragment item offset overflow"),
        })
}
fn basic_frame_starts(
    table: &[u8],
    fragment_starts: &[u64],
    frames: i64,
) -> Result<Vec<usize>, Error> {
    if frames > i64::MAX / 4 || table.len() as i64 != frames.wrapping_mul(4) {
        return Err(Error::layout_detail(format!(
            "Basic Offset Table length={} frames={frames}",
            table.len()
        )));
    }
    let offsets: Vec<u64> = table
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u64::from(u32::from_le_bytes(*c)))
        .collect();
    map_frame_offsets(&offsets, fragment_starts)
}
fn map_frame_offsets(offsets: &[u64], fragment_starts: &[u64]) -> Result<Vec<usize>, Error> {
    validate_offset_order(offsets)?;
    offsets
        .iter()
        .map(|&offset| {
            let index = fragment_starts.partition_point(|&s| s < offset);
            if fragment_starts.get(index) != Some(&offset) {
                return Err(Error::layout_detail(format!(
                    "frame offset {offset} is not aligned to a fragment"
                )));
            }
            Ok(index)
        })
        .collect()
}
fn validate_offset_order(offsets: &[u64]) -> Result<(), Error> {
    if offsets.first() != Some(&0) {
        return Err(Error::layout_detail("first frame offset is not zero"));
    }
    for (i, pair) in (1usize..).zip(offsets.windows(2)) {
        if let &[previous, current] = pair
            && current <= previous
        {
            return Err(Error::layout_detail(format!(
                "frame offsets are not strictly increasing at index {i}"
            )));
        }
    }
    Ok(())
}
pub trait Source {
    fn fragment_count(&self) -> usize;
    fn fragment_size(&self, index: usize) -> Result<u64, Error>;
    fn read_fragment_at(&self, index: usize, buf: &mut [u8], offset: u64) -> Result<usize, Error>;
    fn borrow_fragment(&self, _index: usize) -> Option<&[u8]> {
        None
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Memory<'a> {
    fragments: &'a [&'a [u8]],
}
impl<'a> Memory<'a> {
    pub fn new(fragments: &'a [&'a [u8]]) -> Memory<'a> {
        Memory { fragments }
    }
}
impl Source for Memory<'_> {
    fn fragment_count(&self) -> usize {
        self.fragments.len()
    }
    fn fragment_size(&self, index: usize) -> Result<u64, Error> {
        match self.fragments.get(index) {
            Some(f) => Ok(f.len() as u64),
            None => Err(Error::layout_detail("fragment index")),
        }
    }
    fn read_fragment_at(&self, index: usize, buf: &mut [u8], offset: u64) -> Result<usize, Error> {
        let data = self.fragments.get(index).ok_or_else(Error::layout)?;
        Ok(copy_from(data, buf, offset))
    }
    fn borrow_fragment(&self, index: usize) -> Option<&[u8]> {
        self.fragments.get(index).copied()
    }
}
pub trait ReadAt {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize>;
}
impl ReadAt for [u8] {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        Ok(copy_from(self, buf, offset))
    }
}
fn copy_from(data: &[u8], buf: &mut [u8], offset: u64) -> usize {
    let rest = usize::try_from(offset)
        .ok()
        .and_then(|o| data.get(o..))
        .unwrap_or_default();
    let n = rest.len().min(buf.len());
    if let (Some(dst), Some(src)) = (buf.get_mut(..n), rest.get(..n)) {
        dst.copy_from_slice(src);
    }
    n
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemRange {
    pub offset: i64,
    pub length: u64,
}
pub struct ReaderAtSource<'a, R: ReadAt + ?Sized> {
    reader: &'a R,
    items: &'a [ItemRange],
}
pub fn reader_at<'a, R: ReadAt + ?Sized>(
    reader: &'a R,
    items: &'a [ItemRange],
) -> Result<ReaderAtSource<'a, R>, Error> {
    let mut previous_end = 0i64;
    for (i, item) in items.iter().enumerate() {
        if item.offset < 0
            || item.length < 2
            || item.length & 1 != 0
            || item.length > u64::from(u32::MAX - 1)
            || item.offset as u64 > (i64::MAX as u64).saturating_sub(item.length)
        {
            return Err(Error::layout_detail(format!("invalid Item range {i}")));
        }
        if i > 0 && item.offset < previous_end {
            return Err(Error::layout_detail("overlapping Item ranges"));
        }
        previous_end = item.offset.saturating_add(item.length as i64);
    }
    Ok(ReaderAtSource { reader, items })
}
impl<R: ReadAt + ?Sized> Source for ReaderAtSource<'_, R> {
    fn fragment_count(&self) -> usize {
        self.items.len()
    }
    fn fragment_size(&self, index: usize) -> Result<u64, Error> {
        self.items
            .get(index)
            .map(|i| i.length)
            .ok_or_else(Error::layout)
    }
    fn read_fragment_at(&self, index: usize, buf: &mut [u8], offset: u64) -> Result<usize, Error> {
        let item = self.items.get(index).ok_or_else(Error::layout)?;
        if offset >= item.length {
            return Ok(0);
        }
        let available = item.length.saturating_sub(offset);
        let want = (buf.len() as u64).min(available) as usize;
        let mut done = 0usize;
        while let Some(dst) = buf.get_mut(done..want)
            && !dst.is_empty()
        {
            let at = (item.offset as u64)
                .saturating_add(offset)
                .saturating_add(done as u64);
            let asked = dst.len();
            match self.reader.read_at(dst, at) {
                Ok(0) => break,
                Ok(n) => done = done.saturating_add(n.min(asked)),
                Err(e) => return Err(Error::other(e.to_string())),
            }
        }
        Ok(done)
    }
}
struct PrefixSource<'a> {
    inner: Box<dyn Source + 'a>,
    count: usize,
}
impl Source for PrefixSource<'_> {
    fn fragment_count(&self) -> usize {
        self.count
    }
    fn fragment_size(&self, index: usize) -> Result<u64, Error> {
        self.inner.fragment_size(index)
    }
    fn read_fragment_at(&self, index: usize, buf: &mut [u8], offset: u64) -> Result<usize, Error> {
        self.inner.read_fragment_at(index, buf, offset)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recovery {
    pub ignore_inconsistent_basic_offset_table: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub basic_offset_table_ignored: bool,
    pub basic_offset_table_entries: usize,
    pub frames: i64,
    pub reason: String,
}
#[derive(Debug, Clone, Copy)]
struct FrameRange {
    start: usize,
    end: usize,
    length: u64,
}
pub struct Plan<'a> {
    source: Box<dyn Source + 'a>,
    sizes: Vec<u64>,
    frames: Vec<FrameRange>,
    input_bytes: u64,
    max_frame_bytes: u64,
    recovery: RecoveryReport,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View<'p> {
    pub data: Cow<'p, [u8]>,
    pub borrowed: bool,
    pub padded: bool,
    pub pad_byte: u8,
}
impl<'a> Plan<'a> {
    fn empty(source: Box<dyn Source + 'a>, n: usize) -> Plan<'a> {
        Plan {
            source,
            sizes: vec![0; n],
            frames: Vec::new(),
            input_bytes: 0,
            max_frame_bytes: 0,
            recovery: RecoveryReport::default(),
        }
    }
    pub fn new(
        source: Box<dyn Source + 'a>,
        tables: &Tables,
        frames: i64,
        format: Format,
        limits: Limits,
        recovery: Recovery,
    ) -> Result<Plan<'a>, Error> {
        if !format.valid() {
            return Err(Error::layout_detail("unsupported frame format"));
        }
        let l = limits.normalized()?;
        if frames <= 0 {
            return Err(Error::layout_detail("invalid source or frame count"));
        }
        let n = source.fragment_count();
        if frames > l.max_frames || n as i64 > l.max_fragments {
            return Err(Error::resource());
        }
        if n == 0 {
            return Err(Error::layout_detail("no fragments"));
        }
        if frames > n as i64 {
            return Err(Error::layout_frame_count(None));
        }
        let frames = frames as usize;
        let mut p = Plan::empty(source, n);
        let mut starts = vec![0u64; n];
        let mut item_offset = 0u64;
        for i in 0..n {
            let size = p.source.fragment_size(i).map_err(Error::source_detail)?;
            if size == 0 {
                return Err(Error::layout_detail("empty fragment"));
            }
            if size > MAX_INT || size > l.max_bytes.saturating_sub(p.input_bytes) {
                return Err(Error::resource());
            }
            if let (Some(s), Some(o)) = (p.sizes.get_mut(i), starts.get_mut(i)) {
                *s = size;
                *o = item_offset;
            }
            p.input_bytes = p.input_bytes.saturating_add(size);
            item_offset = next_fragment_item_offset(item_offset, size)?;
        }
        if tables.extended_present != tables.lengths_present {
            return Err(Error::layout_detail("EOT and Lengths must both be present"));
        }
        if !tables.extended_present && (!tables.extended.is_empty() || !tables.lengths.is_empty()) {
            return Err(Error::layout_detail("EOT presence inconsistent"));
        }
        let frame_starts = if tables.extended_present {
            if !tables.basic.is_empty()
                || n != frames
                || tables.extended.len() != frames
                || tables.lengths.len() != frames
            {
                return Err(Error::layout_detail(
                    "EOT requires empty BOT and one Item per frame with matching counts",
                ));
            }
            map_frame_offsets(&tables.extended, &starts)?
        } else if !tables.basic.is_empty() {
            match basic_frame_starts(&tables.basic, &starts, frames as i64) {
                Err(e)
                    if e.is(ErrorKind::Layout)
                        && recovery.ignore_inconsistent_basic_offset_table =>
                {
                    let starts = infer_frames(&p, frames, format, &l)?;
                    p.recovery = RecoveryReport {
                        basic_offset_table_ignored: true,
                        basic_offset_table_entries: tables.basic.len() / 4,
                        frames: frames as i64,
                        reason: e.to_string(),
                    };
                    starts
                }
                other => other?,
            }
        } else if n == frames {
            (0..frames).collect()
        } else {
            infer_frames(&p, frames, format, &l)?
        };
        p.frames = Vec::with_capacity(frames);
        for (i, &start) in frame_starts.iter().enumerate() {
            let next = i.saturating_add(1);
            let end = if next < frames {
                frame_starts.get(next).copied().unwrap_or(n)
            } else {
                n
            };
            if start >= end || end > n {
                return Err(Error::layout());
            }
            if (end.saturating_sub(start)) as i64 > l.max_fragments_per_frame {
                return Err(Error::resource());
            }
            let mut size = 0u64;
            let items = p.sizes.get(start..end).ok_or_else(Error::layout)?;
            let last = items.len().saturating_sub(1);
            for (j, &item) in items.iter().enumerate() {
                if j < last && item & 1 != 0 {
                    return Err(Error::layout_detail("odd non-final fragment"));
                }
                if item > l.max_frame_bytes.saturating_sub(size) {
                    return Err(Error::resource());
                }
                size = size.saturating_add(item);
            }
            if size > MAX_INT {
                return Err(Error::resource());
            }
            if tables.extended_present {
                let length = tables.lengths.get(i).copied().unwrap_or_default();
                if start != i
                    || length == 0
                    || length > size
                    || size.saturating_sub(length) > 1
                    || (size != length && length & 1 == 0)
                {
                    return Err(Error::layout_detail("invalid EOT frame length"));
                }
                if size != length {
                    let mut padding = [0u8; 1];
                    read_exact(p.source.as_ref(), start, &mut padding, length)?;
                    if !valid_frame_pad(format, padding[0], size) {
                        return Err(Error::layout_detail("invalid EOT padding"));
                    }
                }
                size = length;
            }
            p.frames.push(FrameRange {
                start,
                end,
                length: size,
            });
            p.max_frame_bytes = p.max_frame_bytes.max(size);
        }
        Ok(p)
    }
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    pub fn input_bytes(&self) -> u64 {
        self.input_bytes
    }
    pub fn frame_length(&self, index: usize) -> Option<u64> {
        self.frames.get(index).map(|f| f.length)
    }
    pub fn max_frame_bytes(&self) -> u64 {
        self.max_frame_bytes
    }
    pub fn recovery(&self) -> &RecoveryReport {
        &self.recovery
    }
    pub fn frame(&self, index: usize) -> Result<View<'_>, Error> {
        let f = *self.frames.get(index).ok_or_else(Error::layout)?;
        let changed = || Error::source_detail("source size changed");
        if f.end == f.start.saturating_add(1)
            && let Some(data) = self.source.borrow_fragment(f.start)
        {
            if Some(&(data.len() as u64)) != self.sizes.get(f.start) {
                return Err(changed());
            }
            let frame = data.get(..f.length as usize).ok_or_else(changed)?;
            return Ok(padded(Cow::Borrowed(frame), true));
        }
        let mut data = vec![0u8; f.length as usize];
        let mut pos = 0usize;
        let sizes = self.sizes.get(f.start..f.end).ok_or_else(Error::layout)?;
        for (i, &fragment) in (f.start..).zip(sizes) {
            let size = fragment.min(data.len().saturating_sub(pos) as u64);
            let mut offset = 0u64;
            while offset < size {
                let chunk = (64u64 << 10).min(size.saturating_sub(offset));
                let end = pos.saturating_add(chunk as usize);
                let dst = data.get_mut(pos..end).ok_or_else(Error::layout)?;
                read_exact(self.source.as_ref(), i, dst, offset)?;
                pos = end;
                offset = offset.saturating_add(chunk);
            }
        }
        Ok(padded(Cow::Owned(data), false))
    }
}
fn padded(data: Cow<'_, [u8]>, borrowed: bool) -> View<'_> {
    let (pad_byte, padded) = match trailing_pad(&data) {
        Some(b) => (b, true),
        None => (0, false),
    };
    View {
        data,
        borrowed,
        padded,
        pad_byte,
    }
}
fn read_exact(
    source: &dyn Source,
    index: usize,
    data: &mut [u8],
    offset: u64,
) -> Result<(), Error> {
    match source.read_fragment_at(index, data, offset) {
        Ok(n) if n == data.len() => Ok(()),
        Ok(n) if n > data.len() => Err(Error::source_detail("invalid read count")),
        Ok(_) => Err(Error::source_detail(format!(
            "fragment {index}: unexpected EOF"
        ))),
        Err(e) => Err(Error::source_detail(format!("fragment {index}: {e}"))),
    }
}
pub fn complete_prefix<'a>(
    source: Box<dyn Source + 'a>,
    tables: &Tables,
    frames: i64,
    format: Format,
    limits: Limits,
) -> Result<(Plan<'a>, PrefixReport), Error> {
    let l = limits.normalized()?;
    if frames < 1
        || (format != Format::JPEG && format != Format::JPEGLS)
        || tables.extended_present
        || tables.lengths_present
        || !tables.extended.is_empty()
        || !tables.lengths.is_empty()
    {
        return Err(Error::layout());
    }
    let n = source.fragment_count();
    if frames > l.max_frames || n as i64 > l.max_fragments {
        return Err(Error::resource());
    }
    if n < 1 {
        return Err(Error::frame_count());
    }
    let mut p = Plan::empty(source, n);
    let mut item_starts = vec![0u64; n];
    let mut offset = 0u64;
    for i in 0..n {
        let size = p.source.fragment_size(i).map_err(Error::source_detail)?;
        if size == 0 || size & 1 != 0 {
            return Err(Error::layout());
        }
        if size > MAX_INT || size > l.max_bytes.saturating_sub(p.input_bytes) {
            return Err(Error::resource());
        }
        if let (Some(s), Some(o)) = (p.sizes.get_mut(i), item_starts.get_mut(i)) {
            *s = size;
            *o = offset;
        }
        p.input_bytes = p.input_bytes.saturating_add(size);
        offset = next_fragment_item_offset(offset, size)?;
    }
    if !tables.basic.is_empty() {
        basic_frame_starts(&tables.basic, &item_starts, frames)?;
    }
    let (starts, retained, items) = scan_frame_starts(&p, frames as usize, format, &l, true)?;
    if retained < 1 {
        return Err(Error::frame_count());
    }
    let mut basic = tables.basic.clone();
    if !basic.is_empty() {
        let declared = basic_frame_starts(&basic, &item_starts, frames)?;
        if declared.get(..retained) != starts.get(..retained) {
            return Err(Error::layout());
        }
        basic.truncate(retained.saturating_mul(4));
    }
    let report = PrefixReport {
        declared_frames: frames,
        retained_frames: retained,
        retained_fragments: items,
        discarded_fragments: n.saturating_sub(items),
        discarded_bytes: p
            .sizes
            .get(items..)
            .unwrap_or_default()
            .iter()
            .fold(0, |a, &b| a.saturating_add(b)),
    };
    let prefix = PrefixSource {
        inner: p.source,
        count: items,
    };
    let tables = Tables {
        basic,
        ..Tables::default()
    };
    let plan = Plan::new(
        Box::new(prefix),
        &tables,
        retained as i64,
        format,
        l,
        Recovery::default(),
    )?;
    Ok((plan, report))
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrefixReport {
    pub declared_frames: i64,
    pub retained_frames: usize,
    pub retained_fragments: usize,
    pub discarded_fragments: usize,
    pub discarded_bytes: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ScanState {
    #[default]
    Soi,
    SoiCode,
    Marker,
    MarkerCode,
    LengthHigh,
    LengthLow,
    Payload,
    Entropy,
    EntropyCode,
    End,
}
#[derive(Debug, Clone, Default)]
struct MarkerScanner {
    state: ScanState,
    length: usize,
    remaining: usize,
    marker: u8,
    format: Option<Format>,
    resume_entropy: bool,
}
impl MarkerScanner {
    fn new(format: Format) -> MarkerScanner {
        MarkerScanner {
            format: Some(format),
            ..MarkerScanner::default()
        }
    }
    fn jpegls(&self) -> bool {
        self.format == Some(Format::JPEGLS)
    }
    fn feed(&mut self, b: u8) -> Result<bool, Error> {
        match self.state {
            ScanState::Soi => {
                if b != 0xff {
                    return Err(Error::layout_detail("missing SOI"));
                }
                self.state = ScanState::SoiCode;
            }
            ScanState::SoiCode => {
                if b != 0xd8 {
                    return Err(Error::layout_detail("missing SOI"));
                }
                self.state = ScanState::Marker;
            }
            ScanState::Marker => {
                if b != 0xff {
                    return Err(Error::layout_detail("expected marker"));
                }
                self.state = ScanState::MarkerCode;
            }
            ScanState::MarkerCode => {
                if b == 0xff {
                    return Ok(false);
                }
                return self.begin_marker(b, false);
            }
            ScanState::LengthHigh => {
                self.length = usize::from(b) << 8;
                self.state = ScanState::LengthLow;
            }
            ScanState::LengthLow => {
                self.length |= usize::from(b);
                if self.length < 2 {
                    return Err(Error::layout_detail("invalid marker length"));
                }
                self.remaining = self.length.saturating_sub(2);
                self.state = ScanState::Payload;
                if self.remaining == 0 {
                    self.end_segment();
                }
            }
            ScanState::Payload => {
                self.remaining = self.remaining.saturating_sub(1);
                if self.remaining == 0 {
                    self.end_segment();
                }
            }
            ScanState::Entropy => {
                if b == 0xff {
                    self.state = ScanState::EntropyCode;
                }
            }
            ScanState::EntropyCode => {
                if b == 0xff {
                    return Ok(false);
                }
                if b == 0 || self.jpegls() && b < 0x80 {
                    self.state = ScanState::Entropy;
                    return Ok(false);
                }
                return self.begin_marker(b, true);
            }
            ScanState::End => return Err(Error::layout_detail("data after EOI")),
        }
        Ok(false)
    }
    fn begin_marker(&mut self, b: u8, entropy: bool) -> Result<bool, Error> {
        if b == 0 || b == 0xd8 {
            return Err(Error::layout_detail("invalid/nested marker"));
        }
        if b == 0xd9 {
            self.state = ScanState::End;
            return Ok(true);
        }
        if b == 0x01 || (0xd0..=0xd7).contains(&b) {
            if !entropy {
                return Err(Error::layout_detail("standalone marker outside scan"));
            }
            self.state = ScanState::Entropy;
            return Ok(false);
        }
        self.marker = b;
        self.resume_entropy = entropy && b == 0xdc;
        self.state = ScanState::LengthHigh;
        Ok(false)
    }
    fn end_segment(&mut self) {
        self.state = if self.marker == 0xda || self.resume_entropy {
            ScanState::Entropy
        } else {
            ScanState::Marker
        };
    }
    fn scan(&mut self, data: &[u8]) -> Result<(usize, bool), Error> {
        let mut offset = 0usize;
        while let Some(rest) = data.get(offset..)
            && let Some(&b) = rest.first()
        {
            match self.state {
                ScanState::Payload => {
                    let n = self.remaining.min(rest.len());
                    self.remaining = self.remaining.saturating_sub(n);
                    offset = offset.saturating_add(n);
                    if self.remaining == 0 {
                        self.end_segment();
                    }
                    continue;
                }
                ScanState::Entropy => {
                    if b != 0xff {
                        match rest.iter().position(|&b| b == 0xff) {
                            Some(i) => offset = offset.saturating_add(i),
                            None => {
                                offset = data.len();
                                continue;
                            }
                        }
                    }
                    self.state = ScanState::EntropyCode;
                    offset = offset.saturating_add(1);
                    continue;
                }
                ScanState::EntropyCode => {
                    if b == 0xff {
                        offset = offset.saturating_add(1);
                        continue;
                    }
                    if b == 0 || self.jpegls() && b < 0x80 {
                        self.state = ScanState::Entropy;
                        offset = offset.saturating_add(1);
                        continue;
                    }
                }
                _ => {}
            }
            let end = self.feed(b)?;
            offset = offset.saturating_add(1);
            if end {
                return Ok((offset, true));
            }
        }
        Ok((data.len(), false))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum J2kState {
    #[default]
    Soc,
    SocCode,
    Marker,
    MarkerCode,
    LengthHigh,
    LengthLow,
    Segment,
    SotFields,
    TilePart,
    HeaderMarker,
    HeaderMarkerCode,
    HeaderLengthHigh,
    HeaderLengthLow,
    HeaderSegment,
    Bitstream,
    BitstreamCode,
    End,
}
#[derive(Debug, Clone, Default)]
struct J2kScanner {
    state: J2kState,
    length: usize,
    remaining: i64,
    sot: [u8; 8],
    sot_read: usize,
    marker: u8,
}
impl J2kScanner {
    fn feed(&mut self, b: u8) -> Result<bool, Error> {
        use J2kState as S;
        match self.state {
            S::Soc => {
                if b != 0xff {
                    return Err(Error::layout_detail("missing JPEG 2000 SOC"));
                }
                self.state = S::SocCode;
            }
            S::SocCode => {
                if b != 0x4f {
                    return Err(Error::layout_detail("missing JPEG 2000 SOC"));
                }
                self.state = S::Marker;
            }
            S::Marker => {
                if b != 0xff {
                    return Err(Error::layout_detail("expected JPEG 2000 marker"));
                }
                self.state = S::MarkerCode;
            }
            S::MarkerCode => {
                if b == 0xd9 {
                    self.state = S::End;
                    return Ok(true);
                }
                if b != 0x90 && (b < 0x30 || b == 0x4f || b == 0x93) {
                    return Err(Error::layout_detail(format!(
                        "unexpected JPEG 2000 marker FF{b:02X}"
                    )));
                }
                self.marker = b;
                self.state = S::LengthHigh;
            }
            S::LengthHigh => {
                self.length = usize::from(b) << 8;
                self.state = S::LengthLow;
            }
            S::LengthLow => {
                self.length |= usize::from(b);
                if self.length < 2 {
                    return Err(Error::layout_detail("invalid JPEG 2000 marker length"));
                }
                if self.marker == 0x90 {
                    if self.length != 10 {
                        return Err(Error::layout_detail("invalid SOT length"));
                    }
                    self.sot_read = 0;
                    self.state = S::SotFields;
                    return Ok(false);
                }
                self.remaining = (self.length as i64).saturating_sub(2);
                self.state = if self.remaining == 0 {
                    S::Marker
                } else {
                    S::Segment
                };
            }
            S::Segment => {
                self.remaining = self.remaining.saturating_sub(1);
                if self.remaining == 0 {
                    self.state = S::Marker;
                }
            }
            S::SotFields => {
                if let Some(slot) = self.sot.get_mut(self.sot_read) {
                    *slot = b;
                }
                self.sot_read = self.sot_read.saturating_add(1);
                if self.sot_read < self.sot.len() {
                    return Ok(false);
                }
                let [_, _, p0, p1, p2, p3, ..] = self.sot;
                let psot = i64::from(u32::from_be_bytes([p0, p1, p2, p3]));
                if psot == 0 {
                    self.state = S::HeaderMarker;
                    return Ok(false);
                }
                if psot < 14 {
                    return Err(Error::layout_detail("invalid Psot"));
                }
                self.remaining = psot.saturating_sub(12);
                self.state = S::TilePart;
            }
            S::TilePart => {
                self.remaining = self.remaining.saturating_sub(1);
                if self.remaining == 0 {
                    self.state = S::Marker;
                }
            }
            S::HeaderMarker => {
                if b != 0xff {
                    return Err(Error::layout_detail("expected tile-part header marker"));
                }
                self.state = S::HeaderMarkerCode;
            }
            S::HeaderMarkerCode => {
                if b == 0x93 {
                    self.state = S::Bitstream;
                    return Ok(false);
                }
                if b < 0x30 || b == 0x4f || b == 0x90 || b == 0xd9 {
                    return Err(Error::layout_detail(format!(
                        "unexpected tile-part header marker FF{b:02X}"
                    )));
                }
                self.state = S::HeaderLengthHigh;
            }
            S::HeaderLengthHigh => {
                self.length = usize::from(b) << 8;
                self.state = S::HeaderLengthLow;
            }
            S::HeaderLengthLow => {
                self.length |= usize::from(b);
                if self.length < 2 {
                    return Err(Error::layout_detail("invalid JPEG 2000 marker length"));
                }
                self.remaining = (self.length as i64).saturating_sub(2);
                self.state = if self.remaining == 0 {
                    S::HeaderMarker
                } else {
                    S::HeaderSegment
                };
            }
            S::HeaderSegment => {
                self.remaining = self.remaining.saturating_sub(1);
                if self.remaining == 0 {
                    self.state = S::HeaderMarker;
                }
            }
            S::Bitstream => {
                if b == 0xff {
                    self.state = S::BitstreamCode;
                }
            }
            S::BitstreamCode => match b {
                0xd9 => {
                    self.state = S::End;
                    return Ok(true);
                }
                0..=0x8f | 0x91 | 0x92 => self.state = S::Bitstream,
                0xff => {}
                _ => {
                    return Err(Error::layout_detail(format!(
                        "unexpected marker FF{b:02X} in JPEG 2000 packet data"
                    )));
                }
            },
            S::End => return Err(Error::layout_detail("data after EOC")),
        }
        Ok(false)
    }
}
enum FrameScanner {
    Marker(MarkerScanner),
    J2k(J2kScanner),
}
impl FrameScanner {
    fn new(format: Format) -> FrameScanner {
        if format == Format::JPEG2000 {
            FrameScanner::J2k(J2kScanner::default())
        } else {
            FrameScanner::Marker(MarkerScanner::new(format))
        }
    }
    fn scan(&mut self, data: &[u8]) -> Result<(usize, bool), Error> {
        match self {
            FrameScanner::Marker(s) => s.scan(data),
            FrameScanner::J2k(s) => {
                for (consumed, &b) in (1usize..).zip(data) {
                    if s.feed(b)? {
                        return Ok((consumed, true));
                    }
                }
                Ok((data.len(), false))
            }
        }
    }
}
fn valid_frame_pad(format: Format, b: u8, length: u64) -> bool {
    length.is_multiple_of(2) || format != Format::JPEG2000 && b == 0
}
pub fn trailing_pad(data: &[u8]) -> Option<u8> {
    match data {
        [.., 0xff, 0xd9, pad] => Some(*pad),
        _ => None,
    }
}
pub fn validate_frame(data: &[u8], format: Format) -> Result<(), Error> {
    if !format.valid() {
        return Err(Error::layout());
    }
    let (consumed, end) = FrameScanner::new(format).scan(data)?;
    if !end {
        return Err(Error::layout_detail("missing EOI"));
    }
    let remaining = data.get(consumed..).unwrap_or_default();
    if remaining.len() > 1
        || matches!(remaining, [pad] if !valid_frame_pad(format, *pad, data.len() as u64))
    {
        return Err(Error::layout_detail("trailing bytes after EOI"));
    }
    Ok(())
}
pub fn jpeg_scan_selection_values(data: &[u8]) -> Result<Vec<i64>, Error> {
    let mut s = MarkerScanner::new(Format::JPEG);
    let mut values = Vec::new();
    let invalid = || Error::layout_detail("invalid SOS");
    for (i, &b) in data.iter().enumerate() {
        if s.state == ScanState::LengthLow && s.marker == 0xda && (s.length | usize::from(b)) < 8 {
            return Err(invalid());
        }
        if s.state == ScanState::Payload
            && s.marker == 0xda
            && s.remaining == s.length.saturating_sub(2)
        {
            if s.remaining < 6 {
                return Err(invalid());
            }
            let payload = data
                .get(i..i.saturating_add(s.remaining))
                .ok_or_else(invalid)?;
            let count = usize::from(payload.first().copied().unwrap_or_default());
            if count < 1 || Some(payload.len()) != count.checked_mul(2).map(|c| c.saturating_add(4))
            {
                return Err(invalid());
            }
            let ss = payload.get(count.saturating_mul(2).saturating_add(1));
            values.push(i64::from(ss.copied().unwrap_or_default()));
        }
        if s.feed(b)? {
            let remaining = data.get(i.saturating_add(1)..).unwrap_or_default();
            if remaining.len() > 1
                || matches!(remaining, [pad] if !valid_frame_pad(Format::JPEG, *pad, data.len() as u64))
                || values.is_empty()
            {
                return Err(Error::layout_detail("missing scan or trailing bytes"));
            }
            return Ok(values);
        }
    }
    Err(Error::layout_detail("missing EOI"))
}
fn infer_frames(
    p: &Plan<'_>,
    frames: usize,
    format: Format,
    limits: &Limits,
) -> Result<Vec<usize>, Error> {
    scan_frame_starts(p, frames, format, limits, false).map(|(starts, _, _)| starts)
}
fn scan_frame_starts(
    p: &Plan<'_>,
    frames: usize,
    format: Format,
    limits: &Limits,
    prefix: bool,
) -> Result<(Vec<usize>, usize, usize), Error> {
    let mut starts = Vec::with_capacity(frames.min(p.sizes.len()).saturating_add(1));
    starts.push(0usize);
    let mut scanner = FrameScanner::new(format);
    let mut buffer = vec![0u8; 32 << 10];
    let mut completed = 0usize;
    let mut last_complete = 0usize;
    let mut frame_bytes = 0u64;
    let n = p.sizes.len();
    for (i, &size) in p.sizes.iter().enumerate() {
        let current = *starts.last().expect("nonempty");
        if (i.saturating_sub(current).saturating_add(1)) as i64 > limits.max_fragments_per_frame
            || size > limits.max_frame_bytes.saturating_sub(frame_bytes)
        {
            return Err(Error::resource());
        }
        frame_bytes = frame_bytes.saturating_add(size);
        let mut end = false;
        let mut eoi_offset = 0u64;
        let mut offset = 0u64;
        while offset < size {
            let chunk = (buffer.len() as u64).min(size.saturating_sub(offset));
            let data = buffer.get_mut(..chunk as usize).unwrap_or_default();
            read_exact(p.source.as_ref(), i, data, offset)?;
            let mut rest: &[u8] = data;
            if !end {
                let (consumed, complete) = scanner.scan(rest)?;
                end = complete;
                if end {
                    eoi_offset = offset.saturating_add(consumed as u64).saturating_sub(1);
                }
                rest = rest.get(consumed..).unwrap_or_default();
            }
            let next = offset.saturating_add(chunk);
            if end
                && !rest.is_empty()
                && (next != size
                    || next.saturating_sub(1) != eoi_offset.saturating_add(1)
                    || !matches!(rest, [pad] if valid_frame_pad(format, *pad, size)))
            {
                return Err(Error::layout_detail(
                    "frame ends inside an Item or invalid padding",
                ));
            }
            offset = next;
        }
        if !end {
            if size & 1 != 0 {
                return Err(Error::layout_detail("odd non-final fragment"));
            }
            continue;
        }
        completed = completed.saturating_add(1);
        let next = i.saturating_add(1);
        last_complete = next;
        if completed > frames || !prefix && completed == frames && next < n {
            return Err(Error::layout_frame_count(None));
        }
        if next < n {
            starts.push(next);
            scanner = FrameScanner::new(format);
            frame_bytes = 0;
        }
    }
    if prefix && completed > 0 {
        starts.truncate(completed);
        return Ok((starts, completed, last_complete));
    }
    if completed != frames || starts.len() != frames {
        return Err(Error::layout_frame_count(Some(
            "incomplete codestream boundaries",
        )));
    }
    Ok((starts, completed, last_complete))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct J2kTilePart {
    pub tile: usize,
    pub part: usize,
    pub parts: usize,
    pub start: usize,
    pub header_end: usize,
    pub end: usize,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct J2kLayout {
    pub main_header_end: usize,
    pub tile_parts: Vec<J2kTilePart>,
    pub eoc: usize,
}
pub fn parse_j2k_layout(data: &[u8]) -> Result<J2kLayout, Error> {
    validate_frame(data, Format::JPEG2000)?;
    let truncated = || Error::layout_detail("truncated JPEG 2000 marker segment");
    let byte = |i: usize| data.get(i).copied().ok_or_else(truncated);
    let marker = |at: usize| byte(at.saturating_add(1));
    let segment = |at: usize| -> Result<usize, Error> {
        let Some(&[_, _, h, l]) = data.get(at..at.saturating_add(4)) else {
            return Err(truncated());
        };
        let length = usize::from(u16::from_be_bytes([h, l]));
        let next = at.saturating_add(2).saturating_add(length);
        if length < 2 || next.saturating_add(2) > data.len() {
            return Err(Error::layout_detail("invalid JPEG 2000 marker length"));
        }
        Ok(next)
    };
    let mut layout = J2kLayout::default();
    let mut at = 2usize;
    while marker(at)? != 0x90 {
        if marker(at)? == 0xd9 {
            return Err(Error::layout_detail(
                "JPEG 2000 codestream has no tile-part",
            ));
        }
        at = segment(at)?;
    }
    layout.main_header_end = at;
    let mut eoc = data.len().saturating_sub(2);
    if !matches!(data.get(eoc..), Some([0xff, 0xd9, ..])) {
        eoc = eoc.saturating_sub(1);
    }
    while marker(at)? == 0x90 {
        let Some(&[_, _, _, _, t0, t1, p0, p1, p2, p3, part, parts, ..]) =
            data.get(at..at.saturating_add(14))
        else {
            return Err(Error::layout_detail("truncated SOT"));
        };
        let mut part = J2kTilePart {
            tile: usize::from(u16::from_be_bytes([t0, t1])),
            part: usize::from(part),
            parts: usize::from(parts),
            start: at,
            header_end: 0,
            end: 0,
        };
        let psot = u32::from_be_bytes([p0, p1, p2, p3]) as usize;
        let mut header = at.saturating_add(12);
        while marker(header)? != 0x93 {
            header = segment(header)?;
        }
        part.header_end = header.saturating_add(2);
        if psot == 0 {
            part.end = eoc;
        } else {
            if psot as i64 > (eoc as i64).saturating_sub(at as i64) {
                return Err(Error::layout_detail("invalid Psot"));
            }
            part.end = at.saturating_add(psot);
        }
        if part.end < part.header_end {
            return Err(Error::layout_detail("invalid Psot"));
        }
        layout.tile_parts.push(part);
        at = part.end;
    }
    if at != eoc {
        return Err(Error::layout_detail("expected EOC"));
    }
    layout.eoc = at;
    Ok(layout)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub index: usize,
    pub data: Vec<u8>,
    pub metadata: Metadata,
    pub transfer_syntax_uid: String,
}
pub trait EncodedFrameSink {
    fn handle_encoded_frame(&mut self, frame: EncodedFrame) -> Result<(), Error>;
    fn close(&mut self) -> Result<(), Error>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableHeader {
    pub group: u16,
    pub element: u16,
    pub vr_ov: bool,
    pub length: u32,
}
impl TableHeader {
    fn is_eot(&self) -> bool {
        self.group == 0x7FE0 && self.element == 0x0001
    }
    fn is_eot_lengths(&self) -> bool {
        self.group == 0x7FE0 && self.element == 0x0002
    }
}
const STREAM_SYNTAXES_JPEG: &[&str] = &[
    "1.2.840.10008.1.2.4.50",
    "1.2.840.10008.1.2.4.51",
    "1.2.840.10008.1.2.4.57",
    "1.2.840.10008.1.2.4.70",
];
const STREAM_SYNTAXES_JPEGLS: &[&str] = &["1.2.840.10008.1.2.4.80", "1.2.840.10008.1.2.4.81"];
pub struct Stream<S: EncodedFrameSink> {
    sink: S,
    limits: Limits,
    closed: bool,
    eot: Option<Vec<u8>>,
    eot_lengths: Option<Vec<u8>>,
    tables: Tables,
    metadata: Metadata,
    syntax_uid: String,
    format: Format,
    started: bool,
    ended: bool,
    bot: bool,
    offsets: Vec<u64>,
    item_offset: u64,
    input_bytes: u64,
    frame_bytes: u64,
    fragments: i64,
    delivered: usize,
    parts: Vec<Vec<u8>>,
    scanner: MarkerScanner,
}
impl<S: EncodedFrameSink> Stream<S> {
    pub fn new(sink: S, limits: Limits) -> Result<Stream<S>, Error> {
        let limits = limits.normalized()?;
        Ok(Stream {
            sink,
            limits,
            closed: false,
            eot: None,
            eot_lengths: None,
            tables: Tables::default(),
            metadata: Metadata::default(),
            syntax_uid: String::new(),
            format: Format::JPEG,
            started: false,
            ended: false,
            bot: false,
            offsets: Vec::new(),
            item_offset: 0,
            input_bytes: 0,
            frame_bytes: 0,
            fragments: 0,
            delivered: 0,
            parts: Vec::new(),
            scanner: MarkerScanner::new(Format::JPEG),
        })
    }
    fn live(&self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::bare(ErrorKind::Canceled));
        }
        Ok(())
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub fn close(&mut self) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.sink.close()
    }
    pub fn check_extended_table(&self, h: &TableHeader) -> Result<(), Error> {
        self.live()?;
        if self.started
            || !(h.is_eot() || h.is_eot_lengths())
            || !h.vr_ov
            || h.length == u32::MAX
            || h.length == 0
            || !h.length.is_multiple_of(8)
        {
            return Err(Error::layout_detail("invalid or late EOT attribute"));
        }
        if u64::from(h.length) / 8 > self.limits.max_frames as u64 {
            return Err(Error::resource());
        }
        let present = if h.is_eot() {
            self.eot.is_some()
        } else {
            self.eot_lengths.is_some()
        };
        if present {
            return Err(Error::layout_detail("duplicate EOT attribute"));
        }
        Ok(())
    }
    pub fn extended_table(&mut self, h: &TableHeader, value: &[u8]) -> Result<(), Error> {
        self.check_extended_table(h)?;
        if h.is_eot() {
            self.eot = Some(value.to_vec());
        } else {
            self.eot_lengths = Some(value.to_vec());
        }
        Ok(())
    }
    pub fn start_pixel_data(
        &mut self,
        metadata: Metadata,
        transfer_syntax_uid: &str,
    ) -> Result<(), Error> {
        self.live()?;
        if self.started {
            return Err(Error::layout());
        }
        if metadata.number_of_frames <= 0 {
            return Err(Error::frame_count());
        }
        if metadata.number_of_frames > self.limits.max_frames
            || metadata.number_of_frames > i64::from(i32::MAX)
        {
            return Err(Error::resource());
        }
        self.format = if STREAM_SYNTAXES_JPEG.contains(&transfer_syntax_uid) {
            Format::JPEG
        } else if STREAM_SYNTAXES_JPEGLS.contains(&transfer_syntax_uid) {
            Format::JPEGLS
        } else {
            return Err(Error::layout_detail(
                "unsupported streaming transfer syntax",
            ));
        };
        fn attr(v: &Option<Vec<u8>>) -> Option<TableAttribute<'_>> {
            v.as_deref().map(|bytes| TableAttribute {
                vr_ov: true,
                value: TableValue::Raw(bytes),
            })
        }
        let t = read_tables(
            &[],
            attr(&self.eot),
            attr(&self.eot_lengths),
            metadata.number_of_frames,
        )?;
        if t.extended_present != t.lengths_present {
            return Err(Error::layout_detail("incomplete EOT pair"));
        }
        if t.extended_present {
            validate_offset_order(&t.extended)?;
            for &length in &t.lengths {
                if length == 0 {
                    return Err(Error::layout());
                }
                if length > self.limits.max_frame_bytes {
                    return Err(Error::resource());
                }
            }
            self.offsets = t.extended.clone();
        }
        self.tables = t;
        self.metadata = metadata;
        self.syntax_uid = transfer_syntax_uid.to_owned();
        self.scanner = MarkerScanner::new(self.format);
        self.started = true;
        Ok(())
    }
    fn read(&self, r: &mut dyn Read, data: &mut [u8]) -> Result<(), Error> {
        for chunk in data.chunks_mut(32 << 10) {
            if let Err(e) = r.read_exact(chunk) {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    return Err(Error::source_detail("unexpected EOF"));
                }
                return Err(Error::source_detail(e));
            }
        }
        Ok(())
    }
    pub fn item(&mut self, length: u32, basic: bool, value: &mut dyn Read) -> Result<(), Error> {
        self.live()?;
        if !self.started || self.ended {
            return Err(Error::layout());
        }
        let frames = self.metadata.number_of_frames as u64;
        let length64 = u64::from(length);
        if basic {
            if self.bot || length64 / 4 > self.limits.max_frames as u64 {
                return Err(Error::resource());
            }
            if length != 0
                && (Some(length64) != frames.checked_mul(4) || self.tables.extended_present)
            {
                return Err(Error::layout());
            }
            if length64 > self.limits.max_bytes.wrapping_sub(self.input_bytes) {
                return Err(Error::resource());
            }
            let mut data = vec![0u8; length as usize];
            self.read(value, &mut data)?;
            if length != 0 {
                self.offsets = data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| u64::from(u32::from_le_bytes(*c)))
                    .collect();
                validate_offset_order(&self.offsets)?;
            }
            self.bot = true;
            self.input_bytes = self.input_bytes.saturating_add(length64);
            return Ok(());
        }
        if !self.bot || self.delivered as u64 >= frames {
            return Err(Error {
                kinds: [Some(ErrorKind::FrameCount), None],
                text: format!("{FRAME_COUNT}: unexpected fragment"),
            });
        }
        if length < 2 || length & 1 != 0 || length == u32::MAX {
            return Err(Error::layout());
        }
        if self.fragments >= self.limits.max_fragments
            || self.parts.len() as i64 >= self.limits.max_fragments_per_frame
            || length64 > self.limits.max_frame_bytes.wrapping_sub(self.frame_bytes)
            || length64 > self.limits.max_bytes.wrapping_sub(self.input_bytes)
        {
            return Err(Error::resource());
        }
        if self.parts.is_empty()
            && !self.offsets.is_empty()
            && self.offsets.get(self.delivered) != Some(&self.item_offset)
        {
            return Err(Error::layout_detail("frame offset not at Item boundary"));
        }
        if self.tables.extended_present && !self.parts.is_empty() {
            return Err(Error::layout_detail("EOT frame spans Items"));
        }
        let mut data = vec![0u8; length as usize];
        self.read(value, &mut data)?;
        self.fragments = self.fragments.saturating_add(1);
        self.input_bytes = self.input_bytes.saturating_add(length64);
        self.frame_bytes = self.frame_bytes.saturating_add(length64);
        let next_offset = next_fragment_item_offset(self.item_offset, length64)?;
        let (eoi_length, end) = self.scanner.scan(&data)?;
        if end
            && eoi_length != data.len()
            && !matches!(data.get(eoi_length..), Some([pad]) if valid_frame_pad(self.format, *pad, self.frame_bytes))
        {
            return Err(Error::layout_detail("frame ends inside Item"));
        }
        if let Some(&next_frame) = self.offsets.get(self.delivered.saturating_add(1))
            && (next_offset > next_frame
                || end && next_offset != next_frame
                || !end && next_offset == next_frame)
        {
            return Err(Error::layout_detail("BOT contradicts codestream boundary"));
        }
        if self.tables.extended_present
            && (!end || self.tables.lengths.get(self.delivered) != Some(&(eoi_length as u64)))
        {
            return Err(Error::layout_detail(
                "EOT length contradicts codestream boundary",
            ));
        }
        self.item_offset = next_offset;
        self.parts.push(data);
        if !end {
            return Ok(());
        }
        let frame_data = {
            let parts: Vec<&[u8]> = self.parts.iter().map(Vec::as_slice).collect();
            let tables = Tables {
                basic: vec![0, 0, 0, 0],
                ..Tables::default()
            };
            let plan = Plan::new(
                Box::new(Memory::new(&parts)),
                &tables,
                1,
                self.format,
                self.limits,
                Recovery::default(),
            )?;
            plan.frame(0)?.data.into_owned()
        };
        let frame = EncodedFrame {
            index: self.delivered,
            data: frame_data,
            metadata: self.metadata.clone(),
            transfer_syntax_uid: self.syntax_uid.clone(),
        };
        self.parts.clear();
        self.frame_bytes = 0;
        self.scanner = MarkerScanner::new(self.format);
        self.sink.handle_encoded_frame(frame)?;
        self.live()?;
        self.delivered = self.delivered.saturating_add(1);
        Ok(())
    }
    pub fn end_pixel_data(&mut self) -> Result<(), Error> {
        self.live()?;
        if !self.started
            || self.ended
            || !self.bot
            || !self.parts.is_empty()
            || self.delivered as i64 != self.metadata.number_of_frames
        {
            return Err(Error::frame_count());
        }
        self.ended = true;
        Ok(())
    }
}
