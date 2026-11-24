#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
use super::bitio::{BitWriter, bits_n, bits_off, pack_signed, val};
use super::codestream::{COLOUR_SPACE_GREY, FrameHeader};
use super::entropy_enc::{EntropyEncoder, TokenStream};
use super::modular::{
    Channel, DEFAULT_WP, MaTree, ModularImage, NUM_QUANT_TABLES, PRED_WEIGHTED, PixelCoder,
    TRANSFORM_RCT, TransformInfo, TreeNode, WpHeader, walk_channel,
};
use super::{Error, ErrorKind, Image, LosslessOptions, MAX_IMAGE_PIXELS, limit};
pub(crate) fn write_size_header(w: &mut BitWriter, width: usize, height: usize) {
    w.bool(false);
    w.u32(
        height as u32,
        bits_off(9, 1),
        bits_off(13, 1),
        bits_off(18, 1),
        bits_off(30, 1),
    );
    w.write(3, 0);
    w.u32(
        width as u32,
        bits_off(9, 1),
        bits_off(13, 1),
        bits_off(18, 1),
        bits_off(30, 1),
    );
}
fn write_bit_depth(w: &mut BitWriter, bits: u32) {
    w.bool(false);
    w.u32(bits, val(8), val(10), val(12), bits_off(6, 1));
}
pub(crate) fn write_image_metadata(w: &mut BitWriter, bits: u32, grey: bool) {
    w.bool(false);
    w.bool(false);
    write_bit_depth(w, bits);
    w.bool(bits <= 12);
    w.u32(0, val(0), val(1), bits_off(4, 2), bits_off(12, 1));
    w.bool(false);
    if grey {
        w.bool(false);
        w.bool(false);
        w.enum_value(COLOUR_SPACE_GREY);
        w.enum_value(1);
        w.bool(false);
        w.enum_value(13);
        w.enum_value(1);
    } else {
        w.bool(true);
    }
    w.u64(0);
    w.bool(true);
}
#[derive(Clone, Copy, Default)]
pub(crate) struct FrameSpec {
    pub modular: bool,
    pub do_ycbcr: bool,
    pub jpeg_upsampling: [u32; 3],
    pub group_size_shift: u32,
    pub flags: u64,
}
pub(crate) fn write_frame_header(w: &mut BitWriter, fs: FrameSpec) {
    w.bool(false);
    w.write(2, 0);
    w.write(1, u64::from(fs.modular));
    w.u64(fs.flags);
    w.bool(fs.do_ycbcr);
    if fs.do_ycbcr {
        for m in fs.jpeg_upsampling {
            w.write(2, u64::from(m));
        }
    }
    w.u32(1, val(1), val(2), val(4), val(8));
    if fs.modular {
        w.write(2, u64::from(fs.group_size_shift));
    }
    w.u32(1, val(1), val(2), val(3), bits_off(3, 4));
    w.bool(false);
    w.u32(0, val(0), val(1), val(2), bits_off(2, 3));
    w.bool(true);
    w.u32(0, val(0), bits_n(4), bits_off(5, 16), bits_off(10, 48));
    w.bool(false);
    w.bool(false);
    w.write(2, 0);
    w.u64(0);
    w.u64(0);
}
pub(crate) fn write_frame(w: &mut BitWriter, sections: &[Vec<u8>]) {
    w.bool(false);
    w.zero_pad();
    for s in sections {
        w.u32(
            s.len() as u32,
            bits_n(10),
            bits_off(14, 1024),
            bits_off(22, 17408),
            bits_off(30, 4211712),
        );
    }
    w.zero_pad();
    for s in sections {
        w.buf.extend_from_slice(s);
    }
}
pub(crate) struct TNode {
    pub prop: i64,
    pub split: i32,
    pub left: Option<Box<TNode>>,
    pub right: Option<Box<TNode>>,
    pub pred: u8,
    pub offset: i64,
}
pub(crate) fn leaf_node(pred: u8) -> Box<TNode> {
    Box::new(TNode {
        prop: -1,
        split: 0,
        left: None,
        right: None,
        pred,
        offset: 0,
    })
}
fn balanced_split(prop: i64, cutoffs: &[i32], leaf: &dyn Fn() -> Box<TNode>) -> Box<TNode> {
    if cutoffs.is_empty() {
        return leaf();
    }
    let mid = cutoffs.len() / 2;
    Box::new(TNode {
        prop,
        split: cutoffs[mid],
        left: Some(balanced_split(prop, &cutoffs[mid + 1..], leaf)),
        right: Some(balanced_split(prop, &cutoffs[..mid], leaf)),
        pred: 0,
        offset: 0,
    })
}
const WP_CUTOFFS: [i32; 33] = [
    -500, -392, -255, -191, -127, -95, -63, -47, -31, -23, -15, -11, -7, -4, -3, -1, 0, 1, 3, 5, 7,
    11, 15, 23, 31, 47, 63, 95, 127, 191, 255, 392, 500,
];
pub(crate) fn weighted_tree(bit_depth: u32) -> Box<TNode> {
    let scale: i32 = if bit_depth > 8 {
        1 << (bit_depth - 8).min(16)
    } else {
        1
    };
    let cut: Vec<i32> = WP_CUTOFFS.iter().map(|&c| c.wrapping_mul(scale)).collect();
    balanced_split(15, &cut, &|| leaf_node(PRED_WEIGHTED))
}
pub(crate) fn flatten_tree(root: &TNode) -> (MaTree, Vec<&TNode>) {
    let mut t = MaTree {
        nodes: Vec::new(),
        max_prop: -1,
        uses_wp: false,
    };
    let mut order: Vec<&TNode> = vec![root];
    let mut i = 0;
    while i < order.len() {
        let n = order[i];
        if n.prop >= 0
            && let (Some(l), Some(r)) = (n.left.as_deref(), n.right.as_deref())
        {
            order.push(l);
            order.push(r);
        }
        i += 1;
    }
    let mut leaf: i32 = 0;
    let mut to_decode: i32 = 1;
    for n in &order {
        to_decode -= 1;
        if n.prop < 0 {
            t.nodes.push(TreeNode {
                property: -1,
                lchild: leaf,
                predictor: n.pred,
                offset: n.offset,
                multiplier: 1,
                ..Default::default()
            });
            if n.pred == PRED_WEIGHTED {
                t.uses_wp = true;
            }
            leaf += 1;
            continue;
        }
        let idx = t.nodes.len() as i32;
        t.nodes.push(TreeNode {
            property: n.prop as i32,
            split_val: n.split,
            lchild: idx + to_decode + 1,
            rchild: idx + to_decode + 2,
            ..Default::default()
        });
        t.max_prop = t.max_prop.max(n.prop);
        if n.prop == 15 {
            t.uses_wp = true;
        }
        to_decode += 2;
    }
    (t, order)
}
pub(crate) fn write_tree(w: &mut BitWriter, order: &[&TNode]) {
    let mut ts = TokenStream::default();
    for n in order {
        if n.prop < 0 {
            ts.add(1, 0);
            ts.add(2, u32::from(n.pred));
            ts.add(3, pack_signed(n.offset as i32));
            ts.add(4, 0);
            ts.add(5, 0);
            continue;
        }
        ts.add(1, (n.prop + 1) as u32);
        ts.add(0, pack_signed(n.split));
    }
    let enc = EntropyEncoder::build(6, &[&ts]);
    enc.write_header(w);
    enc.write_stream(w, &ts.tokens);
}
struct ModularStream {
    tokens: TokenStream,
    has_data: bool,
}
struct TokenCollector<'t> {
    tokens: &'t mut TokenStream,
}
impl PixelCoder for TokenCollector<'_> {
    fn code(&mut self, node: &TreeNode, pred: i64, cur: i32) -> i32 {
        let r = i64::from(cur).wrapping_sub(pred).wrapping_sub(node.offset);
        self.tokens.add(node.lchild as usize, pack_signed(r as i32));
        cur
    }
    fn row_done(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
fn encode_modular_stream(
    img: &mut ModularImage,
    tree: &MaTree,
    wp: &WpHeader,
    stream_id: usize,
    max_chan_size: usize,
) -> ModularStream {
    let mut ms = ModularStream {
        tokens: TokenStream::default(),
        has_data: false,
    };
    for i in 0..img.channels.len() {
        let c = &img.channels[i];
        if c.w == 0 || c.h == 0 {
            continue;
        }
        if i as i64 >= img.nb_meta && (c.w > max_chan_size || c.h > max_chan_size) {
            break;
        }
        ms.has_data = true;
        let mut coder = TokenCollector {
            tokens: &mut ms.tokens,
        };
        let _ = walk_channel(tree, wp, img, i, stream_id, &mut coder);
    }
    ms
}
pub(crate) fn modular_tokens(
    img: &mut ModularImage,
    tree: &MaTree,
    wp: &WpHeader,
    stream_id: usize,
) -> TokenStream {
    encode_modular_stream(img, tree, wp, stream_id, 1 << 30).tokens
}
pub(crate) fn write_group_header(
    w: &mut BitWriter,
    use_global: bool,
    transforms: &[TransformInfo],
) {
    w.bool(use_global);
    w.bool(true);
    w.u32(
        transforms.len() as u32,
        val(0),
        val(1),
        bits_off(4, 2),
        bits_off(8, 18),
    );
    for t in transforms {
        w.u32(t.id, val(0), val(1), val(2), val(3));
        w.u32(
            t.begin_c as u32,
            bits_n(3),
            bits_off(6, 8),
            bits_off(10, 72),
            bits_off(13, 1096),
        );
        w.u32(
            t.rct_type,
            val(6),
            bits_n(2),
            bits_off(4, 2),
            bits_off(6, 10),
        );
    }
}
fn forward_ycocg(r: &mut [i32], g: &mut [i32], b: &mut [i32]) {
    for i in 0..r.len() {
        let (rr, gg, bb) = (r[i], g[i], b[i]);
        let co = rr - bb;
        let tmp = bb + (co >> 1);
        let cg = gg - tmp;
        let y = tmp + (cg >> 1);
        r[i] = y;
        g[i] = co;
        b[i] = cg;
    }
}
pub fn encode_lossless(img: &Image, opts: LosslessOptions) -> Result<Vec<u8>, Error> {
    let mut bits = opts.bits_per_sample;
    if bits == 0 {
        bits = img.bits_per_sample;
    }
    if !(1..=16).contains(&bits) {
        return Err(Error::new(
            ErrorKind::Unsupported,
            Some(format!("{bits} bits per sample")),
        ));
    }
    if img.channels != 1 && img.channels != 3 {
        return Err(Error::new(
            ErrorKind::Unsupported,
            Some(format!("{} channels", img.channels)),
        ));
    }
    if img.width == 0
        || img.height == 0
        || (img.width as u64).saturating_mul(img.height as u64) > MAX_IMAGE_PIXELS
    {
        return Err(limit());
    }
    let n = img.width * img.height;
    if img.samples.len() != n * img.channels {
        return Err(Error::new(
            ErrorKind::InvalidCodestream,
            Some("sample count".into()),
        ));
    }
    let lim = 1i32 << bits;
    let mut full = ModularImage {
        bit_depth: bits,
        ..Default::default()
    };
    for c in 0..img.channels {
        let mut ch = Channel::new(img.width, img.height);
        for i in 0..n {
            let v = img.samples[i * img.channels + c];
            if v < 0 || v >= lim {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    Some(format!("sample {v} outside {bits}-bit range")),
                ));
            }
            ch.data[i] = v;
        }
        full.channels.push(ch);
    }
    let mut transforms = Vec::new();
    if img.channels == 3 {
        let [r, g, b] = &mut full.channels[..] else {
            return Err(Error::new(
                ErrorKind::InvalidCodestream,
                Some("sample count".into()),
            ));
        };
        forward_ycocg(&mut r.data, &mut g.data, &mut b.data);
        transforms.push(TransformInfo {
            id: TRANSFORM_RCT,
            begin_c: 0,
            rct_type: 6,
            ..Default::default()
        });
    }
    let fs = FrameSpec {
        modular: true,
        group_size_shift: 3,
        ..Default::default()
    };
    let mut f = FrameHeader {
        width: img.width,
        height: img.height,
        group_size_shift: fs.group_size_shift,
        num_passes: 1,
        ..Default::default()
    };
    f.compute_dims();
    let root = if img.channels == 3 {
        Box::new(TNode {
            prop: 0,
            split: 0,
            left: Some(weighted_tree(bits + 1)),
            right: Some(weighted_tree(bits)),
            pred: 0,
            offset: 0,
        })
    } else {
        weighted_tree(bits)
    };
    let (tree, order) = flatten_tree(&root);
    let num_ctx = tree.num_leaves();
    let global = encode_modular_stream(&mut full, &tree, &DEFAULT_WP, 0, f.group_dim);
    let mut groups = Vec::new();
    if !global.has_data {
        for g in 0..f.num_groups {
            let (gx, gy) = (g % f.x_groups, g / f.x_groups);
            let (x0, y0) = (gx * f.group_dim, gy * f.group_dim);
            let mut gi = ModularImage {
                bit_depth: bits,
                ..Default::default()
            };
            for fc in &full.channels {
                let w = f.group_dim.min(fc.w - x0);
                let h = f.group_dim.min(fc.h - y0);
                let mut ch = Channel::new(w, h);
                for y in 0..h {
                    let src = (y0 + y) * fc.w + x0;
                    ch.data[y * w..(y + 1) * w].copy_from_slice(&fc.data[src..src + w]);
                }
                gi.channels.push(ch);
            }
            let stream = 1 + 3 * f.num_lf_groups + NUM_QUANT_TABLES + g;
            groups.push(encode_modular_stream(
                &mut gi,
                &tree,
                &DEFAULT_WP,
                stream,
                1 << 30,
            ));
        }
    }
    let mut all: Vec<&TokenStream> = vec![&global.tokens];
    all.extend(groups.iter().map(|g| &g.tokens));
    let enc = EntropyEncoder::build(num_ctx, &all);
    let mut lf = BitWriter::default();
    lf.bool(true);
    lf.bool(true);
    write_tree(&mut lf, &order);
    enc.write_header(&mut lf);
    write_group_header(&mut lf, true, &transforms);
    enc.write_stream(&mut lf, &global.tokens.tokens);
    let mut cs = BitWriter::default();
    cs.write(16, 0x0AFF);
    write_size_header(&mut cs, img.width, img.height);
    write_image_metadata(&mut cs, bits, img.channels == 1);
    cs.zero_pad();
    write_frame_header(&mut cs, fs);
    let mut sections = vec![lf.into_bytes()];
    if f.num_groups != 1 {
        sections.extend((0..f.num_lf_groups).map(|_| Vec::new()));
        sections.push(Vec::new());
        for g in &groups {
            let mut gw = BitWriter::default();
            write_group_header(&mut gw, true, &[]);
            enc.write_stream(&mut gw, &g.tokens.tokens);
            sections.push(gw.into_bytes());
        }
    }
    write_frame(&mut cs, &sections);
    Ok(cs.into_bytes())
}
