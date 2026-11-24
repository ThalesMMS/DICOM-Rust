use super::{Error, Sentinel, Volume, finite};
use crate::display::VoiLut;
use crate::render::WindowLevel;
impl Volume {
    pub fn slice_image(&self, k: i64, t: i64, window: &WindowLevel) -> Result<Vec<u8>, Error> {
        if k < 0 || k >= self.dimensions[2] || t < 0 || t >= self.dimensions[3] {
            return Err(Error::Sentinel(Sentinel::UnsupportedDimensions));
        }
        if !finite(window.center)
            || !finite(window.width)
            || window.width <= 0.0
            || window.lut.is_some()
        {
            return Err(Error::Sentinel(Sentinel::UnsupportedScaling));
        }
        let (nx, ny) = (self.dimensions[0], self.dimensions[1]);
        if nx <= 0 || ny <= 0 || self.bytes_per_voxel() == 0 {
            return Err(Error::Sentinel(Sentinel::UnsupportedDimensions));
        }
        let n = (nx as u64)
            .checked_mul(ny as u64)
            .filter(|&n| n <= i64::MAX as u64)
            .ok_or(Error::Sentinel(Sentinel::LimitExceeded))?;
        let total = self.len() as u64;
        let start = (t as u64)
            .checked_mul(self.dimensions[2] as u64)
            .and_then(|planes| planes.checked_add(k as u64))
            .and_then(|plane| plane.checked_mul(n))
            .filter(|&start| start <= total && n <= total - start)
            .ok_or(Error::Sentinel(Sentinel::UnsupportedDimensions))?;
        let mapper = VoiLut {
            center: window.center,
            width: window.width,
            function: window.function,
            ..VoiLut::default()
        };
        let mut pixels = vec![0u8; n as usize];
        for (offset, pixel) in pixels.iter_mut().enumerate() {
            let value = self.value(start as usize + offset);
            if finite(value) {
                *pixel = mapper.window_byte(value);
            }
        }
        Ok(pixels)
    }
}
