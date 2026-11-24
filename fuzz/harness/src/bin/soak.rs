use dicom_rust_fuzz_harness::{TARGETS, seeds, target};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
const MAX_ALLOCATION: usize = 1 << 30;
static CURRENT_INPUT: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static CURRENT_LEN: AtomicUsize = AtomicUsize::new(0);
static IN_ALLOC_REPORT: AtomicBool = AtomicBool::new(false);
static ARTIFACT_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
struct Guarded;
unsafe impl GlobalAlloc for Guarded {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        check_allocation(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        check_allocation(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        check_allocation(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
#[global_allocator]
static ALLOCATOR: Guarded = Guarded;
fn check_allocation(size: usize) {
    if size <= MAX_ALLOCATION || IN_ALLOC_REPORT.swap(true, Ordering::SeqCst) {
        return;
    }
    let ptr = CURRENT_INPUT.load(Ordering::SeqCst);
    let len = CURRENT_LEN.load(Ordering::SeqCst);
    eprintln!("soak: allocation of {size} bytes exceeds {MAX_ALLOCATION}");
    eprintln!("{}", std::backtrace::Backtrace::force_capture());
    if !ptr.is_null()
        && let Some(dir) = ARTIFACT_DIR.get()
    {
        let input = unsafe { std::slice::from_raw_parts(ptr, len) };
        let path = dir.join(format!("alloc-{:016x}.bin", fnv(input)));
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&path, input);
        eprintln!("soak: input saved to {}", path.display());
    }
    std::process::abort();
}
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}
fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
const INTERESTING: [u32; 14] = [
    0,
    1,
    2,
    0x7F,
    0x80,
    0xFF,
    0x100,
    0x7FFF,
    0x8000,
    0xFFFF,
    0x1_0000,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFF,
];
fn mutate(rng: &mut Rng, data: &mut Vec<u8>, corpus: &[Vec<u8>], max_len: usize) {
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        let len = data.len();
        match rng.below(12) {
            0 if len > 0 => {
                let i = rng.below(len);
                data[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                data[i] = rng.next() as u8;
            }
            2 if len >= 2 => {
                let i = rng.below(len - 1);
                let v = INTERESTING[rng.below(INTERESTING.len())] as u16;
                let bytes = if rng.below(2) == 0 {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                };
                data[i..i + 2].copy_from_slice(&bytes);
            }
            3 if len >= 4 => {
                let i = rng.below(len - 3);
                let v = INTERESTING[rng.below(INTERESTING.len())];
                let bytes = if rng.below(2) == 0 {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                };
                data[i..i + 4].copy_from_slice(&bytes);
            }
            4 if len > 0 => {
                let i = rng.below(len);
                let n = 1 + rng.below((len - i).min(64));
                data.drain(i..i + n);
            }
            5 if len > 0 => {
                let i = rng.below(len);
                let n = 1 + rng.below((len - i).min(64));
                let chunk = data[i..i + n].to_vec();
                let at = rng.below(len + 1);
                data.splice(at..at, chunk);
            }
            6 => {
                let at = rng.below(len + 1);
                let n = 1 + rng.below(16);
                let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                data.splice(at..at, bytes);
            }
            7 if len > 0 => data.truncate(rng.below(len)),
            8 if !corpus.is_empty() => {
                let other = &corpus[rng.below(corpus.len())];
                let cut = rng.below(len + 1);
                let from = rng.below(other.len() + 1);
                data.truncate(cut);
                data.extend_from_slice(&other[from..]);
            }
            9 if !corpus.is_empty() && len > 0 => {
                let other = &corpus[rng.below(corpus.len())];
                if !other.is_empty() {
                    let from = rng.below(other.len());
                    let n = 1 + rng.below((other.len() - from).min(128));
                    let at = rng.below(len);
                    let end = (at + n).min(len);
                    data.splice(at..end, other[from..from + n].iter().copied());
                }
            }
            10 if len >= 4 => {
                let i = rng.below(len - 3);
                let delta = (rng.below(33) as i64 - 16) as u32;
                let le = rng.below(2) == 0;
                let mut b = [0u8; 4];
                b.copy_from_slice(&data[i..i + 4]);
                let v = if le {
                    u32::from_le_bytes(b)
                } else {
                    u32::from_be_bytes(b)
                };
                let v = v.wrapping_add(delta);
                let b = if le { v.to_le_bytes() } else { v.to_be_bytes() };
                data[i..i + 4].copy_from_slice(&b);
            }
            _ if len > 0 => {
                let i = rng.below(len);
                let n = 1 + rng.below((len - i).min(16));
                let v = [0u8, 0xFF, 0x20, 0x41][rng.below(4)];
                data[i..i + n].fill(v);
            }
            _ => data.push(rng.next() as u8),
        }
    }
    data.truncate(max_len);
}
const HANG_SECONDS: u64 = 120;
static INPUT_BEGAN_MS: AtomicU64 = AtomicU64::new(0);
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}
fn start_watchdog() {
    let _ = EPOCH.get_or_init(Instant::now);
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let began = INPUT_BEGAN_MS.load(Ordering::SeqCst);
            if began == 0 || now_ms().saturating_sub(began) < HANG_SECONDS * 1000 {
                continue;
            }
            let ptr = CURRENT_INPUT.load(Ordering::SeqCst);
            let len = CURRENT_LEN.load(Ordering::SeqCst);
            eprintln!("soak: one input ran for more than {HANG_SECONDS} s");
            if !ptr.is_null()
                && let Some(dir) = ARTIFACT_DIR.get()
            {
                let input = unsafe { std::slice::from_raw_parts(ptr, len) };
                let path = dir.join(format!("hang-{:016x}.bin", fnv(input)));
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(&path, input);
                eprintln!("soak: input saved to {}", path.display());
            }
            std::process::abort();
        }
    });
}
thread_local! {
    static LAST_PANIC: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}
struct Config {
    seconds: u64,
    seed: u64,
    corpus: PathBuf,
    artifacts: PathBuf,
    max_len: usize,
    record_inputs: bool,
}
fn load_corpus(name: &str, corpus: &Path) -> Vec<Vec<u8>> {
    let mut all = seeds::seeds(name);
    let mut files: Vec<PathBuf> = std::fs::read_dir(corpus.join(name))
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for path in files {
        if let Ok(data) = std::fs::read(&path)
            && !all.contains(&data)
        {
            all.push(data);
        }
    }
    all
}
fn run_target(name: &str, config: &Config) -> usize {
    let Some(function) = target(name) else {
        eprintln!("soak: unknown target {name}");
        return 1;
    };
    let dir = config.artifacts.join(name);
    let _ = ARTIFACT_DIR.set(dir.clone());
    start_watchdog();
    let corpus = load_corpus(name, &config.corpus);
    if corpus.is_empty() {
        eprintln!("soak: {name}: no seeds");
        return 1;
    }
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".into());
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        LAST_PANIC.with(|p| *p.borrow_mut() = Some((location, message)));
    }));
    let mut rng = Rng(config.seed ^ fnv(name.as_bytes()) | 1);
    let mut seen = BTreeSet::new();
    let mut slow = 0usize;
    let start = Instant::now();
    let budget = Duration::from_secs(config.seconds);
    let mut executions = 0u64;
    let last_input = dir.join("last-input.bin");
    if config.record_inputs {
        let _ = std::fs::create_dir_all(&dir);
    }
    let mut input = Vec::new();
    loop {
        input.clear();
        if (executions as usize) < corpus.len() {
            input.extend_from_slice(&corpus[executions as usize]);
        } else {
            if start.elapsed() >= budget {
                break;
            }
            input.extend_from_slice(&corpus[rng.below(corpus.len())]);
            mutate(&mut rng, &mut input, &corpus, config.max_len);
        }
        if config.record_inputs {
            let _ = std::fs::write(&last_input, &input);
        }
        CURRENT_LEN.store(input.len(), Ordering::SeqCst);
        CURRENT_INPUT.store(input.as_mut_ptr(), Ordering::SeqCst);
        INPUT_BEGAN_MS.store(now_ms(), Ordering::SeqCst);
        let began = Instant::now();
        let result = std::panic::catch_unwind(|| function(&input));
        INPUT_BEGAN_MS.store(0, Ordering::SeqCst);
        CURRENT_INPUT.store(std::ptr::null_mut(), Ordering::SeqCst);
        executions += 1;
        if result.is_err() {
            let (location, message) = LAST_PANIC
                .with(|p| p.borrow_mut().take())
                .unwrap_or_else(|| ("unknown".into(), String::new()));
            if seen.insert(location.clone()) {
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join(format!("crash-{:016x}.bin", fnv(&input)));
                let _ = std::fs::write(&path, &input);
                eprintln!(
                    "soak: {name}: PANIC at {location}: {message}\n      input: {}",
                    path.display()
                );
            }
        } else if began.elapsed() > Duration::from_secs(10) {
            slow += 1;
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(format!("slow-{:016x}.bin", fnv(&input)));
            let _ = std::fs::write(&path, &input);
            eprintln!(
                "soak: {name}: SLOW ({:.1} s), input: {}",
                began.elapsed().as_secs_f64(),
                path.display()
            );
        }
    }
    if config.record_inputs {
        let _ = std::fs::remove_file(&last_input);
    }
    let _ = std::panic::take_hook();
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "soak: {name}: {executions} executions in {elapsed:.0} s ({:.0}/s), {} seeds, {} panic locations, {slow} slow",
        executions as f64 / elapsed.max(0.001),
        corpus.len(),
        seen.len()
    );
    seen.len() + slow
}
fn value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        for (name, _) in TARGETS {
            println!("{name}");
        }
        return;
    }
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let config = Config {
        seconds: value(&args, "--seconds")
            .and_then(|v| v.parse().ok())
            .unwrap_or(60),
        seed: value(&args, "--seed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1),
        corpus: value(&args, "--corpus").map_or_else(|| here.join("corpus"), PathBuf::from),
        artifacts: value(&args, "--artifacts")
            .map_or_else(|| here.join("artifacts"), PathBuf::from),
        max_len: value(&args, "--max-len")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1 << 16),
        record_inputs: args.iter().any(|a| a == "--record-inputs"),
    };
    if let Some(dir) = value(&args, "--write-seeds") {
        for (name, _) in TARGETS {
            let out = Path::new(&dir).join(name);
            let _ = std::fs::create_dir_all(&out);
            let all = seeds::seeds(name);
            for seed in &all {
                let _ = std::fs::write(out.join(format!("seed-{:016x}", fnv(seed))), seed);
            }
            println!("soak: {name}: {} seeds", all.len());
        }
        return;
    }
    if let Some(file) = value(&args, "--replay") {
        let name = value(&args, "--target").unwrap_or_default();
        let (Some(function), Ok(data)) = (target(&name), std::fs::read(&file)) else {
            eprintln!("soak: --replay FILE needs --target NAME and a readable file");
            std::process::exit(2);
        };
        function(&data);
        println!("soak: {name}: {file} ran without a panic");
        return;
    }
    let name = value(&args, "--target").unwrap_or_else(|| "all".into());
    if name != "all" {
        std::process::exit(i32::from(run_target(&name, &config) != 0));
    }
    let jobs = value(&args, "--jobs")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1usize)
        .max(1);
    let exe = std::env::current_exe().expect("the path of this program");
    let spawn = |target: &str, record: bool| {
        let mut command = std::process::Command::new(&exe);
        command
            .args(["--target", target])
            .args(["--seconds", &config.seconds.to_string()])
            .args(["--seed", &config.seed.to_string()])
            .args(["--max-len", &config.max_len.to_string()])
            .arg("--corpus")
            .arg(&config.corpus)
            .arg("--artifacts")
            .arg(&config.artifacts);
        if record {
            command.arg("--record-inputs");
        }
        command.spawn()
    };
    let mut failed = Vec::new();
    for batch in TARGETS.chunks(jobs) {
        let children: Vec<_> = batch
            .iter()
            .filter_map(|(name, _)| spawn(name, false).ok().map(|child| (*name, child)))
            .collect();
        for (name, mut child) in children {
            let status = child.wait().ok();
            match status.and_then(|s| s.code()) {
                Some(0) => {}
                Some(1) => failed.push(name),
                code => {
                    eprintln!(
                        "soak: {name}: process ended with {code:?}; running again to keep the input"
                    );
                    let _ = spawn(name, true).and_then(|mut c| c.wait());
                    let last = config.artifacts.join(name).join("last-input.bin");
                    if let Ok(data) = std::fs::read(&last) {
                        let kept = last.with_file_name(format!("abort-{:016x}.bin", fnv(&data)));
                        let _ = std::fs::rename(&last, &kept);
                        eprintln!("soak: {name}: input kept at {}", kept.display());
                    }
                    failed.push(name);
                }
            }
        }
    }
    if failed.is_empty() {
        println!("soak: all {} targets ran with no failure", TARGETS.len());
    } else {
        println!("soak: FAILED targets: {}", failed.join(", "));
        std::process::exit(1);
    }
}
