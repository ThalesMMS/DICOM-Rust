mod codecs;
mod compare;
mod dicomdir;
mod files;
mod harness;
mod inputs;
mod part10;
mod profile;
mod render;
mod synth;
mod volume;
mod volume_copy;
use harness::{Runner, Settings};
use inputs::Inputs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
struct Args {
    settings: Settings,
    rounds: usize,
    list: bool,
    save: Option<PathBuf>,
    baseline: Option<PathBuf>,
    compare_only: Option<PathBuf>,
    tolerance: f64,
    machine: String,
    quiet_machine: bool,
    check: bool,
    profile: bool,
    j2k_loop: Option<f64>,
    target: profile::Target,
    seconds: f64,
    interval_us: u64,
}
fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        settings: Settings {
            target: Duration::from_millis(1000),
            samples: 15,
            filters: Vec::new(),
            skips: Vec::new(),
        },
        rounds: 3,
        list: false,
        save: None,
        baseline: None,
        compare_only: None,
        tolerance: 0.20,
        machine: std::env::var("DICOM_RUST_BENCH_MACHINE").unwrap_or_else(|_| "unnamed".into()),
        quiet_machine: false,
        check: true,
        profile: false,
        j2k_loop: None,
        target: profile::Target::default(),
        seconds: 10.0,
        interval_us: 250,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--bench" => {}
            "--rounds" => {
                a.rounds = value("--rounds")?
                    .parse()
                    .map_err(|e| format!("--rounds: {e}"))?
            }
            "--samples" => {
                a.settings.samples = value("--samples")?
                    .parse()
                    .map_err(|e| format!("--samples: {e}"))?
            }
            "--target-ms" => {
                let ms: f64 = value("--target-ms")?
                    .parse()
                    .map_err(|e| format!("--target-ms: {e}"))?;
                a.settings.target = Duration::from_secs_f64(ms / 1e3);
            }
            "--quick" => {
                a.rounds = 1;
                a.settings.target = Duration::from_millis(300);
                a.settings.samples = 7;
            }
            "--skip" => a.settings.skips.push(value("--skip")?),
            "--list" => a.list = true,
            "--save" => a.save = Some(value("--save")?.into()),
            "--baseline" => a.baseline = Some(value("--baseline")?.into()),
            "--compare" => a.compare_only = Some(value("--compare")?.into()),
            "--tolerance" => {
                a.tolerance = value("--tolerance")?
                    .parse()
                    .map_err(|e| format!("--tolerance: {e}"))?
            }
            "--machine" => a.machine = value("--machine")?,
            "--quiet-machine" => a.quiet_machine = true,
            "--no-check" => a.check = false,
            "--profile-j2k" => a.profile = true,
            "--j2k-loop" => {
                a.j2k_loop = Some(
                    value("--j2k-loop")?
                        .parse()
                        .map_err(|e| format!("--j2k-loop: {e}"))?,
                )
            }
            "--codec" => a.target.codec = value("--codec")?,
            "--image" => a.target.image = value("--image")?,
            "--seconds" => {
                a.seconds = value("--seconds")?
                    .parse()
                    .map_err(|e| format!("--seconds: {e}"))?
            }
            "--interval-us" => {
                a.interval_us = value("--interval-us")?
                    .parse()
                    .map_err(|e| format!("--interval-us: {e}"))?
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s => a.settings.filters.push(s.to_string()),
        }
    }
    Ok(a)
}
fn all_names() -> Vec<String> {
    let mut v: Vec<String> = files::NAMES.iter().map(|s| s.to_string()).collect();
    v.extend(codecs::names());
    v.extend(dicomdir::NAMES.iter().map(|s| s.to_string()));
    v.extend(volume::NAMES.iter().map(|s| s.to_string()));
    v.extend(volume_copy::NAMES.iter().map(|s| s.to_string()));
    v.extend(render::NAMES.iter().map(|s| s.to_string()));
    v
}
fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}
fn read_results(path: &PathBuf) -> Result<Vec<harness::Summary>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v = compare::parse(&text);
    if v.is_empty() {
        return Err(format!("{}: no benchmark lines", path.display()));
    }
    Ok(v)
}
fn checks(inp: &Inputs, r: &Runner) -> Result<(), String> {
    compare::self_check()?;
    let wants = |names: &[String]| names.iter().any(|n| r.selected(n));
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    if wants(&s(files::NAMES)) {
        files::check(inp)?;
    }
    if wants(&codecs::names()) {
        codecs::check(inp)?;
    }
    if wants(&s(dicomdir::NAMES)) {
        dicomdir::check(inp)?;
    }
    if wants(&s(volume::NAMES)) {
        volume::check(inp)?;
    }
    if wants(&s(volume_copy::NAMES)) {
        volume_copy::check(inp)?;
    }
    if wants(&s(render::NAMES)) {
        render::check(inp)?;
    }
    Ok(())
}
fn run() -> Result<bool, String> {
    let a = parse_args()?;
    if a.list {
        for n in all_names() {
            println!("{n}");
        }
        return Ok(true);
    }
    if let Some(run) = &a.compare_only {
        let base = a.baseline.as_ref().ok_or("--compare needs --baseline")?;
        let (table, regressions) =
            compare::compare(&read_results(base)?, &read_results(run)?, a.tolerance);
        println!("{table}");
        return Ok(regressions == 0);
    }
    let inp = Inputs::default();
    if let Some(seconds) = a.j2k_loop {
        println!("{}", profile::decode_loop(&inp, &a.target, seconds)?);
        return Ok(true);
    }
    if a.profile {
        println!(
            "{}",
            profile::profile(&inp, &a.target, a.seconds, a.interval_us)?
        );
        return Ok(true);
    }
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    let mut r = Runner::new(a.settings.clone());
    eprintln!(
        "dicom-rust bench suite: {} ({threads} logical processors), {} rounds",
        harness::cpu_brand(),
        a.rounds
    );
    if a.check {
        let t = Instant::now();
        checks(&inp, &r)?;
        eprintln!(
            "inputs built and checked in {:.1} s",
            t.elapsed().as_secs_f64()
        );
    }
    for round in 0..a.rounds {
        eprintln!("round {}/{}", round + 1, a.rounds);
        files::run(&mut r, &inp);
        codecs::run(&mut r, &inp);
        dicomdir::run(&mut r, &inp);
        volume::run(&mut r, &inp);
        volume_copy::run(&mut r, &inp);
        render::run(&mut r, &inp);
    }
    let header = format!(
        "{{\"suite\":\"dicom-rust\",\"format\":1,\"machine\":\"{}\",\"cpu\":\"{}\",\"threads\":{threads},\"os\":\"{}\",\"arch\":\"{}\",\"date\":\"{}\",\"rounds\":{},\"target_ms\":{},\"samples\":{},\"quiet_machine\":{},\"crate_version\":\"{}\"}}",
        harness::json_escape(&a.machine),
        harness::json_escape(&harness::cpu_brand()),
        std::env::consts::OS,
        std::env::consts::ARCH,
        today(),
        a.rounds,
        a.settings.target.as_millis(),
        a.settings.samples,
        a.quiet_machine,
        env!("CARGO_PKG_VERSION"),
    );
    let lines = r.json_lines(&header);
    let last = inputs::cache_dir().join("last-run.json");
    let _ = std::fs::create_dir_all(inputs::cache_dir());
    let _ = std::fs::write(&last, &lines);
    if let Some(p) = &a.save {
        std::fs::write(p, &lines).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    println!(
        "Machine: {} ({}), {threads} logical processors, {} {}; {} rounds, {} ms and {} samples per benchmark and round{}.\n",
        a.machine,
        harness::cpu_brand(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        a.rounds,
        a.settings.target.as_millis(),
        a.settings.samples,
        if a.quiet_machine {
            ""
        } else {
            "; machine not reserved (provisional numbers)"
        }
    );
    println!("{}", r.table());
    eprintln!("results: {}", a.save.as_ref().unwrap_or(&last).display());
    if let Some(base) = &a.baseline {
        let base_rows = read_results(base)?;
        let run_rows: Vec<harness::Summary> = r.records.iter().map(|x| x.summary()).collect();
        if let Some(m) =
            compare::header_field(&std::fs::read_to_string(base).unwrap_or_default(), "cpu")
            && m != harness::cpu_brand()
        {
            println!(
                "Note: the baseline was taken on {m}; this run is on {}.\n",
                harness::cpu_brand()
            );
        }
        let (table, regressions) = compare::compare(&base_rows, &run_rows, a.tolerance);
        println!("{table}");
        return Ok(regressions == 0);
    }
    Ok(true)
}
fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}
