fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
fn key(seed: u64, x: usize, y: usize, z: usize) -> u64 {
    seed ^ ((x as u64) << 1) ^ ((y as u64) << 21) ^ ((z as u64) << 41)
}
fn uniform(k: u64) -> f64 {
    (mix(k) >> 11) as f64 / (1u64 << 53) as f64
}
fn gauss(k: u64) -> f64 {
    let a = mix(k);
    let b = mix(a);
    let s =
        ((a & 0xFFFF_FFFF) as f64 + (a >> 32) as f64 + (b & 0xFFFF_FFFF) as f64 + (b >> 32) as f64)
            / 4_294_967_296.0;
    (s - 2.0) * 3f64.sqrt()
}
fn inside(x: f64, y: f64, cx: f64, cy: f64, rx: f64, ry: f64) -> f64 {
    let dx = (x - cx) / rx;
    let dy = (y - cy) / ry;
    dx * dx + dy * dy
}
pub struct Gray16 {
    pub width: usize,
    pub height: usize,
    pub bits_stored: u16,
    pub pixels: Vec<u16>,
}
impl Gray16 {
    pub fn le_bytes(&self) -> Vec<u8> {
        self.pixels.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
    pub fn windowed8(&self, c: f64, w: f64) -> Vec<u8> {
        self.pixels
            .iter()
            .map(|&v| (((v as f64 - (c - w / 2.0)) / w) * 255.0).clamp(0.0, 255.0) as u8)
            .collect()
    }
}
pub struct Rgb8 {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
pub fn ct_slice(z: usize, depth: usize) -> Gray16 {
    let (w, h) = (512usize, 512usize);
    let t = z as f64 / depth.max(1) as f64;
    let mut px = Vec::with_capacity(w * h);
    let body_rx = 200.0 + 12.0 * (t * 6.0).sin();
    let body_ry = 145.0 + 6.0 * (t * 4.0).cos();
    let lung_ry = 70.0 + 30.0 * (t * std::f64::consts::PI).sin();
    for y in 0..h {
        for x in 0..w {
            let (xf, yf) = (x as f64, y as f64);
            let k = key(0xC7, x, y, z);
            let fov = inside(xf, yf, 256.0, 256.0, 250.0, 250.0);
            let hu: f64 = if fov > 1.0 {
                -1024.0
            } else {
                let body = inside(xf, yf, 256.0, 260.0, body_rx, body_ry);
                let table = (yf - 418.0).abs() < 6.0 && (xf - 256.0).abs() < 220.0;
                if body > 1.0 {
                    if table {
                        250.0 + 15.0 * gauss(k)
                    } else {
                        -1000.0 + 8.0 * gauss(k)
                    }
                } else {
                    let fat = inside(xf, yf, 256.0, 260.0, body_rx - 14.0, body_ry - 14.0) > 1.0;
                    let left = inside(xf, yf, 180.0, 245.0, 62.0, lung_ry);
                    let right = inside(xf, yf, 332.0, 245.0, 62.0, lung_ry);
                    let spine = inside(xf, yf, 256.0, 345.0, 30.0, 26.0);
                    let aorta = inside(xf, yf, 268.0, 300.0, 13.0, 13.0);
                    let base = if spine <= 1.0 {
                        if spine > 0.62 { 1150.0 } else { 280.0 }
                    } else if aorta <= 1.0 {
                        210.0
                    } else if left <= 1.0 || right <= 1.0 {
                        let (cx, cy) = ((xf / 9.0).floor(), (yf / 9.0).floor());
                        let cell = mix((cx as u64) << 20 ^ (cy as u64) ^ ((z as u64 / 3) << 40));
                        let (ox, oy) = (cx * 9.0 + 4.5, cy * 9.0 + 4.5);
                        let r2 = (xf - ox).powi(2) + (yf - oy).powi(2);
                        if cell.is_multiple_of(5) && r2 < 3.0 {
                            30.0
                        } else {
                            -860.0
                        }
                    } else if fat {
                        -100.0
                    } else {
                        40.0 + 12.0 * ((xf * 0.05).sin() * (yf * 0.043).cos())
                    };
                    base + 11.0 * gauss(k)
                }
            };
            px.push((hu + 1024.0).round().clamp(0.0, 4095.0) as u16);
        }
    }
    Gray16 {
        width: w,
        height: h,
        bits_stored: 12,
        pixels: px,
    }
}
pub fn mr_slice(z: usize) -> Gray16 {
    let (w, h) = (256usize, 256usize);
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (xf, yf) = (x as f64, y as f64);
            let k = key(0x3E, x, y, z);
            let head = inside(xf, yf, 128.0, 128.0, 96.0, 112.0);
            let signal: f64 = if head > 1.0 {
                0.0
            } else if head > 0.86 {
                if head > 0.94 { 1400.0 } else { 120.0 }
            } else {
                let brain = inside(xf, yf, 128.0, 128.0, 84.0, 100.0);
                let vent = inside(xf, yf, 120.0, 122.0, 9.0, 24.0)
                    .min(inside(xf, yf, 138.0, 122.0, 9.0, 24.0));
                let gyri =
                    ((xf * 0.21).sin() + (yf * 0.17).cos() + (xf * 0.07 + yf * 0.09).sin()) * 0.5;
                if vent <= 1.0 {
                    260.0
                } else if brain > 0.75 || gyri > 0.55 {
                    780.0
                } else {
                    1010.0
                }
            };
            let n1 = signal + 22.0 * gauss(k);
            let n2 = 22.0 * gauss(k ^ 0x5555);
            let v = (n1 * n1 + n2 * n2).sqrt();
            px.push(v.round().clamp(0.0, 4095.0) as u16);
        }
    }
    Gray16 {
        width: w,
        height: h,
        bits_stored: 12,
        pixels: px,
    }
}
pub fn dx_image() -> Gray16 {
    let (w, h) = (2048usize, 2048usize);
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (xf, yf) = (x as f64, y as f64);
            let k = key(0xD8, x, y, 0);
            let collimated = !(80..1968).contains(&x) || !(60..1990).contains(&y);
            let v: f64 = if collimated {
                3900.0 + 10.0 * gauss(k)
            } else {
                let mut v = 2600.0 - 500.0 * ((yf - 1024.0) / 1200.0).powi(2);
                let left = inside(xf, yf, 720.0, 980.0, 360.0, 620.0);
                let right = inside(xf, yf, 1328.0, 980.0, 360.0, 620.0);
                if left <= 1.0 || right <= 1.0 {
                    v -= 900.0 * (1.0 - left.min(right)).sqrt();
                    let band = ((yf + 0.00025 * (xf - 1024.0).powi(2)) / 70.0).sin();
                    if band > 0.55 {
                        v += 420.0;
                    }
                }
                if (xf - 1024.0).abs() < 90.0 {
                    v += 600.0;
                }
                v + 18.0 * gauss(k)
            };
            px.push(v.round().clamp(0.0, 4095.0) as u16);
        }
    }
    Gray16 {
        width: w,
        height: h,
        bits_stored: 12,
        pixels: px,
    }
}
pub fn us_rgb() -> Rgb8 {
    let (w, h) = (640usize, 480usize);
    let mut px = Vec::with_capacity(w * h * 3);
    let (ax, ay) = (320.0f64, 40.0f64);
    for y in 0..h {
        for x in 0..w {
            let (xf, yf) = (x as f64, y as f64);
            let k = key(0x05, x, y, 0);
            let r = ((xf - ax).powi(2) + (yf - ay).powi(2)).sqrt();
            let angle = (xf - ax).atan2(yf - ay);
            let mut rgb = [0u8; 3];
            if y < 28 {
                let g = if (x / 7 + y / 9) % 5 == 0 { 200 } else { 40 };
                rgb = [g, g, g];
            } else if (60.0..430.0).contains(&r) && angle.abs() < 0.62 {
                let u = uniform(k).max(1e-9);
                let speckle = (-2.0 * u.ln()).sqrt();
                let tissue = 90.0 + 50.0 * ((r * 0.05).sin() + (angle * 9.0).cos()) * 0.5;
                let cyst = inside(xf, yf, 300.0, 260.0, 40.0, 30.0) <= 1.0;
                let g = if cyst {
                    8.0 * speckle
                } else {
                    tissue * speckle * (1.0 - r / 900.0)
                };
                let g = g.clamp(0.0, 255.0) as u8;
                rgb = [g, g, g];
                let flow = inside(xf, yf, 360.0, 200.0, 26.0, 18.0);
                if flow <= 1.0 {
                    let level = ((1.0 - flow) * 230.0) as u8;
                    rgb = if xf < 360.0 {
                        [level, 20, 10]
                    } else {
                        [10, 30, level]
                    };
                }
            }
            px.extend_from_slice(&rgb);
        }
    }
    Rgb8 {
        width: w,
        height: h,
        pixels: px,
    }
}
