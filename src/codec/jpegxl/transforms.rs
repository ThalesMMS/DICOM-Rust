use super::modular::{
    Channel, ModularImage, PRED_WEIGHTED, SqueezeParams, TRANSFORM_PALETTE, TRANSFORM_RCT,
    TRANSFORM_SQUEEZE, TransformInfo, WpHeader, WpState, predict_value,
};
use super::{Error, corrupt, fault, unsupported};
fn check_equal_channels(img: &ModularImage, begin: i64, end: i64) -> Result<(), Error> {
    if begin < 0 || end >= img.channels.len() as i64 || begin > end {
        return Err(corrupt("transform channel range"));
    }
    if begin < img.nb_meta && end >= img.nb_meta {
        return Err(corrupt("transform mixes meta and image channels"));
    }
    let Some((c0, rest)) = img
        .channels
        .get(begin as usize..=end as usize)
        .and_then(|r| r.split_first())
    else {
        return Err(corrupt("transform channel range"));
    };
    for c in rest {
        if c.w != c0.w || c.h != c0.h || c.hshift != c0.hshift || c.vshift != c0.vshift {
            return Err(corrupt("transform over unequal channels"));
        }
    }
    Ok(())
}
pub(crate) fn meta_apply(img: &mut ModularImage, t: &mut TransformInfo) -> Result<(), Error> {
    match t.id {
        TRANSFORM_RCT => check_equal_channels(img, t.begin_c, t.begin_c.saturating_add(2)),
        TRANSFORM_PALETTE => {
            let end = t.begin_c.saturating_add(t.num_c).saturating_sub(1);
            check_equal_channels(img, t.begin_c, end)?;
            if t.begin_c >= img.nb_meta {
                img.nb_meta = img.nb_meta.saturating_add(1);
            } else {
                let added = 2i64.saturating_sub(t.num_c);
                if img.nb_meta < added {
                    return Err(corrupt("palette of meta channels"));
                }
                img.nb_meta = img.nb_meta.saturating_add(added);
            }
            img.channels
                .drain((t.begin_c as usize).saturating_add(1)..(end as usize).saturating_add(1));
            let size = t.nb_colours.saturating_add(t.nb_deltas);
            if size.saturating_mul(t.num_c) > 1 << 26 {
                return Err(corrupt("palette too large"));
            }
            let mut pal = Channel::new(size as usize, t.num_c as usize);
            pal.hshift = -1;
            pal.vshift = -1;
            img.channels.insert(0, pal);
            Ok(())
        }
        TRANSFORM_SQUEEZE => meta_squeeze(img, t),
        _ => Err(corrupt("invalid transform")),
    }
}
fn default_squeeze(img: &ModularImage) -> Vec<SqueezeParams> {
    let mut ps = Vec::new();
    let first = img.nb_meta;
    let nb = (img.channels.len() as i64).saturating_sub(first);
    let Some(c0) = usize::try_from(first)
        .ok()
        .and_then(|f| img.channels.get(f))
        .filter(|_| nb > 0)
    else {
        return ps;
    };
    let (mut w, mut h) = (c0.w, c0.h);
    let c1 = img.channels.get((first as usize).saturating_add(1));
    if nb > 2 && c1.is_some_and(|c| c.w == w && c.h == h) {
        ps.push(SqueezeParams {
            horizontal: true,
            in_place: false,
            begin_c: first.saturating_add(1),
            num_c: 2,
        });
        ps.push(SqueezeParams {
            horizontal: false,
            in_place: false,
            begin_c: first.saturating_add(1),
            num_c: 2,
        });
    }
    let mut p = SqueezeParams {
        horizontal: false,
        in_place: true,
        begin_c: first,
        num_c: nb,
    };
    const MAX_FIRST: usize = 8;
    if w <= h && h > MAX_FIRST {
        p.horizontal = false;
        ps.push(p);
        h = h.div_ceil(2);
    }
    while w > MAX_FIRST || h > MAX_FIRST {
        if w > MAX_FIRST {
            p.horizontal = true;
            ps.push(p);
            w = w.div_ceil(2);
        }
        if h > MAX_FIRST {
            p.horizontal = false;
            ps.push(p);
            h = h.div_ceil(2);
        }
    }
    ps
}
fn meta_squeeze(img: &mut ModularImage, t: &mut TransformInfo) -> Result<(), Error> {
    if t.squeezes.is_empty() {
        t.squeezes = default_squeeze(img);
    }
    for p in &t.squeezes {
        let (begin, end) = (
            p.begin_c,
            p.begin_c.saturating_add(p.num_c).saturating_sub(1),
        );
        if begin < 0 || end >= img.channels.len() as i64 || p.num_c <= 0 {
            return Err(corrupt("squeeze channel range"));
        }
        if begin < img.nb_meta {
            if end >= img.nb_meta {
                return Err(corrupt("squeeze mixes meta and image channels"));
            }
            if !p.in_place {
                return Err(corrupt("squeeze of meta channels must be in place"));
            }
            img.nb_meta = img.nb_meta.saturating_add(p.num_c);
        }
        let offset = if p.in_place {
            (end as usize).saturating_add(1)
        } else {
            img.channels.len()
        };
        for c in begin as usize..=end as usize {
            let Some(ch) = img.channels.get_mut(c) else {
                return Err(corrupt("squeeze channel range"));
            };
            if ch.hshift > 30 || ch.vshift > 30 {
                return Err(corrupt("too many squeezes"));
            }
            let (mut w, mut h) = (ch.w, ch.h);
            if w == 0 || h == 0 {
                return Err(corrupt("squeezing an empty channel"));
            }
            if p.horizontal {
                ch.w = w.div_ceil(2);
                if ch.hshift >= 0 {
                    ch.hshift = ch.hshift.saturating_add(1);
                }
                w = w.saturating_sub(ch.w);
            } else {
                ch.h = h.div_ceil(2);
                if ch.vshift >= 0 {
                    ch.vshift = ch.vshift.saturating_add(1);
                }
                h = h.saturating_sub(ch.h);
            }
            ch.data = vec![0; ch.w.saturating_mul(ch.h)];
            let mut res = Channel::new(w, h);
            res.hshift = ch.hshift;
            res.vshift = ch.vshift;
            let at = offset.saturating_add(c.saturating_sub(begin as usize));
            if at > img.channels.len() {
                return Err(fault());
            }
            img.channels.insert(at, res);
        }
    }
    Ok(())
}
pub(crate) fn undo_transforms(
    img: &mut ModularImage,
    ts: &[TransformInfo],
    wp: &WpHeader,
) -> Result<(), Error> {
    for t in ts.iter().rev() {
        match t.id {
            TRANSFORM_RCT => inv_rct(img, t.begin_c, t.rct_type)?,
            TRANSFORM_PALETTE => inv_palette(img, t, wp)?,
            TRANSFORM_SQUEEZE => inv_squeeze(img, &t.squeezes)?,
            _ => {}
        }
    }
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn inv_rct(img: &mut ModularImage, m: i64, rct_type: u32) -> Result<(), Error> {
    if m < 0 || m.saturating_add(2) >= img.channels.len() as i64 {
        return Err(corrupt("RCT channel range"));
    }
    let m = m as usize;
    let perm = rct_type as usize / 7;
    let kind = rct_type % 7;
    let o0 = m + perm % 3;
    let o1 = m + (perm + 1 + perm / 3) % 3;
    let o2 = m + (perm + 2 - perm / 3) % 3;
    let c0 = std::mem::take(&mut img.channels[m]);
    let c1 = std::mem::take(&mut img.channels[m + 1]);
    let c2 = std::mem::take(&mut img.channels[m + 2]);
    if kind == 0 {
        img.channels[o0] = c0;
        img.channels[o1] = c1;
        img.channels[o2] = c2;
        return Ok(());
    }
    let n = c0.data.len();
    if c1.data.len() < n || c2.data.len() < n {
        return Err(fault());
    }
    let (in0, in1, in2) = (&c0.data, &c1.data, &c2.data);
    let mut out0 = vec![0i32; n];
    let mut out1 = vec![0i32; n];
    let mut out2 = vec![0i32; n];
    let (second, third) = (kind >> 1, kind & 1);
    for i in 0..n {
        if kind == 6 {
            let (y, co, cg) = (in0[i], in1[i], in2[i]);
            let tmp = y.wrapping_sub(cg >> 1);
            let g = cg.wrapping_add(tmp);
            let b = tmp.wrapping_sub(co >> 1);
            let r = b.wrapping_add(co);
            out0[i] = r;
            out1[i] = g;
            out2[i] = b;
            continue;
        }
        let (a, mut b, mut c) = (in0[i], in1[i], in2[i]);
        if third == 1 {
            c = c.wrapping_add(a);
        }
        match second {
            1 => b = b.wrapping_add(a),
            2 => b = b.wrapping_add(((i64::from(a) + i64::from(c)) as i32) >> 1),
            _ => {}
        }
        out0[i] = a;
        out1[i] = b;
        out2[i] = c;
    }
    let like = |data| Channel {
        w: c0.w,
        h: c0.h,
        hshift: c0.hshift,
        vshift: c0.vshift,
        data,
    };
    img.channels[o0] = like(out0);
    img.channels[o1] = like(out1);
    img.channels[o2] = like(out2);
    Ok(())
}
fn scale_palette(v: i64, bit_depth: u32, denom: i64) -> i64 {
    v.saturating_mul((1i64 << bit_depth).saturating_sub(1))
        .checked_div(denom)
        .unwrap_or(0)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn palette_value(pal: &Channel, index: i64, c: usize, bit_depth: u32) -> Result<i64, Error> {
    let size = pal.w as i64;
    if index < 0 {
        return Err(unsupported("delta palette entries"));
    }
    if index < size {
        return Ok(i64::from(pal.data[c * pal.w + index as usize]));
    }
    if c >= 3 {
        return Ok(0);
    }
    if index < size + 64 {
        let i = (index - size) >> (2 * c);
        return Ok(scale_palette(i % 4, bit_depth, 4) + (1i64 << bit_depth.saturating_sub(3)));
    }
    let mut i = index - size - 64;
    for _ in 0..c {
        i /= 5;
    }
    Ok(scale_palette(i % 5, bit_depth, 4))
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn inv_palette(img: &mut ModularImage, t: &TransformInfo, wph: &WpHeader) -> Result<(), Error> {
    if img.nb_meta < 1 || img.channels.len() < 2 {
        return Err(corrupt("palette without palette channel"));
    }
    let c0 = (t.begin_c as usize).saturating_add(1);
    let (Some(pal), Some(idx)) = (img.channels.first(), img.channels.get(c0)) else {
        return Err(corrupt("palette channel range"));
    };
    let nb = pal.h;
    if nb < 1 {
        return Err(corrupt("palette channel range"));
    }
    let (w, h) = (idx.w, idx.h);
    if idx.data.len() != w.saturating_mul(h) || pal.data.len() != pal.w.saturating_mul(pal.h) {
        return Err(fault());
    }
    let bit_depth = img.bit_depth.min(24);
    let mut out: Vec<Channel> = Vec::with_capacity(nb);
    for c in 0..nb {
        let mut data = vec![0i32; w * h];
        let mut wp = (t.d_pred == u32::from(PRED_WEIGHTED) && t.nb_deltas > 0)
            .then(|| WpState::new(*wph, w));
        for y in 0..h {
            for x in 0..w {
                let index = i64::from(idx.data[y * w + x]);
                let mut v = palette_value(pal, index, c, bit_depth)?;
                if index < t.nb_deltas {
                    v = v.wrapping_add(predict_at(&data, w, x, y, t.d_pred as u8, wp.as_mut()));
                }
                data[y * w + x] = v as i32;
                if let Some(wp) = wp.as_mut() {
                    wp.update(i64::from(data[y * w + x]), x, y);
                }
            }
        }
        out.push(Channel {
            w,
            h,
            hshift: idx.hshift,
            vshift: idx.vshift,
            data,
        });
    }
    let rest = img.channels.split_off(c0.saturating_add(1));
    img.channels.truncate(c0);
    img.channels.remove(0);
    img.channels.extend(out);
    img.channels.extend(rest);
    img.nb_meta = img.nb_meta.saturating_sub(1);
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn predict_at(data: &[i32], w: usize, x: usize, y: usize, p: u8, wp: Option<&mut WpState>) -> i64 {
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
    let wp_pred = wp.map_or(0, |wp| wp.predict(x, y, n, ww_, ne, nw, nn).0);
    predict_value(p, ww_, n, nw, ne, nn, wwv, nee, wp_pred)
}
#[allow(clippy::arithmetic_side_effects)]
fn smooth_tendency(b: i64, a: i64, n: i64) -> i64 {
    let mut diff = 0;
    if b >= a && a >= n {
        diff = (4 * b - 3 * n - a + 6) / 12;
        if diff - (diff & 1) > 2 * (b - a) {
            diff = 2 * (b - a) + 1;
        }
        if diff + (diff & 1) > 2 * (a - n) {
            diff = 2 * (a - n);
        }
    } else if b <= a && a <= n {
        diff = (4 * b - 3 * n - a - 6) / 12;
        if diff + (diff & 1) < 2 * (b - a) {
            diff = 2 * (b - a) - 1;
        }
        if diff - (diff & 1) < 2 * (a - n) {
            diff = 2 * (a - n);
        }
    }
    diff
}
fn inv_squeeze(img: &mut ModularImage, ps: &[SqueezeParams]) -> Result<(), Error> {
    for p in ps.iter().rev() {
        let (begin, end) = (
            p.begin_c,
            p.begin_c.saturating_add(p.num_c).saturating_sub(1),
        );
        let len = img.channels.len() as i64;
        let offset = if p.in_place {
            end.saturating_add(1)
        } else {
            len.saturating_add(begin)
                .saturating_sub(end)
                .saturating_sub(1)
        };
        if begin < img.nb_meta {
            img.nb_meta = img.nb_meta.saturating_sub(p.num_c);
        }
        for c in begin..=end {
            let rc = offset.saturating_add(c.saturating_sub(begin));
            if rc >= img.channels.len() as i64 {
                return Err(corrupt("squeeze residual channel"));
            }
            if c >= img.channels.len() as i64 || rc < 0 {
                return Err(fault());
            }
            let (c, rc) = (c as usize, rc as usize);
            let Some(res) = img.channels.get(rc).cloned() else {
                return Err(fault());
            };
            let Some(avg) = img.channels.get_mut(c) else {
                return Err(fault());
            };
            if avg.data.len() != avg.w.saturating_mul(avg.h)
                || res.data.len() != res.w.saturating_mul(res.h)
            {
                return Err(fault());
            }
            if avg.w < res.w || avg.h < res.h {
                return Err(corrupt("squeeze dimensions"));
            }
            if p.horizontal {
                inv_h_squeeze(avg, &res);
            } else {
                inv_v_squeeze(avg, &res)?;
            }
        }
        let offset = offset as usize;
        let end = offset.saturating_add(p.num_c as usize);
        if end > img.channels.len() || offset > end {
            return Err(fault());
        }
        img.channels.drain(offset..end);
    }
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn inv_h_squeeze(avg: &mut Channel, res: &Channel) {
    if res.w == 0 || avg.h != res.h {
        avg.hshift -= 1;
        return;
    }
    let ow = avg.w + res.w;
    let mut out = vec![0i32; ow * avg.h];
    for y in 0..avg.h {
        let pa = &avg.data[y * avg.w..(y + 1) * avg.w];
        let pr = &res.data[y * res.w..(y + 1) * res.w];
        let po = &mut out[y * ow..(y + 1) * ow];
        for x in 0..res.w {
            let a = i64::from(pa[x]);
            let next = if x + 1 < avg.w {
                i64::from(pa[x + 1])
            } else {
                a
            };
            let left = if x > 0 { i64::from(po[2 * x - 1]) } else { a };
            let diff = i64::from(pr[x]) + smooth_tendency(left, a, next);
            let aa = a + diff / 2;
            po[2 * x] = aa as i32;
            po[2 * x + 1] = (aa - diff) as i32;
        }
        if avg.w > res.w {
            po[2 * res.w] = pa[res.w];
        }
    }
    avg.w = ow;
    avg.data = out;
    avg.hshift -= 1;
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn inv_v_squeeze(avg: &mut Channel, res: &Channel) -> Result<(), Error> {
    if res.h == 0 {
        avg.vshift -= 1;
        return Ok(());
    }
    let w = avg.w;
    if res.w < w {
        return Err(fault());
    }
    let oh = avg.h + res.h;
    let mut out = vec![0i32; w * oh];
    for y in 0..res.h {
        for x in 0..w {
            let a = i64::from(avg.data[y * w + x]);
            let next = if y + 1 < avg.h {
                i64::from(avg.data[(y + 1) * w + x])
            } else {
                a
            };
            let top = if y > 0 {
                i64::from(out[(2 * y - 1) * w + x])
            } else {
                a
            };
            let diff = i64::from(res.data[y * res.w + x]) + smooth_tendency(top, a, next);
            let aa = a + diff / 2;
            out[2 * y * w + x] = aa as i32;
            out[(2 * y + 1) * w + x] = (aa - diff) as i32;
        }
    }
    if avg.h > res.h {
        out[2 * res.h * w..(2 * res.h + 1) * w]
            .copy_from_slice(&avg.data[res.h * w..(res.h + 1) * w]);
    }
    avg.h = oh;
    avg.data = out;
    avg.vshift -= 1;
    Ok(())
}
