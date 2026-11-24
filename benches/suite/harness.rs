use std::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Work {
    Bytes(u64),
    Pixels(u64),
    Items(u64),
}
impl Work {
    fn per_second(self, ns: f64) -> String {
        let s = ns / 1e9;
        match self {
            Work::Bytes(b) => format!("{:.1} MB/s", b as f64 / 1e6 / s),
            Work::Pixels(p) => format!("{:.1} Mpx/s", p as f64 / 1e6 / s),
            Work::Items(n) => format!("{:.0} /s", n as f64 / s),
        }
    }
    fn json(self) -> String {
        match self {
            Work::Bytes(b) => format!("\"work_unit\":\"bytes\",\"work\":{b}"),
            Work::Pixels(p) => format!("\"work_unit\":\"pixels\",\"work\":{p}"),
            Work::Items(n) => format!("\"work_unit\":\"items\",\"work\":{n}"),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Record {
    pub name: String,
    pub work: Work,
    pub note: String,
    pub rounds: Vec<Vec<f64>>,
    pub calls_per_sample: Vec<u64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub name: String,
    pub median_ns: f64,
    pub p25_ns: f64,
    pub p75_ns: f64,
    pub min_ns: f64,
    pub round_min_ns: f64,
    pub round_max_ns: f64,
    pub samples: usize,
    pub rounds: usize,
}
pub fn quantile(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let pos = q * (s.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    s[lo] + (s[hi] - s[lo]) * (pos - lo as f64)
}
impl Record {
    pub fn summary(&self) -> Summary {
        let all: Vec<f64> = self.rounds.concat();
        let medians: Vec<f64> = self.rounds.iter().map(|r| quantile(r, 0.5)).collect();
        Summary {
            name: self.name.clone(),
            median_ns: quantile(&all, 0.5),
            p25_ns: quantile(&all, 0.25),
            p75_ns: quantile(&all, 0.75),
            min_ns: all.iter().copied().fold(f64::INFINITY, f64::min),
            round_min_ns: medians.iter().copied().fold(f64::INFINITY, f64::min),
            round_max_ns: medians.iter().copied().fold(0.0, f64::max),
            samples: all.len(),
            rounds: self.rounds.len(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Settings {
    pub target: Duration,
    pub samples: usize,
    pub filters: Vec<String>,
    pub skips: Vec<String>,
}
pub struct Runner {
    pub settings: Settings,
    pub records: Vec<Record>,
}
#[derive(Debug, Clone, Copy)]
pub struct Precision {
    pub samples_factor: f64,
    pub time_factor: f64,
}
impl Precision {
    pub const NORMAL: Precision = Precision {
        samples_factor: 1.0,
        time_factor: 1.0,
    };
    pub const HIGH: Precision = Precision {
        samples_factor: 2.0,
        time_factor: 3.0,
    };
}
impl Runner {
    pub fn new(settings: Settings) -> Runner {
        Runner {
            settings,
            records: Vec::new(),
        }
    }
    pub fn selected(&self, name: &str) -> bool {
        (self.settings.filters.is_empty() || self.settings.filters.iter().any(|f| name.contains(f)))
            && !self.settings.skips.iter().any(|s| name.contains(s))
    }
    pub fn any_selected(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.selected(n))
    }
    pub fn bench<T>(&mut self, name: &str, work: Work, f: impl FnMut() -> T) {
        self.bench_with(name, work, "", Precision::NORMAL, f);
    }
    pub fn bench_with<T>(
        &mut self,
        name: &str,
        work: Work,
        note: &str,
        precision: Precision,
        mut f: impl FnMut() -> T,
    ) {
        if !self.selected(name) {
            return;
        }
        let target = self.settings.target.mul_f64(precision.time_factor);
        let samples = ((self.settings.samples as f64 * precision.samples_factor) as usize).max(3);
        let warm_until = Instant::now() + target / 10;
        let mut calls = 0u64;
        let started = Instant::now();
        loop {
            black_box(f());
            calls += 1;
            if Instant::now() >= warm_until && calls >= 1 {
                break;
            }
        }
        let per_call = started.elapsed().as_nanos() as f64 / calls as f64;
        let sample_ns = target.as_nanos() as f64 / samples as f64;
        let n = ((sample_ns / per_call.max(1.0)) as u64).clamp(1, 1 << 24);
        let samples = if per_call > sample_ns {
            samples.min(((target.as_nanos() as f64 / per_call) as usize).max(3))
        } else {
            samples
        };
        let mut obs = Vec::with_capacity(samples);
        for _ in 0..samples {
            let t = Instant::now();
            for _ in 0..n {
                black_box(f());
            }
            obs.push(t.elapsed().as_nanos() as f64 / n as f64);
        }
        let idx = match self.records.iter().position(|r| r.name == name) {
            Some(i) => i,
            None => {
                self.records.push(Record {
                    name: name.to_string(),
                    work,
                    note: note.to_string(),
                    rounds: Vec::new(),
                    calls_per_sample: Vec::new(),
                });
                self.records.len() - 1
            }
        };
        let rec = &mut self.records[idx];
        rec.rounds.push(obs);
        rec.calls_per_sample.push(n);
        let med = quantile(rec.rounds.last().map(Vec::as_slice).unwrap_or(&[]), 0.5);
        eprintln!("  {name:<52} {:>12}  {}", fmt_ns(med), work.per_second(med));
    }
    pub fn table(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "| Benchmark | Median | p25-p75 | Round medians (min-max) | Throughput | Samples |"
        );
        let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|");
        for r in &self.records {
            let s = r.summary();
            let _ = writeln!(
                out,
                "| `{}` | {} | {}-{} | {}-{} | {} | {}x{} |",
                s.name,
                fmt_ns(s.median_ns),
                fmt_ns(s.p25_ns),
                fmt_ns(s.p75_ns),
                fmt_ns(s.round_min_ns),
                fmt_ns(s.round_max_ns),
                r.work.per_second(s.median_ns),
                s.rounds,
                s.samples / s.rounds.max(1),
            );
        }
        out
    }
    pub fn json_lines(&self, header: &str) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "{header}");
        for r in &self.records {
            let s = r.summary();
            let _ = writeln!(
                out,
                "{{\"name\":\"{}\",\"median_ns\":{:.1},\"p25_ns\":{:.1},\"p75_ns\":{:.1},\"min_ns\":{:.1},\"round_min_ns\":{:.1},\"round_max_ns\":{:.1},\"samples\":{},\"rounds\":{},{},\"note\":\"{}\"}}",
                json_escape(&s.name),
                s.median_ns,
                s.p25_ns,
                s.p75_ns,
                s.min_ns,
                s.round_min_ns,
                s.round_max_ns,
                s.samples,
                s.rounds,
                r.work.json(),
                json_escape(&r.note),
            );
        }
        out
    }
}
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}
pub fn fmt_ns(ns: f64) -> String {
    if !ns.is_finite() {
        "-".into()
    } else if ns >= 1e9 {
        format!("{:.3} s", ns / 1e9)
    } else if ns >= 1e6 {
        format!("{:.2} ms", ns / 1e6)
    } else if ns >= 1e3 {
        format!("{:.1} µs", ns / 1e3)
    } else {
        format!("{ns:.0} ns")
    }
}
pub fn cpu_brand() -> String {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        let mut b = Vec::new();
        for leaf in 0x8000_0002u32..=0x8000_0004 {
            let r = __cpuid(leaf);
            for x in [r.eax, r.ebx, r.ecx, r.edx] {
                b.extend_from_slice(&x.to_le_bytes());
            }
        }
        String::from_utf8_lossy(&b)
            .trim_matches(['\0', ' '])
            .to_string()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        std::env::consts::ARCH.to_string()
    }
}
