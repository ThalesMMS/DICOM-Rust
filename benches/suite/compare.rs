use crate::harness::{Summary, fmt_ns};
use std::fmt::Write as _;
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat)? + pat.len();
    let rest = &line[start..];
    if let Some(s) = rest.strip_prefix('"') {
        let mut end = 0;
        let bytes = s.as_bytes();
        while end < bytes.len() {
            match bytes[end] {
                b'\\' => end += 2,
                b'"' => break,
                _ => end += 1,
            }
        }
        Some(&s[..end.min(s.len())])
    } else {
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        Some(rest[..end].trim())
    }
}
fn num(line: &str, key: &str) -> f64 {
    field(line, key)
        .and_then(|v| v.parse().ok())
        .unwrap_or(f64::NAN)
}
pub fn header_field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find(|l| l.contains("\"suite\""))
        .and_then(|l| field(l, key))
        .map(str::to_string)
}
pub fn parse(text: &str) -> Vec<Summary> {
    text.lines()
        .filter(|l| l.starts_with("{\"name\""))
        .filter_map(|l| {
            Some(Summary {
                name: field(l, "name")?.to_string(),
                median_ns: num(l, "median_ns"),
                p25_ns: num(l, "p25_ns"),
                p75_ns: num(l, "p75_ns"),
                min_ns: num(l, "min_ns"),
                round_min_ns: num(l, "round_min_ns"),
                round_max_ns: num(l, "round_max_ns"),
                samples: num(l, "samples") as usize,
                rounds: num(l, "rounds") as usize,
            })
        })
        .collect()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Regression,
    Improvement,
    Same,
}
pub fn verdict(base: &Summary, run: &Summary, tolerance: f64) -> Verdict {
    let ratio = run.median_ns / base.median_ns;
    if ratio > 1.0 + tolerance && run.p25_ns > base.p75_ns {
        Verdict::Regression
    } else if ratio < 1.0 / (1.0 + tolerance) && run.p75_ns < base.p25_ns {
        Verdict::Improvement
    } else {
        Verdict::Same
    }
}
pub fn compare(base: &[Summary], run: &[Summary], tolerance: f64) -> (String, usize) {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "| Benchmark | Baseline median | Run median | Run / baseline | Verdict |"
    );
    let _ = writeln!(out, "|---|---:|---:|---:|---|");
    let mut regressions = 0;
    let mut improvements = 0;
    for r in run {
        let Some(b) = base.iter().find(|b| b.name == r.name) else {
            let _ = writeln!(
                out,
                "| `{}` | - | {} | - | new |",
                r.name,
                fmt_ns(r.median_ns)
            );
            continue;
        };
        let v = verdict(b, r, tolerance);
        let label = match v {
            Verdict::Regression => {
                regressions += 1;
                "REGRESSION"
            }
            Verdict::Improvement => {
                improvements += 1;
                "faster"
            }
            Verdict::Same => "",
        };
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {:.2}x | {} |",
            r.name,
            fmt_ns(b.median_ns),
            fmt_ns(r.median_ns),
            r.median_ns / b.median_ns,
            label
        );
    }
    let missing: Vec<&str> = base
        .iter()
        .filter(|b| !run.iter().any(|r| r.name == b.name))
        .map(|b| b.name.as_str())
        .collect();
    let _ = writeln!(
        out,
        "\n{regressions} regression(s), {improvements} faster, tolerance {:.0}% (median above it and p25 above the baseline's p75).",
        tolerance * 100.0
    );
    if !missing.is_empty() {
        let _ = writeln!(out, "Not in this run: {}.", missing.join(", "));
    }
    (out, regressions)
}
pub fn self_check() -> Result<(), String> {
    let s = |med: f64, p25: f64, p75: f64| Summary {
        name: "a".into(),
        median_ns: med,
        p25_ns: p25,
        p75_ns: p75,
        min_ns: p25,
        round_min_ns: med,
        round_max_ns: med,
        samples: 10,
        rounds: 1,
    };
    let b = s(100.0, 95.0, 105.0);
    let cases = [
        (s(130.0, 125.0, 140.0), Verdict::Regression),
        (s(130.0, 100.0, 160.0), Verdict::Same),
        (s(70.0, 65.0, 75.0), Verdict::Improvement),
    ];
    for (run, want) in cases {
        if verdict(&b, &run, 0.2) != want {
            return Err(format!("compare: {run:?} is not {want:?}"));
        }
    }
    let line = "{\"name\":\"x/y\",\"median_ns\":12.5,\"p25_ns\":10.0,\"p75_ns\":15.0,\"min_ns\":9.0,\"round_min_ns\":11.0,\"round_max_ns\":13.0,\"samples\":30,\"rounds\":3,\"work_unit\":\"pixels\",\"work\":4,\"note\":\"a \\\"b\\\"\"}";
    let v = parse(line);
    if v.len() != 1 || v[0].name != "x/y" || v[0].median_ns != 12.5 || v[0].rounds != 3 {
        return Err(format!("compare: parse gave {v:?}"));
    }
    Ok(())
}
