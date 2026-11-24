use super::codestream::{
    BLOCK_BYPASS, BLOCK_HT, BLOCK_MIXED, BLOCK_TERM_ALL, Codestream, PROGRESSION_CPRL,
    PROGRESSION_PCRL, PROGRESSION_RLCP, PROGRESSION_RPCL, ProgressionChange,
};
use super::geometry::{CodeBlock, TileComponent, floor_div_pow2, log2, shl};
use super::{CoreError, Result, errorf};
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Segment {
    pub data: Vec<u8>,
    pub passes: i64,
    pub placeholder: bool,
}
#[derive(Debug, Clone, Default)]
pub(super) struct CodeBlockData {
    pub block: CodeBlock,
    pub band_type: i64,
    pub resolution: i64,
    pub component: i64,
    pub zero_bit_planes: i64,
    pub passes: i64,
    pub segments: Vec<Segment>,
    pub style: u8,
}
pub(super) struct TileData {
    pub components: Vec<TileComponent>,
    pub blocks: Vec<CodeBlockData>,
}
#[derive(Default, Clone)]
struct BlockState {
    included: bool,
    zero_bit_planes: i64,
    lblock: i64,
    passes: i64,
    kept: i64,
    segments: Vec<Segment>,
    open: bool,
}
struct PrecinctState {
    inclusion: TagTree,
    zero_bits: TagTree,
    blocks: Vec<BlockState>,
}
const MAX_PACKETS_PER_TILE: i64 = 1 << 20;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PacketFilter {
    pub layers: i64,
    pub reduce: i64,
}
impl PacketFilter {
    fn needs(&self, step: &PacketStep, comps: &[TileComponent]) -> bool {
        if self.layers > 0 && step.layer as i64 >= self.layers {
            return false;
        }
        usize::try_from(step.component)
            .ok()
            .and_then(|c| comps.get(c))
            .is_some_and(|tc| {
                (step.resolution as i64) < (tc.resolutions.len() as i64).saturating_sub(self.reduce)
            })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PacketStep {
    layer: i32,
    resolution: i32,
    component: i32,
    precinct: i32,
    x: i32,
    y: i32,
}
#[derive(Clone, Copy)]
struct Contribution {
    key: [i64; 4],
    block: usize,
    length: i64,
    passes: i64,
    block_term: bool,
    placeholder: bool,
}
impl Codestream {
    pub(super) fn block_style_for(&self, tile: i64, comp: i64) -> u8 {
        let style = self.coding_for_tile(tile, comp).block_style;
        if style & BLOCK_MIXED == 0 {
            return style;
        }
        if self.roi_shift_for_tile(tile, comp) > 0 {
            style & !(BLOCK_HT | BLOCK_MIXED)
        } else {
            style & !BLOCK_MIXED
        }
    }
    pub(super) fn read_tile_packets_filtered(
        &self,
        tile: i64,
        filter: PacketFilter,
    ) -> Result<TileData> {
        let mut body: Vec<u8> = Vec::new();
        let mut headers: Vec<u8> = Vec::new();
        let mut packed = false;
        for part in &self.tile_parts {
            if part.tile != tile {
                continue;
            }
            body.extend_from_slice(&part.data);
            if part.packed_headers {
                packed = true;
                headers.extend_from_slice(&part.headers);
            }
        }
        if body.is_empty() && headers.is_empty() {
            return Err(errorf(format_args!("tile {tile} has no data")));
        }
        if !packed {
            headers = body.clone();
        }
        let mut components = Vec::with_capacity(self.components.len());
        for comp in 0..self.components.len() as i64 {
            components.push(self.tile_component_for(tile, comp)?);
        }
        let mut steps = self.packet_sequence(tile, &components, headers.len() as i64)?;
        if filter != PacketFilter::default() {
            let mut last: i64 = -1;
            for (i, step) in steps.iter().enumerate() {
                if filter.needs(step, &components) {
                    last = i as i64;
                }
            }
            steps.truncate(last.saturating_add(1) as usize);
        }
        let mut states: HashMap<[i64; 4], PrecinctState> = HashMap::new();
        let (mut hpos, mut bpos) = (0usize, 0usize);
        for step in &steps {
            if hpos >= headers.len() {
                break;
            }
            let keep = filter == PacketFilter::default() || filter.needs(step, &components);
            let (h, b) = self.read_packet(
                &headers,
                hpos,
                &body,
                bpos,
                packed,
                tile,
                step,
                keep,
                &mut states,
                &components,
            )?;
            hpos = h;
            bpos = b;
            if !packed {
                hpos = bpos;
            }
        }
        let mut keys: Vec<[i64; 4]> = states.keys().copied().collect();
        keys.sort();
        let mut blocks = Vec::new();
        for key in keys {
            let state = states.remove(&key).expect("key");
            let [comp, res, band, precinct] = key;
            let at = |i: i64| usize::try_from(i).ok();
            let Some(band_info) = at(comp)
                .and_then(|c| components.get(c))
                .and_then(|tc| tc.resolutions.get(at(res)?))
                .and_then(|r| r.bands.get(at(band)?))
            else {
                continue;
            };
            let Some(area) = at(precinct).and_then(|p| band_info.precincts.get(p)) else {
                continue;
            };
            let style = self.block_style_for(tile, comp);
            for (b, &block) in state.blocks.into_iter().zip(&area.blocks) {
                if !b.included || b.segments.is_empty() {
                    continue;
                }
                blocks.push(CodeBlockData {
                    block,
                    band_type: band_info.kind,
                    resolution: res,
                    component: comp,
                    zero_bit_planes: b.zero_bit_planes,
                    passes: b.kept,
                    segments: b.segments,
                    style,
                });
            }
        }
        Ok(TileData { components, blocks })
    }
    fn packet_sequence(
        &self,
        tile: i64,
        comps: &[TileComponent],
        stream_bytes: i64,
    ) -> Result<Vec<PacketStep>> {
        let style = self.coding_for_tile(tile, 0);
        let mut changes: Vec<ProgressionChange> = self.progression_changes_for_tile(tile).to_vec();
        if changes.is_empty() {
            changes = vec![ProgressionChange {
                resolution_end: 33,
                component_end: comps.len() as i64,
                layer_end: style.layers,
                order: style.progression,
                ..ProgressionChange::default()
            }];
        }
        let mut total = 0i64;
        for tc in comps {
            for res in &tc.resolutions {
                total = total.wrapping_add(res.precincts());
            }
        }
        total = total.wrapping_mul(style.layers);
        let limit = MAX_PACKETS_PER_TILE.max(stream_bytes);
        if total > limit {
            return Err(CoreError::Limit {
                resource: "packets per tile",
                value: total,
                limit,
            });
        }
        let (tx0, ty0, _, _) = self.tile_bounds(tile);
        let mut emitted: Option<HashSet<PacketStep>> = if changes.len() > 1 {
            Some(HashSet::new())
        } else {
            None
        };
        let mut steps = Vec::new();
        for change in &changes {
            let mut volume = Vec::new();
            let comp_end = change.component_end.min(comps.len() as i64);
            let mut comp = change.component_start.max(0);
            while comp < comp_end {
                let (Some(tc), Some(component)) =
                    (comps.get(comp as usize), self.components.get(comp as usize))
                else {
                    break;
                };
                let levels = (tc.resolutions.len() as i64).saturating_sub(1);
                let (dx, dy) = (component.dx, component.dy);
                let res_end = change.resolution_end.min(tc.resolutions.len() as i64);
                let mut r = change.resolution_start.max(0);
                while r < res_end {
                    let Some(res) = tc.resolutions.get(r as usize) else {
                        break;
                    };
                    let wide = res.precincts_wide;
                    for p in 0..res.precincts() {
                        let px = shl(
                            floor_div_pow2(res.x0, res.ppx)
                                .saturating_add(p.checked_rem(wide).unwrap_or(0)),
                            res.ppx,
                        );
                        let py = shl(
                            floor_div_pow2(res.y0, res.ppy)
                                .saturating_add(p.checked_div(wide).unwrap_or(0)),
                            res.ppy,
                        );
                        let x = shl(px.wrapping_mul(dx), levels.saturating_sub(r)).max(tx0);
                        let y = shl(py.wrapping_mul(dy), levels.saturating_sub(r)).max(ty0);
                        let layer_end = change.layer_end.min(style.layers);
                        for l in 0..layer_end {
                            let step = PacketStep {
                                layer: l as i32,
                                resolution: r as i32,
                                component: comp as i32,
                                precinct: p as i32,
                                x: x as i32,
                                y: y as i32,
                            };
                            if let Some(set) = emitted.as_mut() {
                                let key = PacketStep { x: 0, y: 0, ..step };
                                if !set.insert(key) {
                                    continue;
                                }
                            }
                            volume.push(step);
                        }
                    }
                    r = r.saturating_add(1);
                }
                comp = comp.saturating_add(1);
            }
            sort_progression(&mut volume, change.order);
            steps.extend(volume);
        }
        Ok(steps)
    }
    #[allow(clippy::too_many_arguments)]
    fn read_packet(
        &self,
        headers: &[u8],
        mut hpos: usize,
        body: &[u8],
        mut bpos: usize,
        packed: bool,
        tile: i64,
        step: &PacketStep,
        keep: bool,
        states: &mut HashMap<[i64; 4], PrecinctState>,
        comps: &[TileComponent],
    ) -> Result<(usize, usize)> {
        let (comp, resolution, precinct) = (
            step.component as i64,
            step.resolution as i64,
            step.precinct as i64,
        );
        let style = self.coding_for_tile(tile, comp);
        let block_style = self.block_style_for(tile, comp);
        let Some(res) = usize::try_from(comp)
            .ok()
            .and_then(|c| comps.get(c))
            .and_then(|tc| tc.resolutions.get(usize::try_from(resolution).ok()?))
        else {
            return Ok((hpos, bpos));
        };
        if style.sop
            && be16(body.get(bpos..).unwrap_or(&[])) == Some(0xFF91)
            && body.len() >= bpos.saturating_add(6)
        {
            bpos = bpos.saturating_add(6);
            if !packed {
                hpos = bpos;
            }
        }
        let mut r = BitReader::new(headers.get(hpos..).unwrap_or(&[]));
        let non_empty = r.read_bit()?;
        let mut contributions: Vec<Contribution> = Vec::new();
        if non_empty == 1 {
            for (band_index, band) in res.bands.iter().enumerate() {
                let Some(area) = usize::try_from(precinct)
                    .ok()
                    .and_then(|p| band.precincts.get(p))
                else {
                    continue;
                };
                if area.blocks.is_empty() {
                    continue;
                }
                let key = [comp, resolution, band_index as i64, precinct];
                let state = states.entry(key).or_insert_with(|| PrecinctState {
                    inclusion: TagTree::new(area.blocks_wide, area.blocks_high),
                    zero_bits: TagTree::new(area.blocks_wide, area.blocks_high),
                    blocks: vec![BlockState::default(); area.blocks.len()],
                });
                for i in 0..area.blocks.len() {
                    let included = read_inclusion(&mut r, state, i, step.layer as i64)?;
                    if !included {
                        continue;
                    }
                    if !state.blocks.get(i).is_some_and(|b| b.included) {
                        let zero = read_zero_bit_planes(&mut r, state, i)?;
                        let Some(b) = state.blocks.get_mut(i) else {
                            return Err(errorf("a code-block outside its precinct"));
                        };
                        b.zero_bit_planes = zero;
                        b.lblock = 3;
                        b.included = true;
                    }
                    let Some(b) = state.blocks.get_mut(i) else {
                        return Err(errorf("a code-block outside its precinct"));
                    };
                    let passes = read_pass_count(&mut r)?;
                    read_lblock_increment(&mut r, b)?;
                    if block_style & BLOCK_HT != 0 {
                        ht_contributions(&mut r, b, passes, key, i, &mut contributions)?;
                        b.passes = b.passes.saturating_add(passes);
                        continue;
                    }
                    let (first, last) =
                        (b.passes, b.passes.saturating_add(passes).saturating_sub(1));
                    let mut start = first;
                    while start <= last {
                        let mut end = start;
                        while end < last && !terminates_after(end, block_style) {
                            end = end.saturating_add(1);
                        }
                        let count = end.saturating_sub(start).saturating_add(1);
                        let length = r.read_bits(b.lblock.saturating_add(log2(count)))?;
                        contributions.push(Contribution {
                            key,
                            block: i,
                            length: length as i64,
                            passes: count,
                            block_term: terminates_after(end, block_style),
                            placeholder: false,
                        });
                        start = end.saturating_add(1);
                    }
                    b.passes = b.passes.saturating_add(passes);
                }
            }
        }
        r.align();
        hpos = hpos.saturating_add(r.offset());
        if style.eph && be16(headers.get(hpos..).unwrap_or(&[])) == Some(0xFF92) {
            hpos = hpos.saturating_add(2);
        }
        if !packed {
            bpos = hpos;
        }
        for part in &contributions {
            let rest = body.get(bpos..).unwrap_or(&[]);
            let Some(data) = usize::try_from(part.length)
                .ok()
                .and_then(|n| rest.get(..n))
            else {
                return Err(errorf(format_args!(
                    "packet body runs past the tile: {} bytes with {} left",
                    part.length,
                    rest.len()
                )));
            };
            let length = data.len();
            if !keep {
                bpos = bpos.saturating_add(length);
                continue;
            }
            let Some(block) = states
                .get_mut(&part.key)
                .and_then(|state| state.blocks.get_mut(part.block))
            else {
                return Err(errorf(format_args!(
                    "packet contribution without its code-block"
                )));
            };
            block.kept = block.kept.saturating_add(part.passes);
            if part.placeholder {
                block.segments.push(Segment {
                    data: Vec::new(),
                    passes: part.passes,
                    placeholder: true,
                });
                block.open = false;
                continue;
            }
            if block.open
                && let Some(last) = block.segments.last_mut()
            {
                last.data.extend_from_slice(data);
                last.passes = last.passes.saturating_add(part.passes);
            } else {
                block.segments.push(Segment {
                    data: data.to_vec(),
                    passes: part.passes,
                    placeholder: false,
                });
            }
            block.open = !part.block_term;
            bpos = bpos.saturating_add(length);
        }
        Ok((hpos, bpos))
    }
}
fn ht_contributions(
    r: &mut BitReader<'_>,
    b: &BlockState,
    passes: i64,
    key: [i64; 4],
    block: usize,
    out: &mut Vec<Contribution>,
) -> Result<()> {
    let (first, last) = (b.passes, b.passes.saturating_add(passes).saturating_sub(1));
    let part = |length: u32, passes: i64, block_term: bool, placeholder: bool| Contribution {
        key,
        block,
        length: length as i64,
        passes,
        block_term,
        placeholder,
    };
    let cleanup = last.saturating_sub(last % 3);
    let bits = |passes: i64| b.lblock.saturating_add(log2(passes));
    if cleanup < first {
        let length = r.read_bits(bits(passes))?;
        out.push(part(length, passes, last % 3 == 2, false));
        return Ok(());
    }
    if cleanup < last {
        let ahead = r.clone();
        if r.read_bits(bits(passes))? == 0 {
            out.push(part(0, passes, true, true));
            return Ok(());
        }
        *r = ahead;
    }
    let before = cleanup.saturating_sub(first);
    if before > 0 {
        out.push(part(0, before, true, true));
    }
    let length = r.read_bits(bits(before.saturating_add(1)))?;
    out.push(part(length, 1, true, false));
    if cleanup < last {
        let n = last.saturating_sub(cleanup);
        let length = r.read_bits(bits(n))?;
        out.push(part(length, n, last % 3 == 2, false));
    }
    Ok(())
}
fn be16(b: &[u8]) -> Option<u16> {
    b.first_chunk::<2>().map(|&x| u16::from_be_bytes(x))
}
fn sort_progression(steps: &mut [PacketStep], order: i64) {
    let key = |s: &PacketStep| -> [i32; 5] {
        match order {
            PROGRESSION_RLCP => [s.resolution, s.layer, s.component, s.precinct, 0],
            PROGRESSION_RPCL => [s.resolution, s.y, s.x, s.component, s.layer],
            PROGRESSION_PCRL => [s.y, s.x, s.component, s.resolution, s.layer],
            PROGRESSION_CPRL => [s.component, s.y, s.x, s.resolution, s.layer],
            _ => [s.layer, s.resolution, s.component, s.precinct, 0],
        }
    };
    steps.sort_by_key(key);
}
pub(super) fn terminates_after(pass: i64, style: u8) -> bool {
    if style & BLOCK_TERM_ALL != 0 {
        return true;
    }
    if style & BLOCK_BYPASS != 0 {
        if pass < 10 {
            return pass == 9;
        }
        return pass.saturating_sub(10) % 3 != 0;
    }
    false
}
pub(super) fn raw_pass(pass: i64, style: u8) -> bool {
    style & BLOCK_BYPASS != 0 && pass >= 10 && pass.saturating_sub(10) % 3 != 2
}
fn read_inclusion(
    r: &mut BitReader<'_>,
    state: &mut PrecinctState,
    index: usize,
    layer: i64,
) -> Result<bool> {
    if state.blocks.get(index).is_some_and(|b| b.included) {
        let bit = r.read_bit()?;
        return Ok(bit == 1);
    }
    let (value, known) =
        state
            .inclusion
            .decode(r, index as i64, (layer as i32).saturating_add(1))?;
    Ok(known && value <= layer as i32)
}
fn read_zero_bit_planes(
    r: &mut BitReader<'_>,
    state: &mut PrecinctState,
    index: usize,
) -> Result<i64> {
    for threshold in 1..74 {
        let (value, known) = state.zero_bits.decode(r, index as i64, threshold)?;
        if known {
            return Ok(value as i64);
        }
    }
    Err(errorf("a code-block claims more than 73 zero bit planes"))
}
fn read_pass_count(r: &mut BitReader<'_>) -> Result<i64> {
    if r.read_bit()? == 0 {
        return Ok(1);
    }
    if r.read_bit()? == 0 {
        return Ok(2);
    }
    let two = i64::from(r.read_bits(2)?);
    if two != 3 {
        return Ok(two.saturating_add(3));
    }
    let five = i64::from(r.read_bits(5)?);
    if five != 31 {
        return Ok(five.saturating_add(6));
    }
    let seven = i64::from(r.read_bits(7)?);
    Ok(seven.saturating_add(37))
}
fn read_lblock_increment(r: &mut BitReader<'_>, b: &mut BlockState) -> Result<()> {
    loop {
        if r.read_bit()? == 0 {
            return Ok(());
        }
        b.lblock = b.lblock.saturating_add(1);
        if b.lblock > 32 {
            return Err(errorf("a code-block length field grew past 32 bits"));
        }
    }
}
struct TagTreeLevel {
    w: i64,
    value: Vec<i32>,
    known: Vec<bool>,
}
struct TagTree {
    levels: Vec<TagTreeLevel>,
}
impl TagTree {
    fn new(mut w: i64, mut h: i64) -> TagTree {
        let mut t = TagTree { levels: Vec::new() };
        while w > 0 && h > 0 {
            let n = w.saturating_mul(h) as usize;
            t.levels.push(TagTreeLevel {
                w,
                value: vec![0; n],
                known: vec![false; n],
            });
            if w == 1 && h == 1 {
                break;
            }
            w = w.saturating_add(1) / 2;
            h = h.saturating_add(1) / 2;
        }
        t
    }
    fn decode(&mut self, r: &mut BitReader<'_>, leaf: i64, threshold: i32) -> Result<(i32, bool)> {
        let Some(leaves) = self.levels.first() else {
            return Err(errorf("tag tree has no levels"));
        };
        if leaf < 0 || leaf >= leaves.value.len() as i64 {
            return Err(errorf(format_args!(
                "tag tree leaf {leaf} of {}",
                leaves.value.len()
            )));
        }
        let (x, y) = (
            leaf.checked_rem(leaves.w).unwrap_or(0),
            leaf.checked_div(leaves.w).unwrap_or(0),
        );
        let mut lower = 0i32;
        for (level, node) in self.levels.iter_mut().enumerate().rev() {
            let index = (y >> level)
                .saturating_mul(node.w)
                .saturating_add(x >> level) as usize;
            let (Some(value), Some(known)) = (node.value.get_mut(index), node.known.get_mut(index))
            else {
                return Err(errorf(format_args!(
                    "tag tree node {index} of level {level}"
                )));
            };
            if *value < lower {
                *value = lower;
            }
            while !*known && *value < threshold {
                if r.read_bit()? == 1 {
                    *known = true;
                } else {
                    *value = value.saturating_add(1);
                }
            }
            lower = *value;
            if !*known {
                return Ok((lower, false));
            }
        }
        Ok((lower, true))
    }
}
#[derive(Clone)]
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    bits: u32,
    last: u8,
}
impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader {
            data,
            pos: 0,
            buf: 0,
            bits: 0,
            last: 0,
        }
    }
    fn read_bit(&mut self) -> Result<i32> {
        if self.bits == 0 {
            let Some(&b) = self.data.get(self.pos) else {
                return Err(errorf("packet header ended mid-value"));
            };
            self.pos = self.pos.saturating_add(1);
            if self.last == 0xFF {
                if b > 0x7F {
                    return Err(errorf(format_args!(
                        "packet header holds the marker {:02X}{b:02X}",
                        self.last
                    )));
                }
                self.buf = b as u32;
                self.bits = 7;
            } else {
                self.buf = b as u32;
                self.bits = 8;
            }
            self.last = b;
        }
        self.bits = self.bits.saturating_sub(1);
        Ok(((self.buf >> self.bits) & 1) as i32)
    }
    fn read_bits(&mut self, n: i64) -> Result<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let bit = self.read_bit()?;
            v = (v << 1) | bit as u32;
        }
        Ok(v)
    }
    fn align(&mut self) {
        self.bits = 0;
        if self.last == 0xFF {
            if self.pos < self.data.len() {
                self.pos = self.pos.saturating_add(1);
            }
            self.last = 0;
        }
    }
    fn offset(&self) -> usize {
        self.pos
    }
}
