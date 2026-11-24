use dicom_rust::pixel::Metadata;
use dicom_rust::render::error::ErrorKind;
use dicom_rust::render::frame::Frame;
use dicom_rust::render::parallel::CancelToken;
use dicom_rust::render::vec3::Vec3;
use dicom_rust::render::volume::{Stack, Volume, build_volume};
use dicom_rust::render::volume_link::VolumeDecodeProgress;
use dicom_rust::render::{Rescale, WindowLevel};
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::time::Duration;
const THREADS: usize = 6;
fn stack(cols: usize, rows: usize, depth: usize, irregular: bool) -> Stack {
    let window = WindowLevel::new(40.0, 400.0);
    let orientation = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let mut stack = Stack::new(Vec::new());
    stack.uid = "1.2.826.0.1.3680043.10.1561.9".into();
    stack.default_window = window.clone();
    stack.pixel_spacing = vec![0.8, 0.8];
    stack.slice_thickness = 1.5;
    for z in 0..depth {
        let mut pixels = vec![0u8; cols * rows * 2];
        for y in 0..rows {
            for x in 0..cols {
                let v = ((x * 7 + y * 13 + z * 29) % 900) as i16 - 200;
                let o = (y * cols + x) * 2;
                pixels[o..o + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        let along = z as f64 * 1.5
            + if irregular {
                0.4 * (z as f64 * 1.7).sin()
            } else {
                0.0
            };
        let position = Vec3::new(-10.0, -12.0, 5.0).add(Vec3::new(0.0, 0.0, along));
        let frame = Frame {
            sop_instance_uid: format!("1.2.826.0.1.3680043.10.1561.9.{}", z + 1),
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
            image_orientation: orientation.clone(),
            pixel_spacing: vec![0.8, 0.8],
            slice_thickness: 1.5,
            instance_number: z as i64 + 1,
            ..Default::default()
        };
        stack.frames.push(Some(Arc::new(frame)));
    }
    stack
}
fn volume(irregular: bool) -> Arc<Volume> {
    match build_volume(&stack(24, 20, 10, irregular)) {
        Ok(v) => v,
        Err(e) => panic!("build_volume: {e}"),
    }
}
type Seen = Result<(u64, bool), String>;
fn race(volume: &Arc<Volume>) -> Vec<Seen> {
    let barrier = Barrier::new(THREADS);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|i| {
                let barrier = &barrier;
                scope.spawn(move || -> Seen {
                    barrier.wait();
                    let mut built = false;
                    if i % 2 == 0 {
                        let (stats, err) = volume.prepare_snapshot_context(None);
                        if let Some(e) = err {
                            return Err(e.to_string());
                        }
                        built = !stats.reused;
                    }
                    let lease = volume
                        .acquire_store_snapshot_context(None)
                        .map_err(|e| e.to_string())?;
                    let generation = lease.generation();
                    lease.snapshot().map_err(|e| e.to_string())?;
                    lease.release();
                    Ok((generation, built))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err("thread panicked".into())))
            .collect()
    })
}
fn stress(irregular: bool, rounds: usize) {
    let mut lost = 0;
    let mut failures = Vec::new();
    for round in 0..rounds {
        let v = volume(irregular);
        let seen = race(&v);
        let mut generations: Vec<u64> = Vec::new();
        let mut builds = 0;
        for s in &seen {
            match s {
                Ok((generation, built)) => {
                    generations.push(*generation);
                    builds += usize::from(*built);
                }
                Err(e) => {
                    lost += 1;
                    if failures.len() < 5 {
                        failures.push(format!("round {round}: {e}"));
                    }
                }
            }
        }
        if generations.len() != THREADS {
            continue;
        }
        generations.dedup();
        assert_eq!(
            generations.len(),
            1,
            "round {round}: threads hold different generations {generations:?}"
        );
        assert!(builds <= 1, "round {round}: {builds} preparations built");
        assert_eq!(v.volume_generation(), generations[0], "round {round}");
        assert!(v.snapshot_ready(), "round {round}");
        if !irregular {
            assert_eq!(v.volume_store_stats().replacements, 1, "round {round}");
        }
        v.close();
    }
    assert!(
        lost == 0,
        "{lost} of {} first snapshots failed over {rounds} rounds: {failures:?}",
        rounds * THREADS
    );
}
#[test]
fn concurrent_first_snapshots_share_one_generation() {
    stress(false, 300);
}
#[test]
fn concurrent_first_snapshots_of_a_resampled_volume() {
    stress(true, 60);
}
#[test]
fn a_waiter_cancels_while_another_thread_builds() {
    let v = volume(false);
    let token = CancelToken::new();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (start_tx, start_rx) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let v = &v;
        let builder = scope.spawn(move || {
            let mut first = true;
            let mut observe = |_: &VolumeDecodeProgress<'_>| {
                if first {
                    first = false;
                    let _ = entered_tx.send(());
                    let _ = start_rx.recv_timeout(Duration::from_secs(30));
                }
            };
            v.prepare_snapshot_progressive_context(None, Some(&mut observe))
        });
        entered_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the builder reports progress");
        let waiter = scope.spawn(|| v.acquire_store_snapshot_context(Some(&token)));
        std::thread::sleep(Duration::from_millis(50));
        token.cancel();
        let waited = waiter.join().expect("waiter thread");
        match waited {
            Err(e) => assert!(e.is(ErrorKind::Canceled), "waiter: {e}"),
            Ok(_) => panic!("the waiter got a lease while the build was held"),
        }
        let _ = start_tx.send(());
        let (stats, err) = builder.join().expect("builder thread");
        assert!(err.is_none(), "builder: {err:?}");
        assert!(!stats.reused);
    });
    let lease = v
        .acquire_store_snapshot_context(None)
        .expect("acquire after the build");
    assert_eq!(lease.generation(), v.volume_generation());
    lease.release();
    assert_eq!(v.volume_store_stats().replacements, 1);
}
#[test]
fn a_waiter_builds_when_the_builder_is_canceled() {
    let v = volume(false);
    let token = CancelToken::new();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (start_tx, start_rx) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let (v, token) = (&v, &token);
        let builder = scope.spawn(move || {
            let mut first = true;
            let mut observe = |_: &VolumeDecodeProgress<'_>| {
                if first {
                    first = false;
                    let _ = entered_tx.send(());
                    let _ = start_rx.recv_timeout(Duration::from_secs(30));
                }
            };
            v.prepare_snapshot_progressive_context(Some(token), Some(&mut observe))
        });
        entered_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the builder reports progress");
        let waiter = scope.spawn(|| v.acquire_store_snapshot_context(None));
        std::thread::sleep(Duration::from_millis(50));
        token.cancel();
        let _ = start_tx.send(());
        let (_, err) = builder.join().expect("builder thread");
        assert!(
            err.as_ref().is_some_and(|e| e.is(ErrorKind::Canceled)),
            "builder: {err:?}"
        );
        let lease = waiter
            .join()
            .expect("waiter thread")
            .expect("the waiter builds after the cancellation");
        assert_eq!(lease.generation(), v.volume_generation());
        lease.release();
    });
}
struct OtherKindCancel(std::sync::atomic::AtomicBool);
impl dicom_rust::render::parallel::Cancellation for OtherKindCancel {
    fn err(&self) -> Option<dicom_rust::render::RenderError> {
        self.0
            .load(std::sync::atomic::Ordering::SeqCst)
            .then(|| dicom_rust::render::RenderError::other("context canceled"))
    }
}
#[test]
fn a_build_canceled_with_any_error_is_not_cached() {
    let v = volume(false);
    let token = OtherKindCancel(std::sync::atomic::AtomicBool::new(false));
    let mut first = true;
    let mut observe = |_: &VolumeDecodeProgress<'_>| {
        if first {
            first = false;
            token.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    };
    let (_, err) = v.prepare_snapshot_progressive_context(Some(&token), Some(&mut observe));
    assert!(err.is_some(), "the canceled build fails");
    let lease = v
        .acquire_store_snapshot_context(None)
        .expect("the next caller builds the volume");
    assert_eq!(lease.generation(), v.volume_generation());
    lease.release();
}
