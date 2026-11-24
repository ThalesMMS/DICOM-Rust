#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::huffman::{BitReader, HuffTable, extend_difference, receive_extend, skip_marker_fill};
use super::{DecodedImage, Error, Metadata, diagnostic};
use std::rc::Rc;
const MARKER_SOI: u8 = 0xd8;
const MARKER_EOI: u8 = 0xd9;
const MARKER_SOF3: u8 = 0xc3;
const MARKER_DHT: u8 = 0xc4;
const MARKER_SOS: u8 = 0xda;
const MARKER_DRI: u8 = 0xdd;
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Quirks {
    pub(crate) ignore_se: bool,
    pub(crate) lenient_fill_bits: bool,
    pub(crate) category16_extra_bits: bool,
    pub(crate) predictor6_overflow: bool,
}
impl Quirks {
    pub(crate) fn diagnostics(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.ignore_se {
            out.push(diagnostic::SE_IGNORED);
        }
        if self.lenient_fill_bits {
            out.push(diagnostic::FILL_BITS);
        }
        if self.category16_extra_bits {
            out.push(diagnostic::CATEGORY16_EXTRA_BITS);
        }
        if self.predictor6_overflow {
            out.push(diagnostic::PREDICTOR6_OVERFLOW);
        }
        out
    }
}
const QUIRK_ATTEMPTS: [Quirks; 2] = [
    Quirks {
        ignore_se: true,
        lenient_fill_bits: true,
        category16_extra_bits: false,
        predictor6_overflow: false,
    },
    Quirks {
        ignore_se: true,
        lenient_fill_bits: true,
        category16_extra_bits: true,
        predictor6_overflow: false,
    },
];
pub(super) fn decode_frame_with_recovery(
    data: &[u8],
    selection_value: u8,
    metadata: &Metadata,
    base: Quirks,
    allow_quirks: bool,
) -> (Result<DecodedImage, Error>, Quirks) {
    let (result, seen) = decode_frame_state(data, selection_value, metadata, base, true);
    match result {
        Ok(image) => (Ok(image), seen),
        Err(err) if !allow_quirks => (Err(err), Quirks::default()),
        Err(err) => {
            for mut quirks in QUIRK_ATTEMPTS {
                quirks.predictor6_overflow = base.predictor6_overflow;
                let (retried, seen) =
                    decode_frame_state(data, selection_value, metadata, quirks, true);
                if let Ok(image) = retried {
                    return (Ok(image), seen);
                }
            }
            (Err(err), Quirks::default())
        }
    }
}
struct ScanComponent {
    index: usize,
    table: Rc<HuffTable>,
}
#[derive(Default)]
struct FrameHeader {
    precision: usize,
    height: usize,
    width: usize,
    components: Vec<u8>,
}
struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
    frame: FrameHeader,
    image: Option<DecodedImage>,
    component_by_id: [Option<usize>; 256],
    component_scanned: Vec<bool>,
    huffman: [Option<Rc<HuffTable>>; 4],
    restart_interval: usize,
    seen_sof: bool,
    seen_sos: bool,
    scan_count: i64,
    dri_at_scan_count: i64,
    selection_value: u8,
    metadata: &'a Metadata,
    pack_mono: bool,
    quirks: Quirks,
    seen: Quirks,
}
fn decode_frame_state(
    data: &[u8],
    selection_value: u8,
    metadata: &Metadata,
    quirks: Quirks,
    pack_mono: bool,
) -> (Result<DecodedImage, Error>, Quirks) {
    if data.len() < 4 || !data.starts_with(&[0xff, MARKER_SOI]) {
        return (Err(Error::invalid("missing SOI")), Quirks::default());
    }
    let mut d = Decoder {
        data,
        pos: 2,
        frame: FrameHeader::default(),
        image: None,
        component_by_id: [None; 256],
        component_scanned: Vec::new(),
        huffman: Default::default(),
        restart_interval: 0,
        seen_sof: false,
        seen_sos: false,
        scan_count: 0,
        dri_at_scan_count: -1,
        selection_value,
        metadata,
        pack_mono,
        quirks,
        seen: Quirks::default(),
    };
    let result = d.run();
    (result, d.seen)
}
fn all_components_scanned(scanned: &[bool]) -> bool {
    !scanned.is_empty() && scanned.iter().all(|&s| s)
}
impl<'a> Decoder<'a> {
    fn run(&mut self) -> Result<DecodedImage, Error> {
        loop {
            let marker = self.next_marker()?;
            match marker {
                MARKER_SOF3 => {
                    if self.seen_sof || self.seen_sos {
                        return Err(Error::invalid("duplicate or out-of-order SOF3"));
                    }
                    let segment = self.read_segment()?;
                    let frame = parse_sof3_segment(segment)?;
                    let md = self.metadata;
                    if frame.width != md.columns as usize || frame.height != md.rows as usize {
                        return Err(Error::size(format_args!(
                            "got {}x{} want {}x{}",
                            frame.width, frame.height, md.columns, md.rows
                        )));
                    }
                    if frame.precision != md.bits_stored as usize
                        || frame.components.len() != md.samples_per_pixel as usize
                    {
                        return Err(Error::size(
                            "SOF3 precision/components disagree with metadata",
                        ));
                    }
                    validate_frame_allocation(&frame)?;
                    for (index, &id) in frame.components.iter().enumerate() {
                        if let Some(slot) = self.component_by_id.get_mut(usize::from(id)) {
                            *slot = Some(index);
                        }
                    }
                    let n = frame.components.len();
                    self.image = Some(DecodedImage {
                        width: frame.width,
                        height: frame.height,
                        precision: frame.precision,
                        planes: vec![Vec::new(); n],
                        point_transform: vec![0; n],
                        packed_mono: None,
                    });
                    self.component_scanned = vec![false; n];
                    self.frame = frame;
                    self.seen_sof = true;
                }
                MARKER_DHT => {
                    if self.seen_sos && all_components_scanned(&self.component_scanned) {
                        return Err(Error::invalid("DHT after complete scan"));
                    }
                    let segment = self.read_segment()?;
                    let mut parsed: [Option<Rc<HuffTable>>; 4] = Default::default();
                    parse_dht_segment(segment, &mut parsed)?;
                    for (slot, table) in self.huffman.iter_mut().zip(parsed) {
                        if table.is_some() {
                            *slot = table;
                        }
                    }
                }
                MARKER_DRI => {
                    if !self.seen_sof
                        || all_components_scanned(&self.component_scanned)
                        || self.dri_at_scan_count == self.scan_count
                    {
                        return Err(Error::invalid("duplicate or out-of-order DRI"));
                    }
                    let &[hi, lo] = self.read_segment()? else {
                        return Err(Error::invalid("invalid DRI length"));
                    };
                    self.restart_interval = usize::from(u16::from_be_bytes([hi, lo]));
                    self.dri_at_scan_count = self.scan_count;
                }
                MARKER_SOS => {
                    if !self.seen_sof {
                        return Err(Error::invalid("SOS before SOF3"));
                    }
                    let segment = self.read_segment()?;
                    self.decode_scan(segment)?;
                    self.seen_sos = true;
                    self.scan_count = self.scan_count.saturating_add(1);
                }
                MARKER_EOI => {
                    if !self.seen_sos || !all_components_scanned(&self.component_scanned) {
                        return Err(Error::invalid("EOI before all component scans"));
                    }
                    if self.data.len().saturating_sub(self.pos) > 1 {
                        return Err(Error::invalid("trailing data after EOI"));
                    }
                    return self
                        .image
                        .take()
                        .ok_or_else(|| Error::invalid("missing SOF3"));
                }
                0xe0..=0xef | 0xfe => {
                    self.read_segment()?;
                }
                _ => {
                    return Err(Error::invalid(format_args!(
                        "unsupported marker 0xff{marker:02x}"
                    )));
                }
            }
        }
    }
    fn next_marker(&mut self) -> Result<u8, Error> {
        if self.data.get(self.pos) != Some(&0xff) {
            return Err(Error::invalid(format_args!(
                "expected marker at {}",
                self.pos
            )));
        }
        self.pos = skip_marker_fill(self.data, self.pos);
        let Some(&marker) = self.data.get(self.pos).filter(|&&b| b != 0) else {
            return Err(Error::invalid("truncated marker"));
        };
        self.pos = self.pos.saturating_add(1);
        Ok(marker)
    }
    fn read_segment(&mut self) -> Result<&'a [u8], Error> {
        let data = self.data;
        let Some(&[hi, lo]) = data.get(self.pos..self.pos.saturating_add(2)) else {
            return Err(Error::invalid("truncated segment length"));
        };
        let length = usize::from(u16::from_be_bytes([hi, lo]));
        let end = self.pos.saturating_add(length);
        let segment = match data.get(self.pos.saturating_add(2)..end) {
            Some(segment) if length >= 2 => segment,
            _ => return Err(Error::invalid("invalid segment length")),
        };
        self.pos = end;
        Ok(segment)
    }
    fn decode_scan(&mut self, segment: &[u8]) -> Result<(), Error> {
        let (true, Some((&count, rest))) = (segment.len() >= 6, segment.split_first()) else {
            return Err(Error::invalid("short SOS"));
        };
        let count = usize::from(count);
        if count != 1 && count != self.frame.components.len() {
            return Err(Error::unsupported(format_args!("scan components={count}")));
        }
        if rest.len() != count.saturating_mul(2).saturating_add(3) {
            return Err(Error::invalid("invalid SOS length"));
        }
        let (selectors, tail) = rest.split_at(count.saturating_mul(2).min(rest.len()));
        let &[predictor, se, ahal] = tail else {
            return Err(Error::invalid("invalid SOS length"));
        };
        let mut components = Vec::with_capacity(count);
        let mut selected = vec![false; self.frame.components.len()];
        for &[id, selector] in selectors.as_chunks::<2>().0 {
            let known = self.component_by_id.get(usize::from(id)).copied().flatten();
            let component_index = match known {
                Some(i)
                    if selected.get(i) == Some(&false)
                        && self.component_scanned.get(i) == Some(&false) =>
                {
                    i
                }
                _ => return Err(Error::invalid("invalid or duplicate SOS component")),
            };
            if selector & 0x0f != 0 {
                return Err(Error::unsupported("lossless AC table selector is nonzero"));
            }
            let table_id = (selector >> 4) as usize;
            let table = match self.huffman.get(table_id).and_then(|t| t.clone()) {
                Some(t) => t,
                None => {
                    return Err(Error::invalid(format_args!(
                        "missing Huffman table {table_id}"
                    )));
                }
            };
            if let Some(s) = selected.get_mut(component_index) {
                *s = true;
            }
            components.push(ScanComponent {
                index: component_index,
                table,
            });
        }
        let predictor = usize::from(predictor);
        if !(1..=7).contains(&predictor) {
            return Err(Error::unsupported(format_args!("predictor={predictor}")));
        }
        if self.selection_value != 0 && predictor != self.selection_value as usize {
            return Err(Error::unsupported(format_args!(
                "predictor={predictor} selection value={}",
                self.selection_value
            )));
        }
        if se != 0 && self.quirks.ignore_se {
            self.seen.ignore_se = true;
        } else if se != 0 || ahal >> 4 != 0 {
            return Err(Error::unsupported(
                "unsupported SOS spectral/successive parameters",
            ));
        }
        let point_transform = (ahal & 0x0f) as usize;
        if point_transform >= self.frame.precision {
            return Err(Error::invalid(format_args!(
                "point transform={point_transform} precision={}",
                self.frame.precision
            )));
        }
        let mut reader = BitReader::new(self.data, self.pos, self.quirks.lenient_fill_bits);
        let coded_precision = self
            .frame
            .precision
            .saturating_sub(point_transform)
            .clamp(1, 16);
        let default_prediction = 1i32 << coded_precision.wrapping_sub(1);
        let modulus_mask = ((1u32 << coded_precision).wrapping_sub(1)) as i32;
        let only = match components.as_slice() {
            [only] => Some(only.table.clone()),
            _ => None,
        };
        let result = if let Some(table) = only.filter(|_| {
            self.pack_mono
                && self.frame.components.len() == 1
                && predictor == 1
                && !self.quirks.category16_extra_bits
        }) {
            self.decode_packed_sv1(
                &mut reader,
                &table,
                default_prediction,
                modulus_mask,
                point_transform as u32,
            )
        } else {
            self.decode_plane_scan(
                &mut reader,
                &components,
                predictor,
                default_prediction,
                modulus_mask,
            )
        };
        let result = result.and_then(|()| reader.finish_entropy());
        self.seen.lenient_fill_bits |= reader.fill_quirk;
        self.pos = result?;
        let image = self
            .image
            .as_mut()
            .ok_or_else(|| Error::invalid("missing SOF3"))?;
        for component in &components {
            if let Some(s) = self.component_scanned.get_mut(component.index) {
                *s = true;
            }
            if let Some(t) = image.point_transform.get_mut(component.index) {
                *t = point_transform as u32;
            }
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn decode_plane_scan(
        &mut self,
        reader: &mut BitReader,
        components: &[ScanComponent],
        predictor: usize,
        default_prediction: i32,
        modulus_mask: i32,
    ) -> Result<(), Error> {
        let width = self.frame.width;
        let height = self.frame.height;
        let pixels = width * height;
        let restart_interval = self.restart_interval;
        let quirks = self.quirks;
        let image = self
            .image
            .as_mut()
            .ok_or_else(|| Error::invalid("missing SOF3"))?;
        for component in components {
            image.planes[component.index] = vec![0; pixels];
        }
        if predictor == 1 && restart_interval == 0 && !quirks.category16_extra_bits {
            return decode_predictor1(
                reader,
                components,
                &mut image.planes,
                width,
                height,
                default_prediction,
                modulus_mask,
            );
        }
        let mut restart_count = 0;
        let mut restart_start = 0;
        let mut restart_marker = 0xd0u8;
        let mut pixel_index = 0;
        for y in 0..height {
            for x in 0..width {
                for component in components {
                    let difference = if quirks.category16_extra_bits {
                        let symbol = component.table.decode_symbol(reader)?;
                        if symbol == 16 {
                            let bits = reader.read_bits(16)?;
                            self.seen.category16_extra_bits = true;
                            extend_difference(bits as i32, 16)
                        } else {
                            receive_extend(reader, symbol as i64)?
                        }
                    } else {
                        component.table.decode_difference(reader)?
                    };
                    let plane = &mut image.planes[component.index];
                    let mut prediction = predict(plane, x, y, width, predictor, default_prediction);
                    if predictor == 6 && quirks.predictor6_overflow && x > 0 && y > 0 {
                        let overflowed = predict6_overflow(plane, x, y, width);
                        if (overflowed ^ prediction) & modulus_mask != 0 {
                            self.seen.predictor6_overflow = true;
                        }
                        prediction = overflowed;
                    }
                    if pixel_index == restart_start {
                        prediction = default_prediction;
                    }
                    plane[pixel_index] = prediction.wrapping_add(difference) & modulus_mask;
                }
                if restart_interval > 0 {
                    restart_count += 1;
                    if restart_count == restart_interval && pixel_index != pixels - 1 {
                        reader.align_to_restart(restart_marker)?;
                        restart_marker = 0xd0 + ((restart_marker - 0xd0 + 1) & 7);
                        restart_count = 0;
                        restart_start = pixel_index + 1;
                    }
                }
                pixel_index += 1;
            }
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn decode_packed_sv1(
        &mut self,
        reader: &mut BitReader,
        table: &HuffTable,
        initial: i32,
        mask: i32,
        point_transform: u32,
    ) -> Result<(), Error> {
        let sample_bytes = (self.metadata.bits_allocated / 8) as usize;
        let width = self.frame.width;
        let stride = width * sample_bytes;
        let mut out = vec![0u8; stride * self.frame.height];
        let mut above_first = initial;
        let mut marker = 0xd0u8;
        let mut remaining = self.restart_interval;
        for row in out.chunks_mut(stride) {
            let mut prediction = above_first;
            let mut x = 0;
            while x < width {
                let mut end = width;
                if self.restart_interval > 0 {
                    if remaining == 0 {
                        reader.align_to_restart(marker)?;
                        marker = 0xd0 + ((marker - 0xd0 + 1) & 7);
                        remaining = self.restart_interval;
                        prediction = initial;
                    }
                    end = end.min(x + remaining);
                    remaining -= end - x;
                }
                let dst = &mut row[x * sample_bytes..end * sample_bytes];
                prediction = if sample_bytes == 1 {
                    decode_packed_run8(reader, table, dst, prediction, mask, point_transform)?
                } else {
                    decode_packed_run16(reader, table, dst, prediction, mask, point_transform)?
                };
                x = end;
            }
            above_first = if sample_bytes == 1 {
                (row[0] as i32) >> point_transform
            } else {
                (u16::from_le_bytes([row[0], row[1]]) as i32) >> point_transform
            };
        }
        self.image
            .as_mut()
            .ok_or_else(|| Error::invalid("missing SOF3"))?
            .packed_mono = Some(out);
        Ok(())
    }
}
fn decode_packed_run8(
    reader: &mut BitReader,
    table: &HuffTable,
    dst: &mut [u8],
    mut prediction: i32,
    mask: i32,
    shift: u32,
) -> Result<i32, Error> {
    for o in dst {
        let difference = table.decode_difference(reader)?;
        prediction = prediction.wrapping_add(difference) & mask;
        *o = (prediction << shift) as u8;
    }
    Ok(prediction)
}
fn decode_packed_run16(
    reader: &mut BitReader,
    table: &HuffTable,
    dst: &mut [u8],
    mut prediction: i32,
    mask: i32,
    shift: u32,
) -> Result<i32, Error> {
    for o in dst.as_chunks_mut::<2>().0 {
        let difference = table.decode_difference(reader)?;
        prediction = prediction.wrapping_add(difference) & mask;
        o.copy_from_slice(&((prediction << shift) as u16).to_le_bytes());
    }
    Ok(prediction)
}
#[allow(clippy::needless_range_loop)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_predictor1(
    reader: &mut BitReader,
    components: &[ScanComponent],
    planes: &mut [Vec<i32>],
    width: usize,
    height: usize,
    initial: i32,
    mask: i32,
) -> Result<(), Error> {
    let mut predictions = [0i32; 3];
    for y in 0..height {
        let start = y * width;
        for (c, component) in components.iter().enumerate() {
            predictions[c] = if y != 0 {
                planes[component.index][start - width]
            } else {
                initial
            };
        }
        for i in start..start + width {
            for (c, component) in components.iter().enumerate() {
                let difference = component.table.decode_difference(reader)?;
                predictions[c] = predictions[c].wrapping_add(difference) & mask;
                planes[component.index][i] = predictions[c];
            }
        }
    }
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn predict(
    samples: &[i32],
    x: usize,
    y: usize,
    width: usize,
    predictor: usize,
    default_prediction: i32,
) -> i32 {
    if x == 0 && y == 0 {
        return default_prediction;
    }
    if y == 0 {
        return samples[y * width + x - 1];
    }
    if x == 0 {
        return samples[(y - 1) * width + x];
    }
    let a = samples[y * width + x - 1];
    if predictor == 1 {
        return a;
    }
    let b = samples[(y - 1) * width + x];
    if predictor == 2 {
        return b;
    }
    let c = samples[(y - 1) * width + x - 1];
    predict_neighbors(a, b, c, predictor)
}
pub(super) fn predict_neighbors(a: i32, b: i32, c: i32, predictor: usize) -> i32 {
    match predictor {
        2 => b,
        3 => c,
        4 => a.wrapping_add(b).wrapping_sub(c),
        5 => a.wrapping_add(b.wrapping_sub(c) >> 1),
        6 => b.wrapping_add(a.wrapping_sub(c) >> 1),
        7 => a.wrapping_add(b) / 2,
        _ => a,
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn predict6_overflow(samples: &[i32], x: usize, y: usize, width: usize) -> i32 {
    let a = samples[y * width + x - 1] as i16 as i32;
    let b = samples[(y - 1) * width + x];
    let c = samples[(y - 1) * width + x - 1] as i16 as i32;
    b + ((a - c) >> 1)
}
#[allow(clippy::arithmetic_side_effects)]
fn validate_frame_allocation(frame: &FrameHeader) -> Result<(), Error> {
    let pixels = frame.width as u64 * frame.height as u64;
    let components = frame.components.len() as u64;
    if pixels == 0 || components == 0 || pixels > u64::MAX / components {
        return Err(Error::invalid("decoded frame size overflow"));
    }
    let samples = pixels * components;
    let max_int = i64::MAX as u64;
    if samples > max_int / 4 || samples * 4 > super::MAX_REQUEST_BYTES {
        return Err(Error::invalid("decoded frame exceeds resource limit"));
    }
    Ok(())
}
fn parse_sof3_segment(segment: &[u8]) -> Result<FrameHeader, Error> {
    if segment.len() < 6 {
        return Err(Error::invalid("short SOF3"));
    }
    let &[precision, h0, h1, w0, w1, components, ..] = segment else {
        return Err(Error::invalid("short SOF3"));
    };
    let precision = usize::from(precision);
    let height = usize::from(u16::from_be_bytes([h0, h1]));
    let width = usize::from(u16::from_be_bytes([w0, w1]));
    let components = usize::from(components);
    if !(2..=16).contains(&precision) || width == 0 || height == 0 {
        return Err(Error::invalid("invalid SOF3 precision or dimensions"));
    }
    if components != 1 && components != 3 {
        return Err(Error::unsupported(format_args!(
            "SOF3 components={components}"
        )));
    }
    if segment.len() != components.saturating_mul(3).saturating_add(6) {
        return Err(Error::invalid("invalid SOF3 component length"));
    }
    let mut frame = FrameHeader {
        precision,
        height,
        width,
        components: Vec::with_capacity(components),
    };
    let mut seen = [false; 256];
    let (chunks, _) = segment.as_chunks::<3>();
    for (index, &[id, sampling, quant]) in chunks.get(2..).unwrap_or_default().iter().enumerate() {
        let repeated = seen.get(usize::from(id)).copied().unwrap_or(true);
        if sampling != 0x11 && !repeated && quant == 0 {
            return Err(Error::unsupported(format_args!(
                "SOF3 component {index} sampling factors {}x{} (subsampled lossless JPEG is not supported)",
                sampling >> 4,
                sampling & 0x0f
            )));
        }
        if repeated || sampling != 0x11 || quant != 0 {
            return Err(Error::unsupported("invalid SOF3 component parameters"));
        }
        if let Some(s) = seen.get_mut(usize::from(id)) {
            *s = true;
        }
        frame.components.push(id);
    }
    Ok(frame)
}
fn parse_dht_segment(segment: &[u8], tables: &mut [Option<Rc<HuffTable>>; 4]) -> Result<(), Error> {
    let mut defined = [false; 4];
    let mut rest = segment;
    while !rest.is_empty() {
        let Some((&[selector, ref bits @ ..], after)) = rest.split_first_chunk::<17>() else {
            return Err(Error::invalid("truncated DHT"));
        };
        rest = after;
        if selector >> 4 != 0 || selector & 0x0f > 3 {
            return Err(Error::invalid("invalid lossless Huffman table selector"));
        }
        let table_id = usize::from(selector & 0x0f);
        match defined.get_mut(table_id) {
            Some(d) if !*d => *d = true,
            _ => {
                return Err(Error::invalid(format_args!(
                    "duplicate Huffman table {table_id} in one DHT segment"
                )));
            }
        }
        let mut counts = [0usize; 17];
        let mut total = 0usize;
        let mut available: i64 = 1;
        for (count_slot, &count) in counts.iter_mut().skip(1).zip(bits) {
            let count = usize::from(count);
            *count_slot = count;
            total = total.saturating_add(count);
            available = available.saturating_mul(2).saturating_sub(count as i64);
            if available < 0 {
                return Err(Error::invalid("oversubscribed Huffman table"));
            }
        }
        let Some((values, after)) = rest.split_at_checked(total) else {
            return Err(Error::invalid("invalid Huffman table"));
        };
        if available == 0 || total == 0 || total > 256 {
            return Err(Error::invalid("invalid Huffman table"));
        }
        let values = values.to_vec();
        rest = after;
        if let Some(&value) = values.iter().find(|&&v| v > 16) {
            return Err(Error::invalid(format_args!(
                "invalid lossless Huffman symbol {value}"
            )));
        }
        if let Some(slot) = tables.get_mut(table_id) {
            *slot = Some(Rc::new(HuffTable::build(&counts, values)));
        }
    }
    if segment.is_empty() {
        return Err(Error::invalid("empty DHT"));
    }
    Ok(())
}
