#![forbid(unsafe_code)]
use dicom_rust::pixel::Metadata;
use dicom_rust::render::frame::Frame;
use dicom_rust::render::parallel::set_budget;
use dicom_rust::render::volume::{Stack, Volume, build_volume};
use dicom_rust::render::volume_link::{RetainedVolumeBandReader, VolumeDecodeProgress};
use dicom_rust::render::{Rescale, WindowLevel};
use std::sync::Arc;
use std::sync::mpsc;
const COLS: usize = 32;
const ROWS: usize = 32;
const DEPTH: usize = 66;
fn stored(x: usize, y: usize, z: usize) -> i16 {
    ((x * 7 + y * 13 + z * 29) % 900) as i16 - 200
}
fn volume() -> Arc<Volume> {
    let window = WindowLevel::new(40.0, 400.0);
    let mut stack = Stack::new(Vec::new());
    stack.uid = "1.2.826.0.1.3680043.10.1561.41".into();
    stack.default_window = window.clone();
    stack.pixel_spacing = vec![0.8, 0.8];
    stack.slice_thickness = 1.5;
    for z in 0..DEPTH {
        let mut pixels = vec![0u8; COLS * ROWS * 2];
        for y in 0..ROWS {
            for x in 0..COLS {
                let o = (y * COLS + x) * 2;
                pixels[o..o + 2].copy_from_slice(&stored(x, y, z).to_le_bytes());
            }
        }
        stack.frames.push(Some(Arc::new(Frame {
            sop_instance_uid: format!("1.2.826.0.1.3680043.10.1561.41.{}", z + 1),
            metadata: Metadata {
                rows: ROWS as u16,
                columns: COLS as u16,
                samples_per_pixel: 1,
                bits_allocated: 16,
                bits_stored: 16,
                high_bit: 15,
                pixel_representation: 1,
                photometric_interpretation: "MONOCHROME2".into(),
                number_of_frames: 1,
                ..Default::default()
            },
            pixel_bytes: Arc::new(pixels),
            default_window: window.clone(),
            rescale: Rescale {
                slope: 1.0,
                intercept: -1024.0,
            },
            image_position: vec![-10.0, -12.0, 5.0 + z as f64 * 1.5],
            image_orientation: vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            pixel_spacing: vec![0.8, 0.8],
            slice_thickness: 1.5,
            instance_number: z as i64 + 1,
            ..Default::default()
        })));
    }
    build_volume(&stack).unwrap_or_else(|e| panic!("build_volume: {e}"))
}
struct Band {
    reader: RetainedVolumeBandReader,
    slices: Vec<usize>,
}
fn holds_its_slices(band: &Band) -> bool {
    let mut held = Vec::new();
    band.reader.append_native_le(&mut held);
    let mut expected = Vec::with_capacity(held.len());
    for &z in &band.slices {
        for y in 0..ROWS {
            for x in 0..COLS {
                expected.extend_from_slice(&stored(x, y, z).to_le_bytes());
            }
        }
    }
    held == expected
}
fn decode_and_read(
    step: usize,
    prepare: impl FnOnce(&Arc<Volume>, &mut dyn FnMut(&VolumeDecodeProgress<'_>)),
) {
    set_budget(2);
    let v = volume();
    let (tx, rx) = mpsc::channel::<Band>();
    let bands = std::thread::scope(|scope| {
        let reader = scope.spawn(move || {
            let mut bands = Vec::new();
            for band in rx {
                assert!(holds_its_slices(&band), "band of {:?}", band.slices);
                bands.push(band);
            }
            bands
        });
        let mut observe = |p: &VolumeDecodeProgress<'_>| {
            let (start, end) = (p.band_start as usize, p.band_end as usize);
            if let Some(band) = &p.band {
                let _ = tx.send(Band {
                    reader: band.retain().unwrap_or_else(|e| panic!("retain: {e}")),
                    slices: (start..end).collect(),
                });
            }
            if let Some(band) = p.even_slices.as_ref().filter(|_| end > start) {
                let _ = tx.send(Band {
                    reader: band.retain().unwrap_or_else(|e| panic!("retain: {e}")),
                    slices: (start..end).step_by(step).collect(),
                });
            }
        };
        prepare(&v, &mut observe);
        drop(tx);
        reader.join().unwrap_or_else(|_| panic!("reader thread"))
    });
    let mut covered = [false; DEPTH];
    for band in &bands {
        assert!(holds_its_slices(band), "band of {:?}", band.slices);
        for &z in &band.slices {
            covered[z] = true;
        }
    }
    assert!(covered.iter().all(|&c| c), "slices without a band");
    v.close();
}
#[test]
fn a_crew_decodes_in_order_while_bands_are_read() {
    decode_and_read(1, |v, observe| {
        let (_, err) = v.prepare_snapshot_bands_in_order_context(None, observe);
        assert!(err.is_none(), "{err:?}");
    });
}
#[test]
fn a_crew_decodes_the_even_slices_first_while_bands_are_read() {
    decode_and_read(2, |v, observe| {
        let (_, err) = v.prepare_snapshot_streaming_evens_context(None, observe);
        assert!(err.is_none(), "{err:?}");
    });
}
