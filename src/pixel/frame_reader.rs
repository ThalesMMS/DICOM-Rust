use super::Metadata;
use super::encapsulated::{
    self, Format, ItemRange, Limits, Memory, Plan, Recovery, TableAttribute, TableValue,
};
use super::pixeldata::{
    self as pd, PixelData, PixelErrorKind, extract_metadata, extract_view, frame_size, total_size,
};
use super::transcode::decompress::{
    DecompressLimits, check_admission, checked_product, limit_error, little_endian_frames,
    native_frame_bytes, normalized, output_metadata,
};
use super::transcode::{
    DecoderRegistry, Error, ErrorKind, file_source_syntax, fragments_element, object_from,
    replace_element, strings_element, swap_elements,
};
use crate::object::deferred::{DeferOptions, open_source_file, read_file_deferred};
use crate::object::{
    self, Element, File, Object, ReadFileOptions, Tag, UNDEFINED_LENGTH, Value, Vr,
};
use crate::textfmt;
use crate::transfer::{self, Syntax};
mod into;
mod partial;
mod region;
pub use into::{
    BufferLayout, BufferMode, BufferResult, ERR_BUFFER_LAYOUT_UNAVAILABLE, ERR_BUFFER_TOO_SMALL,
};
pub use partial::{
    ERR_INVALID_QUALITY_LAYERS, ERR_INVALID_REDUCTION, ERR_PARTIAL_DECODE_UNSUPPORTED,
    MAX_REDUCTION_LEVEL, PartialDecodeCapabilities, PartialDecodePath, PartialDecodeRequest,
    PartialDecodeResult, reduced_region,
};
pub(crate) use partial::{
    Part, PartCodec, builtin_caps, builtin_j2k_part, builtin_region, part_codec,
};
pub use region::{ERR_INVALID_REGION, ERR_REGION_UNSUPPORTED, FrameRegion};
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
const TAG_ENCAPSULATED_TOTAL_LENGTH: Tag = Tag::new(0x7FE0, 0x0003);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const ERR_FRAME_LAYOUT: &str = "dicom: frames cannot be located in Pixel Data";
const ERR_ENCAPSULATED_LIMIT: &str = "dicom: encapsulated frame resource limit exceeded";
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameReaderOptions {
    pub decoder: Option<DecoderRegistry>,
    pub limits: DecompressLimits,
    pub max_frames: i64,
    pub max_fragments: i64,
}
enum Encoded<'a> {
    Planned(Vec<Result<Vec<u8>, encapsulated::Error>>),
    Items(Vec<&'a [u8]>),
    Collection(Vec<u8>),
}
enum Stored<'a> {
    Native(&'a [u8]),
    Encapsulated(Encoded<'a>),
}
pub struct FrameReader<'a> {
    file: &'a File,
    syntax: Syntax,
    metadata: Metadata,
    options: FrameReaderOptions,
    proxy: Vec<Element>,
    pixel_vr: Vr,
    stored: Stored<'a>,
    deferred_input: Option<i64>,
}
const ERR_DEFERRED_SOURCE: &str = "deferred Pixel Data needs its seekable source";
fn read_at(source: &[u8], offset: i64, length: i64) -> Result<&[u8], Error> {
    usize::try_from(offset)
        .ok()
        .zip(usize::try_from(length).ok())
        .and_then(|(start, n)| source.get(start..start.checked_add(n)?))
        .ok_or_else(|| Error::new(ErrorKind::Other, "EOF"))
}
fn scan_items(
    source: &[u8],
    offset: i64,
    length: i64,
    max_items: i64,
    max_frames: i64,
) -> Result<(Vec<ItemRange>, &[u8]), Error> {
    let Some(end) = offset
        .checked_add(length)
        .filter(|_| offset >= 0 && length >= 0)
    else {
        return Err(layout("invalid encapsulated value extent"));
    };
    let mut offset = offset;
    let mut items = Vec::new();
    let mut basic: &[u8] = &[];
    let mut first = true;
    loop {
        if end.saturating_sub(offset) < 8 {
            return Err(layout("Items run past the value"));
        }
        let Some(&[g0, g1, e0, e1, l0, l1, l2, l3]) =
            read_at(source, offset, 8)?.first_chunk::<8>()
        else {
            return Err(layout("Items run past the value"));
        };
        let tag = Tag::new(u16::from_le_bytes([g0, g1]), u16::from_le_bytes([e0, e1]));
        let item_length = u32::from_le_bytes([l0, l1, l2, l3]);
        offset = offset.saturating_add(8);
        if tag == Tag::new(0xFFFE, 0xE0DD) {
            return Ok((items, basic));
        }
        if tag != Tag::new(0xFFFE, 0xE000)
            || item_length == u32::MAX
            || i64::from(item_length) > end.saturating_sub(offset)
        {
            return Err(layout("malformed Item"));
        }
        if first {
            if u64::from(item_length).div_ceil(4) > max_frames as u64 {
                return Err(layout_limit(
                    "Basic Offset Table exceeds the frame index limit",
                ));
            }
            basic = read_at(source, offset, i64::from(item_length))?;
            first = false;
        } else {
            if items.len() as i64 >= max_items {
                return Err(layout_limit(format_args!(
                    "fragment count exceeds {max_items}"
                )));
            }
            items.push(ItemRange {
                offset,
                length: u64::from(item_length),
            });
        }
        offset = offset.saturating_add(i64::from(item_length));
    }
}
fn layout(detail: impl std::fmt::Display) -> Error {
    Error::new(
        ErrorKind::FrameLayout,
        format!("{ERR_FRAME_LAYOUT}: {detail}"),
    )
}
fn no_independent_codestream() -> Error {
    layout("a component collection has no independent frame codestream")
}
fn layout_bare() -> Error {
    Error::new(ErrorKind::FrameLayout, ERR_FRAME_LAYOUT)
}
fn layout_limit(detail: impl std::fmt::Display) -> Error {
    let inner = format!("{ERR_ENCAPSULATED_LIMIT}: {detail}");
    Error::wrapping(
        ErrorKind::FrameLayout,
        format!("{ERR_FRAME_LAYOUT}: {inner}"),
        Error::new(ErrorKind::EncapsulatedResourceLimit, inner),
    )
}
fn encapsulated_error(e: &encapsulated::Error) -> Error {
    let kind = if e.is(encapsulated::ErrorKind::ResourceLimit) {
        ErrorKind::EncapsulatedResourceLimit
    } else {
        ErrorKind::Other
    };
    Error::new(kind, e.to_string())
}
fn layout_wrap(e: Error) -> Error {
    Error::wrapping(
        ErrorKind::FrameLayout,
        format!("{ERR_FRAME_LAYOUT}: {e}"),
        e,
    )
}
fn type_label(v: &Value) -> &'static str {
    match v {
        Value::Nil => "<nil>",
        Value::Raw(_) => "core.RawValue",
        Value::Sequence(_) => "core.SequenceValue",
        Value::Strings(_) => "core.StringValue",
        Value::Fragments { .. } => "core.FragmentSequence",
        Value::BulkData(_) => "core.BulkDataValue",
    }
}
fn stream_frame_format(uid: &str) -> Option<Format> {
    match transfer::normalize_uid(uid) {
        "1.2.840.10008.1.2.4.50"
        | "1.2.840.10008.1.2.4.51"
        | "1.2.840.10008.1.2.4.57"
        | "1.2.840.10008.1.2.4.70" => Some(Format::JPEG),
        "1.2.840.10008.1.2.4.80" | "1.2.840.10008.1.2.4.81" => Some(Format::JPEGLS),
        "1.2.840.10008.1.2.4.90"
        | "1.2.840.10008.1.2.4.91"
        | "1.2.840.10008.1.2.4.201"
        | "1.2.840.10008.1.2.4.202"
        | "1.2.840.10008.1.2.4.203" => Some(Format::JPEG2000),
        _ => None,
    }
}
fn table_attribute(obj: &Object, tag: Tag) -> Option<TableAttribute<'_>> {
    let e = obj.get(tag)?;
    Some(TableAttribute {
        vr_ov: e.vr() == Vr::OV,
        value: match &e.value {
            Value::Raw(r) => TableValue::Raw(r),
            _ => TableValue::Unsupported("core.Value"),
        },
    })
}
fn packed_bytes(bits: i64) -> i64 {
    (bits / 8).saturating_add(i64::from(bits % 8 != 0))
}
fn swap_words(data: &mut [u8]) {
    for pair in data.as_chunks_mut::<2>().0 {
        pair.swap(0, 1);
    }
}
impl<'a> FrameReader<'a> {
    pub fn new(file: &'a File, options: FrameReaderOptions) -> Result<FrameReader<'a>, Error> {
        Self::build(file, None, options)
    }
    pub fn with_source(
        file: &'a File,
        source: &'a [u8],
        options: FrameReaderOptions,
    ) -> Result<FrameReader<'a>, Error> {
        Self::build(file, Some(source), options)
    }
    fn build(
        file: &'a File,
        source: Option<&'a [u8]>,
        options: FrameReaderOptions,
    ) -> Result<FrameReader<'a>, Error> {
        let syntax = file_source_syntax(file)?;
        let dataset = &file.dataset;
        let Some(pixel) = dataset.get(TAG_PIXEL_DATA) else {
            return Err(Error::pixel_bare(PixelErrorKind::PixelDataNotFound));
        };
        let metadata = extract_metadata(dataset)?;
        let mut options = options;
        if options.max_frames < 0 || options.max_fragments < 0 {
            return Err(layout_limit("negative frame index limit"));
        }
        let defaults = Limits::defaults();
        if options.max_frames == 0 {
            options.max_frames = defaults.max_frames;
        }
        if options.max_fragments == 0 {
            options.max_fragments = defaults.max_fragments;
        }
        if metadata.number_of_frames > options.max_frames {
            if syntax.encapsulated {
                return Err(layout_limit(format_args!(
                    "frame count {} exceeds {}",
                    metadata.number_of_frames, options.max_frames
                )));
            }
            return Err(layout_wrap(limit_error(
                "MaxFrames",
                metadata.number_of_frames,
                options.max_frames,
            )));
        }
        if metadata.bits_allocated < 8 && (metadata.bits_allocated != 1 || syntax.encapsulated) {
            return Err(layout(format_args!(
                "BitsAllocated {} packs frames across bytes",
                metadata.bits_allocated
            )));
        }
        let mut proxy = Vec::new();
        for e in dataset.elements() {
            match e.tag() {
                TAG_PIXEL_DATA
                | TAG_EXTENDED_OFFSET_TABLE
                | TAG_EXTENDED_OFFSET_TABLE_LENGTHS
                | TAG_ENCAPSULATED_TOTAL_LENGTH => {}
                TAG_NUMBER_OF_FRAMES => proxy.push(strings_element(
                    TAG_NUMBER_OF_FRAMES,
                    Vr::IS,
                    vec!["1".into()],
                )),
                _ => proxy.push(e.clone()),
            }
        }
        let mut r = FrameReader {
            file,
            syntax,
            metadata,
            options,
            proxy,
            pixel_vr: pixel.vr(),
            stored: Stored::Native(&[]),
            deferred_input: None,
        };
        if r.syntax.encapsulated {
            let (encoded, input) = r.index_encapsulated(pixel, source)?;
            r.stored = Stored::Encapsulated(encoded);
            r.deferred_input = input;
        } else {
            let (value, input) = r.index_native(pixel, source)?;
            r.stored = Stored::Native(value);
            r.deferred_input = input;
        }
        Ok(r)
    }
    fn index_native(
        &self,
        pixel: &'a Element,
        source: Option<&'a [u8]>,
    ) -> Result<(&'a [u8], Option<i64>), Error> {
        let mut deferred = None;
        let value: &'a [u8] = match &pixel.value {
            Value::Raw(raw) => raw,
            Value::Nil => {
                let locations = self.file.value_locations(TAG_PIXEL_DATA);
                let (Some(source), [location]) = (source, locations.as_slice()) else {
                    return Err(layout(ERR_DEFERRED_SOURCE));
                };
                if pixel.header.length == UNDEFINED_LENGTH {
                    return Err(layout(ERR_DEFERRED_SOURCE));
                }
                deferred = Some(location.length);
                read_at(source, location.value_offset, location.length)?
            }
            v => {
                return Err(layout(format_args!(
                    "native Pixel Data value {}",
                    type_label(v)
                )));
            }
        };
        let mut total = total_size(&self.metadata);
        if self.metadata.bits_allocated == 1 {
            total = pd::native_packed_byte_count(&self.metadata);
        }
        if total <= 0 {
            return Err(layout("invalid native Pixel Data dimensions"));
        }
        if self.ow_lanes() {
            total = total.saturating_add(total % 2);
            if !value.len().is_multiple_of(2) {
                return Err(Error::pixel(
                    PixelErrorKind::PixelDataSizeMismatch,
                    "Big Endian OW Pixel Data ends with an incomplete word",
                ));
            }
        }
        if (value.len() as i64) < total {
            return Err(Error::pixel(
                PixelErrorKind::PixelDataSizeMismatch,
                format_args!(
                    "{} bytes of Pixel Data, need {total} for {} frames",
                    value.len(),
                    self.metadata.number_of_frames
                ),
            ));
        }
        Ok((value, deferred))
    }
    fn index_encapsulated(
        &self,
        pixel: &'a Element,
        source: Option<&'a [u8]>,
    ) -> Result<(Encoded<'a>, Option<i64>), Error> {
        let mut deferred = None;
        let (basic, fragments): (&'a [u8], Vec<&'a [u8]>) = match &pixel.value {
            Value::Fragments {
                offset_table,
                fragments,
            } => {
                if fragments.len() as i64 > self.options.max_fragments {
                    return Err(layout_limit(format_args!(
                        "fragment count {} exceeds {}",
                        fragments.len(),
                        self.options.max_fragments
                    )));
                }
                if (offset_table.len() as u64).div_ceil(4) > self.options.max_frames as u64 {
                    return Err(layout_limit(
                        "Basic Offset Table exceeds the frame index limit",
                    ));
                }
                (offset_table, fragments.iter().map(Vec::as_slice).collect())
            }
            Value::Nil => {
                let locations = self.file.value_locations(TAG_PIXEL_DATA);
                let (Some(source), [location]) = (source, locations.as_slice()) else {
                    return Err(layout(ERR_DEFERRED_SOURCE));
                };
                let (items, basic) = scan_items(
                    source,
                    location.value_offset,
                    location.length,
                    self.options.max_fragments,
                    self.options.max_frames,
                )?;
                encapsulated::reader_at(source, &items)
                    .map_err(|e| layout_wrap(encapsulated_error(&e)))?;
                let mut input = basic.len() as i64;
                let mut fragments = Vec::with_capacity(items.len());
                for item in &items {
                    input = input.saturating_add(item.length as i64);
                    fragments.push(read_at(source, item.offset, item.length as i64)?);
                }
                deferred = Some(input);
                (basic, fragments)
            }
            v => {
                return Err(layout(format_args!(
                    "encapsulated Pixel Data value {}",
                    type_label(v)
                )));
            }
        };
        let frames = self.metadata.number_of_frames;
        if matches!(
            transfer::normalize_uid(&self.syntax.uid),
            "1.2.840.10008.1.2.4.92" | "1.2.840.10008.1.2.4.93"
        ) && let [only] = fragments.as_slice()
            && frames > 1
        {
            let collection = self.index_part2_collection(basic, only)?;
            return Ok((Encoded::Collection(collection), deferred));
        }
        if let Some(format) = stream_frame_format(&self.syntax.uid) {
            let dataset = &self.file.dataset;
            let tables = encapsulated::read_tables(
                basic,
                table_attribute(dataset, TAG_EXTENDED_OFFSET_TABLE),
                table_attribute(dataset, TAG_EXTENDED_OFFSET_TABLE_LENGTHS),
                frames,
            )
            .map_err(|e| layout_wrap(encapsulated_error(&e)))?;
            let plan = Plan::new(
                Box::new(Memory::new(&fragments)),
                &tables,
                frames,
                format,
                Limits {
                    max_frames: self.options.max_frames,
                    max_fragments: self.options.max_fragments,
                    ..Limits::default()
                },
                Recovery::default(),
            )
            .map_err(|e| layout_wrap(encapsulated_error(&e)))?;
            let planned = (0..plan.len())
                .map(|i| plan.frame(i).map(|v| v.data.into_owned()))
                .collect();
            return Ok((Encoded::Planned(planned), deferred));
        }
        if fragments.len() as i64 != frames {
            return Err(layout(format_args!(
                "{} Items for {frames} frames",
                fragments.len()
            )));
        }
        if basic.is_empty() {
            return Ok((Encoded::Items(fragments), deferred));
        }
        if Some(basic.len() as i64) != frames.checked_mul(4) {
            return Err(layout(format_args!(
                "Basic Offset Table of {} bytes for {frames} frames",
                basic.len()
            )));
        }
        let mut next = 0u64;
        for (i, (f, entry)) in fragments.iter().zip(basic.as_chunks::<4>().0).enumerate() {
            let entry = u32::from_le_bytes(*entry);
            if u64::from(entry) != next {
                return Err(layout(format_args!(
                    "Basic Offset Table entry {i} does not point at Item {i}"
                )));
            }
            let size = f.len() as u64;
            next = next
                .saturating_add(size)
                .saturating_add(size % 2)
                .saturating_add(8);
        }
        Ok((Encoded::Items(fragments), deferred))
    }
    fn index_part2_collection(&self, basic: &[u8], item: &[u8]) -> Result<Vec<u8>, Error> {
        let dataset = &self.file.dataset;
        if !basic.is_empty()
            || dataset.get(TAG_EXTENDED_OFFSET_TABLE).is_some()
            || dataset.get(TAG_EXTENDED_OFFSET_TABLE_LENGTHS).is_some()
        {
            return Err(layout("Annex J collection requires empty offset tables"));
        }
        let limits = normalized(self.options.limits)?;
        let size = item.len() as u64;
        if size > 32 << 20 || size > limits.max_input_bytes as u64 {
            return Err(Error::new(
                ErrorKind::ResourceLimit,
                super::transcode::ERR_RESOURCE_LIMIT,
            ));
        }
        #[cfg(feature = "jpeg2000")]
        if let Ok(c) = crate::codec::jpeg2000::j2kpart2::inspect(item) {
            let m = &self.metadata;
            if c.profile.frames == m.number_of_frames
                && c.rows == i64::from(m.rows)
                && c.columns == i64::from(m.columns)
                && c.profile.bits == i64::from(m.bits_stored)
                && c.profile.signed == (m.pixel_representation == 1)
            {
                return Ok(item.to_vec());
            }
        }
        Err(layout("invalid Annex J collection"))
    }
    fn decode_part2_collection_frame(
        &self,
        collection: &[u8],
        i: i64,
    ) -> Result<(Vec<u8>, Metadata), Error> {
        let mut elements = self.file.dataset.elements().to_vec();
        replace_element(
            &mut elements,
            fragments_element(Vec::new(), vec![collection.to_vec()]),
        );
        let proxy = object_from(elements, self.syntax.big_endian);
        let (decoded, _) = super::transcode::decompress::decompress_data_set(
            &proxy,
            &self.syntax.uid,
            &super::transcode::decompress::DecompressOptions {
                registry: self.options.decoder,
                limits: self.options.limits,
                ..Default::default()
            },
        )?;
        let frames = pd::extract_native_frames(&decoded)
            .ok()
            .filter(|f| f.data.len() as i64 == self.metadata.number_of_frames)
            .ok_or_else(layout_bare)?;
        let mut metadata = frames.metadata;
        metadata.number_of_frames = 1;
        let raw = frames
            .data
            .into_iter()
            .nth(i as usize)
            .ok_or_else(layout_bare)?;
        Ok((raw, metadata))
    }
    fn collection(&self) -> Option<&[u8]> {
        match &self.stored {
            Stored::Encapsulated(Encoded::Collection(c)) => Some(c),
            _ => None,
        }
    }
    pub(crate) fn input_bytes(&self) -> i64 {
        if let Some(input) = self.deferred_input {
            return input;
        }
        match self.file.dataset.get(TAG_PIXEL_DATA).map(|e| &e.value) {
            Some(Value::Raw(raw)) => raw.len() as i64,
            Some(Value::Fragments {
                offset_table,
                fragments,
            }) => super::transcode::payload_bytes(&PixelData::Encapsulated {
                offset_table,
                fragments,
            }),
            _ => 0,
        }
    }
    pub(crate) fn is_native(&self) -> bool {
        matches!(self.stored, Stored::Native(_))
    }
    pub(crate) fn admitted_frame_input_bytes(&self, i: i64) -> Result<i64, Error> {
        self.check_index(i)?;
        let (limits, decode_metadata) = self.prepare_decode()?;
        let input = self.raw_frame_length(i)?;
        check_admission(&limits, &decode_metadata, &self.syntax, input)?;
        Ok(input)
    }
    pub(crate) fn decode_frame_object(
        &self,
        i: i64,
    ) -> Result<(Object, Vec<u8>, Vec<String>), Error> {
        use super::transcode::decompress::{
            detect_lossy_state_crate, stamp_lossy_state, with_native_pixel_data,
        };
        let bare = object_from(self.proxy.clone(), self.syntax.big_endian);
        if let Some(collection) = self.collection() {
            self.check_index(i)?;
            let (raw, metadata) = self.decode_part2_collection_frame(collection, i)?;
            let decoded = with_native_pixel_data(&bare, &metadata, raw.clone(), &self.syntax)?;
            return Ok((decoded, raw, Vec::new()));
        }
        let (raw, metadata, input, diagnostics, lossy) = match &self.stored {
            Stored::Native(value) if self.shared_units() => {
                let (raw, metadata) = self.native_frame(value, i)?;
                let input = self.native_request_input_bytes(value, i)?;
                let lossy = detect_lossy_state_crate(&bare, &self.syntax.uid);
                (raw, metadata, input, Vec::new(), lossy)
            }
            _ => {
                self.check_index(i)?;
                let (limits, decode_metadata) = self.prepare_decode()?;
                let input = self.raw_frame_length(i)?;
                check_admission(&limits, &decode_metadata, &self.syntax, input)?;
                let frame = self.raw_frame(i)?;
                let mut elements = self.proxy.clone();
                if self.syntax.encapsulated {
                    replace_element(&mut elements, fragments_element(Vec::new(), vec![frame]));
                } else {
                    let len = frame.len() as u32;
                    replace_element(
                        &mut elements,
                        Element {
                            header: crate::object::ElementHeader::new(
                                TAG_PIXEL_DATA,
                                self.pixel_vr,
                                len,
                            ),
                            value: Value::Raw(frame),
                        },
                    );
                }
                let proxy = object_from(elements, self.syntax.big_endian);
                let view = extract_view(&proxy)?;
                let frames = self
                    .registry()
                    .decode_frames(&self.syntax.uid, &view, &proxy)?;
                let diagnostics = frames.diagnostics.iter().map(|d| d.to_string()).collect();
                let metadata = output_metadata(&decode_metadata, &self.syntax, &frames);
                let frames = little_endian_frames(&metadata, &self.syntax, frames)?;
                let raw = native_frame_bytes(&metadata, &frames)?;
                let lossy = detect_lossy_state_crate(&proxy, &self.syntax.uid);
                (raw, metadata, input, diagnostics, lossy)
            }
        };
        let mut decoded = with_native_pixel_data(&bare, &metadata, raw.clone(), &self.syntax)?;
        stamp_lossy_state(&mut decoded, &lossy, raw.len() as i64, input);
        Ok((decoded, raw, diagnostics))
    }
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    pub fn transfer_syntax(&self) -> &Syntax {
        &self.syntax
    }
    pub fn file(&self) -> &'a File {
        self.file
    }
    fn check_index(&self, i: i64) -> Result<(), Error> {
        if i < 0 || i >= self.metadata.number_of_frames {
            return Err(Error::pixel(
                PixelErrorKind::InvalidMetadata,
                format_args!("frame {i} of {}", self.metadata.number_of_frames),
            ));
        }
        Ok(())
    }
    fn ow_lanes(&self) -> bool {
        self.syntax.big_endian && self.pixel_vr == Vr::OW && self.metadata.bits_allocated <= 8
    }
    fn shared_units(&self) -> bool {
        self.metadata.bits_allocated == 1 || self.ow_lanes()
    }
    pub fn raw_frame(&self, i: i64) -> Result<Vec<u8>, Error> {
        self.check_index(i)?;
        match &self.stored {
            Stored::Native(value) => self.raw_native_frame(value, i),
            Stored::Encapsulated(Encoded::Planned(frames)) => {
                match frames.get(i as usize).ok_or_else(layout_bare)? {
                    Ok(data) => Ok(data.clone()),
                    Err(e) => Err(encapsulated_error(e)),
                }
            }
            Stored::Encapsulated(Encoded::Items(items)) => {
                Ok(items.get(i as usize).ok_or_else(layout_bare)?.to_vec())
            }
            Stored::Encapsulated(Encoded::Collection(_)) => Err(no_independent_codestream()),
        }
    }
    pub fn raw_frame_length(&self, i: i64) -> Result<i64, Error> {
        self.check_index(i)?;
        match &self.stored {
            Stored::Native(_) => {
                let mut length = frame_size(&self.metadata);
                if self.ow_lanes() {
                    length = length.saturating_add(length % 2);
                }
                Ok(length)
            }
            Stored::Encapsulated(Encoded::Planned(frames)) => {
                match frames.get(i as usize).ok_or_else(layout_bare)? {
                    Ok(data) => Ok(data.len() as i64),
                    Err(_) => Err(layout(format_args!("frame {}", i.saturating_add(1)))),
                }
            }
            Stored::Encapsulated(Encoded::Items(items)) => {
                Ok(items.get(i as usize).ok_or_else(layout_bare)?.len() as i64)
            }
            Stored::Encapsulated(Encoded::Collection(_)) => Err(no_independent_codestream()),
        }
    }
    fn registry(&self) -> DecoderRegistry {
        self.options.decoder.unwrap_or(DecoderRegistry::Builtin)
    }
    fn frame_metadata(&self) -> Metadata {
        let mut m = self.metadata.clone();
        m.number_of_frames = 1;
        m
    }
    fn prepare_decode(&self) -> Result<(DecompressLimits, Metadata), Error> {
        let mut limits = self.options.limits;
        limits.max_frames = 1;
        let limits = normalized(limits)?;
        if !self.syntax.supported && !self.syntax.requires_codec() {
            return Err(Error::new(
                ErrorKind::UnsupportedDecompression,
                format!(
                    "dicom: unsupported still-image decompression: source transfer syntax {} is not supported",
                    textfmt::quote(&self.syntax.uid)
                ),
            ));
        }
        let proxy = object_from(self.proxy.clone(), self.syntax.big_endian);
        let metadata = extract_metadata(&proxy)?;
        if self.syntax.big_endian {
            swap_elements(&mut self.proxy.clone(), true)?;
        }
        Ok((limits, metadata))
    }
    pub fn frame(&self, i: i64) -> Result<(Vec<u8>, Metadata), Error> {
        match &self.stored {
            Stored::Native(value) => self.native_frame(value, i),
            Stored::Encapsulated(_) => {
                let (frame, mut metadata) = self.decode_frame(i)?;
                metadata.photometric_interpretation = metadata
                    .photometric_interpretation
                    .trim_matches([' ', '\0'])
                    .to_string();
                if metadata.photometric_interpretation.is_empty() {
                    return Err(Error::pixel(
                        PixelErrorKind::MissingMetadata,
                        "PhotometricInterpretation (0028,0004)",
                    ));
                }
                Ok((frame, metadata))
            }
        }
    }
    fn decode_frame(&self, i: i64) -> Result<(Vec<u8>, Metadata), Error> {
        self.check_index(i)?;
        if let Some(collection) = self.collection() {
            return self.decode_part2_collection_frame(collection, i);
        }
        let (limits, decode_metadata) = self.prepare_decode()?;
        let input = self.raw_frame_length(i)?;
        check_admission(&limits, &decode_metadata, &self.syntax, input)?;
        let frame = self.raw_frame(i)?;
        let mut elements = self.proxy.clone();
        replace_element(&mut elements, fragments_element(Vec::new(), vec![frame]));
        let proxy = object_from(elements, self.syntax.big_endian);
        let view = extract_view(&proxy)?;
        if !matches!(view, PixelData::Encapsulated { .. }) {
            return Err(Error::pixel(
                PixelErrorKind::IncompatiblePixelData,
                format_args!(
                    "transfer syntax {} expects encapsulated pixel data",
                    textfmt::quote(&self.syntax.uid)
                ),
            ));
        }
        let frames = self
            .registry()
            .decode_frames(&self.syntax.uid, &view, &proxy)?;
        let metadata = output_metadata(&decode_metadata, &self.syntax, &frames);
        let frames = little_endian_frames(&metadata, &self.syntax, frames)?;
        let raw = native_frame_bytes(&metadata, &frames)?;
        Ok((raw, metadata))
    }
    fn native_span(&self, i: i64) -> (i64, i64) {
        let units = if self.metadata.bits_allocated == 1 {
            pd::packed_frame_bits(&self.metadata)
        } else {
            frame_size(&self.metadata)
        };
        (i.saturating_mul(units), units)
    }
    fn native_wire_range(&self, start: i64, length: i64) -> (i64, i64) {
        let (mut first, mut end) = (start, start.saturating_add(length));
        if self.metadata.bits_allocated == 1 {
            first /= 8;
            end = packed_bytes(end);
        }
        if self.ow_lanes() {
            first &= !1;
            end = end.saturating_add(end % 2);
        }
        (first, end)
    }
    fn native_request_input_bytes(&self, value: &[u8], i: i64) -> Result<i64, Error> {
        if !self.shared_units() {
            return Ok(frame_size(&self.metadata));
        }
        let (start, length) = self.native_span(i);
        let (first, end) = self.native_wire_range(start, length);
        if first < 0 || end < first || end > value.len() as i64 {
            return Err(layout_bare());
        }
        Ok(end.saturating_sub(first))
    }
    fn native_frame(&self, value: &[u8], i: i64) -> Result<(Vec<u8>, Metadata), Error> {
        self.check_index(i)?;
        let m = self.frame_metadata();
        let bytes = frame_size(&m);
        if m.samples_per_pixel == 0 || bytes <= 0 {
            return Err(layout_bare());
        }
        let mut limits = self.options.limits;
        limits.max_frames = 1;
        let limits = normalized(limits)?;
        if self.syntax.big_endian {
            self.prepare_decode()?;
        }
        let input = self.native_request_input_bytes(value, i)?;
        check_frame_read_limits(&limits, &m, input, bytes)?;
        let data = self.native_into(value, i)?;
        Ok((data, m))
    }
    fn native_into(&self, value: &[u8], i: i64) -> Result<Vec<u8>, Error> {
        let size = frame_size(&self.metadata);
        if !self.shared_units() {
            let start = i.saturating_mul(size) as usize;
            let mut data = value
                .get(start..start.saturating_add(size as usize))
                .ok_or_else(layout_bare)?
                .to_vec();
            let width = (self.metadata.bits_allocated as usize).div_ceil(8);
            if self.syntax.big_endian && width > 1 {
                for sample in data.chunks_exact_mut(width) {
                    sample.reverse();
                }
            }
            return Ok(data);
        }
        let (start, length) = self.native_span(i);
        let (first, end) = self.native_wire_range(start, length);
        if first < 0 || end < first || end > value.len() as i64 {
            return Err(layout_bare());
        }
        let mut chunk = value
            .get(first as usize..end as usize)
            .ok_or_else(layout_bare)?
            .to_vec();
        if self.ow_lanes() {
            swap_words(&mut chunk);
        }
        let mut dst = vec![0u8; size as usize];
        if self.metadata.bits_allocated == 1 {
            let at = start.saturating_sub(first.saturating_mul(8));
            pd::copy_packed_bits(&mut dst, 0, &chunk, at, length);
            let touched = packed_bytes(length) as usize;
            if length % 8 != 0
                && let Some(last) = dst.get_mut(touched.wrapping_sub(1))
            {
                *last &= ((1u16 << (length % 8)).wrapping_sub(1)) as u8;
            }
        } else {
            let at = start.saturating_sub(first) as usize;
            let from = chunk
                .get(at..at.saturating_add(length as usize))
                .ok_or_else(layout_bare)?;
            if dst.len() != from.len() {
                return Err(layout_bare());
            }
            dst.copy_from_slice(from);
        }
        Ok(dst)
    }
    fn raw_native_frame(&self, value: &[u8], i: i64) -> Result<Vec<u8>, Error> {
        let size = self.raw_frame_length(i)?;
        if self.shared_units() {
            let mut frame = self.native_into(value, i)?;
            frame.resize(size as usize, 0);
            if self.ow_lanes() {
                swap_words(&mut frame);
            }
            return Ok(frame);
        }
        let start = i.saturating_mul(size) as usize;
        Ok(value
            .get(start..start.saturating_add(size as usize))
            .ok_or_else(layout_bare)?
            .to_vec())
    }
}
fn check_frame_read_limits(
    l: &DecompressLimits,
    m: &Metadata,
    input: i64,
    native: i64,
) -> Result<(), Error> {
    if input < 0 {
        return Err(limit_error("MaxInputBytes", i64::MAX, l.max_input_bytes));
    }
    if input > l.max_input_bytes {
        return Err(limit_error("MaxInputBytes", input, l.max_input_bytes));
    }
    if m.rows == 0
        || m.columns == 0
        || m.samples_per_pixel == 0
        || m.bits_allocated == 0
        || m.number_of_frames <= 0
    {
        return Err(Error::pixel(
            PixelErrorKind::InvalidMetadata,
            format_args!(
                "rows={} columns={} samples_per_pixel={} bits_allocated={} number_of_frames={}",
                m.rows, m.columns, m.samples_per_pixel, m.bits_allocated, m.number_of_frames
            ),
        ));
    }
    let Some(pixels) =
        checked_product(&[i64::from(m.rows), i64::from(m.columns), m.number_of_frames])
    else {
        return Err(limit_error("MaxPixels", i64::MAX, l.max_pixels));
    };
    if pixels > l.max_pixels {
        return Err(limit_error("MaxPixels", pixels, l.max_pixels));
    }
    if native <= 0 {
        return Err(limit_error("MaxNativeBytes", i64::MAX, l.max_native_bytes));
    }
    if native > l.max_native_bytes {
        return Err(limit_error("MaxNativeBytes", native, l.max_native_bytes));
    }
    Ok(())
}
struct FilePixels<'f> {
    metadata: Metadata,
    pixels: PixelData<'f>,
    special: bool,
}
fn file_frame_index_error(index: i64, count: i64) -> Error {
    Error::pixel(
        PixelErrorKind::InvalidMetadata,
        format_args!("decoded frame index {index} out of range [0,{count})"),
    )
}
fn file_frame_pixels(file: &File) -> Result<FilePixels<'_>, Error> {
    if file.partial_read.is_some() || file.incomplete_pixel_data.is_some() {
        return Err(Error::new(
            ErrorKind::PartialFile,
            "dicom: file is the result of a partial read",
        ));
    }
    let uid = transfer::normalize_uid(file.transfer_syntax.uid);
    let metadata = extract_metadata(&file.dataset);
    if pd::is_jpip_referenced_transfer_syntax(uid) {
        return Ok(FilePixels {
            metadata: metadata?,
            pixels: PixelData::Native(&[]),
            special: true,
        });
    }
    let pixels = extract_view(&file.dataset);
    if pd::is_video_transfer_syntax(uid) {
        let metadata = metadata?;
        return match pixels {
            Ok(pixels) => Ok(FilePixels {
                metadata,
                pixels,
                special: true,
            }),
            Err(e) => Err(Error::new(
                ErrorKind::Pixel(PixelErrorKind::MediaPayloadNotRenderable),
                format!("{}: {e}", pd::media_error(uid, None)),
            )),
        };
    }
    let pixels = pixels?;
    let special = matches!(pixels, PixelData::Encapsulated { .. });
    Ok(FilePixels {
        metadata: metadata?,
        pixels,
        special,
    })
}
fn decode_file_frames(
    file: &File,
    p: &FilePixels<'_>,
    registry: Option<DecoderRegistry>,
) -> Result<(Metadata, Vec<Vec<u8>>), Error> {
    if !p.special {
        let frames = pd::split_native_frame_range(
            &p.pixels,
            &file.dataset,
            file.dataset.value_big_endian(),
            &p.metadata,
        )?;
        return Ok((p.metadata.clone(), frames));
    }
    let frames = registry.unwrap_or(DecoderRegistry::Builtin).decode_frames(
        file.transfer_syntax.uid,
        &p.pixels,
        &file.dataset,
    )?;
    let syntax = Syntax {
        uid: file.transfer_syntax.uid.to_string().into(),
        ..Syntax::default()
    };
    let mut metadata = output_metadata(&p.metadata, &syntax, &frames);
    metadata.photometric_interpretation = metadata
        .photometric_interpretation
        .trim_matches([' ', '\0'])
        .to_string();
    if metadata.photometric_interpretation.is_empty() {
        return Err(Error::pixel(
            PixelErrorKind::MissingMetadata,
            "PhotometricInterpretation (0028,0004)",
        ));
    }
    Ok((metadata, frames.data))
}
pub fn decode_file_frames_view(
    file: &File,
    registry: Option<DecoderRegistry>,
) -> Result<(Metadata, Vec<Vec<u8>>, bool), Error> {
    DecoderRegistry::refuse(registry, file.transfer_syntax.uid)?;
    let p = file_frame_pixels(file)?;
    let (metadata, frames) = decode_file_frames(file, &p, registry)?;
    Ok((metadata, frames, p.special))
}
pub fn decode_file_frame(
    file: &File,
    index: i64,
    registry: Option<DecoderRegistry>,
) -> Result<(Metadata, Vec<u8>, bool), Error> {
    DecoderRegistry::refuse(registry, file.transfer_syntax.uid)?;
    let p = file_frame_pixels(file)?;
    let uid = transfer::normalize_uid(file.transfer_syntax.uid);
    let external = pd::is_video_transfer_syntax(uid) || pd::is_jpip_referenced_transfer_syntax(uid);
    let count = p.metadata.number_of_frames;
    if !external && (index < 0 || index >= count) {
        return Err(file_frame_index_error(index, count));
    }
    let encapsulated = matches!(p.pixels, PixelData::Encapsulated { .. });
    if !external && encapsulated && count > 1 {
        let options = FrameReaderOptions {
            decoder: registry,
            ..FrameReaderOptions::default()
        };
        match FrameReader::new(file, options) {
            Ok(reader) => {
                let (frame, mut decoded) = reader.frame(index)?;
                decoded.number_of_frames = count;
                return Ok((decoded, frame, true));
            }
            Err(e) => {
                if !e.is(ErrorKind::UnknownTransferSyntax)
                    && (!e.is(ErrorKind::FrameLayout)
                        || e.is(ErrorKind::EncapsulatedResourceLimit)
                        || e.is(ErrorKind::DecompressResourceLimit))
                {
                    return Err(e);
                }
            }
        }
    }
    let (metadata, frames, index) = if p.special {
        let (metadata, frames) = decode_file_frames(file, &p, registry)?;
        (metadata, frames, index)
    } else {
        let frames = pd::split_native_frame_range(
            &p.pixels,
            &file.dataset,
            file.dataset.value_big_endian(),
            &p.metadata,
        )?;
        let frame = frames.into_iter().nth(index as usize);
        (p.metadata.clone(), frame.into_iter().collect(), 0)
    };
    if index < 0 || index as usize >= frames.len() {
        return Err(file_frame_index_error(index, frames.len() as i64));
    }
    let frame = frames
        .get(index as usize)
        .ok_or_else(|| file_frame_index_error(index, frames.len() as i64))?;
    Ok((metadata, frame.clone(), p.special))
}
pub struct FrameSource {
    file: File,
    source: Vec<u8>,
}
fn object_error(e: object::Error) -> Error {
    let kind = if e.is(object::Kind::UnknownTransferSyntax) {
        ErrorKind::UnknownTransferSyntax
    } else {
        ErrorKind::Other
    };
    Error::new(kind, e.to_string())
}
impl FrameSource {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<FrameSource, Error> {
        let path = path.as_ref().to_string_lossy();
        let path: &str = &path;
        FrameSource::from_bytes(open_source_file(path).map_err(object_error)?)
    }
    pub fn from_bytes(source: Vec<u8>) -> Result<FrameSource, Error> {
        let defer = DeferOptions {
            defer_pixel_data: true,
            ..DeferOptions::default()
        };
        let file = read_file_deferred(&source, true, ReadFileOptions::default(), &defer)
            .map_err(|(_, e)| object_error(e))?;
        Ok(FrameSource { file, source })
    }
    pub fn file(&self) -> &File {
        &self.file
    }
    pub fn source(&self) -> &[u8] {
        &self.source
    }
    pub fn reader(&self, options: FrameReaderOptions) -> Result<FrameReader<'_>, Error> {
        FrameReader::with_source(&self.file, &self.source, options)
    }
}
