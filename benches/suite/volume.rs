use crate::harness::{Runner, Work};
use crate::inputs::{self, Inputs, par_map};
use dicom_rust::object::{File, ReadFileOptions, Tag, read_file_with_options};
use dicom_rust::pixel::pixeldata;
use dicom_rust::render::frame::Frame;
use dicom_rust::render::volume::{Stack, Volume, build_volume};
use dicom_rust::render::{Rescale, WindowLevel};
use std::sync::Arc;
pub const NAMES: &[&str] = &[
    "volume/stack_from_files/ct512x300",
    "volume/build_with_voxels/ct512x300",
    "volume/series_open/ct512x300",
];
fn floats(f: &File, g: u16, e: u16) -> Vec<f64> {
    f.get_floats(Tag::new(g, e)).unwrap_or_default()
}
fn float(f: &File, g: u16, e: u16, default: f64) -> f64 {
    floats(f, g, e).first().copied().unwrap_or(default)
}
pub fn frame_of(f: &File) -> Frame {
    let native = pixeldata::extract_native_frames(&f.dataset).expect("native frames");
    let position = floats(f, 0x0020, 0x0032);
    let window = WindowLevel::new(
        float(f, 0x0028, 0x1050, 40.0),
        float(f, 0x0028, 0x1051, 400.0),
    );
    let z = position.get(2).copied().unwrap_or(0.0);
    Frame {
        sop_instance_uid: f.get_uid(Tag::new(0x0008, 0x0018)).unwrap_or_default(),
        frame_index: 0,
        metadata: native.metadata,
        transfer_syntax_uid: f.transfer_syntax.uid.to_string(),
        pixel_bytes: Arc::new(native.data.into_iter().next().unwrap_or_default()),
        default_window: window,
        rescale: Rescale {
            slope: float(f, 0x0028, 0x1053, 1.0),
            intercept: float(f, 0x0028, 0x1052, 0.0),
        },
        image_position: position,
        image_orientation: floats(f, 0x0020, 0x0037),
        pixel_spacing: floats(f, 0x0028, 0x0030),
        slice_thickness: float(f, 0x0018, 0x0050, 0.0),
        slice_location: z,
        slice_location_ok: true,
        instance_number: f.get_int(Tag::new(0x0020, 0x0013)).unwrap_or(0),
        sort: z,
        ..Frame::default()
    }
}
pub fn stack_of(frames: Vec<Frame>) -> Stack {
    let first = frames.first().cloned().unwrap_or_default();
    let mut s = Stack::new(frames.into_iter().map(|f| Some(Arc::new(f))).collect());
    s.pixel_spacing = first.pixel_spacing.clone();
    s.slice_thickness = first.slice_thickness;
    s.default_window = first.default_window.clone();
    s.modality = "CT".into();
    s
}
fn read(bytes: &[u8]) -> File {
    read_file_with_options(bytes, ReadFileOptions::default())
        .unwrap_or_else(|(_, e)| panic!("read: {e}"))
}
pub fn ct_stack(inp: &Inputs) -> Stack {
    let files = inp.ct_files();
    stack_of(par_map(files.len(), |i| frame_of(&read(&files[i]))))
}
pub fn volume_with_voxels(stack: &Stack) -> Arc<Volume> {
    let v = build_volume(stack).expect("build volume");
    v.hu_range().expect("voxels");
    v
}
pub fn run(r: &mut Runner, inp: &Inputs) {
    if !r.any_selected(NAMES) {
        return;
    }
    let files = inp.ct_files();
    let slices = Work::Items(files.len() as u64);
    let parsed: Vec<File> = files.iter().map(|b| read(b)).collect();
    r.bench("volume/stack_from_files/ct512x300", slices, || {
        stack_of(parsed.iter().map(frame_of).collect())
    });
    let stack = ct_stack(inp);
    let voxels = Work::Pixels(512 * 512 * inputs::CT_SLICES as u64);
    r.bench("volume/build_with_voxels/ct512x300", voxels, || {
        volume_with_voxels(&stack)
    });
    r.bench("volume/series_open/ct512x300", voxels, || {
        let frames: Vec<Frame> = files.iter().map(|b| frame_of(&read(b))).collect();
        volume_with_voxels(&stack_of(frames))
    });
}
pub fn check(inp: &Inputs) -> Result<(), String> {
    let stack = ct_stack(inp);
    let v = build_volume(&stack).map_err(|e| e.to_string())?;
    if (v.cols, v.rows, v.depth) != (512, 512, inputs::CT_SLICES as i64) {
        return Err(format!("volume is {}x{}x{}", v.cols, v.rows, v.depth));
    }
    let (lo, hi) = v.hu_range().ok_or("no voxel range")?;
    if lo > -1000.0 || hi < 1000.0 {
        return Err(format!("voxel range {lo}..{hi} is not the phantom's"));
    }
    if (v.slice_spacing - 1.0).abs() > 1e-6 {
        return Err(format!("slice spacing {}", v.slice_spacing));
    }
    Ok(())
}
