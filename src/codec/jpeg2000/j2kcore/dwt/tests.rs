use super::super::geometry::{BAND_HH, BAND_HL, BAND_LH, BAND_LL, Resolution, TileComponent};
use super::{SubbandGrid, fast, inverse_dwt, inverse_dwt_plain};
struct Random(u64);
impl Random {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}
fn ceil_half(v: i64) -> i64 {
    (v + 1).div_euclid(2)
}
fn floor_half(v: i64) -> i64 {
    v.div_euclid(2)
}
fn grid(
    r: &mut Random,
    rect: (i64, i64, i64, i64),
    reversible: bool,
    magnitude: u32,
) -> Option<SubbandGrid> {
    let (x0, y0, x1, y1) = rect;
    let n = ((x1 - x0).max(0) * (y1 - y0).max(0)) as usize;
    let values = (0..n)
        .map(|_| {
            let v = ((r.next() << 1) as i32) >> (31 - magnitude);
            if reversible {
                v as f32
            } else {
                v as f32 * 0.372_f32
            }
        })
        .collect();
    Some(SubbandGrid {
        x0,
        y0,
        x1,
        y1,
        values,
    })
}
fn random_case(r: &mut Random, reversible: bool) -> (TileComponent, Vec<[Option<SubbandGrid>; 4]>) {
    let levels = 1 + r.below(5) as usize;
    let small = r.below(3) == 0;
    let (x0, y0) = (r.below(9) as i64, r.below(9) as i64);
    let (w, h) = if small {
        (1 + r.below(4) as i64, 1 + r.below(4) as i64)
    } else {
        (1 + r.below(70) as i64, 1 + r.below(70) as i64)
    };
    let magnitude = match r.below(4) {
        0 => 30,
        1 => 24,
        _ => 12,
    };
    let mut rects = vec![(x0, y0, x0 + w, y0 + h)];
    for _ in 0..levels {
        let (a, b, c, d) = rects[0];
        rects.insert(0, (ceil_half(a), ceil_half(b), ceil_half(c), ceil_half(d)));
    }
    let mut tc = TileComponent {
        x0,
        y0,
        x1: x0 + w,
        y1: y0 + h,
        ..TileComponent::default()
    };
    let mut bands = Vec::new();
    for (index, &(a, b, c, d)) in rects.iter().enumerate() {
        tc.resolutions.push(Resolution {
            index: index as i64,
            x0: a,
            y0: b,
            x1: c,
            y1: d,
            ..Resolution::default()
        });
        let mut entry: [Option<SubbandGrid>; 4] = [None, None, None, None];
        if index == 0 {
            entry[BAND_LL as usize] = grid(r, (a, b, c, d), reversible, magnitude);
        } else {
            let (fa, fb, fc, fd) = (floor_half(a), floor_half(b), floor_half(c), floor_half(d));
            let (ca, cb, cc, cd) = (ceil_half(a), ceil_half(b), ceil_half(c), ceil_half(d));
            entry[BAND_HL as usize] = grid(r, (fa, cb, fc, cd), reversible, magnitude);
            entry[BAND_LH as usize] = grid(r, (ca, fb, cc, fd), reversible, magnitude);
            entry[BAND_HH as usize] = grid(r, (fa, fb, fc, fd), reversible, magnitude);
        }
        bands.push(entry);
    }
    (tc, bands)
}
fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}
#[test]
fn the_fast_transform_gives_the_plain_transforms_samples() {
    let mut r = Random(0x5EED_0003);
    for reversible in [true, false] {
        let (mut fast_path, mut declined) = (0, 0);
        for case in 0..6_000 {
            let (tc, bands) = random_case(&mut r, reversible);
            let expected = inverse_dwt_plain(&tc, bands.clone(), reversible).expect("plain");
            let threads = [1, 3][case % 2];
            match fast::inverse(&tc, &bands, reversible, threads) {
                Some(got) => {
                    fast_path += 1;
                    assert_eq!(
                        bits(&got),
                        bits(&expected),
                        "case {case}, reversible {reversible}: {:?}",
                        tc.resolutions
                            .iter()
                            .map(|res| (res.x0, res.y0, res.x1, res.y1))
                            .collect::<Vec<_>>()
                    );
                }
                None => declined += 1,
            }
            let chosen = inverse_dwt(&tc, bands, reversible, 1).expect("chosen");
            assert_eq!(bits(&chosen), bits(&expected), "case {case}");
        }
        assert!(fast_path > 3_000, "{fast_path} on the fast path");
        assert!(declined > 50, "{declined} declined");
    }
}
#[test]
fn irregular_grids_are_left_to_the_plain_transform() {
    let mut r = Random(0x5EED_0004);
    for case in 0..500 {
        let (mut tc, mut bands) = random_case(&mut r, true);
        let level = 1 + r.below(tc.resolutions.len() as u32 - 1) as usize;
        match r.below(4) {
            0 => bands[level][BAND_HL as usize] = None,
            1 => {
                if let Some(g) = bands[level][BAND_HH as usize].as_mut() {
                    g.x0 += 1;
                    g.x1 += 1;
                }
            }
            2 => tc.resolutions[level - 1].x1 += 1,
            _ => {
                if let Some(g) = bands[level][BAND_LH as usize].as_mut() {
                    g.values.pop();
                }
            }
        }
        if let Some(got) = fast::inverse(&tc, &bands, true, 1) {
            let expected = inverse_dwt_plain(&tc, bands, true).expect("plain");
            assert_eq!(bits(&got), bits(&expected), "case {case}");
        }
    }
}
#[test]
fn a_large_level_on_threads_gives_the_same_samples() {
    let mut r = Random(0x5EED_0005);
    for reversible in [true, false] {
        let levels = 3usize;
        let (x0, y0, w, h) = (3i64, 5i64, 531i64, 397i64);
        let mut rects = vec![(x0, y0, x0 + w, y0 + h)];
        for _ in 0..levels {
            let (a, b, c, d) = rects[0];
            rects.insert(0, (ceil_half(a), ceil_half(b), ceil_half(c), ceil_half(d)));
        }
        let mut tc = TileComponent {
            x0,
            y0,
            x1: x0 + w,
            y1: y0 + h,
            ..TileComponent::default()
        };
        let mut bands = Vec::new();
        for (index, &(a, b, c, d)) in rects.iter().enumerate() {
            tc.resolutions.push(Resolution {
                index: index as i64,
                x0: a,
                y0: b,
                x1: c,
                y1: d,
                ..Resolution::default()
            });
            let mut entry: [Option<SubbandGrid>; 4] = [None, None, None, None];
            if index == 0 {
                entry[BAND_LL as usize] = grid(&mut r, (a, b, c, d), reversible, 12);
            } else {
                let (fa, fb, fc, fd) = (floor_half(a), floor_half(b), floor_half(c), floor_half(d));
                let (ca, cb, cc, cd) = (ceil_half(a), ceil_half(b), ceil_half(c), ceil_half(d));
                entry[BAND_HL as usize] = grid(&mut r, (fa, cb, fc, cd), reversible, 12);
                entry[BAND_LH as usize] = grid(&mut r, (ca, fb, cc, fd), reversible, 12);
                entry[BAND_HH as usize] = grid(&mut r, (fa, fb, fc, fd), reversible, 12);
            }
            bands.push(entry);
        }
        let expected = inverse_dwt_plain(&tc, bands.clone(), reversible).expect("plain");
        for threads in [1, 2, 3, 8] {
            let got = fast::inverse(&tc, &bands, reversible, threads).expect("regular");
            assert_eq!(bits(&got), bits(&expected), "{threads} threads");
        }
    }
}
#[test]
fn a_lone_sample_is_halved_only_by_the_reversible_transform() {
    use super::synthesize_1d;
    for (start, reversible, want) in [
        (3, true, 3.0),
        (3, false, 6.0),
        (4, true, 6.0),
        (4, false, 6.0),
    ] {
        let mut signal = [6.0f32];
        synthesize_1d(&mut signal, start, start + 1, reversible);
        assert_eq!(signal[0], want, "start {start}, reversible {reversible}");
    }
}
