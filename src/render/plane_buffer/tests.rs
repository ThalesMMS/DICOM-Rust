use super::{PlaneBuffer, Samples};
use std::sync::{Arc, Barrier};
const PLANE: usize = 16;
const PLANES: usize = 6;
fn value(plane: usize, i: usize) -> i16 {
    (plane * 100 + i) as i16 - 300
}
#[test]
fn disjoint_planes_are_written_while_finished_planes_are_read() {
    let buffer = PlaneBuffer::new(vec![0i16; PLANE * PLANES]);
    {
        let first = unsafe { buffer.range_mut(0, PLANE) };
        for (i, v) in first.iter_mut().enumerate() {
            *v = value(0, i);
        }
    }
    let barrier = Barrier::new(PLANES);
    std::thread::scope(|scope| {
        let reader = {
            let (buffer, barrier) = (Arc::clone(&buffer), &barrier);
            scope.spawn(move || {
                barrier.wait();
                let mut sum = 0i64;
                for _ in 0..4 {
                    let plane = unsafe { buffer.range(0, PLANE) };
                    sum += plane.iter().map(|&v| i64::from(v)).sum::<i64>();
                }
                sum
            })
        };
        for p in 1..PLANES {
            let (buffer, barrier) = (Arc::clone(&buffer), &barrier);
            scope.spawn(move || {
                barrier.wait();
                let plane = unsafe { buffer.range_mut(p * PLANE, PLANE) };
                for (i, v) in plane.iter_mut().enumerate() {
                    *v = value(p, i);
                }
            });
        }
        let expected: i64 = (0..PLANE).map(|i| i64::from(value(0, i))).sum();
        assert_eq!(reader.join().ok(), Some(4 * expected));
    });
    let samples = Samples::finished(buffer);
    let shared = samples.clone();
    let expected: Vec<i16> = (0..PLANES)
        .flat_map(|p| (0..PLANE).map(move |i| value(p, i)))
        .collect();
    assert_eq!(&*samples, &expected[..]);
    drop(samples);
    assert_eq!(shared.len(), PLANE * PLANES);
}
#[test]
fn empty_and_spare_capacity_buffers_round_trip() {
    let empty: Samples<f32> = Vec::new().into();
    assert!(empty.is_empty());
    let mut spare = Vec::with_capacity(32);
    spare.extend([1.5f32, -2.0, 3.25]);
    let spare: Samples<f32> = spare.into();
    assert_eq!(&*spare, &[1.5, -2.0, 3.25]);
    let zero_sized: Samples<()> = vec![(); 5].into();
    assert_eq!(zero_sized.len(), 5);
}
#[test]
fn values_with_their_own_allocation_are_dropped_once() {
    let marker = Arc::new(());
    let values: Vec<Arc<()>> = (0..4).map(|_| Arc::clone(&marker)).collect();
    let samples: Samples<Arc<()>> = values.into();
    assert_eq!(Arc::strong_count(&marker), 5);
    let clone = samples.clone();
    drop(samples);
    assert_eq!(Arc::strong_count(&marker), 5);
    drop(clone);
    assert_eq!(Arc::strong_count(&marker), 1);
}
#[test]
#[should_panic(expected = "plane outside the buffer")]
fn a_plane_past_the_end_is_refused() {
    let buffer = PlaneBuffer::new(vec![0u16; 8]);
    let _ = unsafe { buffer.range(4, 5) };
}
