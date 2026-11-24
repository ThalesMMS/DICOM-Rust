use std::path::{Path, PathBuf};
pub fn set_dir(set: &str) -> Option<PathBuf> {
    let required = std::env::var_os("DICOM_RUST_REQUIRE_CONFORMANCE").is_some();
    let dir = std::env::var_os("DICOM_RUST_CONFORMANCE_CACHE")
        .map(|cache| Path::new(&cache).join(set).join("data"));
    match dir {
        Some(dir) if dir.is_dir() => Some(dir),
        other => {
            let note = match other {
                Some(dir) => format!("conformance: {set} not run, {} is missing", dir.display()),
                None => {
                    format!("conformance: {set} not run, DICOM_RUST_CONFORMANCE_CACHE is not set")
                }
            };
            assert!(!required, "{note}");
            eprintln!("{note}");
            None
        }
    }
}
pub fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Pass(String),
    Differs(String),
    Refused(String),
}
pub struct Report {
    set: &'static str,
    lines: Vec<String>,
    unexpected: Vec<String>,
    passed: usize,
    explained: usize,
}
impl Report {
    pub fn new(set: &'static str) -> Report {
        Report {
            set,
            lines: Vec::new(),
            unexpected: Vec::new(),
            passed: 0,
            explained: 0,
        }
    }
    pub fn case(&mut self, name: &str, outcome: Outcome, reason: Option<&str>) {
        let line = match (&outcome, reason) {
            (Outcome::Pass(detail), None) => {
                self.passed += 1;
                format!("pass      {name}: {detail}")
            }
            (Outcome::Pass(detail), Some(reason)) => {
                self.unexpected.push(format!(
                    "{name} passes ({detail}) but is listed as: {reason}"
                ));
                format!("PASSES    {name}: {detail}")
            }
            (Outcome::Differs(detail) | Outcome::Refused(detail), Some(reason)) => {
                self.explained += 1;
                let kind = if matches!(outcome, Outcome::Differs(_)) {
                    "differs"
                } else {
                    "refused"
                };
                format!("{kind:<9} {name}: {detail} [{reason}]")
            }
            (Outcome::Differs(detail), None) => {
                self.unexpected.push(format!("{name} differs: {detail}"));
                format!("DIFFERS   {name}: {detail}")
            }
            (Outcome::Refused(detail), None) => {
                self.unexpected.push(format!("{name} refused: {detail}"));
                format!("REFUSED   {name}: {detail}")
            }
        };
        self.lines.push(line);
    }
    pub fn note(&mut self, text: String) {
        self.lines.push(format!("note      {text}"));
    }
    pub fn finish(self) {
        println!(
            "== {}: {} pass, {} explained, {} unexpected",
            self.set,
            self.passed,
            self.explained,
            self.unexpected.len()
        );
        for line in &self.lines {
            println!("{line}");
        }
        assert!(
            self.unexpected.is_empty(),
            "{}: unexpected outcomes:\n{}",
            self.set,
            self.unexpected.join("\n")
        );
        assert!(
            self.passed + self.explained > 0,
            "{}: no case ran",
            self.set
        );
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    pub width: usize,
    pub height: usize,
    pub samples: Vec<i32>,
}
pub fn errors(got: &Plane, want: &Plane) -> Result<(i64, f64), String> {
    if (got.width, got.height) != (want.width, want.height) {
        return Err(format!(
            "size {}x{}, reference {}x{}",
            got.width, got.height, want.width, want.height
        ));
    }
    if got.samples.len() != want.samples.len() {
        return Err(format!(
            "{} samples, reference {}",
            got.samples.len(),
            want.samples.len()
        ));
    }
    let (mut peak, mut sum) = (0i64, 0f64);
    for (a, b) in got.samples.iter().zip(&want.samples) {
        let d = (i64::from(*a) - i64::from(*b)).abs();
        peak = peak.max(d);
        sum += (d * d) as f64;
    }
    Ok((peak, sum / got.samples.len().max(1) as f64))
}
pub fn compare(got: &[Plane], want: &[Plane], peak: &[i64], mse: &[f64]) -> Outcome {
    if got.len() < want.len() {
        return Outcome::Differs(format!(
            "{} components, reference {}",
            got.len(),
            want.len()
        ));
    }
    let mut detail = Vec::new();
    let mut ok = true;
    for (i, w) in want.iter().enumerate() {
        match errors(&got[i], w) {
            Err(e) => return Outcome::Differs(format!("component {i}: {e}")),
            Ok((p, m)) => {
                let (pt, mt) = (
                    peak.get(i).copied().unwrap_or(0),
                    mse.get(i).copied().unwrap_or(0.0),
                );
                ok &= p <= pt && m <= mt;
                detail.push(if pt == 0 && mt == 0.0 && p == 0 {
                    "exact".to_string()
                } else {
                    format!("peak {p}/{pt} mse {m:.3}/{mt}")
                });
            }
        }
    }
    detail.dedup();
    let detail = detail.join(", ");
    if ok {
        Outcome::Pass(detail)
    } else {
        Outcome::Differs(detail)
    }
}
fn token(data: &[u8], at: &mut usize) -> Result<String, String> {
    loop {
        while *at < data.len() && data[*at].is_ascii_whitespace() {
            *at += 1;
        }
        if *at < data.len() && data[*at] == b'#' {
            while *at < data.len() && data[*at] != b'\n' {
                *at += 1;
            }
            continue;
        }
        break;
    }
    let start = *at;
    while *at < data.len() && !data[*at].is_ascii_whitespace() {
        *at += 1;
    }
    if start == *at {
        return Err("short header".into());
    }
    Ok(String::from_utf8_lossy(&data[start..*at]).into_owned())
}
fn number(data: &[u8], at: &mut usize) -> Result<usize, String> {
    let t = token(data, at)?;
    t.parse().map_err(|_| format!("bad header number {t:?}"))
}
pub fn read_pnm(data: &[u8]) -> Result<Vec<Plane>, String> {
    let mut at = 0;
    let channels = match token(data, &mut at)?.as_str() {
        "P5" => 1,
        "P6" => 3,
        other => return Err(format!("not a binary PNM: {other:?}")),
    };
    let width = number(data, &mut at)?;
    let height = number(data, &mut at)?;
    let max = number(data, &mut at)?;
    at += 1;
    let wide = max > 255;
    let bytes = if wide { 2 } else { 1 };
    let body = data.get(at..).unwrap_or_default();
    if body.len() < width * height * channels * bytes {
        return Err("short PNM body".into());
    }
    let mut planes = vec![
        Plane {
            width,
            height,
            samples: Vec::with_capacity(width * height),
        };
        channels
    ];
    for (i, sample) in body
        .chunks_exact(bytes)
        .take(width * height * channels)
        .enumerate()
    {
        let v = if wide {
            i32::from(u16::from_be_bytes([sample[0], sample[1]]))
        } else {
            i32::from(sample[0])
        };
        planes[i % channels].samples.push(v);
    }
    Ok(planes)
}
pub fn extend(v: u32, bits: u32, signed: bool) -> i32 {
    let unused = 32 - bits.clamp(1, 32);
    if signed {
        ((v << unused) as i32) >> unused
    } else {
        ((v << unused) >> unused) as i32
    }
}
pub fn read_pgx(data: &[u8]) -> Result<Plane, String> {
    let mut at = 0;
    if token(data, &mut at)? != "PG" {
        return Err("not a PGX file".into());
    }
    let big = match token(data, &mut at)?.as_str() {
        "ML" => true,
        "LM" => false,
        other => return Err(format!("bad PGX byte order {other:?}")),
    };
    let mut depth = token(data, &mut at)?;
    if depth == "+" || depth == "-" {
        depth.push_str(&token(data, &mut at)?);
    }
    let signed = depth.starts_with('-');
    let depth: u32 = depth
        .trim_start_matches(['+', '-'])
        .parse()
        .map_err(|_| format!("bad PGX depth {depth:?}"))?;
    let width = number(data, &mut at)?;
    let height = number(data, &mut at)?;
    at += 1;
    let bytes = depth.div_ceil(8).max(1) as usize;
    let body = data.get(at..).unwrap_or_default();
    if body.len() < width * height * bytes {
        return Err("short PGX body".into());
    }
    let samples = body
        .chunks_exact(bytes)
        .take(width * height)
        .map(|c| {
            let mut v: u32 = 0;
            for i in 0..bytes {
                let b = if big { c[i] } else { c[bytes - 1 - i] };
                v = (v << 8) | u32::from(b);
            }
            extend(v, depth, signed)
        })
        .collect();
    Ok(Plane {
        width,
        height,
        samples,
    })
}
pub fn planes_from_interleaved(
    data: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    bytes: usize,
    signed: bool,
    bits: u32,
) -> Vec<Plane> {
    let mut planes = vec![
        Plane {
            width,
            height,
            samples: Vec::with_capacity(width * height),
        };
        channels
    ];
    for (i, c) in data.chunks_exact(bytes).enumerate() {
        let v = match bytes {
            1 => u32::from(c[0]),
            2 => u32::from(u16::from_le_bytes([c[0], c[1]])),
            _ => u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
        };
        planes[i % channels].samples.push(extend(v, bits, signed));
    }
    planes
}
