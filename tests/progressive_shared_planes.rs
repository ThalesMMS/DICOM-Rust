#![forbid(unsafe_code)]
use dicom_rust::pixel::Metadata;
use dicom_rust::render::frame::Frame;
use dicom_rust::render::vec3::Vec3;
use dicom_rust::render::volume::{Stack, Volume, build_volume};
use dicom_rust::render::volume_link::{
    RetainedVolumeBandReader, VolumeDecodePhase, VolumeDecodeProgress,
};
use dicom_rust::render::{Rescale, WindowLevel};
use std::sync::Arc;
use std::sync::mpsc;
const SIZE: (usize, usize, usize) = if cfg!(miri) { (6, 4, 7) } else { (64, 48, 24) };
fn stored(x: usize, y: usize, z: usize) -> i16 {
    ((x * 7 + y * 13 + z * 29) % 900) as i16 - 200
}
fn volume() -> Arc<Volume> {
    let (cols, rows, depth) = SIZE;
    let window = WindowLevel::new(40.0, 400.0);
    let mut stack = Stack::new(Vec::new());
    stack.uid = "1.2.826.0.1.3680043.10.1561.21".into();
    stack.default_window = window.clone();
    stack.pixel_spacing = vec![0.8, 0.8];
    stack.slice_thickness = 1.5;
    for z in 0..depth {
        let mut pixels = vec![0u8; cols * rows * 2];
        for y in 0..rows {
            for x in 0..cols {
                let o = (y * cols + x) * 2;
                pixels[o..o + 2].copy_from_slice(&stored(x, y, z).to_le_bytes());
            }
        }
        let position = Vec3::new(-10.0, -12.0, 5.0 + z as f64 * 1.5);
        stack.frames.push(Some(Arc::new(Frame {
            sop_instance_uid: format!("1.2.826.0.1.3680043.10.1561.21.{}", z + 1),
            metadata: Metadata {
                rows: rows as u16,
                columns: cols as u16,
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
            image_position: vec![position.x, position.y, position.z],
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
fn modality_bytes(band: &Band) -> Vec<u8> {
    let mut out = Vec::new();
    band.reader
        .write_modality_f32_to(Some(&mut out))
        .unwrap_or_else(|e| panic!("write_modality_f32_to: {e}"));
    let mut native = Vec::new();
    band.reader.append_native_le(&mut native);
    if let Some(view) = band.reader.native_le_bytes() {
        assert_eq!(view, &native[..]);
    }
    let count = band.reader.slice_count();
    assert_eq!(count, band.slices.len());
    let per_slice = native.len() / count.max(1);
    for ordinal in 0..count {
        let slice = band.reader.slice_native_le_bytes(ordinal);
        let expected = cfg!(target_endian = "little")
            .then(|| &native[ordinal * per_slice..(ordinal + 1) * per_slice]);
        assert_eq!(slice, expected, "slice {ordinal}");
    }
    out
}
fn expected_bytes(slices: &[usize]) -> Vec<u8> {
    let (cols, rows, _) = SIZE;
    let mut out = Vec::new();
    for &z in slices {
        for y in 0..rows {
            for x in 0..cols {
                let modality = f32::from(stored(x, y, z)) - 1024.0;
                out.extend_from_slice(&modality.to_le_bytes());
            }
        }
    }
    out
}
fn band_slices(p: &VolumeDecodeProgress<'_>) -> Vec<usize> {
    let (start, end) = (p.band_start as usize, p.band_end as usize);
    if p.phase == VolumeDecodePhase::EVEN_SLICES {
        (0..p.total_slices as usize).step_by(2).collect()
    } else if p.phase == VolumeDecodePhase::EVEN_BAND {
        (start..end).step_by(2).collect()
    } else {
        (start..end).collect()
    }
}
fn decode_and_read(
    prepare: impl FnOnce(&Arc<Volume>, &mut dyn FnMut(&VolumeDecodeProgress<'_>)),
) -> Vec<Band> {
    let v = volume();
    let (tx, rx) = mpsc::channel::<Band>();
    let bands = std::thread::scope(|scope| {
        let reader = scope.spawn(move || {
            let mut bands = Vec::new();
            for band in rx {
                assert_eq!(modality_bytes(&band), expected_bytes(&band.slices));
                for earlier in &bands {
                    let earlier: &Band = earlier;
                    assert!(earlier.reader.modality_range().is_some());
                }
                bands.push(band);
            }
            bands
        });
        let mut observe = |p: &VolumeDecodeProgress<'_>| {
            for band in [p.band.as_ref(), p.even_slices.as_ref()]
                .into_iter()
                .flatten()
            {
                let reader = band.retain().unwrap_or_else(|e| panic!("retain: {e}"));
                let _ = tx.send(Band {
                    reader,
                    slices: band_slices(p),
                });
            }
        };
        prepare(&v, &mut observe);
        drop(tx);
        reader.join().unwrap_or_else(|_| panic!("reader thread"))
    });
    v.close();
    bands
}
fn check(bands: &[Band], depth: usize) {
    assert!(!bands.is_empty(), "no band was reported");
    let mut covered = vec![false; depth];
    for band in bands {
        assert_eq!(modality_bytes(band), expected_bytes(&band.slices));
        for &z in &band.slices {
            covered[z] = true;
        }
    }
    assert!(
        covered.iter().all(|&c| c),
        "slices without a band: {covered:?}"
    );
}
#[test]
fn bands_in_order_are_read_while_later_slices_decode() {
    let bands = decode_and_read(|v, observe| {
        let (_, err) = v.prepare_snapshot_bands_in_order_context(None, observe);
        assert!(err.is_none(), "{err:?}");
    });
    check(&bands, SIZE.2);
}
#[test]
fn even_bands_are_read_while_the_odd_slices_decode() {
    let bands = decode_and_read(|v, observe| {
        let (_, err) = v.prepare_snapshot_streaming_evens_context(None, observe);
        assert!(err.is_none(), "{err:?}");
    });
    check(&bands, SIZE.2);
    assert!(
        bands
            .iter()
            .any(|b| b.slices.len() > 1 && b.slices[1] == b.slices[0] + 2)
    );
}
