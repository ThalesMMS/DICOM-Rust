use super::{CoreError, Result};
use std::collections::HashMap;
const MARKER_SOC: u16 = 0xFF4F;
const MARKER_CAP: u16 = 0xFF50;
const MARKER_SIZ: u16 = 0xFF51;
const MARKER_COD: u16 = 0xFF52;
const MARKER_COC: u16 = 0xFF53;
const MARKER_TLM: u16 = 0xFF55;
const MARKER_PLM: u16 = 0xFF57;
const MARKER_PLT: u16 = 0xFF58;
const MARKER_CPF: u16 = 0xFF59;
const MARKER_QCD: u16 = 0xFF5C;
const MARKER_QCC: u16 = 0xFF5D;
const MARKER_RGN: u16 = 0xFF5E;
const MARKER_POC: u16 = 0xFF5F;
const MARKER_PPM: u16 = 0xFF60;
const MARKER_PPT: u16 = 0xFF61;
const MARKER_CRG: u16 = 0xFF63;
const MARKER_COM: u16 = 0xFF64;
const MARKER_SOT: u16 = 0xFF90;
const MARKER_SOD: u16 = 0xFF93;
const MARKER_EOC: u16 = 0xFFD9;
pub(crate) const PROGRESSION_RLCP: i64 = 1;
pub(crate) const PROGRESSION_RPCL: i64 = 2;
pub(crate) const PROGRESSION_PCRL: i64 = 3;
pub(crate) const PROGRESSION_CPRL: i64 = 4;
pub const BLOCK_BYPASS: u8 = 0x01;
pub const BLOCK_RESET: u8 = 0x02;
pub const BLOCK_TERM_ALL: u8 = 0x04;
pub const BLOCK_VSC: u8 = 0x08;
pub const BLOCK_SEG_SYM: u8 = 0x20;
pub const BLOCK_HT: u8 = 0x40;
pub const BLOCK_MIXED: u8 = 0x80;
pub const WAVELET_97: i64 = 0;
pub const WAVELET_53: i64 = 1;
pub const QUANT_NONE: i64 = 0;
pub const QUANT_DERIVED: i64 = 1;
pub const QUANT_EXPOUNDED: i64 = 2;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgressionChange {
    pub resolution_start: i64,
    pub resolution_end: i64,
    pub component_start: i64,
    pub component_end: i64,
    pub layer_end: i64,
    pub order: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Component {
    pub signed: bool,
    pub depth: i64,
    pub dx: i64,
    pub dy: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodingStyle {
    pub progression: i64,
    pub layers: i64,
    pub mct: bool,
    pub levels: i64,
    pub code_block_width: i64,
    pub code_block_height: i64,
    pub block_style: u8,
    pub wavelet: i64,
    pub use_precincts: bool,
    pub precincts: Vec<u8>,
    pub sop: bool,
    pub eph: bool,
}
impl CodingStyle {
    pub(crate) fn precinct_exponents(&self, resolution: i64) -> (i64, i64) {
        if !self.use_precincts || resolution < 0 || resolution >= self.precincts.len() as i64 {
            return (15, 15);
        }
        let p = self
            .precincts
            .get(resolution as usize)
            .copied()
            .unwrap_or(0xFF);
        ((p & 0x0F) as i64, (p >> 4) as i64)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Quantization {
    pub style: i64,
    pub guard_bits: i64,
    pub exponents: Vec<i64>,
    pub mantissas: Vec<i64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TilePart {
    pub tile: i64,
    pub index: i64,
    pub data: Vec<u8>,
    pub headers: Vec<u8>,
    pub packed_headers: bool,
}
type TcKey = (i64, i64);
#[derive(Debug, Clone, Default)]
pub struct Codestream {
    pub width: u32,
    pub height: u32,
    pub x_offset: u32,
    pub y_offset: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    pub tile_x_offset: u32,
    pub tile_y_offset: u32,
    pub components: Vec<Component>,
    pub coding: CodingStyle,
    pub component_coding: HashMap<i64, CodingStyle>,
    pub quant: Quantization,
    pub component_quant: HashMap<i64, Quantization>,
    pub tile_coding: HashMap<i64, CodingStyle>,
    pub tile_component_coding: HashMap<TcKey, CodingStyle>,
    pub tile_quant: HashMap<i64, Quantization>,
    pub tile_component_quant: HashMap<TcKey, Quantization>,
    pub roi_shift: HashMap<i64, i64>,
    pub tile_roi_shift: HashMap<TcKey, i64>,
    pub poc: Vec<ProgressionChange>,
    pub tile_poc: HashMap<i64, Vec<ProgressionChange>>,
    pub tile_parts: Vec<TilePart>,
    ppm: HashMap<i64, Vec<u8>>,
}
pub(super) fn ceil_div_u32(a: u32, b: u32) -> i64 {
    if b == 0 {
        return 0;
    }
    (a.wrapping_add(b)
        .wrapping_sub(1)
        .checked_div(b)
        .unwrap_or(0)) as i64
}
impl Codestream {
    pub(crate) fn progression_changes_for_tile(&self, tile: i64) -> &[ProgressionChange] {
        if let Some(changes) = self.tile_poc.get(&tile) {
            return changes;
        }
        &self.poc
    }
    pub(crate) fn num_tiles(&self) -> i64 {
        let across = ceil_div_u32(self.width.wrapping_sub(self.tile_x_offset), self.tile_width);
        let down = ceil_div_u32(
            self.height.wrapping_sub(self.tile_y_offset),
            self.tile_height,
        );
        across.wrapping_mul(down)
    }
    pub(crate) fn coding_for_tile(&self, tile: i64, component: i64) -> CodingStyle {
        let cod = self.tile_coding.get(&tile).unwrap_or(&self.coding);
        let mut style = match self.tile_component_coding.get(&(tile, component)) {
            Some(s) => s.clone(),
            None => match self.tile_coding.get(&tile) {
                Some(s) => s.clone(),
                None => self.coding_for(component).clone(),
            },
        };
        style.progression = cod.progression;
        style.layers = cod.layers;
        style.mct = cod.mct;
        style.sop = cod.sop;
        style.eph = cod.eph;
        style
    }
    pub(crate) fn quant_for_tile(&self, tile: i64, component: i64) -> &Quantization {
        if let Some(q) = self.tile_component_quant.get(&(tile, component)) {
            return q;
        }
        if let Some(q) = self.tile_quant.get(&tile) {
            return q;
        }
        self.quant_for(component)
    }
    pub(crate) fn roi_shift_for_tile(&self, tile: i64, component: i64) -> i64 {
        if let Some(&s) = self.tile_roi_shift.get(&(tile, component)) {
            return s;
        }
        self.roi_shift.get(&component).copied().unwrap_or(0)
    }
    pub(crate) fn coding_for(&self, component: i64) -> &CodingStyle {
        self.component_coding
            .get(&component)
            .unwrap_or(&self.coding)
    }
    pub(crate) fn quant_for(&self, component: i64) -> &Quantization {
        self.component_quant.get(&component).unwrap_or(&self.quant)
    }
}
fn be16(b: &[u8]) -> i64 {
    b.first_chunk::<2>()
        .map_or(0, |&x| u16::from_be_bytes(x) as i64)
}
fn be32(b: &[u8]) -> u32 {
    b.first_chunk::<4>().map_or(0, |&x| u32::from_be_bytes(x))
}
fn tail(b: &[u8], at: usize) -> &[u8] {
    b.get(at..).unwrap_or(&[])
}
fn byte(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}
fn range(b: &[u8], start: usize, end: usize) -> &[u8] {
    b.get(start..end).unwrap_or(&[])
}
fn plain(text: impl Into<String>) -> CoreError {
    CoreError::Plain(text.into())
}
#[doc(hidden)]
pub fn parse_codestream(data: &[u8]) -> Result<Codestream> {
    let data = match codestream_from_jp2(data)? {
        Some(b) => b,
        None => data,
    };
    if data.len() < 4 || be16(data) != MARKER_SOC as i64 {
        return Err(plain(
            "jpeg2000: not a codestream: it does not begin with SOC",
        ));
    }
    let mut c = Codestream::default();
    let mut pos = 2usize;
    let (mut seen_siz, mut seen_cod, mut seen_qcd) = (false, false, false);
    while pos.saturating_add(1) < data.len() {
        let marker = be16(tail(data, pos)) as u16;
        let at = pos;
        pos = pos.saturating_add(2);
        match marker {
            MARKER_EOC => {
                validate(&mut c, seen_siz, seen_cod, seen_qcd)?;
                return Ok(c);
            }
            MARKER_SOD => {
                return Err(plain(format!(
                    "jpeg2000: SOD outside a tile-part at offset {at}"
                )));
            }
            MARKER_SOT => {
                pos = parse_tile_part(&mut c, data, pos)?;
                continue;
            }
            m if is_segmentless(m) => continue,
            _ => {}
        }
        if pos.saturating_add(2) > data.len() {
            return Err(plain(format!(
                "jpeg2000: truncated segment length for marker {marker:04X}"
            )));
        }
        let length = be16(tail(data, pos)) as usize;
        if length < 2 || pos.saturating_add(length) > data.len() {
            return Err(plain(format!(
                "jpeg2000: marker {marker:04X} declares {length} bytes, {} remain",
                data.len().saturating_sub(pos)
            )));
        }
        let segment = range(data, pos.saturating_add(2), pos.saturating_add(length));
        match marker {
            MARKER_SIZ => {
                parse_siz(&mut c, segment)?;
                seen_siz = true;
            }
            MARKER_COD => {
                c.coding = parse_cod(segment)?;
                seen_cod = true;
            }
            MARKER_COC => parse_coc(&mut c, segment)?,
            MARKER_QCD => {
                c.quant = parse_quantization(segment)?;
                seen_qcd = true;
            }
            MARKER_QCC => parse_qcc(&mut c, segment)?,
            MARKER_COM | MARKER_TLM | MARKER_PLM | MARKER_PLT | MARKER_CRG | MARKER_CPF => {}
            MARKER_CAP => parse_cap(segment)?,
            MARKER_PPM => collect_packed(&mut c.ppm, segment, "PPM")?,
            MARKER_POC => {
                let changes = parse_poc(&c, segment)?;
                c.poc.extend(changes);
            }
            MARKER_PPT => return Err(plain("jpeg2000: PPT in the main header")),
            MARKER_RGN => {
                let (component, shift) = parse_rgn(&c, segment)?;
                c.roi_shift.insert(component, shift);
            }
            _ => {
                return Err(plain(format!(
                    "jpeg2000: unexpected marker {marker:04X} at offset {at}"
                )));
            }
        }
        pos = pos.saturating_add(length);
    }
    validate(&mut c, seen_siz, seen_cod, seen_qcd)?;
    Ok(c)
}
fn is_segmentless(marker: u16) -> bool {
    (0xFF30..=0xFF3F).contains(&marker)
}
fn parse_cap(s: &[u8]) -> Result<()> {
    if s.len() < 4 {
        return Err(plain(format!("jpeg2000: CAP is {} bytes", s.len())));
    }
    const PART15: u32 = 1 << (32 - 15);
    let pcap = be32(s);
    if pcap & !PART15 != 0 {
        return Err(CoreError::Unsupported(format!(
            "extended capabilities 0x{pcap:08X}"
        )));
    }
    let parts = pcap.count_ones() as usize;
    if s.len() != parts.saturating_mul(2).saturating_add(4) {
        return Err(plain(format!(
            "jpeg2000: CAP of {} bytes for {parts} parts",
            s.len()
        )));
    }
    Ok(())
}
fn codestream_from_jp2(data: &[u8]) -> Result<Option<&[u8]>> {
    const JP2_SIGNATURE: u32 = 0x6A50_2020;
    if data.len() < 12 || be32(tail(data, 4)) != JP2_SIGNATURE {
        return Ok(None);
    }
    let mut pos = 0usize;
    while pos.saturating_add(8) <= data.len() {
        let mut length = be32(tail(data, pos)) as i64;
        let box_type = be32(tail(data, pos.saturating_add(4)));
        let mut body = pos.saturating_add(8);
        let remain = data.len().saturating_sub(pos) as i64;
        if length == 1 {
            let Some(&large) = tail(data, body).first_chunk::<8>() else {
                return Err(plain("jpeg2000: truncated JP2 box length"));
            };
            length = u64::from_be_bytes(large) as i64;
            body = pos.saturating_add(16);
            if length < 16 || length > remain {
                return Err(plain(format!(
                    "jpeg2000: JP2 box declares {length} bytes, {remain} remain"
                )));
            }
        } else if length == 0 {
            length = remain;
        } else if length < 8 || length > remain {
            return Err(plain(format!(
                "jpeg2000: JP2 box declares {length} bytes, {remain} remain"
            )));
        }
        let end = pos.saturating_add(length as usize);
        if box_type == 0x6A70_3263 {
            return Ok(Some(range(data, body, end)));
        }
        pos = end;
    }
    Err(plain("jpeg2000: JP2 container holds no codestream box"))
}
fn parse_siz(c: &mut Codestream, s: &[u8]) -> Result<()> {
    if s.len() < 36 {
        return Err(plain(format!(
            "jpeg2000: SIZ is {} bytes, want at least 36",
            s.len()
        )));
    }
    c.width = be32(tail(s, 2));
    c.height = be32(tail(s, 6));
    c.x_offset = be32(tail(s, 10));
    c.y_offset = be32(tail(s, 14));
    c.tile_width = be32(tail(s, 18));
    c.tile_height = be32(tail(s, 22));
    c.tile_x_offset = be32(tail(s, 26));
    c.tile_y_offset = be32(tail(s, 30));
    let count = be16(tail(s, 34)) as usize;
    if c.width <= c.x_offset || c.height <= c.y_offset {
        return Err(plain(format!(
            "jpeg2000: SIZ image is {}x{} with origin ({},{})",
            c.width, c.height, c.x_offset, c.y_offset
        )));
    }
    if c.tile_width == 0 || c.tile_height == 0 {
        return Err(plain("jpeg2000: SIZ declares a zero-sized tile"));
    }
    let (entries, _) = tail(s, 36).as_chunks::<3>();
    if count == 0 || entries.len() < count {
        return Err(plain(format!(
            "jpeg2000: SIZ declares {count} components, segment holds {} bytes",
            s.len()
        )));
    }
    c.components = Vec::with_capacity(count);
    for (i, &[ssiz, dx, dy]) in entries.iter().take(count).enumerate() {
        let comp = Component {
            signed: ssiz & 0x80 != 0,
            depth: i64::from(ssiz & 0x7F).saturating_add(1),
            dx: dx as i64,
            dy: dy as i64,
        };
        if comp.depth > 17 {
            return Err(CoreError::Unsupported(format!(
                "{}-bit components",
                comp.depth
            )));
        }
        if comp.dx == 0 || comp.dy == 0 {
            return Err(plain(format!(
                "jpeg2000: SIZ gives component {i} a zero sample separation"
            )));
        }
        c.components.push(comp);
    }
    Ok(())
}
fn parse_cod(s: &[u8]) -> Result<CodingStyle> {
    if s.len() < 10 {
        return Err(plain(format!(
            "jpeg2000: COD is {} bytes, want at least 10",
            s.len()
        )));
    }
    let scod = byte(s, 0);
    let mut style = CodingStyle {
        use_precincts: scod & 0x01 != 0,
        sop: scod & 0x02 != 0,
        eph: scod & 0x04 != 0,
        progression: byte(s, 1) as i64,
        layers: be16(tail(s, 2)),
        mct: byte(s, 4) != 0,
        ..CodingStyle::default()
    };
    parse_coding_parameters(&mut style, tail(s, 5))?;
    if style.layers == 0 {
        return Err(plain("jpeg2000: COD declares zero layers"));
    }
    if style.progression > PROGRESSION_CPRL {
        return Err(plain(format!(
            "jpeg2000: COD declares progression order {}",
            style.progression
        )));
    }
    Ok(style)
}
fn parse_coc(c: &mut Codestream, s: &[u8]) -> Result<()> {
    if c.components.is_empty() {
        return Err(plain("jpeg2000: COC before SIZ"));
    }
    let (index, rest) = component_index(c, s)?;
    if rest.len() < 6 {
        return Err(plain("jpeg2000: COC is too short"));
    }
    let mut style = c.coding.clone();
    style.use_precincts = byte(rest, 0) & 0x01 != 0;
    parse_coding_parameters(&mut style, tail(rest, 1))?;
    c.component_coding.insert(index, style);
    Ok(())
}
fn parse_tile_coc(c: &mut Codestream, tile: i64, s: &[u8]) -> Result<()> {
    let (index, rest) = component_index(c, s)?;
    if rest.len() < 6 {
        return Err(plain(format!("jpeg2000: COC in tile {tile} is too short")));
    }
    let mut style = match c.tile_coding.get(&tile) {
        Some(t) => t.clone(),
        None => c.coding_for(index).clone(),
    };
    style.use_precincts = byte(rest, 0) & 0x01 != 0;
    parse_coding_parameters(&mut style, tail(rest, 1))?;
    c.tile_component_coding.insert((tile, index), style);
    Ok(())
}
fn parse_tile_qcc(c: &mut Codestream, tile: i64, s: &[u8]) -> Result<()> {
    let (index, rest) = component_index(c, s)?;
    let q = parse_quantization(rest)?;
    c.tile_component_quant.insert((tile, index), q);
    Ok(())
}
fn parse_coding_parameters(style: &mut CodingStyle, s: &[u8]) -> Result<()> {
    if s.len() < 5 {
        return Err(plain(format!(
            "jpeg2000: coding parameters are {} bytes, want 5",
            s.len()
        )));
    }
    let Some(&[levels, xcb, ycb, block_style, wavelet]) = s.first_chunk::<5>() else {
        return Err(plain("jpeg2000: coding parameters are short"));
    };
    style.levels = levels as i64;
    style.code_block_width = 1i64 << (xcb & 0x0F).saturating_add(2);
    style.code_block_height = 1i64 << (ycb & 0x0F).saturating_add(2);
    style.block_style = block_style;
    style.wavelet = wavelet as i64;
    style.precincts = Vec::new();
    if style.levels > 32 {
        return Err(plain(format!(
            "jpeg2000: {} decomposition levels",
            style.levels
        )));
    } else if style.code_block_width > 1024
        || style.code_block_height > 1024
        || style
            .code_block_width
            .saturating_mul(style.code_block_height)
            > 4096
    {
        return Err(plain(format!(
            "jpeg2000: code-block {}x{} exceeds 4096 samples",
            style.code_block_width, style.code_block_height
        )));
    } else if (style.block_style & BLOCK_MIXED != 0 && style.block_style & BLOCK_HT == 0)
        || (style.block_style & BLOCK_HT != 0
            && style.block_style & !(BLOCK_HT | BLOCK_MIXED | BLOCK_VSC) != 0)
    {
        return Err(CoreError::Unsupported(format!(
            "code-block style 0x{:02X}",
            style.block_style
        )));
    } else if style.wavelet != WAVELET_53 && style.wavelet != WAVELET_97 {
        return Err(CoreError::Unsupported(format!(
            "wavelet transform {}",
            style.wavelet
        )));
    }
    if style.use_precincts {
        let sizes = tail(s, 5);
        let n = (style.levels as usize).saturating_add(1);
        let Some(sizes) = sizes.get(..n) else {
            return Err(plain(format!(
                "jpeg2000: {} precinct sizes for {n} resolutions",
                sizes.len()
            )));
        };
        style.precincts = sizes.to_vec();
        for (r, &size) in style.precincts.iter().enumerate() {
            if r > 0 && (size & 0x0F == 0 || size >> 4 == 0) {
                return Err(plain(format!(
                    "jpeg2000: resolution {r} has a precinct exponent of 0"
                )));
            }
        }
    }
    Ok(())
}
fn parse_poc(c: &Codestream, mut s: &[u8]) -> Result<Vec<ProgressionChange>> {
    if c.components.is_empty() {
        return Err(plain("jpeg2000: POC before SIZ"));
    }
    let wide = c.components.len() >= 257;
    let entry = if wide { 9 } else { 7 };
    if s.is_empty() || !s.len().is_multiple_of(entry) {
        return Err(plain(format!("jpeg2000: POC of {} bytes", s.len())));
    }
    let mut out = Vec::new();
    while !s.is_empty() {
        let mut change = ProgressionChange {
            resolution_start: byte(s, 0) as i64,
            ..ProgressionChange::default()
        };
        let mut rest = tail(s, 1);
        if wide {
            change.component_start = be16(rest);
            rest = tail(rest, 2);
        } else {
            change.component_start = byte(rest, 0) as i64;
            rest = tail(rest, 1);
        }
        change.layer_end = be16(rest);
        change.resolution_end = byte(rest, 2) as i64;
        rest = tail(rest, 3);
        if wide {
            change.component_end = be16(rest);
            rest = tail(rest, 2);
            if change.component_end == 0 {
                change.component_end = 16384;
            }
        } else {
            change.component_end = byte(rest, 0) as i64;
            rest = tail(rest, 1);
            if change.component_end == 0 {
                change.component_end = 256;
            }
        }
        change.order = byte(rest, 0) as i64;
        if change.order > PROGRESSION_CPRL {
            return Err(plain(format!(
                "jpeg2000: POC progression order {}",
                change.order
            )));
        }
        out.push(change);
        s = tail(s, entry);
    }
    Ok(out)
}
fn parse_rgn(c: &Codestream, s: &[u8]) -> Result<(i64, i64)> {
    if c.components.is_empty() {
        return Err(plain("jpeg2000: RGN before SIZ"));
    }
    let (index, rest) = component_index(c, s)?;
    let &[style, shift] = rest else {
        return Err(plain(format!(
            "jpeg2000: RGN carries {} bytes after its component, want 2",
            rest.len()
        )));
    };
    if index >= c.components.len() as i64 {
        return Err(plain(format!(
            "jpeg2000: RGN names component {index} of {}",
            c.components.len()
        )));
    }
    if style != 0 {
        return Err(CoreError::Unsupported(format!(
            "region of interest style {style}"
        )));
    }
    Ok((index, shift as i64))
}
fn collect_packed(into: &mut HashMap<i64, Vec<u8>>, s: &[u8], name: &str) -> Result<()> {
    let Some((&first, body)) = s.split_first() else {
        return Err(plain(format!("jpeg2000: empty {name} segment")));
    };
    let index = first as i64;
    if into.contains_key(&index) {
        return Err(plain(format!(
            "jpeg2000: two {name} segments with index {index}"
        )));
    }
    into.insert(index, body.to_vec());
    Ok(())
}
fn join_packed(parts: &HashMap<i64, Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..256 {
        if let Some(p) = parts.get(&i) {
            out.extend_from_slice(p);
        }
    }
    out
}
fn assign_ppm(c: &mut Codestream) -> Result<()> {
    if c.ppm.is_empty() {
        return Ok(());
    }
    let joined = join_packed(&c.ppm);
    let mut data = &joined[..];
    let total = c.tile_parts.len();
    for (i, part) in c.tile_parts.iter_mut().enumerate() {
        if part.packed_headers {
            return Err(plain("jpeg2000: both PPM and PPT"));
        }
        let Some((&nppm, rest)) = data.split_first_chunk::<4>() else {
            return Err(plain(format!(
                "jpeg2000: PPM holds headers for {i} of {total} tile-parts"
            )));
        };
        let n = u32::from_be_bytes(nppm) as usize;
        let Some((headers, rest)) = rest.split_at_checked(n) else {
            return Err(plain(format!(
                "jpeg2000: PPM run of {n} bytes, {} remain",
                rest.len()
            )));
        };
        part.headers = headers.to_vec();
        part.packed_headers = true;
        data = rest;
    }
    Ok(())
}
fn parse_qcc(c: &mut Codestream, s: &[u8]) -> Result<()> {
    if c.components.is_empty() {
        return Err(plain("jpeg2000: QCC before SIZ"));
    }
    let (index, rest) = component_index(c, s)?;
    let q = parse_quantization(rest)?;
    c.component_quant.insert(index, q);
    Ok(())
}
fn parse_quantization(s: &[u8]) -> Result<Quantization> {
    let Some((&sqcd, values)) = s.split_first() else {
        return Err(plain("jpeg2000: quantization segment is empty"));
    };
    let mut q = Quantization {
        style: (sqcd & 0x1F) as i64,
        guard_bits: (sqcd >> 5) as i64,
        ..Quantization::default()
    };
    match q.style {
        QUANT_NONE => {
            for &b in values {
                q.exponents.push((b >> 3) as i64);
                q.mantissas.push(0);
            }
        }
        QUANT_DERIVED | QUANT_EXPOUNDED => {
            for &pair in values.as_chunks::<2>().0 {
                let v = u16::from_be_bytes(pair) as i64;
                q.exponents.push(v >> 11);
                q.mantissas.push(v & 0x07FF);
            }
        }
        _ => {
            return Err(CoreError::Unsupported(format!(
                "quantization style {}",
                q.style
            )));
        }
    }
    if q.exponents.is_empty() {
        return Err(plain("jpeg2000: quantization segment carries no values"));
    }
    Ok(q)
}
fn component_index<'a>(c: &Codestream, s: &'a [u8]) -> Result<(i64, &'a [u8])> {
    if c.components.len() < 257 {
        let Some((&index, rest)) = s.split_first() else {
            return Err(plain("jpeg2000: component index is missing"));
        };
        return Ok((index as i64, rest));
    }
    let Some((&index, rest)) = s.split_first_chunk::<2>() else {
        return Err(plain("jpeg2000: component index is missing"));
    };
    Ok((u16::from_be_bytes(index) as i64, rest))
}
const MAX_TILES: i64 = 65535;
fn parse_tile_part(c: &mut Codestream, data: &[u8], pos: usize) -> Result<usize> {
    let Some(&[l0, l1, i0, i1, p0, p1, p2, p3, tpsot, _]) = tail(data, pos).first_chunk::<10>()
    else {
        return Err(plain("jpeg2000: truncated SOT segment"));
    };
    let sot_length = u16::from_be_bytes([l0, l1]) as usize;
    if sot_length != 10 {
        return Err(plain(format!(
            "jpeg2000: SOT is {sot_length} bytes, want 10"
        )));
    }
    let tile = u16::from_be_bytes([i0, i1]) as i64;
    let psot = u32::from_be_bytes([p0, p1, p2, p3]) as usize;
    let part_index = tpsot as i64;
    let start = pos.saturating_sub(2);
    let mut end = data.len();
    if psot != 0 {
        const SMALLEST_TILE_PART: usize = 2 + 10 + 2;
        if psot < SMALLEST_TILE_PART {
            return Err(plain(format!(
                "jpeg2000: tile-part {tile} declares {psot} bytes, too few to hold its header"
            )));
        }
        if start.saturating_add(psot) > data.len() {
            return Err(plain(format!(
                "jpeg2000: tile-part {tile} declares {psot} bytes, {} remain",
                data.len().saturating_sub(start)
            )));
        }
        end = start.saturating_add(psot);
    }
    let mut ppt: HashMap<i64, Vec<u8>> = HashMap::new();
    let mut cursor = pos.saturating_add(sot_length);
    while cursor.saturating_add(1) < end {
        let marker = be16(tail(data, cursor)) as u16;
        if marker == MARKER_SOD {
            cursor = cursor.saturating_add(2);
            break;
        }
        if is_segmentless(marker) {
            cursor = cursor.saturating_add(2);
            continue;
        }
        if cursor.saturating_add(4) > end {
            return Err(plain(format!(
                "jpeg2000: truncated tile-part header in tile {tile}"
            )));
        }
        let length = be16(tail(data, cursor.saturating_add(2))) as usize;
        let segment_end = cursor.saturating_add(2).saturating_add(length);
        if length < 2 || segment_end > end {
            return Err(plain(format!(
                "jpeg2000: tile-part header segment {marker:04X} declares {length} bytes"
            )));
        }
        let segment = range(data, cursor.saturating_add(4), segment_end);
        match marker {
            MARKER_COD => {
                let style = parse_cod(segment)?;
                c.tile_coding.insert(tile, style);
            }
            MARKER_COC => parse_tile_coc(c, tile, segment)?,
            MARKER_QCD => {
                let q = parse_quantization(segment)?;
                c.tile_quant.insert(tile, q);
            }
            MARKER_QCC => parse_tile_qcc(c, tile, segment)?,
            MARKER_PPT => collect_packed(&mut ppt, segment, "PPT")?,
            MARKER_POC => {
                let changes = parse_poc(c, segment)?;
                c.tile_poc.entry(tile).or_default().extend(changes);
            }
            MARKER_RGN => {
                let (component, shift) = parse_rgn(c, segment)?;
                c.tile_roi_shift.insert((tile, component), shift);
            }
            MARKER_COM | MARKER_PLT => {}
            _ => {
                return Err(plain(format!(
                    "jpeg2000: unexpected marker {marker:04X} in tile-part {tile}"
                )));
            }
        }
        cursor = segment_end;
    }
    if tile >= MAX_TILES {
        return Err(plain(format!("jpeg2000: tile index {tile}")));
    }
    if cursor > end {
        return Err(plain(format!(
            "jpeg2000: tile-part {tile} ends before its header does"
        )));
    }
    let mut part = TilePart {
        tile,
        index: part_index,
        data: range(data, cursor, end).to_vec(),
        ..TilePart::default()
    };
    if !ppt.is_empty() {
        part.headers = join_packed(&ppt);
        part.packed_headers = true;
    }
    c.tile_parts.push(part);
    Ok(end)
}
fn validate(c: &mut Codestream, seen_siz: bool, seen_cod: bool, seen_qcd: bool) -> Result<()> {
    if !seen_siz {
        return Err(plain("jpeg2000: codestream has no SIZ segment"));
    }
    if !seen_cod {
        return Err(plain("jpeg2000: codestream has no COD segment"));
    }
    if !seen_qcd {
        return Err(plain("jpeg2000: codestream has no QCD segment"));
    }
    if c.tile_parts.is_empty() {
        return Err(plain("jpeg2000: codestream has no tile-parts"));
    }
    assign_ppm(c)
}
