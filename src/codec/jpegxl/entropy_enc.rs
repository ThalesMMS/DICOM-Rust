#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
use super::bitio::{BitWriter, ceil_log2, floor_log2_u32};
use super::entropy::{
    ANS_LOG_TAB_SIZE, ANS_SIGNATURE, ANS_TAB_SIZE, HybridConfig, popcount_precision,
};
use crate::fmath;
const ENC_CONFIG: HybridConfig = HybridConfig {
    split_exp: 4,
    msb: 1,
    lsb: 0,
};
const MAX_ENC_CLUSTERS: usize = 255;
impl HybridConfig {
    pub(crate) fn encode(&self, v: u32) -> (u32, u32, u32) {
        let split = 1u32 << self.split_exp;
        if v < split {
            return (v, 0, 0);
        }
        let n = floor_log2_u32(v);
        let m = v - (1 << n);
        let token = split
            + ((n - self.split_exp) << (self.msb + self.lsb))
            + ((m >> (n - self.msb)) << self.lsb)
            + (m & ((1 << self.lsb) - 1));
        let nbits = n - self.msb - self.lsb;
        let extra = (v >> self.lsb) & ((1u32 << nbits) - 1);
        (token, nbits, extra)
    }
}
#[derive(Clone, Copy)]
pub(crate) struct TokenRef {
    pub ctx: usize,
    pub value: u32,
}
#[derive(Default)]
pub(crate) struct TokenStream {
    pub tokens: Vec<TokenRef>,
}
impl TokenStream {
    pub(crate) fn add(&mut self, ctx: usize, v: u32) {
        self.tokens.push(TokenRef { ctx, value: v });
    }
}
#[derive(Clone, Default)]
struct AnsPlan {
    dist: Vec<i32>,
    shift: u32,
    omit: usize,
    logc: Option<Vec<u32>>,
}
pub(crate) struct EntropyEncoder {
    context_map: Vec<u8>,
    clusters: usize,
    log_alpha: u32,
    dists: Vec<Vec<i32>>,
    plans: Vec<AnsPlan>,
    reverse: Vec<Vec<Vec<u16>>>,
}
fn histogram_cost(h: &[u32]) -> f64 {
    let total: u32 = h.iter().fold(0u32, |a, &c| a.wrapping_add(c));
    if total == 0 {
        return 0.0;
    }
    let ft = f64::from(total);
    let mut cost = 0.0;
    for &c in h {
        if c > 0 {
            cost -= f64::from(c) * fmath::log2(f64::from(c) / ft);
        }
    }
    cost
}
fn merged_cost_increase(a: &[u32], b: &[u32], cost_a: f64, cost_b: f64) -> f64 {
    let n = a.len().max(b.len());
    let mut m = vec![0u32; n];
    for (i, &v) in a.iter().enumerate() {
        m[i] = m[i].wrapping_add(v);
    }
    for (i, &v) in b.iter().enumerate() {
        m[i] = m[i].wrapping_add(v);
    }
    histogram_cost(&m) - cost_a - cost_b
}
impl EntropyEncoder {
    pub(crate) fn build(num_contexts: usize, streams: &[&TokenStream]) -> EntropyEncoder {
        let mut hist: Vec<Vec<u32>> = vec![Vec::new(); num_contexts];
        for s in streams {
            for t in &s.tokens {
                let (tok, _, _) = ENC_CONFIG.encode(t.value);
                let h = &mut hist[t.ctx];
                if tok as usize >= h.len() {
                    h.resize(tok as usize + 1, 0);
                }
                h[tok as usize] += 1;
            }
        }
        let mut used: Vec<usize> = (0..num_contexts).filter(|&c| !hist[c].is_empty()).collect();
        let mut e = EntropyEncoder {
            context_map: vec![0; num_contexts],
            clusters: 1,
            log_alpha: 5,
            dists: Vec::new(),
            plans: Vec::new(),
            reverse: Vec::new(),
        };
        if used.is_empty() {
            e.build_ans(&[vec![1]]);
            return e;
        }
        let mut costs = vec![0.0f64; num_contexts];
        for &c in &used {
            costs[c] = histogram_cost(&hist[c]);
        }
        used.sort_by(|&a, &b| {
            if costs[a] > costs[b] || (costs[a] == costs[b] && a < b) {
                std::cmp::Ordering::Less
            } else if costs[b] > costs[a] || (costs[a] == costs[b] && b < a) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        let mut centres = vec![used[0]];
        let mut dist: Vec<f64> = used
            .iter()
            .map(|&c| {
                merged_cost_increase(&hist[c], &hist[centres[0]], costs[c], costs[centres[0]])
            })
            .collect();
        while centres.len() < MAX_ENC_CLUSTERS && centres.len() < used.len() {
            let (mut best, mut best_d) = (None, 64.0);
            for (i, &d) in dist.iter().enumerate() {
                if d > best_d {
                    best = Some(i);
                    best_d = d;
                }
            }
            let Some(best) = best else { break };
            let bc = used[best];
            centres.push(bc);
            for (i, &c) in used.iter().enumerate() {
                let d = merged_cost_increase(&hist[c], &hist[bc], costs[c], costs[bc]);
                if d < dist[i] {
                    dist[i] = d;
                }
            }
        }
        let mut assign: Vec<Option<usize>> = vec![None; num_contexts];
        for (i, &c) in centres.iter().enumerate() {
            assign[c] = Some(i);
        }
        let mut merged: Vec<Vec<u32>> = centres.iter().map(|&c| hist[c].clone()).collect();
        for &c in &used {
            if assign[c].is_some() {
                continue;
            }
            let (mut best, mut best_d) = (0usize, f64::INFINITY);
            for (i, &k) in centres.iter().enumerate() {
                let d = merged_cost_increase(&hist[c], &hist[k], costs[c], costs[k]);
                if d < best_d {
                    best = i;
                    best_d = d;
                }
            }
            assign[c] = Some(best);
            let mb = &mut merged[best];
            if mb.len() < hist[c].len() {
                mb.resize(hist[c].len(), 0);
            }
            for (s, &n) in hist[c].iter().enumerate() {
                mb[s] = mb[s].wrapping_add(n);
            }
        }
        for (c, m) in e.context_map.iter_mut().enumerate() {
            *m = assign[c].unwrap_or(0) as u8;
        }
        e.clusters = centres.len();
        e.build_ans(&merged);
        e
    }
    fn build_ans(&mut self, hists: &[Vec<u32>]) {
        let mut max_alpha = 1;
        for h in hists {
            let mut n = h.len();
            while n > 0 && h[n - 1] == 0 {
                n -= 1;
            }
            max_alpha = max_alpha.max(n);
        }
        self.log_alpha = 5;
        while (1usize << self.log_alpha) < max_alpha {
            self.log_alpha += 1;
        }
        self.dists = Vec::with_capacity(hists.len());
        self.plans = Vec::with_capacity(hists.len());
        self.reverse = Vec::with_capacity(hists.len());
        for h in hists {
            let mut plan = plan_ans_distribution(h);
            if plan.dist.is_empty() {
                plan = AnsPlan {
                    dist: vec![ANS_TAB_SIZE as i32],
                    shift: 13,
                    ..Default::default()
                };
            }
            let d = plan.dist.clone();
            let rev = reverse_slots(&d, self.log_alpha);
            self.dists.push(d);
            self.plans.push(plan);
            self.reverse.push(rev);
        }
    }
    pub(crate) fn write_header(&self, w: &mut BitWriter) {
        w.bool(false);
        if self.context_map.len() > 1 {
            write_context_map(w, &self.context_map, self.clusters);
        }
        w.bool(false);
        w.write(2, u64::from(self.log_alpha - 5));
        let cfg_bits = ceil_log2(u64::from(self.log_alpha) + 1);
        for _ in 0..self.clusters {
            w.write(cfg_bits, u64::from(ENC_CONFIG.split_exp));
            w.write(
                ceil_log2(u64::from(ENC_CONFIG.split_exp) + 1),
                u64::from(ENC_CONFIG.msb),
            );
            w.write(
                ceil_log2(u64::from(ENC_CONFIG.split_exp - ENC_CONFIG.msb) + 1),
                u64::from(ENC_CONFIG.lsb),
            );
        }
        for p in &self.plans {
            write_ans_distribution(w, p);
        }
    }
    pub(crate) fn write_stream(&self, w: &mut BitWriter, tokens: &[TokenRef]) {
        if tokens.is_empty() {
            return;
        }
        let mut words = vec![-1i32; tokens.len()];
        let mut state: u32 = ANS_SIGNATURE;
        for (i, t) in tokens.iter().enumerate().rev() {
            let (tok, _, _) = ENC_CONFIG.encode(t.value);
            let c = self.context_map[t.ctx] as usize;
            let f = self.dists[c][tok as usize] as u32;
            if u64::from(state) >= u64::from(f) << 20 {
                words[i] = (state & 0xFFFF) as i32;
                state >>= 16;
            }
            state = ((state / f) << ANS_LOG_TAB_SIZE)
                + u32::from(self.reverse[c][tok as usize][(state % f) as usize]);
        }
        w.write(32, u64::from(state));
        for (i, t) in tokens.iter().enumerate() {
            if words[i] >= 0 {
                w.write(16, words[i] as u64);
            }
            let (_, nbits, extra) = ENC_CONFIG.encode(t.value);
            w.write(nbits, u64::from(extra));
        }
    }
}
fn reverse_slots(d: &[i32], log_alpha: u32) -> Vec<Vec<u16>> {
    let table = super::entropy::alias_slots(d, log_alpha);
    let mut rev: Vec<Vec<u16>> = d.iter().map(|&c| vec![0u16; c.max(0) as usize]).collect();
    for (slot, (sym, off)) in table.into_iter().enumerate() {
        if let Some(r) = rev.get_mut(sym as usize)
            && let Some(e) = r.get_mut(off as usize)
        {
            *e = slot as u16;
        }
    }
    rev
}
fn quantize_distribution(h: &[u32]) -> Vec<i32> {
    let mut n = h.len();
    while n > 0 && h[n - 1] == 0 {
        n -= 1;
    }
    let h = &h[..n];
    let total: u64 = h.iter().map(|&c| u64::from(c)).sum();
    let mut d = vec![0i32; n];
    let mut sum: i32 = 0;
    for (s, &c) in h.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let v = ((u64::from(c) * u64::from(ANS_TAB_SIZE) + total / 2) / total).max(1) as i32;
        d[s] = v;
        sum += v;
    }
    while sum != ANS_TAB_SIZE as i32 {
        let mut best: Option<usize> = None;
        for s in 0..d.len() {
            if d[s] == 0 {
                continue;
            }
            if sum > ANS_TAB_SIZE as i32 && d[s] <= 1 {
                continue;
            }
            if best.is_none_or(|b| d[s] > d[b]) {
                best = Some(s);
            }
        }
        let Some(best) = best else { break };
        let step = if sum > ANS_TAB_SIZE as i32 {
            -(sum - ANS_TAB_SIZE as i32).min(d[best] - 1)
        } else {
            ANS_TAB_SIZE as i32 - sum
        };
        d[best] += step;
        sum += step;
    }
    d
}
fn plan_ans_distribution(h: &[u32]) -> AnsPlan {
    let mut n = h.len();
    while n > 0 && h[n - 1] == 0 {
        n -= 1;
    }
    let h = &h[..n];
    let exact = quantize_distribution(h);
    let used = h.iter().filter(|&&c| c > 0).count();
    if used <= 2 {
        return AnsPlan {
            dist: exact,
            shift: 13,
            ..Default::default()
        };
    }
    let total: f64 = h.iter().map(|&c| f64::from(c)).sum();
    let length = n.max(3);
    let mut best: Option<AnsPlan> = None;
    let mut best_cost = f64::INFINITY;
    for shift in 0..=13u32 {
        let mut target = vec![0.0f64; length];
        let mut logc = vec![0u32; length];
        for (i, &c) in h.iter().enumerate() {
            target[i] = f64::from(c) * f64::from(ANS_TAB_SIZE) / total;
        }
        let (mut omit, mut omit_log): (usize, i64) = (0, -1);
        for i in 0..n {
            if h[i] == 0 {
                continue;
            }
            let l = floor_log2_u32((target[i] as i64).max(1) as u32) + 1;
            if i64::from(l) > omit_log {
                omit_log = i64::from(l);
                omit = i;
            }
        }
        let mut d = vec![0i32; length];
        let mut sum: i32 = 0;
        for i in 0..n {
            if h[i] == 0 || i == omit {
                continue;
            }
            let mut v = (target[i] as i32).max(1);
            let l = floor_log2_u32(v as u32) + 1;
            if l > 1 {
                let bc = popcount_precision(l - 1, shift);
                let drop = l - 1 - bc;
                v = v >> drop << drop;
            }
            d[i] = v;
            logc[i] = floor_log2_u32(v as u32) + 1;
            sum += v;
        }
        if sum >= ANS_TAB_SIZE as i32 {
            continue;
        }
        d[omit] = ANS_TAB_SIZE as i32 - sum;
        logc[omit] = omit_log as u32;
        let mut bits = 2.0 + 3.0;
        for i in 0..length {
            bits += f64::from(LOG_COUNT_ENCODE[logc[i] as usize].1);
            if i != omit && logc[i] > 1 {
                bits += f64::from(popcount_precision(logc[i] - 1, shift));
            }
        }
        for (i, &c) in h.iter().enumerate() {
            if c > 0 {
                bits += f64::from(c) * fmath::log2(f64::from(ANS_TAB_SIZE) / f64::from(d[i]));
            }
        }
        if bits < best_cost {
            best_cost = bits;
            d.truncate(n);
            best = Some(AnsPlan {
                dist: d,
                shift,
                omit,
                logc: Some(logc),
            });
        }
    }
    best.unwrap_or(AnsPlan {
        dist: exact,
        shift: 13,
        ..Default::default()
    })
}
fn write_var_len_uint8(w: &mut BitWriter, v: usize) {
    if v == 0 {
        w.write(1, 0);
        return;
    }
    let n = floor_log2_u32(v as u32);
    w.write(1, 1);
    w.write(3, u64::from(n));
    w.write(n, (v - (1 << n)) as u64);
}
const LOG_COUNT_ENCODE: [(u8, u8); 14] = [
    (17, 5),
    (11, 4),
    (15, 4),
    (3, 4),
    (9, 4),
    (7, 4),
    (4, 3),
    (2, 3),
    (5, 3),
    (6, 3),
    (0, 3),
    (33, 6),
    (1, 7),
    (65, 7),
];
fn write_ans_distribution(w: &mut BitWriter, p: &AnsPlan) {
    let d = &p.dist;
    let used: Vec<usize> = (0..d.len()).filter(|&s| d[s] > 0).collect();
    if used.len() == 1 {
        w.write(1, 1);
        w.write(1, 0);
        write_var_len_uint8(w, used[0]);
        return;
    }
    if used.len() == 2 {
        w.write(1, 1);
        w.write(1, 1);
        write_var_len_uint8(w, used[0]);
        write_var_len_uint8(w, used[1]);
        w.write(ANS_LOG_TAB_SIZE, d[used[0]] as u64);
        return;
    }
    w.write(1, 0);
    w.write(1, 0);
    let shift = p.shift + 1;
    let log = floor_log2_u32(shift);
    for _ in 0..log {
        w.write(1, 1);
    }
    if log < 3 {
        w.write(1, 0);
    }
    w.write(log, u64::from(shift - (1 << log)));
    let n = d.len().max(3);
    write_var_len_uint8(w, n - 3);
    let mut omit = p.omit;
    let logc = match &p.logc {
        Some(l) => l.clone(),
        None => {
            let mut logc = vec![0u32; n];
            let mut omit_log: i64 = -1;
            for i in 0..n {
                if i < d.len() && d[i] > 0 {
                    logc[i] = floor_log2_u32(d[i] as u32) + 1;
                }
                if i64::from(logc[i]) > omit_log {
                    omit_log = i64::from(logc[i]);
                    omit = i;
                }
            }
            logc
        }
    };
    for &l in logc.iter().take(n) {
        let e = LOG_COUNT_ENCODE[l as usize];
        w.write(u32::from(e.1), u64::from(e.0));
    }
    for i in 0..n {
        if i == omit || logc[i] <= 1 {
            continue;
        }
        let bc = popcount_precision(logc[i] - 1, p.shift);
        w.write(
            bc,
            ((d[i] - (1 << (logc[i] - 1))) as u64) >> (logc[i] - 1 - bc),
        );
    }
}
pub(crate) fn write_context_map(w: &mut BitWriter, m: &[u8], clusters: usize) {
    let mut best: Option<BitWriter> = None;
    if clusters <= 8 {
        let mut simple = BitWriter::default();
        let bits_per = ceil_log2(clusters as u64);
        simple.bool(true);
        simple.write(2, u64::from(bits_per));
        for &v in m {
            simple.write(bits_per, u64::from(v));
        }
        best = Some(simple);
    }
    if m.len() > 2 {
        let mut coded = BitWriter::default();
        coded.bool(false);
        coded.bool(false);
        let mut ts = TokenStream::default();
        for &v in m {
            ts.add(0, u32::from(v));
        }
        let inner = EntropyEncoder::build(1, &[&ts]);
        inner.write_header(&mut coded);
        inner.write_stream(&mut coded, &ts.tokens);
        if best.as_ref().is_none_or(|b| coded.bit_len() < b.bit_len()) {
            best = Some(coded);
        }
    }
    if let Some(b) = best {
        w.append_writer(&b);
    }
}
