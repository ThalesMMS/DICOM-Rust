#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
use super::bitio::{BitWriter, bits_n, bits_off, ceil_log2, pack_signed, val};
use super::codestream::{FLAG_SKIP_LF_SM, FrameHeader};
use super::container;
use super::encode::{
    FrameSpec, TNode, flatten_tree, leaf_node, weighted_tree, write_frame, write_frame_header,
    write_group_header, write_image_metadata, write_size_header, write_tree,
};
use super::entropy_enc::{EntropyEncoder, TokenStream, write_context_map};
use super::jbrd::{APP_UNKNOWN, JpegData};
use super::jpegparse::{not_reversible, parse_jpeg};
use super::modular::{Channel, DEFAULT_WP, ModularImage};
use super::reconstruct::{DEFAULT_COLOUR_FACTOR, jpeg_order, reconstruct_jpeg};
use super::vardct::{
    BlockCtxMap, DEFAULT_BLOCK_CTX_MAP, NUM_QUANT_TABLES, natural_order_dct8, predict_non_zeros,
    zero_density_ctx,
};
use super::{Error, brotli};
pub fn recompress_jpeg(jpeg: &[u8]) -> Result<Vec<u8>, Error> {
    let p = parse_jpeg(jpeg)?;
    if p.sof_marker == 0xC2 {
        return Err(not_reversible("progressive JPEG"));
    }
    let out = encode_recompressed(&p.data, true)?;
    let back = reconstruct_jpeg(&out)
        .map_err(|e| not_reversible(format!("reconstruction failed: {e}")))?;
    if back != jpeg {
        return Err(not_reversible(
            "reconstruction differs from the source JPEG",
        ));
    }
    Ok(out)
}
pub(crate) fn jpeg_is_rgb(j: &JpegData) -> bool {
    if j.components.len() != 3 || j.marker_order.contains(&0xE0) {
        return false;
    }
    let mut app = 0;
    for &m in &j.marker_order {
        if m & 0xF0 != 0xE0 {
            continue;
        }
        if m == 0xEE {
            let d = &j.app_data[app];
            if d.len() == 15 && &d[3..8] == b"Adobe" {
                return d[14] == 0;
            }
        }
        app += 1;
    }
    let c = &j.components;
    c[0].id == u32::from(b'R') && c[1].id == u32::from(b'G') && c[2].id == u32::from(b'B')
}
fn sampling_mode(h: usize, v: usize) -> Option<u32> {
    match (h, v) {
        (1, 1) => Some(0),
        (2, 2) => Some(1),
        (2, 1) => Some(2),
        (1, 2) => Some(3),
        _ => None,
    }
}
struct LfGroupData {
    dc: TokenStream,
    meta: TokenStream,
    count: usize,
}
pub(crate) fn encode_recompressed(j: &JpegData, reconstruct: bool) -> Result<Vec<u8>, Error> {
    let is_gray = j.components.len() == 1;
    let do_ycbcr = is_gray || !jpeg_is_rgb(j);
    let cmap = jpeg_order(is_gray, do_ycbcr && !is_gray);
    let mut f = FrameHeader {
        width: j.width,
        height: j.height,
        group_size_shift: 1,
        num_passes: 1,
        do_ycbcr,
        ..Default::default()
    };
    if do_ycbcr {
        for c in 0..3 {
            if is_gray && c != 1 {
                continue;
            }
            let comp = &j.components[cmap[c]];
            let Some(m) = sampling_mode(comp.h_samp, comp.v_samp) else {
                return Err(not_reversible(format!(
                    "sampling factors {}x{}",
                    comp.h_samp, comp.v_samp
                )));
            };
            f.jpeg_upsampling[c] = m;
        }
    } else if j.components.iter().any(|c| c.h_samp != 1 || c.v_samp != 1) {
        return Err(not_reversible("subsampled RGB JPEG"));
    }
    f.compute_dims();
    let channels: &[usize] = if is_gray { &[1] } else { &[0, 1, 2] };
    for &c in channels {
        let comp = &j.components[cmap[c]];
        if comp.width_blocks != f.xsize_blocks >> f.hshift(c)
            || comp.height_blocks != f.ysize_blocks >> f.vshift(c)
        {
            return Err(not_reversible(
                "component block grid does not match the frame",
            ));
        }
        if comp.h_samp != 1 << (f.max_h_shift - f.hshift(c))
            || comp.v_samp != 1 << (f.max_v_shift - f.vshift(c))
        {
            return Err(not_reversible("sampling factors cannot be restored"));
        }
    }
    let used = j.components.iter().fold(0u32, |u, c| u | 1 << c.quant_idx);
    for (i, q) in j.quant.iter().enumerate() {
        if reconstruct && used & (1 << i) == 0 && (i == 0 || q.values != j.quant[i - 1].values) {
            return Err(not_reversible(format!(
                "unused quantization table {}",
                q.index
            )));
        }
    }
    let mut qt = [[0i32; 64]; 3];
    for (c, t) in qt.iter_mut().enumerate() {
        let src = if is_gray {
            &j.components[0]
        } else {
            &j.components[cmap[c]]
        };
        let vals = &j.quant[src.quant_idx as usize].values;
        for y in 0..8 {
            for x in 0..8 {
                t[y * 8 + x] = vals[x * 8 + y];
            }
        }
    }
    let mut dc_off = [0i32; 3];
    if !do_ycbcr {
        for c in 0..3 {
            dc_off[c] = 1024 / qt[c][0];
        }
    }
    let coeff = |c: usize, bx: usize, by: usize| -> (i32, [i32; 64]) {
        let mut ac = [0i32; 64];
        if is_gray && c != 1 {
            return (0, ac);
        }
        let comp = &j.components[cmap[c]];
        let blk = &comp.coeffs[64 * (by * comp.width_blocks + bx)..];
        for k in 0..64 {
            ac[(k % 8) * 8 + k / 8] = i32::from(blk[k]);
        }
        (i32::from(blk[0]) + dc_off[c], ac)
    };
    let root = TNode {
        prop: 1,
        split: f.num_lf_groups as i32,
        left: Some(leaf_node(0)),
        right: Some(weighted_tree(11)),
        pred: 0,
        offset: 0,
    };
    let (tree, order) = flatten_tree(&root);
    let num_ctx = tree.num_leaves();
    let mut lf_data = Vec::with_capacity(f.num_lf_groups);
    for g in 0..f.num_lf_groups {
        let (gx, gy) = (g % f.x_lf_groups, g / f.x_lf_groups);
        let (x0, y0) = (gx * f.group_dim, gy * f.group_dim);
        let w = f.group_dim.min(f.xsize_blocks - x0);
        let h = f.group_dim.min(f.ysize_blocks - y0);
        let mut img = ModularImage {
            bit_depth: 8,
            ..Default::default()
        };
        for c in [1usize, 0, 2] {
            let (cw, ch) = (w >> f.hshift(c), h >> f.vshift(c));
            let (cx0, cy0) = (x0 >> f.hshift(c), y0 >> f.vshift(c));
            let mut chn = Channel::new(cw, ch);
            for y in 0..ch {
                for x in 0..cw {
                    chn.data[y * cw + x] = coeff(c, cx0 + x, cy0 + y).0;
                }
            }
            img.channels.push(chn);
        }
        let dc = super::encode::modular_tokens(&mut img, &tree, &DEFAULT_WP, 1 + g);
        let (tw, th) = ((w + 7) >> 3, (h + 7) >> 3);
        let count = w * h;
        let mut meta = ModularImage {
            bit_depth: 8,
            channels: vec![
                Channel::new(tw, th),
                Channel::new(tw, th),
                Channel::new(count, 2),
                Channel::new(w, h),
            ],
            ..Default::default()
        };
        for ch in &mut meta.channels[..2] {
            ch.hshift = 3;
            ch.vshift = 3;
        }
        let meta_tokens = super::encode::modular_tokens(
            &mut meta,
            &tree,
            &DEFAULT_WP,
            1 + 2 * f.num_lf_groups + g,
        );
        lf_data.push(LfGroupData {
            dc,
            meta: meta_tokens,
            count,
        });
    }
    let mut qimg = ModularImage {
        bit_depth: 8,
        ..Default::default()
    };
    for t in &qt {
        let mut ch = Channel::new(8, 8);
        ch.data.copy_from_slice(t);
        qimg.channels.push(ch);
    }
    let q_tokens =
        super::encode::modular_tokens(&mut qimg, &tree, &DEFAULT_WP, 1 + 3 * f.num_lf_groups);
    let mut modular_streams: Vec<&TokenStream> = Vec::new();
    for d in &lf_data {
        modular_streams.push(&d.dc);
        modular_streams.push(&d.meta);
    }
    modular_streams.push(&q_tokens);
    let mod_enc = EntropyEncoder::build(num_ctx, &modular_streams);
    let bctx = BlockCtxMap::default_map();
    let num_ac = bctx.num_ac_contexts();
    let natural = natural_order_dct8();
    let gb = f.group_dim / 8;
    let mut hf_streams: Vec<TokenStream> = Vec::with_capacity(f.num_groups);
    for g in 0..f.num_groups {
        let mut ts = TokenStream::default();
        let (gx, gy) = (g % f.x_groups, g / f.x_groups);
        let (bx0, by0) = (gx * gb, gy * gb);
        let bw = gb.min(f.xsize_blocks - bx0);
        let bh = gb.min(f.ysize_blocks - by0);
        let mut nz: [Vec<i32>; 3] = [vec![0; gb * gb], vec![0; gb * gb], vec![0; gb * gb]];
        for by in 0..bh {
            for bx in 0..bw {
                for c in [1usize, 0, 2] {
                    let (hs, vs) = (f.hshift(c), f.vshift(c));
                    let (sbx, sby) = (bx >> hs, by >> vs);
                    if sbx << hs != bx || sby << vs != by {
                        continue;
                    }
                    let pred = {
                        let row = &nz[c][sby * gb..(sby + 1) * gb];
                        let top = (sby > 0).then(|| &nz[c][(sby - 1) * gb..sby * gb]);
                        predict_non_zeros(top, row, sbx)
                    };
                    let block_ctx = bctx.context(0, 1, 0, c);
                    let (_, ac) = coeff(c, (bx0 >> hs) + sbx, (by0 >> vs) + sby);
                    let nzeros = natural[1..].iter().filter(|&&k| ac[k] != 0).count();
                    ts.add(bctx.non_zero_context(pred, block_ctx), nzeros as u32);
                    nz[c][sby * gb + sbx] = nzeros as i32;
                    let hist_off = bctx.zero_density_offset(block_ctx);
                    let mut prev = usize::from(nzeros <= 4);
                    let mut left = nzeros;
                    let mut k = 1;
                    while k < 64 && left != 0 {
                        let v = ac[natural[k]];
                        ts.add(hist_off + zero_density_ctx(left, k, prev), pack_signed(v));
                        if v != 0 {
                            prev = 1;
                            left -= 1;
                        } else {
                            prev = 0;
                        }
                        k += 1;
                    }
                }
            }
        }
        hf_streams.push(ts);
    }
    let hf_refs: Vec<&TokenStream> = hf_streams.iter().collect();
    let hf_enc = EntropyEncoder::build(num_ac, &hf_refs);
    let mut lfg = BitWriter::default();
    lfg.bool(false);
    for t in &qt {
        let dq = 1f32 / ((255 * 8) as f32 / t[0] as f32);
        lfg.f16(dq * 128.0);
    }
    lfg.u32(
        65536,
        bits_off(11, 1),
        bits_off(11, 2049),
        bits_off(12, 4097),
        bits_off(16, 8193),
    );
    lfg.u32(1, val(16), bits_off(5, 1), bits_off(8, 1), bits_off(16, 1));
    lfg.bool(false);
    for _ in 0..4 {
        lfg.write(4, 0);
    }
    write_context_map(&mut lfg, &DEFAULT_BLOCK_CTX_MAP, 15);
    lfg.bool(false);
    lfg.u32(
        DEFAULT_COLOUR_FACTOR as u32,
        val(84),
        val(256),
        bits_off(8, 2),
        bits_off(16, 258),
    );
    lfg.f16(0.0);
    lfg.f16(0.0);
    lfg.write(8, 128);
    lfg.write(8, 128);
    lfg.bool(true);
    write_tree(&mut lfg, &order);
    mod_enc.write_header(&mut lfg);
    let mut lf_sections = Vec::with_capacity(lf_data.len());
    for d in &lf_data {
        let mut w = BitWriter::default();
        w.write(2, 0);
        write_group_header(&mut w, true, &[]);
        mod_enc.write_stream(&mut w, &d.dc.tokens);
        w.write(ceil_log2(d.count as u64), (d.count - 1) as u64);
        write_group_header(&mut w, true, &[]);
        mod_enc.write_stream(&mut w, &d.meta.tokens);
        lf_sections.push(w);
    }
    let mut hfg = BitWriter::default();
    hfg.bool(false);
    hfg.write(3, 7);
    hfg.f16(1.0 / (8.0 * 255.0));
    write_group_header(&mut hfg, true, &[]);
    mod_enc.write_stream(&mut hfg, &q_tokens.tokens);
    for _ in 1..NUM_QUANT_TABLES {
        hfg.write(3, 0);
    }
    hfg.write(ceil_log2(f.num_groups as u64), 0);
    hfg.u32(0, val(0x5F), val(0x13), val(0), bits_n(13));
    hf_enc.write_header(&mut hfg);
    let pass_sections: Vec<BitWriter> = hf_streams
        .iter()
        .map(|ts| {
            let mut w = BitWriter::default();
            hf_enc.write_stream(&mut w, &ts.tokens);
            w
        })
        .collect();
    let mut cs = BitWriter::default();
    cs.write(16, 0x0AFF);
    write_size_header(&mut cs, j.width, j.height);
    write_image_metadata(&mut cs, 8, is_gray);
    cs.zero_pad();
    write_frame_header(
        &mut cs,
        FrameSpec {
            do_ycbcr,
            jpeg_upsampling: f.jpeg_upsampling,
            flags: FLAG_SKIP_LF_SM,
            ..Default::default()
        },
    );
    let sections = if f.num_groups == 1 {
        let mut all = BitWriter::default();
        all.append_writer(&lfg);
        for s in &lf_sections {
            all.append_writer(s);
        }
        all.append_writer(&hfg);
        for s in &pass_sections {
            all.append_writer(s);
        }
        vec![all.into_bytes()]
    } else {
        let mut v = vec![lfg.into_bytes()];
        v.extend(lf_sections.into_iter().map(BitWriter::into_bytes));
        v.push(hfg.into_bytes());
        v.extend(pass_sections.into_iter().map(BitWriter::into_bytes));
        v
    };
    write_frame(&mut cs, &sections);
    if !reconstruct {
        return Ok(cs.into_bytes());
    }
    let jbrd = write_jpeg_data(j)?;
    Ok(container::wrap(&cs.into_bytes(), Some(&jbrd)))
}
pub(crate) fn write_jpeg_data(j: &JpegData) -> Result<Vec<u8>, Error> {
    let mut w = BitWriter::default();
    w.bool(j.components.len() == 1);
    for &m in &j.marker_order {
        w.write(6, u64::from(m.wrapping_sub(0xC0)));
    }
    for (i, a) in j.app_data.iter().enumerate() {
        w.u32(
            j.app_marker_type[i],
            val(0),
            val(1),
            bits_off(1, 2),
            bits_off(2, 4),
        );
        w.write(16, (a.len() - 1) as u64);
    }
    for c in &j.com_data {
        w.write(16, (c.len() - 1) as u64);
    }
    if j.quant.is_empty() || j.quant.len() > 3 {
        return Err(not_reversible(format!(
            "{} quantization tables",
            j.quant.len()
        )));
    }
    w.u32(j.quant.len() as u32, val(1), val(2), val(3), val(4));
    for q in &j.quant {
        w.write(1, u64::from(q.precision));
        w.write(2, u64::from(q.index));
        w.bool(q.is_last);
    }
    let comps = &j.components;
    let ids: Vec<u32> = comps.iter().map(|c| c.id).collect();
    if ids == [1] {
        w.write(2, 0);
    } else if ids == [1, 2, 3] {
        w.write(2, 1);
    } else if ids == [u32::from(b'R'), u32::from(b'G'), u32::from(b'B')] {
        w.write(2, 2);
    } else {
        w.write(2, 3);
        w.u32(comps.len() as u32, val(1), val(2), val(3), val(4));
        for c in comps {
            w.write(8, u64::from(c.id));
        }
    }
    for c in comps {
        w.write(2, u64::from(c.quant_idx));
    }
    let unrepresentable = j.huffman.len() > 89
        || j.huffman.iter().any(|h| h.counts.iter().any(|&c| c > 255))
        || j.scans
            .iter()
            .any(|s| s.extra_zero_runs.len() > 65555 || s.reset_points.len() > 65555);
    if unrepresentable {
        return Err(super::fault());
    }
    w.u32(
        j.huffman.len() as u32,
        val(4),
        bits_off(3, 2),
        bits_off(4, 10),
        bits_off(6, 26),
    );
    for h in &j.huffman {
        w.bool(h.slot_id & 0x10 != 0);
        w.write(2, u64::from(h.slot_id & 3));
        w.bool(h.is_last);
        for &c in &h.counts {
            w.u32(c, val(0), val(1), bits_off(3, 2), bits_n(8));
        }
        for &v in &h.values {
            w.u32(v, bits_n(2), bits_off(2, 4), bits_off(4, 8), bits_off(8, 1));
        }
    }
    for s in &j.scans {
        w.u32(s.num_components, val(1), val(2), val(3), val(4));
        w.write(6, u64::from(s.ss));
        w.write(6, u64::from(s.se));
        w.write(4, u64::from(s.al));
        w.write(4, u64::from(s.ah));
        for c in &s.components[..s.num_components as usize] {
            w.write(2, u64::from(c.comp_idx));
            w.write(2, u64::from(c.ac_tbl));
            w.write(2, u64::from(c.dc_tbl));
        }
        w.u32(0, val(0), val(1), val(2), bits_off(3, 3));
    }
    if j.marker_order.contains(&0xDD) {
        w.write(16, u64::from(j.restart_interval));
    }
    for s in &j.scans {
        w.u32(
            s.reset_points.len() as u32,
            val(0),
            bits_off(2, 1),
            bits_off(4, 4),
            bits_off(16, 20),
        );
        let mut last: i64 = -1;
        for &r in &s.reset_points {
            w.u32(
                (i64::from(r) - last - 1) as u32,
                val(0),
                bits_off(3, 1),
                bits_off(5, 9),
                bits_off(28, 41),
            );
            last = i64::from(r);
        }
        w.u32(
            s.extra_zero_runs.len() as u32,
            val(0),
            bits_off(2, 1),
            bits_off(4, 4),
            bits_off(16, 20),
        );
        last = -1;
        for z in &s.extra_zero_runs {
            w.u32(
                z.num_runs,
                val(1),
                bits_off(2, 2),
                bits_off(4, 5),
                bits_off(8, 20),
            );
            w.u32(
                (i64::from(z.block_idx) - last - 1) as u32,
                val(0),
                bits_off(3, 1),
                bits_off(5, 9),
                bits_off(28, 41),
            );
            last = i64::from(z.block_idx);
        }
    }
    for d in &j.inter_marker {
        if d.len() > 0xFFFF {
            return Err(not_reversible("inter-marker data too long"));
        }
        w.write(16, d.len() as u64);
    }
    if j.tail_data.len() > 4260096 {
        return Err(not_reversible("tail data too long"));
    }
    w.u32(
        j.tail_data.len() as u32,
        val(0),
        bits_off(8, 1),
        bits_off(16, 257),
        bits_off(22, 65793),
    );
    w.bool(j.has_zero_padding);
    if j.has_zero_padding {
        w.write(24, j.padding_bits.len() as u64);
        for &b in &j.padding_bits {
            w.write(1, u64::from(b));
        }
    }
    let mut plain = Vec::new();
    for (i, a) in j.app_data.iter().enumerate() {
        if j.app_marker_type[i] == APP_UNKNOWN {
            plain.extend_from_slice(a);
        }
    }
    for c in &j.com_data {
        plain.extend_from_slice(c);
    }
    for d in &j.inter_marker {
        plain.extend_from_slice(d);
    }
    plain.extend_from_slice(&j.tail_data);
    let mut out = w.into_bytes();
    out.extend_from_slice(&brotli::encode_stored(&plain));
    Ok(out)
}
