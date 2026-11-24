use super::{Error, ErrorKind, finite, fmax, fmin, frame_size_matches};
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shift {
    pub row: f64,
    pub column: f64,
}
impl Shift {
    pub fn displacement(self) -> (f64, f64) {
        (-self.column, self.row)
    }
    pub fn from_displacement(dx: f64, dy: f64) -> Shift {
        Shift {
            row: dy,
            column: -dx,
        }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Shift) -> Shift {
        Shift {
            row: self.row + other.row,
            column: self.column + other.column,
        }
    }
    pub fn is_zero(self) -> bool {
        self.row == 0.0 && self.column == 0.0
    }
    pub(crate) fn is_finite(self) -> bool {
        finite(self.row) && finite(self.column)
    }
    pub(crate) fn field_text(self) -> String {
        format!(
            "{{Row:{} Column:{}}}",
            float_v(self.row),
            float_v(self.column)
        )
    }
}
pub fn average_frames(frames: &[&[f64]]) -> Result<Vec<f64>, Error> {
    if frames.is_empty() {
        return Err(Error::sentinel(ErrorKind::NoFrames));
    }
    if frames.len() == 1 {
        return Ok(frames[0].to_vec());
    }
    let size = frames[0].len();
    let mut sum = vec![0.0f64; size];
    for (index, frame) in frames.iter().enumerate() {
        if frame.len() != size {
            return Err(Error::wrap(
                ErrorKind::FrameSizeMismatch,
                format!("frame {index} has {} values, want {size}", frame.len()),
            ));
        }
        for i in 0..size {
            sum[i] += frame[i];
        }
    }
    let scale = 1.0 / frames.len() as f64;
    for v in &mut sum {
        *v *= scale;
    }
    Ok(sum)
}
pub fn apply_shift(mask: &[f64], columns: i64, rows: i64, shift: Shift) -> Result<Vec<f64>, Error> {
    if !frame_size_matches(columns, rows, &[mask]) {
        return Err(Error::wrap(
            ErrorKind::FrameSizeMismatch,
            format!("{} values for {columns}x{rows}", mask.len()),
        ));
    }
    if !shift.is_finite() {
        return Err(Error::wrap(
            ErrorKind::InvalidPixelShift,
            shift.field_text(),
        ));
    }
    if shift.is_zero() {
        return Ok(mask.to_vec());
    }
    let (dx, dy) = shift.displacement();
    let dx = fmin(fmax(dx, -(columns as f64)), columns as f64);
    let dy = fmin(fmax(dy, -(rows as f64)), rows as f64);
    let mut out = vec![0.0f64; mask.len()];
    let at = |x: i64, y: i64| -> f64 {
        let x = x.clamp(0, columns - 1);
        let y = y.clamp(0, rows - 1);
        mask[(y * columns + x) as usize]
    };
    for y in 0..rows {
        let sy = y as f64 - dy;
        let y0 = sy.floor() as i64;
        let ty = sy - y0 as f64;
        for x in 0..columns {
            let sx = x as f64 - dx;
            let x0 = sx.floor() as i64;
            let tx = sx - x0 as f64;
            let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
            let bottom = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
            out[(y * columns + x) as usize] = top * (1.0 - ty) + bottom * ty;
        }
    }
    Ok(out)
}
pub fn subtract_modality(
    contrast: &[f64],
    mask: &[f64],
    columns: i64,
    rows: i64,
    shift: Shift,
) -> Result<Vec<f64>, Error> {
    if contrast.len() != mask.len() {
        return Err(Error::wrap(
            ErrorKind::FrameSizeMismatch,
            format!(
                "contrast has {} values, mask has {}",
                contrast.len(),
                mask.len()
            ),
        ));
    }
    let mut difference = apply_shift(mask, columns, rows, shift)?;
    for i in 0..contrast.len() {
        difference[i] = contrast[i] - difference[i];
    }
    Ok(difference)
}
