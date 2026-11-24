use crate::fmath;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}
impl Complex {
    pub const fn new(re: f64, im: f64) -> Complex {
        Complex { re, im }
    }
    fn add(self, o: Complex) -> Complex {
        Complex::new(self.re + o.re, self.im + o.im)
    }
    fn sub(self, o: Complex) -> Complex {
        Complex::new(self.re - o.re, self.im - o.im)
    }
    fn mul(self, o: Complex) -> Complex {
        Complex::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}
pub fn next_power_of_two(n: i64) -> i64 {
    let mut p = 1i64;
    while p < n {
        p <<= 1;
    }
    p
}
pub fn transform(data: &mut [Complex], inverse: bool) {
    let n = data.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            data.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut size = 2;
    while size <= n {
        let angle = sign * 2.0 * std::f64::consts::PI / size as f64;
        let step = Complex::new(fmath::cos(angle), fmath::sin(angle));
        let half = size / 2;
        let mut start = 0;
        while start < n {
            let mut w = Complex::new(1.0, 0.0);
            for k in 0..half {
                let even = data[start + k];
                let odd = data[start + k + half].mul(w);
                data[start + k] = even.add(odd);
                data[start + k + half] = even.sub(odd);
                w = w.mul(step);
            }
            start += size;
        }
        size <<= 1;
    }
}
pub fn transform_2d(data: &mut [Complex], width: usize, height: usize, inverse: bool) {
    for y in 0..height {
        transform(&mut data[y * width..(y + 1) * width], inverse);
    }
    let mut column = vec![Complex::default(); height];
    for x in 0..width {
        for (y, c) in column.iter_mut().enumerate() {
            *c = data[y * width + x];
        }
        transform(&mut column, inverse);
        for (y, c) in column.iter().enumerate() {
            data[y * width + x] = *c;
        }
    }
    if inverse {
        let scale = Complex::new(1.0 / (width * height) as f64, 0.0);
        for d in data.iter_mut() {
            *d = d.mul(scale);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        let original: Vec<Complex> = (0..8)
            .map(|i| Complex::new(i as f64, -(i as f64)))
            .collect();
        let mut data = original.clone();
        transform(&mut data, false);
        transform(&mut data, true);
        for (a, b) in data.iter().zip(&original) {
            assert!((a.re / 8.0 - b.re).abs() < 1e-12 && (a.im / 8.0 - b.im).abs() < 1e-12);
        }
        assert_eq!(next_power_of_two(1000), 1024);
    }
}
