use crate::part10::{self, CtPlace};
use crate::synth::{self, Gray16, Rgb8};
use std::cell::OnceCell;
use std::path::PathBuf;
pub const CT_SLICES: usize = 300;
pub const ENHANCED_FRAMES: usize = 200;
#[derive(Default)]
pub struct Inputs {
    ct: OnceCell<Vec<Gray16>>,
    ct_files: OnceCell<Vec<Vec<u8>>>,
    mr: OnceCell<Gray16>,
    dx: OnceCell<Gray16>,
    us: OnceCell<Rgb8>,
    enhanced: OnceCell<Vec<u8>>,
    rtstruct: OnceCell<Vec<u8>>,
}
pub fn par_map<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism()
        .map_or(4, |t| t.get())
        .min(n.max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<Option<T>> = (0..n).map(|_| None).collect();
    let parts: Vec<Vec<(usize, T)>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if i >= n {
                            return mine;
                        }
                        mine.push((i, f(i)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect()
    });
    for (i, v) in parts.into_iter().flatten() {
        out[i] = Some(v);
    }
    out.into_iter().flatten().collect()
}
pub fn cache_dir() -> PathBuf {
    std::env::var_os("DICOM_RUST_BENCH_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dicom-rust-bench"))
}
pub fn ct_place(z: usize) -> CtPlace {
    CtPlace {
        patient: 1,
        study: 1,
        series: 2,
        instance: z as u64,
    }
}
pub fn ct_z_mm(z: usize) -> f64 {
    -150.0 + z as f64
}
impl Inputs {
    pub fn ct(&self) -> &[Gray16] {
        self.ct
            .get_or_init(|| par_map(CT_SLICES, |z| synth::ct_slice(z, CT_SLICES)))
    }
    pub fn ct_slice(&self) -> &Gray16 {
        &self.ct()[CT_SLICES / 2]
    }
    pub fn ct_files(&self) -> &[Vec<u8>] {
        self.ct_files.get_or_init(|| {
            let ct = self.ct();
            par_map(CT_SLICES, |z| {
                part10::ct_native_file(ct_place(z), 512, 512, ct_z_mm(z), &ct[z].le_bytes())
            })
        })
    }
    pub fn mr(&self) -> &Gray16 {
        self.mr.get_or_init(|| synth::mr_slice(10))
    }
    pub fn dx(&self) -> &Gray16 {
        self.dx.get_or_init(synth::dx_image)
    }
    pub fn us(&self) -> &Rgb8 {
        self.us.get_or_init(synth::us_rgb)
    }
    pub fn enhanced(&self) -> &[u8] {
        self.enhanced
            .get_or_init(|| part10::enhanced_ct_file(ENHANCED_FRAMES, &self.ct_slice().le_bytes()))
    }
    pub fn rtstruct(&self) -> &[u8] {
        self.rtstruct
            .get_or_init(|| part10::rtstruct_file(40, 40, 64))
    }
}
