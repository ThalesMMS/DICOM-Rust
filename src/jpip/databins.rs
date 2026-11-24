use super::codestream::{
    HeaderParameters, ImageSize, MARKER_COD, MARKER_PLM, MARKER_PLT, MARKER_POC, MARKER_SOT,
    MARKER_TLM, PROGRESSION_RPCL, SCOD_EPH, SCOD_SOP, TileGeometry, coding_parameters,
    new_tile_geometry, parse_header, precinct_bin_id, walk_segments,
};
use super::message::{
    CLASS_EXTENDED_PRECINCT, CLASS_EXTENDED_TILE, CLASS_MAIN_HEADER, CLASS_PRECINCT, CLASS_TILE,
    CLASS_TILE_HEADER, DataBinClass, Message, Stream,
};
use super::tier2::{PrecinctState, measure_packet};
use super::{ChainError, Sentinel, malformed};
use std::collections::HashMap;
pub(crate) const DEFAULT_MAX_RESPONSE_BYTES: i64 = 256 << 20;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DataBinKey {
    pub stream: u64,
    pub class: DataBinClass,
    pub id: u64,
}
#[derive(Default)]
struct DataBin {
    data: Vec<u8>,
    total: i64,
    segments: Vec<(u64, Vec<u8>)>,
}
impl DataBin {
    fn complete(&self) -> bool {
        self.total >= 0 && self.data.len() as i64 == self.total
    }
}
pub struct DataBinSet {
    max_bytes: i64,
    bytes: i64,
    bins: HashMap<DataBinKey, DataBin>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reconstruction {
    pub codestream: Vec<u8>,
    pub complete: bool,
}
fn incomplete(detail: &str) -> ChainError {
    ChainError::wrap(Sentinel::IncompleteDataBins, detail)
}
impl DataBinSet {
    pub fn new(max_bytes: i64) -> DataBinSet {
        DataBinSet {
            max_bytes: if max_bytes <= 0 {
                DEFAULT_MAX_RESPONSE_BYTES
            } else {
                max_bytes
            },
            bytes: 0,
            bins: HashMap::new(),
        }
    }
    pub fn add_stream(&mut self, stream: &Stream) -> Result<(), ChainError> {
        for m in &stream.messages {
            self.add(m)?;
        }
        Ok(())
    }
    pub fn add(&mut self, m: &Message) -> Result<(), ChainError> {
        let class = match m.class {
            CLASS_EXTENDED_PRECINCT => CLASS_PRECINCT,
            CLASS_EXTENDED_TILE => CLASS_TILE,
            c => c,
        };
        let key = DataBinKey {
            stream: m.stream,
            class,
            id: m.id,
        };
        let max_bytes = self.max_bytes;
        let bin = self.bins.entry(key).or_insert_with(|| DataBin {
            total: -1,
            ..DataBin::default()
        });
        let (end, overflow) = m.offset.overflowing_add(m.data.len() as u64);
        if overflow || end > max_bytes as u64 {
            return Err(ChainError::sentinel(Sentinel::DataBinLimit));
        }
        if m.complete {
            if bin.total >= 0 && bin.total as u64 != end {
                return Err(ChainError::wrap(
                    Sentinel::MalformedStream,
                    "conflicting data-bin lengths",
                ));
            }
            bin.total = end as i64;
        }
        if bin.total >= 0 && end > bin.total as u64 {
            return Err(ChainError::wrap(
                Sentinel::MalformedStream,
                "data past the end of a data-bin",
            ));
        }
        if m.data.is_empty() {
            return Ok(());
        }
        if m.offset > bin.data.len() as u64 {
            if self.bytes + m.data.len() as i64 > max_bytes {
                return Err(ChainError::sentinel(Sentinel::DataBinLimit));
            }
            self.bytes += m.data.len() as i64;
            bin.segments.push((m.offset, m.data.clone()));
            return Ok(());
        }
        extend(&mut self.bytes, max_bytes, bin, m.offset, &m.data)?;
        bin.segments.sort_by_key(|s| s.0);
        let mut segments = std::mem::take(&mut bin.segments);
        let mut kept = 0;
        for i in 0..segments.len() {
            let offset = segments[i].0;
            if offset <= bin.data.len() as u64 {
                self.bytes -= segments[i].1.len() as i64;
                let data = std::mem::take(&mut segments[i].1);
                let result = extend(&mut self.bytes, max_bytes, bin, offset, &data);
                segments[i].1 = data;
                if let Err(e) = result {
                    bin.segments = segments;
                    return Err(e);
                }
                continue;
            }
            segments.swap(kept, i);
            if kept != i {
                segments[i] = segments[kept].clone();
            }
            kept += 1;
        }
        segments.truncate(kept);
        bin.segments = segments;
        Ok(())
    }
    pub fn bytes(&self) -> i64 {
        self.bytes
    }
    fn bin(&self, stream: u64, class: DataBinClass, id: u64) -> Option<&DataBin> {
        self.bins.get(&DataBinKey { stream, class, id })
    }
    pub fn codestream(&self, stream: u64) -> Result<Reconstruction, ChainError> {
        let main = match self.bin(stream, CLASS_MAIN_HEADER, 0) {
            Some(b) if b.complete() => b,
            _ => return Err(incomplete("main header")),
        };
        let (params, size) = parse_header(&main.data, None, true)?;
        if params.cod.is_none() {
            return Err(malformed("missing COD"));
        }
        if self
            .bins
            .keys()
            .any(|k| k.stream == stream && k.class == CLASS_TILE)
        {
            return self.tile_codestream(stream, &main.data, &size);
        }
        self.precinct_codestream(stream, &main.data, &params, &size)
    }
    fn tile_codestream(
        &self,
        stream: u64,
        main: &[u8],
        size: &ImageSize,
    ) -> Result<Reconstruction, ChainError> {
        let mut out = filtered_header(main, false)?;
        let mut complete = true;
        let mut tiles = 0;
        for t in 0..size.num_tiles() {
            let bin = self.bin(stream, CLASS_TILE, t as u64);
            let Some(bin) = bin else {
                complete = false;
                continue;
            };
            if !bin.complete() {
                complete = false;
            }
            let data = &bin.data;
            let mut at = 0;
            while at + 12 <= data.len() {
                if u16::from_be_bytes([data[at], data[at + 1]]) != MARKER_SOT {
                    return Err(malformed("tile data-bin does not hold tile-parts"));
                }
                let mut psot =
                    u32::from_be_bytes([data[at + 6], data[at + 7], data[at + 8], data[at + 9]])
                        as usize;
                if psot == 0 && bin.complete() {
                    psot = data.len() - at;
                }
                if psot < 14 || at + psot > data.len() {
                    break;
                }
                let part = &data[at..at + psot];
                if psot == data.len() - at
                    && u32::from_be_bytes([part[6], part[7], part[8], part[9]]) == 0
                {
                    let mut copy = part.to_vec();
                    copy[6..10].copy_from_slice(&(psot as u32).to_be_bytes());
                    out.extend_from_slice(&copy);
                } else {
                    out.extend_from_slice(part);
                }
                at += psot;
                tiles += 1;
            }
        }
        if tiles == 0 {
            return Err(incomplete("no tile-part"));
        }
        out.extend_from_slice(&[0xff, 0xd9]);
        Ok(Reconstruction {
            codestream: out,
            complete,
        })
    }
    fn precinct_codestream(
        &self,
        stream: u64,
        main: &[u8],
        main_params: &HeaderParameters,
        size: &ImageSize,
    ) -> Result<Reconstruction, ChainError> {
        let relocated = "precinct data-bins with relocated packet headers or Part 2 geometry";
        if main_params.packed || main_params.extended {
            return Err(malformed(relocated));
        }
        let mut out = filtered_header(main, true)?;
        let mut complete = true;
        let mut tiles = 0;
        let num_tiles = size.num_tiles();
        for t in 0..num_tiles {
            let header_bin = match self.bin(stream, CLASS_TILE_HEADER, t as u64) {
                Some(b) if b.complete() => b,
                _ => {
                    complete = false;
                    continue;
                }
            };
            let (params, _) = parse_header(&header_bin.data, Some(size), false)?;
            if params.packed || params.extended {
                return Err(malformed(relocated));
            }
            let (style, comps, _) = coding_parameters(size, main_params, &params)?;
            let g = new_tile_geometry(size, t, &style, &comps, Vec::new())?;
            let tile_header = filtered_header(&header_bin.data, true)?;
            let mut empty_packet = vec![0u8];
            if g.scod & SCOD_EPH != 0 {
                empty_packet.extend_from_slice(&[0xff, 0x92]);
            }
            let mut packets: Vec<Option<Vec<Vec<u8>>>> = vec![None; g.precinct_count];
            for (c, comp) in g.components.iter().enumerate() {
                for (r, res) in comp.resolutions.iter().enumerate() {
                    for k in 0..res.precincts() {
                        let id = precinct_bin_id(
                            t,
                            c,
                            res.precinct_base + k,
                            g.components.len(),
                            num_tiles,
                        );
                        let i = g.precinct_index(c, r, k);
                        let Some(bin) = self.bin(stream, CLASS_PRECINCT, id) else {
                            complete = false;
                            continue;
                        };
                        let split = split_precinct(&g, c, r, k, bin);
                        if split.len() < g.layers {
                            complete = false;
                        }
                        packets[i] = Some(split);
                    }
                }
            }
            let mut body = Vec::new();
            g.for_each_packet(PROGRESSION_RPCL, &[], &mut |l, r, c, k| {
                match packets[g.precinct_index(c, r, k)]
                    .as_ref()
                    .and_then(|s| s.get(l))
                {
                    Some(packet) => body.extend_from_slice(packet),
                    None => body.extend_from_slice(&empty_packet),
                }
                Ok(())
            })?;
            let psot = 12 + tile_header.len() + 2 + body.len();
            if psot as u64 > (1u64 << 32) - 1 {
                return Err(ChainError::wrap(Sentinel::IndexLimit, "tile-part length"));
            }
            out.extend_from_slice(&MARKER_SOT.to_be_bytes());
            out.extend_from_slice(&10u16.to_be_bytes());
            out.extend_from_slice(&(t as u16).to_be_bytes());
            out.extend_from_slice(&(psot as u32).to_be_bytes());
            out.extend_from_slice(&[0, 1]);
            out.extend_from_slice(&tile_header);
            out.extend_from_slice(&[0xff, 0x93]);
            out.extend_from_slice(&body);
            tiles += 1;
        }
        if tiles == 0 {
            return Err(incomplete("no tile header"));
        }
        out.extend_from_slice(&[0xff, 0xd9]);
        Ok(Reconstruction {
            codestream: out,
            complete,
        })
    }
}
fn extend(
    bytes: &mut i64,
    max_bytes: i64,
    bin: &mut DataBin,
    offset: u64,
    data: &[u8],
) -> Result<(), ChainError> {
    let skip = (bin.data.len() as u64).wrapping_sub(offset);
    if skip >= data.len() as u64 {
        return Ok(());
    }
    let tail = &data[skip as usize..];
    if *bytes + tail.len() as i64 > max_bytes {
        return Err(ChainError::sentinel(Sentinel::DataBinLimit));
    }
    *bytes += tail.len() as i64;
    bin.data.extend_from_slice(tail);
    Ok(())
}
pub(crate) fn filtered_header(data: &[u8], rpcl: bool) -> Result<Vec<u8>, ChainError> {
    let mut out = Vec::with_capacity(data.len());
    walk_segments(data, &mut |seg| {
        match seg.marker {
            MARKER_TLM | MARKER_PLM | MARKER_PLT => return Ok(()),
            MARKER_POC if rpcl => return Ok(()),
            MARKER_COD if rpcl => {
                let start = out.len();
                out.extend_from_slice(&data[seg.start..seg.end]);
                out[start + 5] = PROGRESSION_RPCL;
                return Ok(());
            }
            _ => {}
        }
        out.extend_from_slice(&data[seg.start..seg.end]);
        Ok(())
    })?;
    Ok(out)
}
fn split_precinct(g: &TileGeometry, c: usize, r: usize, k: usize, bin: &DataBin) -> Vec<Vec<u8>> {
    if g.ht {
        if !bin.complete() {
            return Vec::new();
        }
        let mut out = vec![Vec::new(); g.layers];
        out[0] = bin.data.clone();
        return out;
    }
    let mut state = PrecinctState::new(&g.code_block_grids(c, r, k));
    let mut out = Vec::new();
    let mut pos = 0;
    let mut l = 0;
    while l < g.layers && pos < bin.data.len() {
        let Ok(end) = measure_packet(
            &bin.data,
            pos,
            &mut state,
            l,
            g.scod,
            g.components[c].coding.cmodes,
        ) else {
            break;
        };
        let mut packet = &bin.data[pos..end];
        if g.scod & SCOD_SOP != 0 && packet.len() >= 6 && packet[0] == 0xff && packet[1] == 0x91 {
            packet = &packet[6..];
        }
        out.push(packet.to_vec());
        pos = end;
        l += 1;
    }
    out
}
