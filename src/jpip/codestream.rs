#![allow(clippy::needless_range_loop)]
use super::{ChainError, Sentinel, malformed};
use std::collections::HashMap;
pub(crate) const MARKER_SOC: u16 = 0xff4f;
pub(crate) const MARKER_SIZ: u16 = 0xff51;
pub(crate) const MARKER_COD: u16 = 0xff52;
pub(crate) const MARKER_COC: u16 = 0xff53;
pub(crate) const MARKER_TLM: u16 = 0xff55;
pub(crate) const MARKER_PLM: u16 = 0xff57;
pub(crate) const MARKER_PLT: u16 = 0xff58;
pub(crate) const MARKER_POC: u16 = 0xff5f;
pub(crate) const MARKER_PPM: u16 = 0xff60;
pub(crate) const MARKER_PPT: u16 = 0xff61;
pub(crate) const MARKER_DFS: u16 = 0xff72;
pub(crate) const MARKER_ADS: u16 = 0xff73;
pub(crate) const MARKER_SOT: u16 = 0xff90;
pub(crate) const MARKER_SOP: u16 = 0xff91;
pub(crate) const MARKER_EPH: u16 = 0xff92;
pub(crate) const MARKER_SOD: u16 = 0xff93;
pub(crate) const MARKER_EOC: u16 = 0xffd9;
pub(crate) const PROGRESSION_LRCP: u8 = 0;
pub(crate) const PROGRESSION_RLCP: u8 = 1;
pub(crate) const PROGRESSION_RPCL: u8 = 2;
pub(crate) const PROGRESSION_PCRL: u8 = 3;
pub(crate) const PROGRESSION_CPRL: u8 = 4;
pub(crate) const SCOD_PRECINCTS: u8 = 0x01;
pub(crate) const SCOD_SOP: u8 = 0x02;
pub(crate) const SCOD_EPH: u8 = 0x04;
const SCOD_PART2_ANCHORS: u8 = 0x18;
pub(crate) const CMODE_BYPASS: u8 = 0x01;
pub(crate) const CMODE_TERM_ALL: u8 = 0x04;
const CMODE_HT: u8 = 0x40;
pub(crate) const MAX_DECOMPOSITION_LEVELS: usize = 32;
pub(crate) const MAX_COMPONENTS: usize = 16384;
fn be16(p: &[u8]) -> u16 {
    u16::from_be_bytes([p[0], p[1]])
}
fn be32(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}
pub(crate) struct Segment<'a> {
    pub marker: u16,
    pub start: usize,
    pub end: usize,
    pub payload: &'a [u8],
}
pub(crate) fn walk_segments<'a>(
    data: &'a [u8],
    f: &mut dyn FnMut(Segment<'a>) -> Result<(), ChainError>,
) -> Result<(), ChainError> {
    let mut at = 0;
    while at < data.len() {
        if at + 2 > data.len() || data[at] != 0xff {
            return Err(malformed("expected a marker"));
        }
        let marker = be16(&data[at..]);
        if marker == MARKER_SOC {
            f(Segment {
                marker,
                start: at,
                end: at + 2,
                payload: &[],
            })?;
            at += 2;
            continue;
        }
        if at + 4 > data.len() {
            return Err(malformed("truncated marker segment"));
        }
        let length = usize::from(be16(&data[at + 2..]));
        if length < 2 || at + 2 + length > data.len() {
            return Err(malformed("marker segment length out of range"));
        }
        f(Segment {
            marker,
            start: at,
            end: at + 2 + length,
            payload: &data[at + 4..at + 2 + length],
        })?;
        at += 2 + length;
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ComponentSize {
    pub xr: i64,
    pub yr: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ImageSize {
    pub x1: i64,
    pub y1: i64,
    pub x0: i64,
    pub y0: i64,
    pub tw: i64,
    pub th: i64,
    pub tx0: i64,
    pub ty0: i64,
    pub comps: Vec<ComponentSize>,
    pub tiles_x: usize,
    pub tiles_y: usize,
    pub component_field_size: usize,
}
impl ImageSize {
    pub(crate) fn num_tiles(&self) -> usize {
        self.tiles_x * self.tiles_y
    }
}
fn parse_siz(p: &[u8]) -> Result<ImageSize, ChainError> {
    if p.len() < 36 {
        return Err(malformed("short SIZ"));
    }
    let u32_at = |at: usize| i64::from(be32(&p[at..]));
    let mut s = ImageSize {
        x1: u32_at(2),
        y1: u32_at(6),
        x0: u32_at(10),
        y0: u32_at(14),
        tw: u32_at(18),
        th: u32_at(22),
        tx0: u32_at(26),
        ty0: u32_at(30),
        ..ImageSize::default()
    };
    let csiz = usize::from(be16(&p[34..]));
    if !(1..=MAX_COMPONENTS).contains(&csiz) || p.len() != 36 + 3 * csiz {
        return Err(malformed("SIZ component count"));
    }
    if s.x1 <= s.x0
        || s.y1 <= s.y0
        || s.tw == 0
        || s.th == 0
        || s.tx0 > s.x0
        || s.ty0 > s.y0
        || s.tx0 + s.tw <= s.x0
        || s.ty0 + s.th <= s.y0
    {
        return Err(malformed("SIZ geometry"));
    }
    for c in 0..csiz {
        let (xr, yr) = (i64::from(p[37 + 3 * c]), i64::from(p[38 + 3 * c]));
        if xr == 0 || yr == 0 {
            return Err(malformed("SIZ sub-sampling"));
        }
        s.comps.push(ComponentSize { xr, yr });
    }
    let tiles_x = ceil_div(s.x1 - s.tx0, s.tw);
    let tiles_y = ceil_div(s.y1 - s.ty0, s.th);
    if tiles_x.wrapping_mul(tiles_y) > 65535 {
        return Err(malformed("too many tiles"));
    }
    s.tiles_x = tiles_x as usize;
    s.tiles_y = tiles_y as usize;
    s.component_field_size = if csiz > 256 { 2 } else { 1 };
    Ok(s)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ComponentCoding {
    pub levels: usize,
    pub xcb: usize,
    pub ycb: usize,
    pub cmodes: u8,
    pub ppx: Vec<usize>,
    pub ppy: Vec<usize>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CodingStyle {
    pub scod: u8,
    pub progression: u8,
    pub layers: usize,
    pub component: ComponentCoding,
}
fn parse_component_coding(
    p: &[u8],
    precincts: bool,
) -> Result<(ComponentCoding, usize), ChainError> {
    if p.len() < 5 {
        return Err(malformed("short coding style"));
    }
    let mut cc = ComponentCoding {
        levels: usize::from(p[0]),
        xcb: usize::from(p[1]) + 2,
        ycb: usize::from(p[2]) + 2,
        cmodes: p[3],
        ..ComponentCoding::default()
    };
    if cc.levels > MAX_DECOMPOSITION_LEVELS || cc.xcb > 10 || cc.ycb > 10 || cc.xcb + cc.ycb > 12 {
        return Err(malformed("coding style parameters"));
    }
    let mut used = 5;
    cc.ppx = vec![15; cc.levels + 1];
    cc.ppy = vec![15; cc.levels + 1];
    for r in 0..=cc.levels {
        if !precincts {
            continue;
        }
        if used >= p.len() {
            return Err(malformed("short precinct sizes"));
        }
        cc.ppx[r] = usize::from(p[used] & 0x0f);
        cc.ppy[r] = usize::from(p[used] >> 4);
        used += 1;
        if r > 0 && (cc.ppx[r] == 0 || cc.ppy[r] == 0) {
            return Err(malformed("precinct size"));
        }
    }
    Ok((cc, used))
}
fn parse_cod(p: &[u8]) -> Result<CodingStyle, ChainError> {
    if p.len() < 4 {
        return Err(malformed("short COD"));
    }
    let mut cs = CodingStyle {
        scod: p[0],
        progression: p[1],
        layers: usize::from(be16(&p[2..])),
        ..CodingStyle::default()
    };
    if cs.progression > PROGRESSION_CPRL || cs.layers == 0 {
        return Err(malformed("COD progression or layers"));
    }
    let rest = p.get(5..).unwrap_or(&[]);
    let (cc, used) = parse_component_coding(rest, cs.scod & SCOD_PRECINCTS != 0)?;
    if 5 + used != p.len() {
        return Err(malformed("COD length"));
    }
    cs.component = cc;
    Ok(cs)
}
fn parse_coc(p: &[u8], size: &ImageSize) -> Result<(usize, ComponentCoding), ChainError> {
    let n = size.component_field_size;
    if p.len() < n + 1 {
        return Err(malformed("short COC"));
    }
    let c = if n == 2 {
        usize::from(be16(p))
    } else {
        usize::from(p[0])
    };
    if c >= size.comps.len() {
        return Err(malformed("COC component"));
    }
    let (cc, used) = parse_component_coding(&p[n + 1..], p[n] & SCOD_PRECINCTS != 0)?;
    if n + 1 + used != p.len() {
        return Err(malformed("COC length"));
    }
    Ok((c, cc))
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProgressionChange {
    pub rs: usize,
    pub cs: usize,
    pub lye: usize,
    pub re: usize,
    pub ce: usize,
    pub order: u8,
}
fn parse_poc(p: &[u8], size: &ImageSize) -> Result<Vec<ProgressionChange>, ChainError> {
    let n = size.component_field_size;
    let entry = 5 + 2 * n;
    if p.is_empty() || !p.len().is_multiple_of(entry) {
        return Err(malformed("POC length"));
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at < p.len() {
        let e = &p[at..];
        let mut change = ProgressionChange {
            rs: usize::from(e[0]),
            ..ProgressionChange::default()
        };
        change.cs = if n == 1 {
            usize::from(e[1])
        } else {
            usize::from(be16(&e[1..]))
        };
        change.lye = usize::from(be16(&e[1 + n..]));
        change.re = usize::from(e[3 + n]);
        if n == 1 {
            change.ce = usize::from(e[4 + n]);
            if change.ce == 0 {
                change.ce = 256;
            }
        } else {
            change.ce = usize::from(be16(&e[4 + n..]));
        }
        change.order = e[4 + 2 * n];
        if change.order > PROGRESSION_CPRL {
            return Err(malformed("POC progression"));
        }
        out.push(change);
        at += entry;
    }
    Ok(out)
}
#[derive(Clone, Debug, Default)]
pub(crate) struct HeaderParameters {
    pub cod: Option<CodingStyle>,
    pub coc: HashMap<usize, ComponentCoding>,
    pub poc: Vec<ProgressionChange>,
    pub packed: bool,
    pub extended: bool,
    pub plt_lengths: Vec<u64>,
    pub plt_valid: bool,
}
pub(crate) fn parse_header(
    data: &[u8],
    size: Option<&ImageSize>,
    main: bool,
) -> Result<(HeaderParameters, ImageSize), ChainError> {
    let mut params = HeaderParameters {
        plt_valid: true,
        ..HeaderParameters::default()
    };
    let mut siz = size.cloned().unwrap_or_default();
    let mut saw_siz = size.is_some();
    let mut plt_seen = false;
    walk_segments(data, &mut |s| {
        match s.marker {
            MARKER_SOC => {
                if !main || s.start != 0 {
                    return Err(malformed("unexpected SOC"));
                }
            }
            MARKER_SIZ => {
                if !main || saw_siz {
                    return Err(malformed("unexpected SIZ"));
                }
                siz = parse_siz(s.payload)?;
                saw_siz = true;
            }
            MARKER_COD => {
                if params.cod.is_some() {
                    return Err(malformed("duplicate COD"));
                }
                let cod = parse_cod(s.payload)?;
                if cod.scod & SCOD_PART2_ANCHORS != 0 {
                    params.extended = true;
                }
                params.cod = Some(cod);
            }
            MARKER_COC => {
                if !saw_siz {
                    return Err(malformed("COC before SIZ"));
                }
                let (c, cc) = parse_coc(s.payload, &siz)?;
                params.coc.insert(c, cc);
            }
            MARKER_POC => {
                if !saw_siz {
                    return Err(malformed("POC before SIZ"));
                }
                let changes = parse_poc(s.payload, &siz)?;
                params.poc.extend(changes);
            }
            MARKER_PPM | MARKER_PPT => params.packed = true,
            MARKER_DFS | MARKER_ADS => params.extended = true,
            MARKER_PLT => {
                plt_seen = true;
                let (lengths, ok) = parse_plt(s.payload);
                if !ok {
                    params.plt_valid = false;
                }
                params.plt_lengths.extend(lengths);
            }
            MARKER_SOT | MARKER_SOD | MARKER_EOC | MARKER_SOP | MARKER_EPH => {
                return Err(malformed("unexpected delimiting marker in a header"));
            }
            _ => {}
        }
        Ok(())
    })?;
    if main && !saw_siz {
        return Err(malformed("missing SIZ"));
    }
    if !plt_seen {
        params.plt_valid = false;
    }
    Ok((params, siz))
}
fn parse_plt(p: &[u8]) -> (Vec<u64>, bool) {
    if p.is_empty() {
        return (Vec::new(), false);
    }
    let mut lengths = Vec::new();
    let mut value = 0u64;
    let mut open = false;
    for &b in &p[1..] {
        if value >> 56 != 0 {
            return (Vec::new(), false);
        }
        value = (value << 7) | u64::from(b & 0x7f);
        open = b & 0x80 != 0;
        if !open {
            lengths.push(value);
            value = 0;
        }
    }
    (lengths, !open)
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BandGeometry {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}
impl BandGeometry {
    fn empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResolutionGeometry {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub ppx: usize,
    pub ppy: usize,
    pub pw: usize,
    pub ph: usize,
    pub precinct_base: usize,
    pub bands: Vec<BandGeometry>,
}
impl ResolutionGeometry {
    pub fn precincts(&self) -> usize {
        self.pw * self.ph
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ComponentGeometry {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub xr: i64,
    pub yr: i64,
    pub coding: ComponentCoding,
    pub resolutions: Vec<ResolutionGeometry>,
    pub precinct_base: usize,
    pub precinct_count: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TileGeometry {
    pub index: usize,
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub scod: u8,
    pub progression: u8,
    pub layers: usize,
    pub poc: Vec<ProgressionChange>,
    pub components: Vec<ComponentGeometry>,
    pub precinct_count: usize,
    pub ht: bool,
}
pub(crate) fn coding_parameters(
    size: &ImageSize,
    main: &HeaderParameters,
    tile: &HeaderParameters,
) -> Result<(CodingStyle, Vec<ComponentCoding>, Vec<ProgressionChange>), ChainError> {
    let Some(main_cod) = &main.cod else {
        return Err(malformed("missing COD"));
    };
    let style = tile.cod.clone().unwrap_or_else(|| main_cod.clone());
    let comps = (0..size.comps.len())
        .map(|c| {
            if let Some(cc) = tile.coc.get(&c) {
                cc.clone()
            } else if let Some(cod) = &tile.cod {
                cod.component.clone()
            } else if let Some(cc) = main.coc.get(&c) {
                cc.clone()
            } else {
                main_cod.component.clone()
            }
        })
        .collect();
    let poc = if tile.poc.is_empty() {
        main.poc.clone()
    } else {
        tile.poc.clone()
    };
    Ok((style, comps, poc))
}
fn index_limit(detail: &str) -> ChainError {
    ChainError::wrap(Sentinel::IndexLimit, detail)
}
pub(crate) fn new_tile_geometry(
    size: &ImageSize,
    index: usize,
    style: &CodingStyle,
    comps: &[ComponentCoding],
    poc: Vec<ProgressionChange>,
) -> Result<TileGeometry, ChainError> {
    let p = (index % size.tiles_x) as i64;
    let q = (index / size.tiles_x) as i64;
    let mut t = TileGeometry {
        index,
        x0: (size.tx0 + p * size.tw).max(size.x0),
        y0: (size.ty0 + q * size.th).max(size.y0),
        x1: (size.tx0 + (p + 1) * size.tw).min(size.x1),
        y1: (size.ty0 + (q + 1) * size.th).min(size.y1),
        scod: style.scod,
        progression: style.progression,
        layers: style.layers,
        poc,
        ..TileGeometry::default()
    };
    for (c, cs) in size.comps.iter().enumerate() {
        let coding = comps[c].clone();
        if coding.cmodes & CMODE_HT != 0 {
            t.ht = true;
        }
        let mut comp = ComponentGeometry {
            x0: ceil_div(t.x0, cs.xr),
            y0: ceil_div(t.y0, cs.yr),
            x1: ceil_div(t.x1, cs.xr),
            y1: ceil_div(t.y1, cs.yr),
            xr: cs.xr,
            yr: cs.yr,
            coding: coding.clone(),
            precinct_base: t.precinct_count,
            ..ComponentGeometry::default()
        };
        let mut base = 0usize;
        for r in 0..=coding.levels {
            let n = (coding.levels - r) as u32;
            let mut res = ResolutionGeometry {
                x0: ceil_div(comp.x0, 1 << n),
                y0: ceil_div(comp.y0, 1 << n),
                x1: ceil_div(comp.x1, 1 << n),
                y1: ceil_div(comp.y1, 1 << n),
                ppx: coding.ppx[r],
                ppy: coding.ppy[r],
                precinct_base: base,
                ..ResolutionGeometry::default()
            };
            if res.x1 > res.x0 && res.y1 > res.y0 {
                let pw = ceil_div(res.x1, 1 << res.ppx) - floor_div(res.x0, 1 << res.ppx);
                let ph = ceil_div(res.y1, 1 << res.ppy) - floor_div(res.y0, 1 << res.ppy);
                if pw.wrapping_mul(ph) > 1 << 24 {
                    return Err(index_limit("precinct count"));
                }
                res.pw = pw as usize;
                res.ph = ph as usize;
            }
            res.bands = if r == 0 {
                vec![band_of(&comp, coding.levels, 0, 0)]
            } else {
                let nb = coding.levels - r + 1;
                vec![
                    band_of(&comp, nb, 1, 0),
                    band_of(&comp, nb, 0, 1),
                    band_of(&comp, nb, 1, 1),
                ]
            };
            base += res.precincts();
            comp.resolutions.push(res);
        }
        comp.precinct_count = base;
        t.precinct_count += base;
        if t.precinct_count > 1 << 24 {
            return Err(index_limit("precinct count"));
        }
        t.components.push(comp);
    }
    Ok(t)
}
fn band_of(comp: &ComponentGeometry, nb: usize, xo: i64, yo: i64) -> BandGeometry {
    if nb == 0 {
        return BandGeometry {
            x0: comp.x0,
            y0: comp.y0,
            x1: comp.x1,
            y1: comp.y1,
        };
    }
    let half = 1i64 << (nb - 1);
    let scale = 1i64 << nb;
    BandGeometry {
        x0: ceil_div(comp.x0 - half * xo, scale),
        y0: ceil_div(comp.y0 - half * yo, scale),
        x1: ceil_div(comp.x1 - half * xo, scale),
        y1: ceil_div(comp.y1 - half * yo, scale),
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CodeBlockGrid {
    pub cw: usize,
    pub ch: usize,
}
pub(crate) type PacketVisitor<'a> =
    dyn FnMut(usize, usize, usize, usize) -> Result<(), ChainError> + 'a;
impl TileGeometry {
    pub(crate) fn code_block_grids(&self, c: usize, r: usize, k: usize) -> Vec<CodeBlockGrid> {
        let comp = &self.components[c];
        let res = &comp.resolutions[r];
        let px = (k % res.pw) as i64;
        let py = (k / res.pw) as i64;
        let (mut pbx, mut pby) = (res.ppx as i64, res.ppy as i64);
        let mut origin_x = floor_div(res.x0, 1 << res.ppx) << res.ppx;
        let mut origin_y = floor_div(res.y0, 1 << res.ppy) << res.ppy;
        if r > 0 {
            pbx -= 1;
            pby -= 1;
            origin_x /= 2;
            origin_y /= 2;
        }
        let xcb = (comp.coding.xcb as i64).min(pbx);
        let ycb = (comp.coding.ycb as i64).min(pby);
        let mut grids = vec![CodeBlockGrid::default(); res.bands.len()];
        for (b, band) in res.bands.iter().enumerate() {
            if band.empty() {
                continue;
            }
            let x0 = (origin_x + (px << pbx)).max(band.x0);
            let y0 = (origin_y + (py << pby)).max(band.y0);
            let x1 = (origin_x + ((px + 1) << pbx)).min(band.x1);
            let y1 = (origin_y + ((py + 1) << pby)).min(band.y1);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            grids[b] = CodeBlockGrid {
                cw: (ceil_div(x1, 1 << xcb) - floor_div(x0, 1 << xcb)) as usize,
                ch: (ceil_div(y1, 1 << ycb) - floor_div(y0, 1 << ycb)) as usize,
            };
        }
        grids
    }
    pub(crate) fn precinct_bounds(&self, c: usize, r: usize, k: usize) -> (i64, i64, i64, i64) {
        let comp = &self.components[c];
        let res = &comp.resolutions[r];
        let px = (k % res.pw) as i64;
        let py = (k / res.pw) as i64;
        let rx0 = ((floor_div(res.x0, 1 << res.ppx) + px) << res.ppx).max(res.x0);
        let ry0 = ((floor_div(res.y0, 1 << res.ppy) + py) << res.ppy).max(res.y0);
        let rx1 = ((floor_div(res.x0, 1 << res.ppx) + px + 1) << res.ppx).min(res.x1);
        let ry1 = ((floor_div(res.y0, 1 << res.ppy) + py + 1) << res.ppy).min(res.y1);
        let n = (comp.coding.levels - r) as u32;
        (
            rx0.wrapping_shl(n).wrapping_mul(comp.xr),
            ry0.wrapping_shl(n).wrapping_mul(comp.yr),
            rx1.wrapping_shl(n).wrapping_mul(comp.xr),
            ry1.wrapping_shl(n).wrapping_mul(comp.yr),
        )
    }
    pub(crate) fn precinct_index(&self, c: usize, r: usize, k: usize) -> usize {
        let comp = &self.components[c];
        comp.precinct_base + comp.resolutions[r].precinct_base + k
    }
    pub(crate) fn for_each_packet(
        &self,
        order: u8,
        poc: &[ProgressionChange],
        visit: &mut PacketVisitor<'_>,
    ) -> Result<(), ChainError> {
        let mut max_res = 0;
        for comp in &self.components {
            max_res = max_res.max(comp.coding.levels + 1);
        }
        let default = [ProgressionChange {
            rs: 0,
            cs: 0,
            lye: self.layers,
            re: max_res,
            ce: self.components.len(),
            order,
        }];
        let changes = if poc.is_empty() { &default[..] } else { poc };
        let mut next = vec![0i32; self.precinct_count];
        let mut emit = |l: usize, r: usize, c: usize, k: usize| -> Result<(), ChainError> {
            let i = self.precinct_index(c, r, k);
            let n = next[i] as usize;
            if n > l {
                return Ok(());
            }
            if n < l {
                return Err(malformed("progression skips a layer"));
            }
            next[i] += 1;
            visit(l, r, c, k)
        };
        for change in changes {
            let lye = change.lye.min(self.layers);
            let ce = change.ce.min(self.components.len());
            let (cs, rs, re) = (change.cs, change.rs, change.re.min(max_res));
            if cs >= ce || rs >= re || lye == 0 {
                continue;
            }
            match change.order {
                PROGRESSION_LRCP => {
                    for l in 0..lye {
                        for r in rs..re {
                            self.each_component_precinct(cs, ce, r, &mut |c, k| emit(l, r, c, k))?;
                        }
                    }
                }
                PROGRESSION_RLCP => {
                    for r in rs..re {
                        for l in 0..lye {
                            self.each_component_precinct(cs, ce, r, &mut |c, k| emit(l, r, c, k))?;
                        }
                    }
                }
                PROGRESSION_RPCL => {
                    for r in rs..re {
                        self.each_position(cs, ce, r, r + 1, &mut |x, y| {
                            for c in cs..ce {
                                if let Some(k) = self.precinct_at(c, r, x, y) {
                                    for l in 0..lye {
                                        emit(l, r, c, k)?;
                                    }
                                }
                            }
                            Ok(())
                        })?;
                    }
                }
                PROGRESSION_PCRL => {
                    self.each_position(cs, ce, rs, re, &mut |x, y| {
                        for c in cs..ce {
                            for r in rs..re {
                                if let Some(k) = self.precinct_at(c, r, x, y) {
                                    for l in 0..lye {
                                        emit(l, r, c, k)?;
                                    }
                                }
                            }
                        }
                        Ok(())
                    })?;
                }
                PROGRESSION_CPRL => {
                    for c in cs..ce {
                        self.each_position(c, c + 1, rs, re, &mut |x, y| {
                            for r in rs..re {
                                if let Some(k) = self.precinct_at(c, r, x, y) {
                                    for l in 0..lye {
                                        emit(l, r, c, k)?;
                                    }
                                }
                            }
                            Ok(())
                        })?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn each_component_precinct(
        &self,
        cs: usize,
        ce: usize,
        r: usize,
        f: &mut dyn FnMut(usize, usize) -> Result<(), ChainError>,
    ) -> Result<(), ChainError> {
        for c in cs..ce {
            let comp = &self.components[c];
            if r > comp.coding.levels {
                continue;
            }
            for k in 0..comp.resolutions[r].precincts() {
                f(c, k)?;
            }
        }
        Ok(())
    }
    fn each_position(
        &self,
        cs: usize,
        ce: usize,
        rs: usize,
        re: usize,
        f: &mut dyn FnMut(i64, i64) -> Result<(), ChainError>,
    ) -> Result<(), ChainError> {
        let step = |v: i64, limit: i64, vertical: bool| -> i64 {
            let mut next = limit;
            for c in cs..ce {
                let comp = &self.components[c];
                for r in rs..re.min(comp.coding.levels + 1) {
                    let res = &comp.resolutions[r];
                    if res.precincts() == 0 {
                        continue;
                    }
                    let n = (comp.coding.levels - r) as u32;
                    let m = if vertical {
                        comp.yr << (res.ppy as u32 + n)
                    } else {
                        comp.xr << (res.ppx as u32 + n)
                    };
                    next = next.min((floor_div(v, m) + 1) * m);
                }
            }
            next
        };
        let mut y = self.y0;
        while y < self.y1 {
            let mut x = self.x0;
            while x < self.x1 {
                f(x, y)?;
                x = step(x, self.x1, false);
            }
            y = step(y, self.y1, true);
        }
        Ok(())
    }
    fn precinct_at(&self, c: usize, r: usize, x: i64, y: i64) -> Option<usize> {
        let comp = &self.components[c];
        if r > comp.coding.levels {
            return None;
        }
        let res = &comp.resolutions[r];
        if res.precincts() == 0 {
            return None;
        }
        let n = (comp.coding.levels - r) as u32;
        if !(y % (comp.yr << (res.ppy as u32 + n)) == 0
            || y == self.y0 && res.y0 % (1 << res.ppy) != 0)
        {
            return None;
        }
        if !(x % (comp.xr << (res.ppx as u32 + n)) == 0
            || x == self.x0 && res.x0 % (1 << res.ppx) != 0)
        {
            return None;
        }
        let px =
            floor_div(ceil_div(x, comp.xr << n), 1 << res.ppx) - floor_div(res.x0, 1 << res.ppx);
        let py =
            floor_div(ceil_div(y, comp.yr << n), 1 << res.ppy) - floor_div(res.y0, 1 << res.ppy);
        if px < 0 || py < 0 || px >= res.pw as i64 || py >= res.ph as i64 {
            return None;
        }
        Some(px as usize + py as usize * res.pw)
    }
}
pub(crate) fn precinct_bin_id(
    t: usize,
    c: usize,
    s: usize,
    num_components: usize,
    num_tiles: usize,
) -> u64 {
    (t as u64).wrapping_add(
        (c as u64)
            .wrapping_add((s as u64).wrapping_mul(num_components as u64))
            .wrapping_mul(num_tiles as u64),
    )
}
pub(crate) fn ceil_div(a: i64, b: i64) -> i64 {
    let mut q = a / b;
    if a % b != 0 && a > 0 {
        q += 1;
    }
    q
}
pub(crate) fn floor_div(a: i64, b: i64) -> i64 {
    let mut q = a / b;
    if a % b != 0 && a < 0 {
        q -= 1;
    }
    q
}
