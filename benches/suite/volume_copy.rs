use crate::harness::{Runner, Work};
use crate::inputs::{Inputs, par_map};
use crate::volume::{ct_stack, stack_of};
use dicom_rust::render::frame::Frame;
use dicom_rust::render::volume::{Stack, Volume, build_volume};
use dicom_rust::render::volume_link::{RetainedVolumeBandReader, VolumeDecodeProgress};
use std::sync::Arc;
const SLICES: usize = 250;
const PLANE: usize = 512 * 512;
const BYTES: usize = SLICES * PLANE * 2;
pub const NAMES: &[&str] = &[
    "volume/reference_copy/ct512x250",
    "volume/reference_copy_all_cores/ct512x250",
    "volume/reference_copy_warm/ct512x250",
    "volume/build_stored/ct512x250",
    "volume/progressive_build/ct512x250",
    "volume/progressive_to_consumer/ct512x250",
    "volume/store_copy_payload/ct512x250",
    "volume/store_write_stored16/ct512x250",
    "volume/store_borrow_to_consumer/ct512x250",
];
fn stack(inp: &Inputs) -> Stack {
    let all = ct_stack(inp);
    let frames: Vec<Frame> = all
        .frames
        .iter()
        .take(SLICES)
        .flatten()
        .map(|f| (**f).clone())
        .collect();
    stack_of(frames)
}
fn volume(stack: &Stack) -> Arc<Volume> {
    build_volume(stack).expect("build volume")
}
fn build_stored(stack: &Stack) -> Arc<Volume> {
    let v = volume(stack);
    let (_, err) = v.prepare_snapshot_context(None);
    assert!(err.is_none(), "prepare: {err:?}");
    v
}
fn progressive(stack: &Stack, mut band: impl FnMut(i64, RetainedVolumeBandReader)) -> Arc<Volume> {
    let v = volume(stack);
    let mut observe = |p: &VolumeDecodeProgress<'_>| {
        if let Some(reader) = &p.band {
            band(p.band_start, reader.retain().expect("retain"));
        }
    };
    let (_, err) = v.prepare_snapshot_bands_in_order_context(None, &mut observe);
    assert!(err.is_none(), "prepare: {err:?}");
    v
}
fn band_to_consumer(first: i64, band: &RetainedVolumeBandReader, consumer: &mut [u8]) {
    let at = first as usize * PLANE * 2;
    match band.native_le_bytes() {
        Some(bytes) => consumer[at..at + bytes.len()].copy_from_slice(bytes),
        None => {
            let mut bytes = Vec::new();
            band.append_native_le(&mut bytes);
            consumer[at..at + bytes.len()].copy_from_slice(&bytes);
        }
    }
}
fn series_bytes(stack: &Stack) -> Vec<u8> {
    let mut out = Vec::with_capacity(BYTES);
    for frame in stack.frames.iter().flatten() {
        out.extend_from_slice(&frame.pixel_bytes);
    }
    out
}
pub fn run(r: &mut Runner, inp: &Inputs) {
    if !r.any_selected(NAMES) {
        return;
    }
    let stack = stack(inp);
    let voxels = Work::Pixels((SLICES * PLANE) as u64);
    let source = series_bytes(&stack);
    r.bench("volume/reference_copy/ct512x250", voxels, || {
        let mut out = vec![0u8; source.len()];
        out.copy_from_slice(&source);
        out
    });
    r.bench("volume/reference_copy_all_cores/ct512x250", voxels, || {
        let mut out = vec![0u8; source.len()];
        let planes: Vec<std::sync::Mutex<&mut [u8]>> = out
            .chunks_mut(PLANE * 2)
            .map(std::sync::Mutex::new)
            .collect();
        par_map(planes.len(), |z| {
            if let Ok(mut plane) = planes[z].lock() {
                plane.copy_from_slice(&source[z * PLANE * 2..(z + 1) * PLANE * 2]);
            }
        });
        drop(planes);
        out
    });
    let mut consumer = vec![0u8; BYTES];
    r.bench("volume/reference_copy_warm/ct512x250", voxels, || {
        consumer.copy_from_slice(&source);
    });
    r.bench("volume/build_stored/ct512x250", voxels, || {
        build_stored(&stack)
    });
    r.bench("volume/progressive_build/ct512x250", voxels, || {
        let mut kept = Vec::new();
        let v = progressive(&stack, |_, band| kept.push(band));
        (v, kept)
    });
    r.bench("volume/progressive_to_consumer/ct512x250", voxels, || {
        let mut kept = Vec::new();
        let v = progressive(&stack, |first, band| {
            band_to_consumer(first, &band, &mut consumer);
            kept.push(band);
        });
        (v, kept)
    });
    let built = build_stored(&stack);
    let lease = built
        .acquire_store_snapshot_context(None)
        .expect("store lease");
    let snapshot = lease.snapshot().expect("snapshot");
    r.bench("volume/store_copy_payload/ct512x250", voxels, || {
        snapshot.copy_payload().expect("payload")
    });
    r.bench("volume/store_write_stored16/ct512x250", voxels, || {
        let mut out = Vec::with_capacity(BYTES);
        snapshot
            .write_stored16_context(None, &mut out)
            .expect("stored rows");
        out
    });
    r.bench("volume/store_borrow_to_consumer/ct512x250", voxels, || {
        store_to_consumer(&snapshot, &mut consumer)
    });
    lease.release();
}
fn store_to_consumer(
    snapshot: &dicom_rust::render::volume_store::VolumeSnapshot,
    consumer: &mut [u8],
) -> usize {
    match snapshot.payload_bytes().expect("payload") {
        Some(bytes) => {
            consumer[..bytes.len()].copy_from_slice(&bytes);
            bytes.len()
        }
        None => {
            let bytes = snapshot.copy_payload().expect("payload");
            consumer[..bytes.len()].copy_from_slice(&bytes);
            bytes.len()
        }
    }
}
pub fn check(inp: &Inputs) -> Result<(), String> {
    let stack = stack(inp);
    let source = series_bytes(&stack);
    if source.len() != BYTES {
        return Err(format!("the series holds {} bytes", source.len()));
    }
    let mut consumer = vec![0u8; BYTES];
    let mut slices = 0;
    let v = progressive(&stack, |first, band| {
        slices += band.slice_count();
        band_to_consumer(first, &band, &mut consumer);
    });
    if slices != SLICES {
        return Err(format!("the bands hold {slices} slices"));
    }
    if consumer != source {
        return Err("the bands are not the series' samples".into());
    }
    let lease = v
        .acquire_store_snapshot_context(None)
        .map_err(|e| e.to_string())?;
    let snapshot = lease.snapshot().map_err(|e| e.to_string())?;
    let payload = snapshot.copy_payload().map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    snapshot
        .write_stored16_context(None, &mut rows)
        .map_err(|e| e.to_string())?;
    consumer.fill(0);
    store_to_consumer(&snapshot, &mut consumer);
    lease.release();
    if payload != source || rows != source || consumer != source {
        return Err("the store's generation is not the series' samples".into());
    }
    Ok(())
}
