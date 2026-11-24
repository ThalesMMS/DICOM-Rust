use super::codestream::{ImageSize, MAX_DECOMPOSITION_LEVELS, ceil_div};
use super::databins::DataBinKey;
use super::index::CodestreamIndex;
use super::message::{
    CLASS_MAIN_HEADER, CLASS_METADATA, CLASS_PRECINCT, CLASS_TILE, CLASS_TILE_HEADER,
    EOR_BYTE_LIMIT_REACHED, EOR_IMAGE_DONE, EOR_RESPONSE_LIMIT_REACHED, EOR_WINDOW_DONE, EorReason,
    Message, append_eor, append_message, append_message_header,
};
use std::collections::HashMap;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ViewWindow {
    pub explicit_size: bool,
    pub fx: i64,
    pub fy: i64,
    pub round: String,
    pub ox: i64,
    pub oy: i64,
    pub sx: i64,
    pub sy: i64,
    pub has_offset: bool,
    pub has_region: bool,
    pub comps: Option<Vec<bool>>,
    pub layers: i64,
    pub length: i64,
}
impl Default for ViewWindow {
    fn default() -> ViewWindow {
        ViewWindow {
            explicit_size: false,
            fx: 0,
            fy: 0,
            round: "round-down".into(),
            ox: 0,
            oy: 0,
            sx: 0,
            sy: 0,
            has_offset: false,
            has_region: false,
            comps: None,
            layers: -1,
            length: -1,
        }
    }
}
pub(crate) struct ResponsePlan<'a> {
    index: &'a CodestreamIndex,
    stream: u64,
    kind: String,
    pub discard: usize,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    comps: Option<Vec<bool>>,
    layers: usize,
    pub fsiz_x: i64,
    pub fsiz_y: i64,
    whole: bool,
}
struct PlanChunk<'a> {
    key: DataBinKey,
    offset: i64,
    data: &'a [u8],
    bin_length: i64,
}
pub(crate) struct ResponseBody {
    pub data: Vec<u8>,
    pub messages: usize,
    pub eor: EorReason,
}
pub(crate) fn reduced_size(size: &ImageSize, d: usize) -> (i64, i64) {
    let scale = 1i64.wrapping_shl(d as u32);
    (
        ceil_div(size.x1, scale) - ceil_div(size.x0, scale),
        ceil_div(size.y1, scale) - ceil_div(size.y0, scale),
    )
}
pub(crate) fn choose_discard_levels(
    size: &ImageSize,
    levels: usize,
    fx: i64,
    fy: i64,
    round: &str,
) -> usize {
    match round {
        "round-up" => {
            for d in (0..=levels).rev() {
                let (w, h) = reduced_size(size, d);
                if w >= fx && h >= fy {
                    return d;
                }
            }
            0
        }
        "closest" => {
            let (mut best, mut best_distance) = (0, -1i64);
            for d in 0..=levels {
                let (w, h) = reduced_size(size, d);
                let distance = (w - fx).abs() + (h - fy).abs();
                if best_distance < 0 || distance < best_distance {
                    best = d;
                    best_distance = distance;
                }
            }
            best
        }
        _ => {
            for d in 0..=levels {
                let (w, h) = reduced_size(size, d);
                if w <= fx && h <= fy {
                    return d;
                }
            }
            levels
        }
    }
}
impl<'a> ResponsePlan<'a> {
    pub fn new(
        index: &'a CodestreamIndex,
        stream: u64,
        kind: &str,
        w: &ViewWindow,
    ) -> ResponsePlan<'a> {
        let size = &index.size;
        let mut p = ResponsePlan {
            index,
            stream,
            kind: kind.into(),
            discard: 0,
            x0: size.x0,
            y0: size.y0,
            x1: size.x1,
            y1: size.y1,
            comps: w.comps.clone(),
            layers: index.layers,
            fsiz_x: 0,
            fsiz_y: 0,
            whole: false,
        };
        if w.layers >= 0 && (w.layers as usize) < p.layers {
            p.layers = w.layers as usize;
        }
        if w.explicit_size {
            p.discard = choose_discard_levels(size, index.levels, w.fx, w.fy, &w.round);
            let d = p.discard as u32;
            let ox = ceil_div(size.x0, 1 << d);
            let oy = ceil_div(size.y0, 1 << d);
            if w.has_offset || w.has_region {
                let (rx0, ry0) = ((ox + w.ox) << d, (oy + w.oy) << d);
                let (mut rx1, mut ry1) = (size.x1, size.y1);
                if w.has_region {
                    rx1 = (ox + w.ox + w.sx) << d;
                    ry1 = (oy + w.oy + w.sy) << d;
                }
                p.x0 = rx0.max(size.x0);
                p.y0 = ry0.max(size.y0);
                p.x1 = rx1.min(size.x1);
                p.y1 = ry1.min(size.y1);
            }
        }
        let (fx, fy) = reduced_size(size, p.discard);
        p.fsiz_x = fx;
        p.fsiz_y = fy;
        let all_comps = (0..size.comps.len()).all(|c| p.component(c));
        p.whole = p.discard == 0
            && all_comps
            && p.layers >= index.layers
            && p.x0 == size.x0
            && p.y0 == size.y0
            && p.x1 == size.x1
            && p.y1 == size.y1;
        p
    }
    fn component(&self, c: usize) -> bool {
        match &self.comps {
            None => true,
            Some(mask) => c < mask.len() && mask[c],
        }
    }
    fn intersects(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> bool {
        x0 < self.x1 && self.x0 < x1 && y0 < self.y1 && self.y0 < y1
    }
    fn key(&self, class: u8, id: u64) -> DataBinKey {
        DataBinKey {
            stream: self.stream,
            class,
            id,
        }
    }
    fn each(&self, f: &mut dyn FnMut(PlanChunk<'a>) -> bool) {
        let x = self.index;
        let main = x.main_header();
        if !f(PlanChunk {
            key: self.key(CLASS_MAIN_HEADER, 0),
            offset: 0,
            data: main,
            bin_length: main.len() as i64,
        }) {
            return;
        }
        if !f(PlanChunk {
            key: self.key(CLASS_METADATA, 0),
            offset: 0,
            data: &[],
            bin_length: 0,
        }) {
            return;
        }
        let tiles: Vec<usize> = (0..x.tiles.len())
            .filter(|&t| {
                let g = &x.tiles[t].geometry;
                self.intersects(g.x0, g.y0, g.x1, g.y1)
            })
            .collect();
        if self.kind == "jpt-stream" {
            for &t in &tiles {
                let tile = &x.tiles[t];
                let mut offset = 0i64;
                for part in &tile.parts {
                    let chunk = PlanChunk {
                        key: self.key(CLASS_TILE, t as u64),
                        offset,
                        data: &x.data[part.start..part.end],
                        bin_length: tile.length as i64,
                    };
                    if !f(chunk) {
                        return;
                    }
                    offset += (part.end - part.start) as i64;
                }
            }
            return;
        }
        for &t in &tiles {
            let header = &x.tiles[t].header;
            if !f(PlanChunk {
                key: self.key(CLASS_TILE_HEADER, t as u64),
                offset: 0,
                data: header,
                bin_length: header.len() as i64,
            }) {
                return;
            }
        }
        let mut by_resolution: Vec<Vec<(usize, usize, i64)>> =
            vec![Vec::new(); MAX_DECOMPOSITION_LEVELS + 1];
        for &t in &tiles {
            let tile = &x.tiles[t];
            let g = &tile.geometry;
            for (i, prec) in tile.precincts.iter().enumerate() {
                let comp = &g.components[prec.c];
                let top = (comp.coding.levels as i64 - self.discard as i64).max(0);
                if !self.component(prec.c) || prec.r as i64 > top {
                    continue;
                }
                let (bx0, by0, bx1, by1) = g.precinct_bounds(prec.c, prec.r, prec.k);
                if !self.intersects(bx0, by0, bx1, by1) {
                    continue;
                }
                by_resolution[prec.r].push((t, i, 0));
            }
        }
        for group in &mut by_resolution {
            for l in 0..self.layers {
                for sel in group.iter_mut() {
                    let prec = &x.tiles[sel.0].precincts[sel.1];
                    if l >= prec.packets.len() {
                        continue;
                    }
                    let span = prec.packets[l];
                    let chunk = PlanChunk {
                        key: self.key(CLASS_PRECINCT, prec.id),
                        offset: sel.2,
                        data: &x.data[span.start..span.end],
                        bin_length: prec.length as i64,
                    };
                    sel.2 += span.len() as i64;
                    if !f(chunk) {
                        return;
                    }
                }
            }
        }
    }
    pub fn write(
        &self,
        model: &mut HashMap<DataBinKey, i64>,
        length: i64,
        max_bytes: usize,
    ) -> ResponseBody {
        let (mut limit, mut reason) = (max_bytes as i64 - 3, EOR_RESPONSE_LIMIT_REACHED);
        if length >= 0 && length < limit {
            limit = length;
            reason = EOR_BYTE_LIMIT_REACHED;
        }
        let mut body = ResponseBody {
            data: Vec::new(),
            messages: 0,
            eor: 0,
        };
        let mut stopped = false;
        self.each(&mut |mut c| {
            let known = model.get(&c.key).copied();
            let sent = known.unwrap_or(0);
            let end = c.offset + c.data.len() as i64;
            if known.is_some() && sent >= end {
                return true;
            }
            if sent > c.offset {
                c.data = &c.data[(sent - c.offset) as usize..];
                c.offset = sent;
            }
            let mut m = Message {
                class: c.key.class,
                stream: c.key.stream,
                id: c.key.id,
                offset: c.offset as u64,
                complete: end == c.bin_length,
                ..Message::default()
            };
            let mut header = Vec::new();
            append_message_header(&mut header, &m, c.data.len());
            let header = header.len() as i64;
            if body.data.len() as i64 + header + c.data.len() as i64 > limit {
                let available = limit - body.data.len() as i64 - header;
                if available > 0 {
                    m.complete = false;
                    m.data = c.data[..available as usize].to_vec();
                    append_message(&mut body.data, &m);
                    body.messages += 1;
                    model.insert(c.key, c.offset + available);
                }
                stopped = true;
                return false;
            }
            m.data = c.data.to_vec();
            append_message(&mut body.data, &m);
            body.messages += 1;
            model.insert(c.key, end);
            true
        });
        body.eor = if stopped {
            reason
        } else if self.whole {
            EOR_IMAGE_DONE
        } else {
            EOR_WINDOW_DONE
        };
        append_eor(&mut body.data, body.eor);
        body
    }
}
