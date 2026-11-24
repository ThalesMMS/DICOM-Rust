#![forbid(unsafe_code)]
use dicom_rust::pixel::Metadata;
use dicom_rust::render::frame::Frame;
use dicom_rust::render::volume::{Stack, Volume, build_volume};
use dicom_rust::render::volume_link::{RetainedVolumeBandReader, VolumeDecodeProgress};
use dicom_rust::render::volume_store::{VolumeLease, VolumeScalarFormat, VolumeSnapshot};
use dicom_rust::render::{Rescale, WindowLevel};
use std::io::Write;
use std::sync::Arc;
const COLS: usize = if cfg!(miri) { 6 } else { 96 };
const ROWS: usize = if cfg!(miri) { 4 } else { 80 };
const DEPTH: usize = if cfg!(miri) { 9 } else { 21 };
fn stored(x: usize, y: usize, z: usize) -> u16 {
    ((x * 7 + y * 13 + z * 29) % 3000) as u16
}
fn volume(signed: bool, rescale_per_slice: bool) -> Arc<Volume> {
    let window = WindowLevel::new(40.0, 400.0);
    let mut stack = Stack::new(Vec::new());
    stack.uid = "1.2.826.0.1.3680043.10.1561.31".into();
    stack.default_window = window.clone();
    stack.pixel_spacing = vec![0.8, 0.8];
    stack.slice_thickness = 1.5;
    for z in 0..DEPTH {
        let mut pixels = vec![0u8; COLS * ROWS * 2];
        for y in 0..ROWS {
            for x in 0..COLS {
                let o = (y * COLS + x) * 2;
                let value = stored(x, y, z);
                let bytes = if signed {
                    (value as i16 - 1500).to_le_bytes()
                } else {
                    value.to_le_bytes()
                };
                pixels[o..o + 2].copy_from_slice(&bytes);
            }
        }
        stack.frames.push(Some(Arc::new(Frame {
            sop_instance_uid: format!("1.2.826.0.1.3680043.10.1561.31.{}", z + 1),
            metadata: Metadata {
                rows: ROWS as u16,
                columns: COLS as u16,
                samples_per_pixel: 1,
                bits_allocated: 16,
                bits_stored: 16,
                high_bit: 15,
                pixel_representation: u16::from(signed),
                photometric_interpretation: "MONOCHROME2".into(),
                number_of_frames: 1,
                ..Default::default()
            },
            pixel_bytes: Arc::new(pixels),
            default_window: window.clone(),
            rescale: Rescale {
                slope: 1.0,
                intercept: if rescale_per_slice {
                    -1024.0 + z as f64
                } else {
                    -1024.0
                },
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
#[derive(Default)]
struct Counted {
    bytes: Vec<u8>,
    writes: Vec<usize>,
}
impl Write for Counted {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(buf);
        self.writes.push(buf.len());
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn snapshot_of(v: &Arc<Volume>) -> (VolumeLease, VolumeSnapshot) {
    let lease = v
        .acquire_store_snapshot_context(None)
        .unwrap_or_else(|e| panic!("lease: {e}"));
    let snapshot = lease.snapshot().unwrap_or_else(|e| panic!("snapshot: {e}"));
    (lease, snapshot)
}
fn check_generation(v: &Arc<Volume>, expected_format: VolumeScalarFormat, expected: &[u8]) {
    let (lease, snapshot) = snapshot_of(v);
    let descriptor = snapshot.descriptor().expect("descriptor");
    assert_eq!(descriptor.scalar_format, expected_format);
    let copied = snapshot.copy_payload().expect("copy_payload");
    assert_eq!(copied, expected);
    let mut written = Counted::default();
    snapshot
        .write_payload_to(&mut written)
        .expect("write_payload_to");
    assert_eq!(written.bytes, expected);
    if expected_format == VolumeScalarFormat::F32_MODALITY_LE {
        assert_eq!(written.writes.len(), expected.len() / 4);
        assert!(written.writes.iter().all(|n| *n == 4));
    } else {
        let whole = expected.len() / (64 << 10);
        assert_eq!(written.writes.len(), whole + 1);
        assert!(written.writes[..whole].iter().all(|n| *n == 64 << 10));
        assert_eq!(written.writes[whole], expected.len() % (64 << 10));
        let mut rows = Counted::default();
        snapshot
            .write_stored16_context(None, &mut rows)
            .expect("write_stored16_context");
        assert_eq!(rows.bytes, expected);
        assert_eq!(rows.writes, vec![COLS * 2; ROWS * DEPTH]);
    }
    let shared = snapshot.payload_bytes().expect("payload_bytes");
    if cfg!(target_endian = "little") {
        let shared = shared.expect("the buffer of the store on a little-endian machine");
        assert_eq!(&shared[..], expected);
        assert_eq!(shared.as_ref(), expected);
        let again = snapshot
            .payload_bytes()
            .expect("payload_bytes")
            .expect("buffer");
        assert_eq!(shared.as_ptr(), again.as_ptr());
        lease.release();
        assert!(
            snapshot.payload_bytes().is_err(),
            "a released lease gives no buffer"
        );
        assert_eq!(&shared[..], expected);
    } else {
        assert!(shared.is_none());
        lease.release();
    }
}
fn expected_stored(signed: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(COLS * ROWS * DEPTH * 2);
    for z in 0..DEPTH {
        for y in 0..ROWS {
            for x in 0..COLS {
                let value = stored(x, y, z);
                if signed {
                    out.extend_from_slice(&(value as i16 - 1500).to_le_bytes());
                } else {
                    out.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    out
}
fn expected_modality() -> Vec<u8> {
    let mut out = Vec::with_capacity(COLS * ROWS * DEPTH * 4);
    for z in 0..DEPTH {
        for y in 0..ROWS {
            for x in 0..COLS {
                let value = f32::from(stored(x, y, z)) - 1024.0 + z as f32;
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    out
}
#[test]
fn a_signed_generation_is_handed_out_as_stored() {
    let v = volume(true, false);
    assert!(v.prepare_snapshot_context(None).1.is_none());
    check_generation(
        &v,
        VolumeScalarFormat::I16_STORED_LE,
        &expected_stored(true),
    );
}
#[test]
fn an_unsigned_generation_is_handed_out_as_stored() {
    let v = volume(false, false);
    assert!(v.prepare_snapshot_context(None).1.is_none());
    check_generation(
        &v,
        VolumeScalarFormat::U16_STORED_LE,
        &expected_stored(false),
    );
}
#[test]
fn a_float32_generation_is_handed_out_as_stored() {
    let v = volume(false, true);
    assert!(v.prepare_snapshot_context(None).1.is_none());
    check_generation(
        &v,
        VolumeScalarFormat::F32_MODALITY_LE,
        &expected_modality(),
    );
}
#[test]
fn the_bands_and_the_generation_share_one_buffer() {
    for (signed, per_slice) in [(true, false), (false, true)] {
        let v = volume(signed, per_slice);
        let mut bands: Vec<(usize, RetainedVolumeBandReader)> = Vec::new();
        let mut observe = |p: &VolumeDecodeProgress<'_>| {
            if let Some(band) = &p.band {
                let kept = band.retain().unwrap_or_else(|e| panic!("retain: {e}"));
                bands.push((p.band_start as usize, kept));
            }
        };
        let (_, err) = v.prepare_snapshot_bands_in_order_context(None, &mut observe);
        assert!(err.is_none(), "prepare: {err:?}");
        assert_eq!(
            bands.iter().map(|(_, b)| b.slice_count()).sum::<usize>(),
            DEPTH
        );
        let expected = if per_slice {
            expected_modality()
        } else {
            expected_stored(signed)
        };
        let (lease, snapshot) = snapshot_of(&v);
        let shared = snapshot.payload_bytes().expect("payload_bytes");
        let slice = expected.len() / DEPTH;
        for (first, band) in &bands {
            let at = first * slice;
            let mut copied = Vec::new();
            band.append_native_le(&mut copied);
            assert_eq!(copied, &expected[at..at + copied.len()]);
            if let (Some(view), Some(shared)) = (band.native_le_bytes(), &shared) {
                assert_eq!(view, &expected[at..at + view.len()]);
                assert_eq!(view.as_ptr(), shared[at..].as_ptr());
            } else {
                #[cfg(target_endian = "little")]
                panic!("a little-endian store must hand out its buffer");
            }
        }
        lease.release();
    }
}
