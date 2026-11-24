use super::bitio::{
    BitReader, bits_n, bits_off, ceil_log2, div_ceil, floor_log2, unpack_signed, unpack_signed64,
    val,
};
use super::codestream::{
    COLOUR_SPACE_GREY, CodestreamHeader, FLAG_NOISE, FLAG_PATCHES, FLAG_SPLINES, FrameHeader,
};
use super::entropy::{EntropyCode, SymbolReader, read_entropy_code};
use super::transforms::{meta_apply, undo_transforms};
use super::{Error, corrupt, limit, unsupported};
pub(crate) const TRANSFORM_RCT: u32 = 0;
pub(crate) const TRANSFORM_PALETTE: u32 = 1;
pub(crate) const TRANSFORM_SQUEEZE: u32 = 2;
pub(crate) const PRED_WEIGHTED: u8 = 6;
pub(crate) const NUM_PREDICTOR: u32 = 14;
pub(crate) const NUM_QUANT_TABLES: usize = 17;
#[derive(Clone, Default, Debug)]
pub(crate) struct Channel {
    pub w: usize,
    pub h: usize,
    pub hshift: i64,
    pub vshift: i64,
    pub data: Vec<i32>,
}
impl Channel {
    pub(crate) fn new(w: usize, h: usize) -> Self {
        Channel {
            w,
            h,
            hshift: 0,
            vshift: 0,
            data: vec![0; w.saturating_mul(h)],
        }
    }
}
#[derive(Default)]
pub(crate) struct ModularImage {
    pub channels: Vec<Channel>,
    pub nb_meta: i64,
    pub bit_depth: u32,
}
#[derive(Clone, Copy)]
pub(crate) struct WpHeader {
    pub p1c: i64,
    pub p2c: i64,
    pub p3ca: i64,
    pub p3cb: i64,
    pub p3cc: i64,
    pub p3cd: i64,
    pub p3ce: i64,
    pub w: [u32; 4],
}
pub(crate) const DEFAULT_WP: WpHeader = WpHeader {
    p1c: 16,
    p2c: 10,
    p3ca: 7,
    p3cb: 7,
    p3cc: 7,
    p3cd: 0,
    p3ce: 0,
    w: [0xd, 0xc, 0xc, 0xc],
};
fn read_wp_header(br: &mut BitReader) -> WpHeader {
    let mut h = DEFAULT_WP;
    if br.bool() {
        return h;
    }
    h.p1c = i64::from(br.u(5));
    h.p2c = i64::from(br.u(5));
    h.p3ca = i64::from(br.u(5));
    h.p3cb = i64::from(br.u(5));
    h.p3cc = i64::from(br.u(5));
    h.p3cd = i64::from(br.u(5));
    h.p3ce = i64::from(br.u(5));
    for w in h.w.iter_mut() {
        *w = br.u(4);
    }
    h
}
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct SqueezeParams {
    pub horizontal: bool,
    pub in_place: bool,
    pub begin_c: i64,
    pub num_c: i64,
}
#[derive(Clone, Default, Debug)]
pub(crate) struct TransformInfo {
    pub id: u32,
    pub begin_c: i64,
    pub rct_type: u32,
    pub num_c: i64,
    pub nb_colours: i64,
    pub nb_deltas: i64,
    pub d_pred: u32,
    pub squeezes: Vec<SqueezeParams>,
}
#[derive(Clone)]
pub(crate) struct GroupHeader {
    pub use_global_tree: bool,
    pub wp: WpHeader,
    pub transforms: Vec<TransformInfo>,
}
impl Default for GroupHeader {
    fn default() -> Self {
        GroupHeader {
            use_global_tree: false,
            wp: DEFAULT_WP,
            transforms: Vec::new(),
        }
    }
}
fn read_group_header(br: &mut BitReader) -> Result<GroupHeader, Error> {
    let mut g = GroupHeader {
        use_global_tree: br.bool(),
        wp: read_wp_header(br),
        transforms: Vec::new(),
    };
    let n = br.u32(val(0), val(1), bits_off(4, 2), bits_off(8, 18));
    if n > 512 {
        return Err(corrupt("too many transforms"));
    }
    let begin_c = |br: &mut BitReader| {
        i64::from(br.u32(
            bits_n(3),
            bits_off(6, 8),
            bits_off(10, 72),
            bits_off(13, 1096),
        ))
    };
    for _ in 0..n {
        let mut t = TransformInfo {
            id: br.u32(val(0), val(1), val(2), val(3)),
            ..Default::default()
        };
        match t.id {
            TRANSFORM_RCT => {
                t.begin_c = begin_c(br);
                t.rct_type = br.u32(val(6), bits_n(2), bits_off(4, 2), bits_off(6, 10));
                if t.rct_type >= 42 {
                    return Err(corrupt("RCT type"));
                }
            }
            TRANSFORM_PALETTE => {
                t.begin_c = begin_c(br);
                t.num_c = i64::from(br.u32(val(1), val(3), val(4), bits_off(13, 1)));
                t.nb_colours = i64::from(br.u32(
                    bits_off(8, 0),
                    bits_off(10, 256),
                    bits_off(12, 1280),
                    bits_off(16, 5376),
                ));
                t.nb_deltas = i64::from(br.u32(
                    val(0),
                    bits_off(8, 1),
                    bits_off(10, 257),
                    bits_off(16, 1281),
                ));
                t.d_pred = br.u(4);
                if t.d_pred >= NUM_PREDICTOR {
                    return Err(corrupt("palette predictor"));
                }
            }
            TRANSFORM_SQUEEZE => {
                let ns = br.u32(val(0), bits_off(4, 1), bits_off(6, 9), bits_off(8, 41));
                for _ in 0..ns {
                    let horizontal = br.bool();
                    let in_place = br.bool();
                    let b = begin_c(br);
                    let num_c = i64::from(br.u32(val(1), val(2), val(3), bits_off(4, 4)));
                    t.squeezes.push(SqueezeParams {
                        horizontal,
                        in_place,
                        begin_c: b,
                        num_c,
                    });
                }
            }
            _ => return Err(corrupt("invalid transform")),
        }
        g.transforms.push(t);
        br.check()?;
    }
    br.check()?;
    Ok(g)
}
#[derive(Clone, Copy, Default)]
pub(crate) struct TreeNode {
    pub property: i32,
    pub split_val: i32,
    pub lchild: i32,
    pub rchild: i32,
    pub predictor: u8,
    pub offset: i64,
    pub multiplier: i64,
}
pub(crate) struct MaTree {
    pub nodes: Vec<TreeNode>,
    pub max_prop: i64,
    pub uses_wp: bool,
}
impl MaTree {
    pub(crate) fn num_leaves(&self) -> usize {
        self.nodes.len().div_ceil(2)
    }
}
pub(crate) fn read_tree(br: &mut BitReader, size_limit: usize) -> Result<MaTree, Error> {
    let code = read_entropy_code(br, 6)?;
    let mut sr = SymbolReader::new(&code, br, 0);
    let mut t = MaTree {
        nodes: Vec::new(),
        max_prop: -1,
        uses_wp: false,
    };
    let mut to_decode: i64 = 1;
    let mut leaf: i32 = 0;
    while to_decode > 0 {
        br.check()?;
        if t.nodes.len() > size_limit {
            return Err(corrupt("MA tree too large"));
        }
        to_decode = to_decode.saturating_sub(1);
        let prop = sr.read_uint(br, 1);
        if prop > 256 {
            return Err(corrupt("tree property"));
        }
        if prop == 0 {
            let pred = sr.read_uint(br, 2);
            if pred >= NUM_PREDICTOR {
                return Err(corrupt("tree predictor"));
            }
            let off = unpack_signed64(u64::from(sr.read_uint(br, 3)));
            let mul_log = sr.read_uint(br, 4);
            if mul_log >= 31 {
                return Err(corrupt("tree multiplier log"));
            }
            let mul_bits = sr.read_uint(br, 5);
            if u64::from(mul_bits) >= (1u64 << 31u32.saturating_sub(mul_log)).saturating_sub(1) {
                return Err(corrupt("tree multiplier"));
            }
            t.nodes.push(TreeNode {
                property: -1,
                lchild: leaf,
                predictor: pred as u8,
                offset: off,
                multiplier: i64::from(mul_bits).saturating_add(1) << mul_log,
                ..Default::default()
            });
            if pred as u8 == PRED_WEIGHTED {
                t.uses_wp = true;
            }
            leaf = leaf.wrapping_add(1);
            continue;
        }
        let split = unpack_signed(sr.read_uint(br, 0));
        let property = prop.saturating_sub(1);
        let first = (t.nodes.len() as i64).saturating_add(to_decode);
        t.nodes.push(TreeNode {
            property: property as i32,
            split_val: split,
            lchild: first.saturating_add(1) as i32,
            rchild: first.saturating_add(2) as i32,
            ..Default::default()
        });
        t.max_prop = t.max_prop.max(i64::from(property));
        if property == 15 {
            t.uses_wp = true;
        }
        to_decode = to_decode.saturating_add(2);
    }
    if !sr.final_state_ok() {
        return Err(corrupt("MA tree ANS final state"));
    }
    br.check()?;
    Ok(t)
}
#[derive(Default)]
pub(crate) struct ModularContext {
    pub tree: Option<MaTree>,
    pub code: Option<EntropyCode>,
}
pub(crate) fn decode_modular(
    br: &mut BitReader,
    img: &mut ModularImage,
    stream_id: usize,
    global: Option<&ModularContext>,
    max_chan_size: usize,
    undo: bool,
) -> Result<GroupHeader, Error> {
    if img.channels.is_empty() {
        return Ok(GroupHeader::default());
    }
    let req: Vec<(usize, usize)> = img.channels.iter().map(|c| (c.w, c.h)).collect();
    let mut hdr = read_group_header(br)?;
    let global_tree = global.and_then(|g| g.tree.as_ref().zip(g.code.as_ref()));
    if hdr.use_global_tree && global_tree.is_none() {
        return Err(corrupt("global tree used but absent"));
    }
    for t in hdr.transforms.iter_mut() {
        if br.guarded
            && t.id == TRANSFORM_PALETTE
            && t.num_c > 0
            && t.nb_colours.saturating_add(t.nb_deltas)
                > (1i64 << 22).checked_div(t.num_c).unwrap_or(0)
        {
            return Err(limit());
        }
        meta_apply(img, t)?;
    }
    let coded = |i: usize, c: &Channel, nb_meta: i64| {
        !(i as i64 >= nb_meta && (c.w > max_chan_size || c.h > max_chan_size))
    };
    let mut dist_mult = 0;
    let mut num_chans = 0usize;
    for (i, c) in img.channels.iter().enumerate() {
        if c.w == 0 || c.h == 0 {
            continue;
        }
        if !coded(i, c, img.nb_meta) {
            break;
        }
        dist_mult = dist_mult.max(c.w);
        num_chans = num_chans.saturating_add(1);
    }
    if num_chans > 0 {
        let local;
        let (tree, code) = match (hdr.use_global_tree, global_tree) {
            (true, Some(g)) => g,
            _ => {
                let mut lim: usize = 1024;
                for c in &img.channels {
                    lim = lim.saturating_add(c.w.saturating_mul(c.h));
                }
                let lim = lim.min(1 << 20);
                let tree = read_tree(br, lim)?;
                let code = read_entropy_code(br, tree.num_leaves())?;
                local = (tree, code);
                (&local.0, &local.1)
            }
        };
        let mut sr = SymbolReader::new(code, br, dist_mult);
        for i in 0..img.channels.len() {
            let Some(c) = img.channels.get(i) else {
                break;
            };
            if c.w == 0 || c.h == 0 {
                continue;
            }
            if !coded(i, c, img.nb_meta) {
                break;
            }
            let mut dec = ChannelDecoder {
                br: &mut *br,
                sr: &mut sr,
            };
            walk_channel(tree, &hdr.wp, img, i, stream_id, &mut dec)?;
        }
        if !sr.final_state_ok() {
            return Err(corrupt("modular ANS final state"));
        }
    }
    if undo {
        undo_transforms(img, &hdr.transforms, &hdr.wp)?;
        if img.channels.len() != req.len() {
            return Err(corrupt("transforms changed the channel count"));
        }
        for (c, &(w, h)) in img.channels.iter().zip(&req) {
            if c.w != w || c.h != h {
                return Err(corrupt("transforms changed channel dimensions"));
            }
        }
    }
    br.check()?;
    Ok(hdr)
}
fn clamped_gradient(n: i64, w: i64, l: i64) -> i64 {
    let (lo, hi) = (n.min(w), n.max(w));
    if l < lo {
        return hi;
    }
    if l > hi {
        return lo;
    }
    n.wrapping_add(w).wrapping_sub(l)
}
fn select_predictor(a: i64, b: i64, c: i64) -> i64 {
    let p = a.wrapping_add(b).wrapping_sub(c);
    if p.wrapping_sub(a).wrapping_abs() < p.wrapping_sub(b).wrapping_abs() {
        a
    } else {
        b
    }
}
pub(crate) struct WpState {
    h: WpHeader,
    pred_err: [Vec<u32>; 4],
    err: Vec<i32>,
    prediction: [i64; 4],
    pred: i64,
    w: usize,
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn wp_div_lookup() -> [u32; 64] {
    let mut t = [0u32; 64];
    let mut i = 0;
    while i < 64 {
        t[i] = (1 << 24) / (i as u32 + 1);
        i += 1;
    }
    t
}
const WP_DIV_LOOKUP: [u32; 64] = wp_div_lookup();
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn error_weight(x: u64, max_weight: u32) -> u32 {
    let shift = (floor_log2(x + 1) as i64 - 5).max(0) as u32;
    4 + ((u64::from(max_weight) * u64::from(WP_DIV_LOOKUP[(x >> shift) as usize])) >> shift) as u32
}
impl WpState {
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn new(h: WpHeader, w: usize) -> Self {
        let n = (w + 2) * 2;
        WpState {
            h,
            pred_err: [vec![0; n], vec![0; n], vec![0; n], vec![0; n]],
            err: vec![0; n],
            prediction: [0; 4],
            pred: 0,
            w,
        }
    }
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn predict(
        &mut self,
        x: usize,
        y: usize,
        n: i64,
        w: i64,
        ne: i64,
        nw: i64,
        nn: i64,
    ) -> (i64, i64) {
        let xs = self.w;
        let (cur, prev) = if y & 1 == 1 { (0, xs + 2) } else { (xs + 2, 0) };
        let pos_n = prev + x;
        let pos_ne = if x + 1 < xs { pos_n + 1 } else { pos_n };
        let pos_nw = if x > 0 { pos_n - 1 } else { pos_n };
        let mut weights = [0u32; 4];
        for (i, wt) in weights.iter_mut().enumerate() {
            let pe = &self.pred_err[i];
            let e = u64::from(pe[pos_n]) + u64::from(pe[pos_ne]) + u64::from(pe[pos_nw]);
            *wt = error_weight(u64::from(e as u32), self.h.w[i]);
        }
        let (n, w, ne, nw, nn) = (n << 3, w << 3, ne << 3, nw << 3, nn << 3);
        let te_w = if x > 0 {
            i64::from(self.err[cur + x - 1])
        } else {
            0
        };
        let te_n = i64::from(self.err[pos_n]);
        let te_nw = i64::from(self.err[pos_nw]);
        let te_ne = i64::from(self.err[pos_ne]);
        let sum_wn = te_n + te_w;
        let mut prop = te_w;
        for t in [te_n, te_nw, te_ne] {
            if t.abs() > prop.abs() {
                prop = t;
            }
        }
        let h = &self.h;
        self.prediction[0] = w.wrapping_add(ne).wrapping_sub(n);
        self.prediction[1] = n.wrapping_sub((sum_wn + te_ne).wrapping_mul(h.p1c) >> 5);
        self.prediction[2] = w.wrapping_sub((sum_wn + te_nw).wrapping_mul(h.p2c) >> 5);
        let p3 = te_nw
            .wrapping_mul(h.p3ca)
            .wrapping_add(te_n.wrapping_mul(h.p3cb))
            .wrapping_add(te_ne.wrapping_mul(h.p3cc))
            .wrapping_add(nn.wrapping_sub(n).wrapping_mul(h.p3cd))
            .wrapping_add(nw.wrapping_sub(w).wrapping_mul(h.p3ce));
        self.prediction[3] = n.wrapping_sub(p3 >> 5);
        let wsum: u32 = weights.iter().sum();
        let logw = floor_log2(u64::from(wsum));
        let mut wsum = 0u32;
        for wt in weights.iter_mut() {
            *wt >>= logw - 4;
            wsum += *wt;
        }
        let mut sum = i64::from(wsum >> 1) - 1;
        for i in 0..4 {
            sum = sum.wrapping_add(self.prediction[i].wrapping_mul(i64::from(weights[i])));
        }
        self.pred = sum.wrapping_mul(i64::from(WP_DIV_LOOKUP[(wsum - 1) as usize])) >> 24;
        if ((te_n ^ te_w) | (te_n ^ te_nw)) > 0 {
            return ((self.pred + 3) >> 3, prop);
        }
        let mx = w.max(ne.max(n));
        let mn = w.min(ne.min(n));
        self.pred = mn.max(mx.min(self.pred));
        ((self.pred + 3) >> 3, prop)
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn update(&mut self, val: i64, x: usize, y: usize) {
        let xs = self.w;
        let (cur, prev) = if y & 1 == 1 { (0, xs + 2) } else { (xs + 2, 0) };
        let val = val << 3;
        self.err[cur + x] = self.pred.wrapping_sub(val) as i32;
        for i in 0..4 {
            let e = (self.prediction[i].wrapping_sub(val).wrapping_abs() + 3) >> 3;
            self.pred_err[i][cur + x] = e as u32;
            self.pred_err[i][prev + x + 1] = self.pred_err[i][prev + x + 1].wrapping_add(e as u32);
        }
    }
}
#[allow(clippy::too_many_arguments)]
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn predict_value(
    p: u8,
    w: i64,
    n: i64,
    nw: i64,
    ne: i64,
    nn: i64,
    ww: i64,
    nee: i64,
    wp: i64,
) -> i64 {
    match p {
        1 => w,
        2 => n,
        3 => (w + n) / 2,
        4 => select_predictor(w, n, nw),
        5 => clamped_gradient(n, w, nw),
        6 => wp,
        7 => ne,
        8 => nw,
        9 => ww,
        10 => (w + nw) / 2,
        11 => (n + nw) / 2,
        12 => (n + ne) / 2,
        13 => (6 * n - 2 * nn + 7 * w + ww + nee + 3 * ne + 8) / 16,
        _ => 0,
    }
}
pub(crate) trait PixelCoder {
    fn code(&mut self, node: &TreeNode, pred: i64, cur: i32) -> i32;
    fn row_done(&mut self) -> Result<(), Error>;
}
struct ChannelDecoder<'r, 'a, 'c> {
    br: &'r mut BitReader<'a>,
    sr: &'r mut SymbolReader<'c>,
}
impl PixelCoder for ChannelDecoder<'_, '_, '_> {
    fn code(&mut self, node: &TreeNode, pred: i64, _cur: i32) -> i32 {
        let token = self.sr.read_uint(self.br, node.lchild as usize);
        i64::from(unpack_signed(token))
            .wrapping_mul(node.multiplier)
            .wrapping_add(node.offset)
            .wrapping_add(pred) as i32
    }
    fn row_done(&mut self) -> Result<(), Error> {
        self.br.check()
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn walk_channel(
    tree: &MaTree,
    wph: &WpHeader,
    img: &mut ModularImage,
    ci: usize,
    stream_id: usize,
    coder: &mut dyn PixelCoder,
) -> Result<(), Error> {
    let Some((before, rest)) = img.channels.split_at_mut_checked(ci) else {
        return Err(corrupt("modular channel index"));
    };
    let Some(ch) = rest.first_mut() else {
        return Err(corrupt("modular channel index"));
    };
    let (w, h) = (ch.w, ch.h);
    let nodes = &tree.nodes;
    if nodes.is_empty()
        || ch.data.len() != w.saturating_mul(h)
        || nodes.iter().any(|n| {
            n.property >= 0
                && (n.lchild < 0
                    || n.rchild < 0
                    || n.lchild as usize >= nodes.len()
                    || n.rchild as usize >= nodes.len()
                    || i64::from(n.property) > tree.max_prop)
        })
    {
        return Err(super::fault());
    }
    let mut num_props = 16;
    let mut num_ref = 0;
    if tree.max_prop >= 16 {
        num_ref = (tree.max_prop.saturating_sub(12) / 4) as usize;
        num_props = num_ref.saturating_mul(4).saturating_add(16);
    }
    let refs: Vec<&Channel> = before
        .iter()
        .rev()
        .filter(|o| {
            o.w == w
                && o.h == h
                && o.hshift == ch.hshift
                && o.vshift == ch.vshift
                && o.data.len() == ch.data.len()
        })
        .take(num_ref)
        .collect();
    let single_leaf = nodes.len() == 1;
    let mut wp = tree.uses_wp.then(|| WpState::new(*wph, w));
    let mut props = vec![0i64; num_props];
    props[0] = ci as i64;
    props[1] = stream_id as i64;
    let data = &mut ch.data;
    for y in 0..h {
        props[2] = y as i64;
        props[9] = 0;
        for x in 0..w {
            let at = |xx: usize, yy: usize| i64::from(data[yy * w + xx]);
            let ww_ = if x > 0 {
                at(x - 1, y)
            } else if y > 0 {
                at(x, y - 1)
            } else {
                0
            };
            let n = if y > 0 { at(x, y - 1) } else { ww_ };
            let nw = if x > 0 && y > 0 {
                at(x - 1, y - 1)
            } else {
                ww_
            };
            let ne = if x + 1 < w && y > 0 {
                at(x + 1, y - 1)
            } else {
                n
            };
            let wwv = if x > 1 { at(x - 2, y) } else { ww_ };
            let nn = if y > 1 { at(x, y - 2) } else { n };
            let nee = if x + 2 < w && y > 0 {
                at(x + 2, y - 1)
            } else {
                ne
            };
            let w_ = ww_;
            let mut wp_pred = 0;
            if let Some(wp) = wp.as_mut() {
                let (p, e) = wp.predict(x, y, n, w_, ne, nw, nn);
                wp_pred = p;
                props[15] = e;
            }
            let mut node = &nodes[0];
            if !single_leaf {
                props[3] = x as i64;
                props[4] = n.wrapping_abs();
                props[5] = w_.wrapping_abs();
                props[6] = n;
                props[7] = w_;
                props[8] = w_.wrapping_sub(props[9]);
                props[9] = w_.wrapping_add(n).wrapping_sub(nw);
                props[10] = w_.wrapping_sub(nw);
                props[11] = nw.wrapping_sub(n);
                props[12] = n.wrapping_sub(ne);
                props[13] = n.wrapping_sub(nn);
                props[14] = w_.wrapping_sub(wwv);
                for (r, rc) in refs.iter().enumerate() {
                    let rc = &rc.data;
                    let v = i64::from(rc[y * w + x]);
                    let vl = if x > 0 {
                        i64::from(rc[y * w + x - 1])
                    } else {
                        0
                    };
                    let vt = if y > 0 {
                        i64::from(rc[(y - 1) * w + x])
                    } else {
                        vl
                    };
                    let vtl = if x > 0 && y > 0 {
                        i64::from(rc[(y - 1) * w + x - 1])
                    } else {
                        vl
                    };
                    let pv = clamped_gradient(vt, vl, vtl);
                    let base = 16 + 4 * r;
                    props[base] = v.wrapping_abs();
                    props[base + 1] = v;
                    props[base + 2] = v.wrapping_sub(pv).wrapping_abs();
                    props[base + 3] = v.wrapping_sub(pv);
                }
                while node.property >= 0 {
                    node = if props[node.property as usize] > i64::from(node.split_val) {
                        &nodes[node.lchild as usize]
                    } else {
                        &nodes[node.rchild as usize]
                    };
                }
            }
            let pred = predict_value(node.predictor, w_, n, nw, ne, nn, wwv, nee, wp_pred);
            let cur = data[y * w + x];
            let v = coder.code(node, pred, cur);
            data[y * w + x] = v;
            if let Some(wp) = wp.as_mut() {
                wp.update(i64::from(v), x, y);
            }
        }
        coder.row_done()?;
    }
    coder.row_done()
}
pub(crate) struct ModularFrame<'f> {
    f: &'f FrameHeader,
    img: ModularImage,
    global: ModularContext,
}
pub(crate) fn decode_modular_frame(
    hdr: &CodestreamHeader,
    f: &FrameHeader,
    readers: &mut [BitReader],
) -> Result<ModularImage, Error> {
    let m = &hdr.meta;
    if f.flags & (FLAG_PATCHES | FLAG_SPLINES | FLAG_NOISE) != 0 {
        return Err(unsupported("patches, splines or noise"));
    }
    if m.xyb_encoded {
        return Err(unsupported("XYB-encoded modular frame"));
    }
    if f.do_ycbcr {
        return Err(unsupported("YCbCr modular frame"));
    }
    if m.depth.float_sample || m.depth.bits_per_sample > 31 {
        return Err(unsupported(format!(
            "{}-bit or floating-point samples",
            m.depth.bits_per_sample
        )));
    }
    let single = readers.len() == 1;
    let section = |i: usize| if single { 0 } else { i };
    let Some(br) = readers.first_mut() else {
        return Err(corrupt("modular frame without sections"));
    };
    if !br.bool() {
        for _ in 0..3 {
            br.f16();
        }
    }
    let nb = if m.colour.colour_space == COLOUR_SPACE_GREY {
        1
    } else {
        3
    };
    let mut mf = ModularFrame {
        f,
        img: ModularImage {
            bit_depth: m.depth.bits_per_sample,
            ..Default::default()
        },
        global: ModularContext::default(),
    };
    if br.bool() {
        let samples = f
            .xsize
            .saturating_mul(f.ysize)
            .saturating_mul(m.extra_channels.len().saturating_add(nb));
        let lim = (1usize << 22).min((samples / 16).saturating_add(1024));
        let tree = read_tree(br, lim)?;
        let code = read_entropy_code(br, tree.num_leaves())?;
        mf.global = ModularContext {
            tree: Some(tree),
            code: Some(code),
        };
    }
    for _ in 0..nb {
        mf.img.channels.push(Channel::new(f.xsize, f.ysize));
    }
    for &up in &f.ec_upsampling {
        let up = up as usize;
        let mut ch = Channel::new(div_ceil(f.xsize, up), div_ceil(f.ysize, up));
        let shift = i64::from(ceil_log2(up as u64));
        ch.hshift = shift;
        ch.vshift = shift;
        mf.img.channels.push(ch);
    }
    let hdr_g = decode_modular(br, &mut mf.img, 0, Some(&mf.global), f.group_dim, false)?;
    for g in 0..f.num_lf_groups {
        let gx = g.checked_rem(f.x_lf_groups).unwrap_or(0);
        let gy = g.checked_div(f.x_lf_groups).unwrap_or(0);
        let d = f.group_dim.saturating_mul(8);
        let Some(br) = readers.get_mut(section(g.saturating_add(1))) else {
            return Err(corrupt("modular section beyond the table of contents"));
        };
        mf.decode_group(
            br,
            gx.saturating_mul(d),
            gy.saturating_mul(d),
            d,
            3,
            1000,
            f.num_lf_groups.saturating_add(g).saturating_add(1),
        )?;
    }
    for p in 0..f.num_passes as usize {
        let (min_shift, max_shift) = (0, 2);
        for g in 0..f.num_groups {
            let gx = g.checked_rem(f.x_groups).unwrap_or(0);
            let gy = g.checked_div(f.x_groups).unwrap_or(0);
            let pass_group = f.num_groups.saturating_mul(p).saturating_add(g);
            let stream = f
                .num_lf_groups
                .saturating_mul(3)
                .saturating_add(1 + NUM_QUANT_TABLES)
                .saturating_add(pass_group);
            let idx = section(f.num_lf_groups.saturating_add(2).saturating_add(pass_group));
            let Some(br) = readers.get_mut(idx) else {
                return Err(corrupt("modular section beyond the table of contents"));
            };
            mf.decode_group(
                br,
                gx.saturating_mul(f.group_dim),
                gy.saturating_mul(f.group_dim),
                f.group_dim,
                min_shift,
                max_shift,
                stream,
            )?;
        }
    }
    undo_transforms(&mut mf.img, &hdr_g.transforms, &hdr_g.wp)?;
    if mf.img.channels.len() != m.extra_channels.len().saturating_add(nb) {
        return Err(corrupt("channel count after transforms"));
    }
    Ok(mf.img)
}
impl ModularFrame<'_> {
    #[allow(clippy::too_many_arguments)]
    fn decode_group(
        &mut self,
        br: &mut BitReader,
        x0: usize,
        y0: usize,
        size: usize,
        min_shift: i64,
        max_shift: i64,
        stream: usize,
    ) -> Result<(), Error> {
        let full = &self.img;
        let mut gi = ModularImage {
            bit_depth: full.bit_depth,
            ..Default::default()
        };
        let mut places = Vec::new();
        let mut c = full.nb_meta.max(0) as usize;
        while let Some(fc) = full.channels.get(c) {
            if fc.w > self.f.group_dim || fc.h > self.f.group_dim {
                break;
            }
            c = c.saturating_add(1);
        }
        for (ci, fc) in full.channels.iter().enumerate().skip(c) {
            let shift = fc.hshift.min(fc.vshift);
            if shift > max_shift || shift < min_shift {
                continue;
            }
            let (hs, vs) = (fc.hshift.clamp(0, 63) as u32, fc.vshift.clamp(0, 63) as u32);
            let (rx, ry) = (x0 >> hs, y0 >> vs);
            let rw = (size >> hs) as i64;
            let rw = rw.min((fc.w as i64).saturating_sub(rx as i64));
            let rh = ((size >> vs) as i64).min((fc.h as i64).saturating_sub(ry as i64));
            if rw <= 0 || rh <= 0 {
                continue;
            }
            let mut ch = Channel::new(rw as usize, rh as usize);
            ch.hshift = fc.hshift;
            ch.vshift = fc.vshift;
            gi.channels.push(ch);
            places.push((ci, rx, ry));
        }
        if gi.channels.is_empty() {
            return Ok(());
        }
        decode_modular(br, &mut gi, stream, Some(&self.global), 1 << 30, true)?;
        for (src, &(c, px, py)) in gi.channels.iter().zip(&places) {
            let Some(dst) = self.img.channels.get_mut(c) else {
                return Err(super::fault());
            };
            for (y, row) in src.data.chunks_exact(src.w.max(1)).take(src.h).enumerate() {
                let at = py
                    .saturating_add(y)
                    .saturating_mul(dst.w)
                    .saturating_add(px);
                let Some(target) = dst.data.get_mut(at..at.saturating_add(row.len())) else {
                    return Err(super::fault());
                };
                target.copy_from_slice(row);
            }
        }
        Ok(())
    }
}
