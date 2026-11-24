use super::codestream::{
    HeaderParameters, ImageSize, MARKER_PLT, MAX_DECOMPOSITION_LEVELS, TileGeometry,
    coding_parameters, new_tile_geometry, parse_header, precinct_bin_id, walk_segments,
};
use super::plan::reduced_size;
use super::tier2::{PrecinctState, measure_packet};
use super::{ChainError, Sentinel, malformed};
use crate::pixel::encapsulated::{J2kLayout, J2kTilePart, parse_j2k_layout};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndexLimits {
    pub max_codestream_bytes: usize,
    pub max_packets: usize,
}
const DEFAULT_MAX_CODESTREAM_BYTES: usize = 512 << 20;
const DEFAULT_MAX_PACKETS: usize = 4 << 20;
impl IndexLimits {
    pub(crate) fn normalized(mut self) -> IndexLimits {
        if self.max_codestream_bytes == 0 {
            self.max_codestream_bytes = DEFAULT_MAX_CODESTREAM_BYTES;
        }
        if self.max_packets == 0 {
            self.max_packets = DEFAULT_MAX_PACKETS;
        }
        self
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ByteSpan {
    pub start: usize,
    pub end: usize,
}
impl ByteSpan {
    pub fn len(&self) -> usize {
        self.end - self.start
    }
}
#[derive(Clone, Debug, Default)]
pub(crate) struct IndexedPrecinct {
    pub id: u64,
    pub c: usize,
    pub r: usize,
    pub k: usize,
    pub packets: Vec<ByteSpan>,
    pub length: usize,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct IndexedTile {
    pub geometry: TileGeometry,
    pub parts: Vec<J2kTilePart>,
    pub header: Vec<u8>,
    pub length: usize,
    pub precincts: Vec<IndexedPrecinct>,
}
#[derive(Clone, Debug)]
pub struct CodestreamIndex {
    pub(crate) data: Vec<u8>,
    pub(crate) layout: J2kLayout,
    pub(crate) size: ImageSize,
    pub(crate) tiles: Vec<IndexedTile>,
    pub(crate) ht: bool,
    pub(crate) layers: usize,
    pub(crate) levels: usize,
    pub(crate) precincts: bool,
    pub(crate) used_plt: bool,
    pub(crate) packet_count: usize,
}
pub fn index_codestream(data: &[u8], limits: IndexLimits) -> Result<CodestreamIndex, ChainError> {
    index_with(data, limits, true)
}
pub(crate) fn index_with(
    data: &[u8],
    limits: IndexLimits,
    use_plt: bool,
) -> Result<CodestreamIndex, ChainError> {
    let limits = limits.normalized();
    if data.len() > limits.max_codestream_bytes {
        return Err(ChainError::wrap(Sentinel::IndexLimit, "codestream size"));
    }
    let layout = parse_j2k_layout(data)
        .map_err(|e| ChainError::join_text(Sentinel::MalformedCodestream, e))?;
    let (main, size) = parse_header(&data[..layout.main_header_end], None, true)?;
    if main.cod.is_none() {
        return Err(malformed("missing COD"));
    }
    let mut tiles = vec![IndexedTile::default(); size.num_tiles()];
    for part in &layout.tile_parts {
        if part.tile >= tiles.len() {
            return Err(malformed("tile index out of range"));
        }
        let tile = &mut tiles[part.tile];
        let out_of_order = match tile.parts.last() {
            Some(last) => part.part != last.part + 1,
            None => part.part != 0,
        };
        if out_of_order {
            return Err(malformed("tile-parts out of order"));
        }
        tile.parts.push(*part);
        tile.length += part.end - part.start;
    }
    let mut index = CodestreamIndex {
        data: data.to_vec(),
        layout,
        size,
        tiles,
        ht: false,
        layers: 0,
        levels: MAX_DECOMPOSITION_LEVELS,
        precincts: false,
        used_plt: false,
        packet_count: 0,
    };
    let mut tile_params = Vec::with_capacity(index.tiles.len());
    let mut precinct_err = false;
    for t in 0..index.tiles.len() {
        if index.tiles[t].parts.is_empty() {
            return Err(malformed("missing tile"));
        }
        let mut headers = Vec::new();
        let mut plt_complete = true;
        let mut plt = Vec::new();
        for part in &index.tiles[t].parts {
            let raw = &data[part.start + 12..part.header_end - 2];
            let (params, _) = parse_header(raw, Some(&index.size), false)?;
            if !params.plt_valid {
                plt_complete = false;
            }
            plt.extend(params.plt_lengths);
            walk_segments(raw, &mut |s| {
                if s.marker != MARKER_PLT {
                    headers.extend_from_slice(&raw[s.start..s.end]);
                }
                Ok(())
            })?;
        }
        let (mut params, _) = parse_header(&headers, Some(&index.size), false)?;
        params.plt_lengths = plt;
        params.plt_valid = plt_complete;
        let (style, comps, poc) = coding_parameters(&index.size, &main, &params)?;
        let geometry = new_tile_geometry(&index.size, t, &style, &comps, poc)?;
        index.ht = index.ht || geometry.ht;
        index.layers = index.layers.max(geometry.layers);
        for comp in &geometry.components {
            index.levels = index.levels.min(comp.coding.levels);
        }
        if main.packed || params.packed || main.extended || params.extended {
            precinct_err = true;
        }
        let tile = &mut index.tiles[t];
        tile.header = headers;
        tile.geometry = geometry;
        tile_params.push(params);
    }
    if !precinct_err {
        precinct_err = index
            .index_precincts(&tile_params, limits, use_plt)
            .is_err();
    }
    index.precincts = !precinct_err;
    if !index.precincts {
        for tile in &mut index.tiles {
            tile.precincts = Vec::new();
        }
    }
    Ok(index)
}
#[derive(Debug)]
pub(crate) struct NoPrecincts;
impl From<ChainError> for NoPrecincts {
    fn from(_: ChainError) -> NoPrecincts {
        NoPrecincts
    }
}
struct PacketCursor<'a> {
    parts: &'a [J2kTilePart],
    part: usize,
    pos: usize,
}
impl PacketCursor<'_> {
    fn new(parts: &[J2kTilePart]) -> PacketCursor<'_> {
        let mut c = PacketCursor {
            parts,
            part: 0,
            pos: parts[0].header_end,
        };
        c.skip_empty();
        c
    }
    fn skip_empty(&mut self) {
        while self.part < self.parts.len() && self.pos >= self.parts[self.part].end {
            self.part += 1;
            if self.part < self.parts.len() {
                self.pos = self.parts[self.part].header_end;
            }
        }
    }
    fn done(&self) -> bool {
        self.part >= self.parts.len()
    }
    fn limit(&self) -> usize {
        self.parts[self.part].end
    }
}
impl CodestreamIndex {
    fn index_precincts(
        &mut self,
        params: &[HeaderParameters],
        limits: IndexLimits,
        use_plt: bool,
    ) -> Result<(), NoPrecincts> {
        let all_plt = use_plt && params.iter().all(|p| p.plt_valid);
        if all_plt {
            match self.index_with_plt(params, limits) {
                Ok(()) => {
                    self.used_plt = true;
                    return Ok(());
                }
                Err(e) if self.ht => return Err(e),
                Err(_) => {}
            }
        }
        if self.ht {
            return Err(NoPrecincts);
        }
        self.index_with_packet_headers(limits)
    }
    fn new_precincts(&mut self, t: usize) {
        let num_tiles = self.tiles.len();
        let tile = &mut self.tiles[t];
        let g = &tile.geometry;
        let mut precincts = vec![IndexedPrecinct::default(); g.precinct_count];
        for (c, comp) in g.components.iter().enumerate() {
            for (r, res) in comp.resolutions.iter().enumerate() {
                for k in 0..res.precincts() {
                    let i = g.precinct_index(c, r, k);
                    precincts[i] = IndexedPrecinct {
                        id: precinct_bin_id(
                            t,
                            c,
                            res.precinct_base + k,
                            g.components.len(),
                            num_tiles,
                        ),
                        c,
                        r,
                        k,
                        ..IndexedPrecinct::default()
                    };
                }
            }
        }
        tile.precincts = precincts;
    }
    fn add_packet(
        packet_count: &mut usize,
        tile: &mut IndexedTile,
        c: usize,
        r: usize,
        k: usize,
        span: ByteSpan,
        limits: IndexLimits,
    ) -> Result<(), ChainError> {
        *packet_count += 1;
        if *packet_count > limits.max_packets {
            return Err(ChainError::wrap(Sentinel::IndexLimit, "packet count"));
        }
        let i = tile.geometry.precinct_index(c, r, k);
        let p = &mut tile.precincts[i];
        p.packets.push(span);
        p.length += span.len();
        Ok(())
    }
    fn index_with_plt(
        &mut self,
        params: &[HeaderParameters],
        limits: IndexLimits,
    ) -> Result<(), NoPrecincts> {
        self.packet_count = 0;
        for t in 0..self.tiles.len() {
            self.new_precincts(t);
            let lengths = &params[t].plt_lengths;
            let tile = &mut self.tiles[t];
            let parts = tile.parts.clone();
            let geometry = tile.geometry.clone();
            let mut cursor = PacketCursor::new(&parts);
            let mut n = 0;
            let packet_count = &mut self.packet_count;
            geometry.for_each_packet(geometry.progression, &geometry.poc, &mut |_l, r, c, k| {
                if cursor.done() || n >= lengths.len() {
                    return Err(malformed("fewer packets than the progression requires"));
                }
                let length = lengths[n];
                n += 1;
                if length > (cursor.limit() - cursor.pos) as u64 {
                    return Err(malformed("PLT length crosses a tile-part"));
                }
                let span = ByteSpan {
                    start: cursor.pos,
                    end: cursor.pos + length as usize,
                };
                cursor.pos = span.end;
                cursor.skip_empty();
                Self::add_packet(packet_count, tile, c, r, k, span, limits)
            })?;
            if !cursor.done() || n != lengths.len() {
                return Err(NoPrecincts);
            }
        }
        Ok(())
    }
    fn index_with_packet_headers(&mut self, limits: IndexLimits) -> Result<(), NoPrecincts> {
        self.packet_count = 0;
        for t in 0..self.tiles.len() {
            self.new_precincts(t);
            let tile = &mut self.tiles[t];
            let parts = tile.parts.clone();
            let g = tile.geometry.clone();
            let mut states: Vec<Option<PrecinctState>> =
                (0..g.precinct_count).map(|_| None).collect();
            let mut cursor = PacketCursor::new(&parts);
            let data = &self.data;
            let packet_count = &mut self.packet_count;
            g.for_each_packet(g.progression, &g.poc, &mut |l, r, c, k| {
                if cursor.done() {
                    return Err(malformed("fewer packets than the progression requires"));
                }
                let i = g.precinct_index(c, r, k);
                let state = states[i]
                    .get_or_insert_with(|| PrecinctState::new(&g.code_block_grids(c, r, k)));
                let end = measure_packet(
                    &data[..cursor.limit()],
                    cursor.pos,
                    state,
                    l,
                    g.scod,
                    g.components[c].coding.cmodes,
                )
                .map_err(|_| ChainError::sentinel(Sentinel::MalformedCodestream))?;
                let span = ByteSpan {
                    start: cursor.pos,
                    end,
                };
                cursor.pos = end;
                cursor.skip_empty();
                Self::add_packet(packet_count, tile, c, r, k, span, limits)
            })?;
            if !cursor.done() {
                return Err(NoPrecincts);
            }
        }
        Ok(())
    }
    pub fn precincts(&self) -> bool {
        self.precincts
    }
    pub fn htj2k(&self) -> bool {
        self.ht
    }
    pub fn size(&self) -> (i64, i64) {
        (self.size.x1 - self.size.x0, self.size.y1 - self.size.y0)
    }
    pub fn reduced_size(&self, discard: i64) -> (i64, i64) {
        reduced_size(&self.size, discard.max(0) as usize)
    }
    pub fn discard_levels(&self) -> usize {
        self.levels
    }
    pub fn layers(&self) -> usize {
        self.layers
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub(crate) fn main_header(&self) -> &[u8] {
        &self.data[..self.layout.main_header_end]
    }
}
