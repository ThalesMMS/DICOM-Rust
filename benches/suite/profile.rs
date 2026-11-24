use crate::codecs::{self, Img};
use crate::inputs::Inputs;
use dicom_rust::codec::jpeg2000::{self, CoreOptions, Registration, encoder as j2k_encoder};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
#[derive(Debug, Clone)]
pub struct Target {
    pub codec: String,
    pub image: String,
}
impl Default for Target {
    fn default() -> Target {
        Target {
            codec: "j2k_lossless".into(),
            image: "ct512".into(),
        }
    }
}
pub fn workload(inp: &Inputs, t: &Target) -> Result<(Img, Vec<u8>, &'static str), String> {
    let (uid, step, lossy) = match t.codec.as_str() {
        "j2k_lossless" => (jpeg2000::UID_LOSSLESS, 0.0, false),
        "j2k_lossy" => (jpeg2000::UID_LOSSY, codecs::J2K_LOSSY_STEP, true),
        "htj2k_lossless" => (jpeg2000::UID_HTJ2K_LOSSLESS, 0.0, false),
        other => {
            return Err(format!(
                "unknown codec {other} (j2k_lossless, j2k_lossy, htj2k_lossless)"
            ));
        }
    };
    let img = codecs::images(inp)
        .into_iter()
        .find(|i| i.name == t.image)
        .ok_or_else(|| format!("unknown image {} (ct512, mr256, dx2048, us640rgb)", t.image))?;
    let opts = j2k_encoder::Options {
        transfer_syntax_uid: uid.into(),
        color_transform: img.md.samples_per_pixel == 3,
        levels: 5,
        step_size: step,
        allow_lossy: lossy,
    };
    let cs = j2k_encoder::Encoder::new(&opts)
        .and_then(|e| e.encode_frame(&img.bytes, &img.md))
        .map_err(|e| e.to_string())?
        .data;
    Ok((img, cs, uid))
}
fn decode(img: &Img, cs: &[u8], uid: &str) {
    let r = jpeg2000::decode_frames(
        &[cs],
        &img.md,
        uid,
        Registration::Default,
        CoreOptions::default(),
    );
    std::hint::black_box(r.ok());
}
pub fn decode_loop(inp: &Inputs, t: &Target, seconds: f64) -> Result<String, String> {
    let (img, cs, uid) = workload(inp, t)?;
    let until = Instant::now() + Duration::from_secs_f64(seconds);
    let mut n = 0u64;
    let start = Instant::now();
    while Instant::now() < until {
        decode(&img, &cs, uid);
        n += 1;
    }
    Ok(format!(
        "{n} decodes of {} {} in {:.1} s ({:.3} ms each)",
        t.codec,
        t.image,
        start.elapsed().as_secs_f64(),
        start.elapsed().as_secs_f64() * 1e3 / n.max(1) as f64
    ))
}
#[cfg(not(all(windows, target_arch = "x86_64")))]
pub fn profile(
    _inp: &Inputs,
    _t: &Target,
    _seconds: f64,
    _interval_us: u64,
) -> Result<String, String> {
    Err("the built-in sampler is Windows x86-64 only; run `--j2k-loop SECONDS` under perf or samply".into())
}
#[cfg(all(windows, target_arch = "x86_64"))]
pub fn profile(inp: &Inputs, t: &Target, seconds: f64, interval_us: u64) -> Result<String, String> {
    use win::{Sampler, Symbols};
    let (img, cs, uid) = workload(inp, t)?;
    let stop = AtomicBool::new(false);
    let decodes = AtomicU64::new(0);
    let (tx, rx) = std::sync::mpsc::channel();
    const DEPTH: usize = 64;
    let mut stacks: Vec<(usize, [u64; DEPTH])> =
        Vec::with_capacity((seconds * 1e6 / interval_us as f64) as usize + 16);
    let mut elapsed = 0.0;
    std::thread::scope(|s| -> Result<(), String> {
        s.spawn(|| {
            let _ = tx.send(win::current_thread());
            while !stop.load(Ordering::Relaxed) {
                decode(&img, &cs, uid);
                decodes.fetch_add(1, Ordering::Relaxed);
            }
        });
        let target = rx.recv().map_err(|e| e.to_string());
        let sampler =
            target.and_then(|t| Sampler::open(t).ok_or_else(|| "OpenThread failed".to_string()));
        let sampler = match sampler {
            Ok(s) => s,
            Err(e) => {
                stop.store(true, Ordering::Relaxed);
                return Err(e);
            }
        };
        std::thread::sleep(Duration::from_millis(200));
        let d0 = decodes.load(Ordering::Relaxed);
        let start = Instant::now();
        let until = start + Duration::from_secs_f64(seconds);
        let mut buf = [0u64; DEPTH];
        while Instant::now() < until {
            let n = sampler.sample(&mut buf);
            if n > 0 && stacks.len() < stacks.capacity() {
                stacks.push((n, buf));
            }
            std::thread::sleep(Duration::from_micros(interval_us));
        }
        elapsed = start.elapsed().as_secs_f64();
        let d = decodes.load(Ordering::Relaxed) - d0;
        stop.store(true, Ordering::Relaxed);
        decodes.store(d, Ordering::Relaxed);
        Ok(())
    })?;
    let n_decodes = decodes.load(Ordering::Relaxed);
    let syms = Symbols::new().ok_or("SymInitialize failed")?;
    let mut cache: HashMap<u64, Vec<Frame>> = HashMap::new();
    let mut sources = Sources::default();
    let mut resolve = |addr: u64, inner: bool| -> Vec<Frame> {
        let a = if inner { addr } else { addr - 1 };
        cache
            .entry(a)
            .or_insert_with(|| {
                syms.resolve(a)
                    .into_iter()
                    .map(|(symbol, line)| {
                        let function = line
                            .as_ref()
                            .and_then(|(f, l)| sources.enclosing_fn(f, *l))
                            .unwrap_or_default();
                        Frame {
                            symbol,
                            line,
                            function,
                        }
                    })
                    .collect()
            })
            .clone()
    };
    let mut stage_self: HashMap<String, usize> = HashMap::new();
    let mut stage_incl: HashMap<String, usize> = HashMap::new();
    let mut fn_self: HashMap<String, usize> = HashMap::new();
    let mut line_self: HashMap<String, usize> = HashMap::new();
    let mut sym_incl: HashMap<String, usize> = HashMap::new();
    let mut with_lines = 0usize;
    for (n, stack) in &stacks {
        let frames: Vec<Frame> = stack[..*n]
            .iter()
            .enumerate()
            .flat_map(|(i, &a)| resolve(a, i == 0))
            .collect();
        if frames[0].line.is_some() {
            with_lines += 1;
        }
        let own_at = frames
            .iter()
            .position(|f| in_crate(f) || classify(f).starts_with("memory"))
            .unwrap_or(0);
        let leaf = &frames[own_at];
        let own = classify(leaf);
        *stage_self.entry(own.clone()).or_default() += 1;
        let owner = frames
            .iter()
            .filter(|f| in_crate(f))
            .map(classify)
            .find(|s| !s.starts_with("memory") && !s.starts_with("other"))
            .unwrap_or(own);
        *stage_incl.entry(owner).or_default() += 1;
        let place = match &leaf.line {
            Some((f, l)) => format!("{}:{l}", short(f)),
            None => leaf.symbol.clone(),
        };
        let func = if leaf.function.is_empty() {
            trim_symbol(&leaf.symbol)
        } else {
            format!(
                "{} ({})",
                leaf.function,
                leaf.line
                    .as_ref()
                    .map(|(f, _)| short(f))
                    .unwrap_or_default()
            )
        };
        *fn_self.entry(func).or_default() += 1;
        *line_self.entry(place).or_default() += 1;
        let mut seen: Vec<&str> = Vec::new();
        for f in frames
            .iter()
            .filter(|f| f.symbol.starts_with("dicom_rust::"))
        {
            if !seen.contains(&f.symbol.as_str()) {
                seen.push(&f.symbol);
                *sym_incl.entry(f.symbol.clone()).or_default() += 1;
            }
        }
    }
    let total = stacks.len().max(1) as f64;
    let pct = |n: usize| 100.0 * n as f64 / total;
    let sorted = |m: &HashMap<String, usize>| {
        let mut v: Vec<(String, usize)> = m.iter().map(|(k, v)| (k.clone(), *v)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    };
    let mut out = String::new();
    let per = elapsed * 1e3 / n_decodes.max(1) as f64;
    let _ = writeln!(
        out,
        "Profile of `{}` decode of `{}` ({}x{}, {} bytes): {} samples every {interval_us} µs over {elapsed:.1} s, \
         {n_decodes} decodes ({per:.3} ms each, single thread, sampler running). {:.0}% of samples have line information.\n",
        t.codec,
        t.image,
        img.md.columns,
        img.md.rows,
        cs.len(),
        stacks.len(),
        100.0 * with_lines as f64 / total
    );
    let _ = writeln!(out, "| Stage | Self | With callees |\n|---|---:|---:|");
    let mut stages: Vec<String> = stage_self
        .keys()
        .chain(stage_incl.keys())
        .cloned()
        .collect();
    stages.sort();
    stages.dedup();
    stages.sort_by(|a, b| {
        let ka = stage_incl.get(a).copied().unwrap_or(0) + stage_self.get(a).copied().unwrap_or(0);
        let kb = stage_incl.get(b).copied().unwrap_or(0) + stage_self.get(b).copied().unwrap_or(0);
        kb.cmp(&ka)
    });
    for s in &stages {
        let _ = writeln!(
            out,
            "| {s} | {:.1}% | {:.1}% |",
            pct(stage_self.get(s).copied().unwrap_or(0)),
            pct(stage_incl.get(s).copied().unwrap_or(0))
        );
    }
    let _ = writeln!(
        out,
        "\nSelf time by source function (top 20):\n\n| Function | Self |\n|---|---:|"
    );
    for (k, v) in sorted(&fn_self).into_iter().take(20) {
        let _ = writeln!(out, "| `{k}` | {:.1}% |", pct(v));
    }
    let _ = writeln!(
        out,
        "\nSelf time by source line (top 25):\n\n| Line | Self |\n|---|---:|"
    );
    for (k, v) in sorted(&line_self).into_iter().take(25) {
        let _ = writeln!(out, "| `{k}` | {:.1}% |", pct(v));
    }
    let _ = writeln!(
        out,
        "\nInclusive time by library function (top 15, inlined frames included):\n\n| Function | Inclusive |\n|---|---:|"
    );
    for (k, v) in sorted(&sym_incl).into_iter().take(15) {
        let _ = writeln!(out, "| `{}` | {:.1}% |", trim_symbol(&k), pct(v));
    }
    Ok(out)
}
#[derive(Clone, Default)]
struct Frame {
    symbol: String,
    line: Option<(String, u32)>,
    function: String,
}
fn in_crate(f: &Frame) -> bool {
    f.line.as_ref().is_some_and(|(file, _)| {
        let p = file.replace('\\', "/");
        p.contains("dicom-rust/src/")
    })
}
fn short(path: &str) -> String {
    let p = path.replace('\\', "/");
    match p.find("/src/") {
        Some(i) => p[i + 5..].to_string(),
        None => p.rsplit('/').next().unwrap_or(&p).to_string(),
    }
}
fn trim_symbol(s: &str) -> String {
    let s = s.split('<').next().unwrap_or(s);
    s.chars().take(110).collect()
}
fn classify(f: &Frame) -> String {
    let sym = f.symbol.as_str();
    let alloc = [
        "RtlAllocateHeap",
        "RtlFreeHeap",
        "RtlpAllocateHeap",
        "RtlpFreeHeap",
        "RtlReAllocateHeap",
        "HeapAlloc",
        "HeapFree",
        "__rust_alloc",
        "__rust_dealloc",
        "__rust_realloc",
        "__rdl_",
        "alloc::alloc",
        "RtlpLowFragHeap",
        "RtlpHpLfh",
        "malloc",
        "free",
    ];
    if alloc.iter().any(|a| sym.contains(a)) {
        return "memory: allocation and release".into();
    }
    if [
        "memcpy",
        "memset",
        "memmove",
        "RtlZeroMemory",
        "RtlFillMemory",
        "RtlCopyMemory",
    ]
    .iter()
    .any(|a| sym.contains(a))
    {
        return "memory: copy and fill".into();
    }
    let Some((file, _)) = &f.line else {
        return format!("other: {}", trim_symbol(sym));
    };
    let s = short(file);
    let stage = if s.contains("jpeg2000/j2kcore/tier1") || s.contains("jpeg2000/j2kcore/mq.rs") {
        "tier-1 entropy decoding (MQ, coding passes)"
    } else if s.contains("jpeg2000/htj2k") {
        "tier-1 HT block decoding"
    } else if s.contains("jpeg2000/j2kcore/packet.rs") {
        "tier-2 packet headers"
    } else if s.contains("jpeg2000/j2kcore/dwt") {
        "inverse wavelet transform"
    } else if s.contains("jpeg2000/j2kcore/par.rs") {
        "code-block scheduling"
    } else if s.contains("jpeg2000/j2kcore/decode.rs") {
        match f.function.as_str() {
            "inverse_component_transform" => "inverse colour transform",
            "decode_blocks" | "with_decode_threads" | "decode_threads" => "code-block scheduling",
            "scale_coefficient" | "step_size" | "descale_roi" => "dequantization",
            "f32_to_i32" | "sample_range" | "round_to_i32" => "DC shift, clamp and output",
            _ => "tile assembly (decode.rs)",
        }
    } else if s.contains("jpeg2000/j2kcore/codestream.rs") {
        "codestream parsing"
    } else if s.contains("jpeg2000/j2kcore/geometry.rs") {
        "tile and band geometry"
    } else if s.contains("jpeg2000/mod.rs") {
        "DICOM sample packing (jpeg2000/mod.rs)"
    } else if in_crate(f) {
        return format!("other library: {s}");
    } else {
        return format!("other: {}", s);
    };
    stage.into()
}
#[derive(Default)]
struct Sources {
    files: HashMap<String, Option<Vec<String>>>,
}
impl Sources {
    fn enclosing_fn(&mut self, path: &str, line: u32) -> Option<String> {
        let lines = self
            .files
            .entry(path.to_string())
            .or_insert_with(|| {
                std::fs::read_to_string(path)
                    .ok()
                    .map(|t| t.lines().map(str::to_string).collect())
            })
            .as_ref()?;
        let mut i = (line as usize).min(lines.len());
        while i > 0 {
            i -= 1;
            let l = lines[i].trim_start();
            let l = l
                .strip_prefix("pub ")
                .or_else(|| l.strip_prefix("pub(super) "))
                .or_else(|| l.strip_prefix("pub(crate) "))
                .unwrap_or(l);
            if let Some(rest) = l.strip_prefix("fn ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                return Some(name);
            }
        }
        None
    }
}
#[cfg(all(windows, target_arch = "x86_64"))]
mod win {
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[repr(C, align(16))]
    struct Context([u8; 1232]);
    const CONTEXT_FULL: u32 = 0x0010_000B;
    const OFF_FLAGS: usize = 0x30;
    const OFF_RSP: usize = 0x98;
    const OFF_RIP: usize = 0xF8;
    const THREAD_ACCESS: u32 = 0x0002 | 0x0008 | 0x0040;
    impl Context {
        fn get(&self, off: usize) -> u64 {
            u64::from_le_bytes(self.0[off..off + 8].try_into().unwrap_or([0; 8]))
        }
        fn set(&mut self, off: usize, v: u64) {
            self.0[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentProcess() -> Handle;
        fn GetCurrentThreadStackLimits(low: *mut usize, high: *mut usize);
        fn OpenThread(access: u32, inherit: i32, id: u32) -> Handle;
        fn CloseHandle(h: Handle) -> i32;
        fn SuspendThread(h: Handle) -> u32;
        fn ResumeThread(h: Handle) -> u32;
        fn GetThreadContext(h: Handle, ctx: *mut Context) -> i32;
        fn RtlLookupFunctionEntry(
            pc: u64,
            image_base: *mut u64,
            history: *mut c_void,
        ) -> *mut c_void;
        fn RtlVirtualUnwind(
            handler_type: u32,
            image_base: u64,
            pc: u64,
            entry: *mut c_void,
            ctx: *mut Context,
            handler_data: *mut *mut c_void,
            establisher: *mut u64,
            ctx_pointers: *mut c_void,
        ) -> *mut c_void;
    }
    #[repr(C)]
    struct SymbolInfoW {
        size_of_struct: u32,
        type_index: u32,
        reserved: [u64; 2],
        index: u32,
        size: u32,
        mod_base: u64,
        flags: u32,
        value: u64,
        address: u64,
        register: u32,
        scope: u32,
        tag: u32,
        name_len: u32,
        max_name_len: u32,
        name: [u16; 1024],
    }
    #[repr(C)]
    struct LineW64 {
        size_of_struct: u32,
        key: *mut c_void,
        line_number: u32,
        file_name: *const u16,
        address: u64,
    }
    #[link(name = "dbghelp")]
    unsafe extern "system" {
        fn SymSetOptions(options: u32) -> u32;
        fn SymInitializeW(process: Handle, search: *const u16, invade: i32) -> i32;
        fn SymFromAddrW(process: Handle, addr: u64, disp: *mut u64, sym: *mut SymbolInfoW) -> i32;
        fn SymGetLineFromAddrW64(
            process: Handle,
            addr: u64,
            disp: *mut u32,
            line: *mut LineW64,
        ) -> i32;
        fn SymCleanup(process: Handle) -> i32;
        fn SymAddrIncludeInlineTrace(process: Handle, addr: u64) -> u32;
        fn SymQueryInlineTrace(
            process: Handle,
            start: u64,
            start_context: u32,
            start_ret: u64,
            current: u64,
            current_context: *mut u32,
            current_frame: *mut u32,
        ) -> i32;
        fn SymFromInlineContextW(
            process: Handle,
            addr: u64,
            context: u32,
            disp: *mut u64,
            sym: *mut SymbolInfoW,
        ) -> i32;
        fn SymGetLineFromInlineContextW(
            process: Handle,
            addr: u64,
            context: u32,
            module_base: u64,
            disp: *mut u32,
            line: *mut LineW64,
        ) -> i32;
    }
    pub struct ThreadInfo {
        id: u32,
        low: u64,
        high: u64,
    }
    unsafe impl Send for ThreadInfo {}
    pub fn current_thread() -> ThreadInfo {
        let (mut low, mut high) = (0usize, 0usize);
        let id = unsafe {
            GetCurrentThreadStackLimits(&mut low, &mut high);
            GetCurrentThreadId()
        };
        ThreadInfo {
            id,
            low: low as u64,
            high: high as u64,
        }
    }
    pub struct Sampler {
        thread: Handle,
        low: u64,
        high: u64,
    }
    impl Drop for Sampler {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.thread);
            }
        }
    }
    impl Sampler {
        pub fn open(t: ThreadInfo) -> Option<Sampler> {
            let thread = unsafe { OpenThread(THREAD_ACCESS, 0, t.id) };
            (!thread.is_null()).then_some(Sampler {
                thread,
                low: t.low,
                high: t.high,
            })
        }
        pub fn sample(&self, out: &mut [u64]) -> usize {
            let mut ctx = Context([0; 1232]);
            ctx.set(OFF_FLAGS, u64::from(CONTEXT_FULL));
            unsafe {
                if SuspendThread(self.thread) == u32::MAX {
                    return 0;
                }
                let n = if GetThreadContext(self.thread, &mut ctx) != 0 {
                    self.walk(&mut ctx, out)
                } else {
                    0
                };
                ResumeThread(self.thread);
                n
            }
        }
        unsafe fn walk(&self, ctx: &mut Context, out: &mut [u64]) -> usize {
            let mut n = 0;
            while n < out.len() {
                let (rip, rsp) = (ctx.get(OFF_RIP), ctx.get(OFF_RSP));
                if rip == 0 || rsp < self.low || rsp + 8 > self.high {
                    break;
                }
                out[n] = rip;
                n += 1;
                let mut base = 0u64;
                let entry = unsafe { RtlLookupFunctionEntry(rip, &mut base, std::ptr::null_mut()) };
                if entry.is_null() {
                    let ret = unsafe { std::ptr::read(rsp as *const u64) };
                    ctx.set(OFF_RIP, ret);
                    ctx.set(OFF_RSP, rsp + 8);
                } else {
                    let mut data = std::ptr::null_mut();
                    let mut frame = 0u64;
                    unsafe {
                        RtlVirtualUnwind(
                            0,
                            base,
                            rip,
                            entry,
                            ctx,
                            &mut data,
                            &mut frame,
                            std::ptr::null_mut(),
                        );
                    }
                }
                if ctx.get(OFF_RSP) <= rsp && ctx.get(OFF_RIP) == rip {
                    break;
                }
            }
            n
        }
    }
    pub struct Symbols {
        process: Handle,
    }
    impl Drop for Symbols {
        fn drop(&mut self) {
            unsafe {
                SymCleanup(self.process);
            }
        }
    }
    fn wide(p: *const u16, max: usize) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut n = 0;
        while n < max && unsafe { *p.add(n) } != 0 {
            n += 1;
        }
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) })
    }
    impl Symbols {
        pub fn new() -> Option<Symbols> {
            unsafe {
                SymSetOptions(0x2 | 0x4 | 0x10);
                let process = GetCurrentProcess();
                let dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                let search: Vec<u16> = dir.encode_utf16().chain(std::iter::once(0)).collect();
                (SymInitializeW(process, search.as_ptr(), 1) != 0).then_some(Symbols { process })
            }
        }
        fn lookup(&self, addr: u64, context: Option<u32>) -> (String, Option<(String, u32)>) {
            let mut sym: Box<SymbolInfoW> = Box::new(SymbolInfoW {
                size_of_struct: 88,
                type_index: 0,
                reserved: [0; 2],
                index: 0,
                size: 0,
                mod_base: 0,
                flags: 0,
                value: 0,
                address: 0,
                register: 0,
                scope: 0,
                tag: 0,
                name_len: 0,
                max_name_len: 1024,
                name: [0; 1024],
            });
            let mut disp = 0u64;
            let found = unsafe {
                match context {
                    Some(c) => SymFromInlineContextW(self.process, addr, c, &mut disp, &mut *sym),
                    None => SymFromAddrW(self.process, addr, &mut disp, &mut *sym),
                }
            };
            let name = if found != 0 {
                let len = (sym.name_len as usize).min(1024);
                String::from_utf16_lossy(&sym.name[..len])
            } else {
                format!("0x{addr:x}")
            };
            let mut line = LineW64 {
                size_of_struct: std::mem::size_of::<LineW64>() as u32,
                key: std::ptr::null_mut(),
                line_number: 0,
                file_name: std::ptr::null(),
                address: 0,
            };
            let mut ldisp = 0u32;
            let ok = unsafe {
                match context {
                    Some(c) => SymGetLineFromInlineContextW(
                        self.process,
                        addr,
                        c,
                        0,
                        &mut ldisp,
                        &mut line,
                    ),
                    None => SymGetLineFromAddrW64(self.process, addr, &mut ldisp, &mut line),
                }
            };
            let place = (ok != 0).then(|| (wide(line.file_name, 4096), line.line_number));
            (name, place)
        }
        pub fn resolve(&self, addr: u64) -> Vec<(String, Option<(String, u32)>)> {
            let mut out = Vec::new();
            let inline = unsafe { SymAddrIncludeInlineTrace(self.process, addr) };
            if inline > 0 {
                let (mut ctx, mut index) = (0u32, 0u32);
                if unsafe {
                    SymQueryInlineTrace(self.process, addr, 0, addr, addr, &mut ctx, &mut index)
                } != 0
                {
                    for k in 0..inline {
                        out.push(self.lookup(addr, Some(ctx + k)));
                    }
                }
            }
            out.push(self.lookup(addr, None));
            out
        }
    }
}
