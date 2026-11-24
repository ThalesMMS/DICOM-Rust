use crate::harness::{Runner, Work};
use crate::inputs::{self, Inputs};
use dicom_rust::object::{
    self, File, ReadFileOptions, open_file_header_with_options, open_file_with_options,
    read_file_with_options, writer,
};
fn read(bytes: &[u8], opts: ReadFileOptions) -> File {
    match read_file_with_options(bytes, opts) {
        Ok(f) => f,
        Err((_, e)) => panic!("bench input does not read: {e}"),
    }
}
fn large() -> ReadFileOptions {
    ReadFileOptions {
        max_pixel_data_bytes: 1 << 31,
        max_total_bytes: 1 << 31,
        max_element_bytes: 1 << 31,
        ..ReadFileOptions::default()
    }
}
pub const NAMES: &[&str] = &[
    "file/read/ct_slice_mem",
    "file/read/ct_slice_header_mem",
    "file/read/ct_slice_disk",
    "file/read/ct_slice_header_disk",
    "file/read/enhanced_ct_200f_mem",
    "file/read/rtstruct_mem",
    "file/write/ct_slice",
    "file/write/enhanced_ct_200f",
    "file/write/rtstruct",
];
pub fn run(r: &mut Runner, inp: &Inputs) {
    if !r.any_selected(NAMES) {
        return;
    }
    let slice = &inp.ct_files()[inputs::CT_SLICES / 2];
    let n = slice.len() as u64;
    r.bench("file/read/ct_slice_mem", Work::Bytes(n), || {
        read(slice, ReadFileOptions::default())
    });
    let header = ReadFileOptions {
        skip_pixel_data: true,
        ..ReadFileOptions::default()
    };
    r.bench("file/read/ct_slice_header_mem", Work::Bytes(n), || {
        read(slice, header)
    });
    let dir = inputs::cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("ct_slice.dcm");
    if std::fs::read(&path).ok().as_deref() != Some(slice.as_slice()) {
        std::fs::write(&path, slice).expect("write the cache file");
    }
    let path = path.to_string_lossy().into_owned();
    r.bench("file/read/ct_slice_disk", Work::Bytes(n), || {
        open_file_with_options(&path, ReadFileOptions::default()).map_err(|(_, e)| e.to_string())
    });
    r.bench("file/read/ct_slice_header_disk", Work::Bytes(n), || {
        open_file_header_with_options(&path, header).map_err(|(_, e)| e.to_string())
    });
    let enhanced = inp.enhanced();
    r.bench(
        "file/read/enhanced_ct_200f_mem",
        Work::Bytes(enhanced.len() as u64),
        || read(enhanced, large()),
    );
    let rt = inp.rtstruct();
    r.bench(
        "file/read/rtstruct_mem",
        Work::Bytes(rt.len() as u64),
        || read(rt, ReadFileOptions::default()),
    );
    let f = read(slice, ReadFileOptions::default());
    r.bench("file/write/ct_slice", Work::Bytes(n), || {
        writer::write_file(&f)
    });
    let f = read(enhanced, large());
    r.bench(
        "file/write/enhanced_ct_200f",
        Work::Bytes(enhanced.len() as u64),
        || writer::write_file(&f),
    );
    let f = read(rt, ReadFileOptions::default());
    r.bench("file/write/rtstruct", Work::Bytes(rt.len() as u64), || {
        writer::write_file(&f)
    });
}
pub fn check(inp: &Inputs) -> Result<(), String> {
    for (name, bytes, opts) in [
        (
            "ct slice",
            &inp.ct_files()[0][..],
            ReadFileOptions::default(),
        ),
        ("enhanced", inp.enhanced(), large()),
        ("rtstruct", inp.rtstruct(), ReadFileOptions::default()),
    ] {
        let f = read_file_with_options(bytes, opts).map_err(|(_, e)| format!("{name}: {e}"))?;
        let out = writer::write_file(&f).map_err(|e| format!("{name}: write: {e}"))?;
        let back =
            read_file_with_options(&out, opts).map_err(|(_, e)| format!("{name}: re-read: {e}"))?;
        if object::data_set(&back).elements.len() != object::data_set(&f).elements.len() {
            return Err(format!("{name}: the written file reads back differently"));
        }
    }
    Ok(())
}
