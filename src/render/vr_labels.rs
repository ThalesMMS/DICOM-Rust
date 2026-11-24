#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::grid::nearest_index;
use super::vec3::Vec3;
use super::volume::Volume;
use super::vr_transfer::{Rgba, finite};
use crate::textfmt::float_v;
pub const ERR_INVALID_VR_LABEL_MAP: &str = "render: invalid VR label map";
pub const MAX_VR_LABEL: usize = 65535;
const VR_BLOCK_SIZE: i64 = 8;
pub(crate) const VR_BLOCK_VISIBLE_LABEL: u8 = 1;
pub(crate) const VR_BLOCK_LABEL_OPACITY: u8 = 2;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrLabelStyle {
    pub color: Rgba,
    pub opacity: f64,
    pub blend: f64,
    pub visible: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct VrLabelMap {
    cols: i64,
    rows: i64,
    depth: i64,
    labels8: Option<Vec<u8>>,
    labels16: Vec<u16>,
    styles: Vec<VrLabelStyle>,
    max_label: u16,
    only_inside: bool,
    block_flags: Vec<u8>,
}
fn invalid(detail: String) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVrLabelMap,
        format!("{ERR_INVALID_VR_LABEL_MAP}: {detail}"),
    )
}
impl VrLabelMap {
    pub fn new<T: Copy + Into<u16>>(
        cols: i64,
        rows: i64,
        depth: i64,
        labels: &[T],
        styles: &[VrLabelStyle],
        only_inside: bool,
    ) -> Result<VrLabelMap, RenderError> {
        if cols <= 0
            || rows <= 0
            || depth <= 0
            || cols > i64::MAX / rows
            || cols * rows > i64::MAX / depth
            || cols * rows * depth != labels.len() as i64
        {
            return Err(invalid(format!(
                "{} labels for a {}×{}×{} volume",
                labels.len(),
                cols,
                rows,
                depth
            )));
        }
        if styles.len() > MAX_VR_LABEL + 1 {
            return Err(invalid(format!("{} styles", styles.len())));
        }
        let mut out_styles = Vec::with_capacity(styles.len());
        for (index, style) in styles.iter().enumerate() {
            for v in [
                style.color.r,
                style.color.g,
                style.color.b,
                style.opacity,
                style.blend,
            ] {
                if !finite(v) || !(0.0..=1.0).contains(&v) {
                    return Err(invalid(format!(
                        "label {} style {{Color:{{R:{} G:{} B:{} A:{}}} Opacity:{} Blend:{} Visible:{}}}",
                        index,
                        float_v(style.color.r),
                        float_v(style.color.g),
                        float_v(style.color.b),
                        float_v(style.color.a),
                        float_v(style.opacity),
                        float_v(style.blend),
                        style.visible
                    )));
                }
            }
            let mut style = *style;
            style.color.a = 1.0;
            out_styles.push(style);
        }
        let labels16: Vec<u16> = labels.iter().map(|l| (*l).into()).collect();
        let max_label = labels16.iter().copied().max().unwrap_or(0);
        let mut out = VrLabelMap {
            cols,
            rows,
            depth,
            labels8: None,
            labels16: Vec::new(),
            styles: out_styles,
            max_label,
            only_inside,
            block_flags: Vec::new(),
        };
        if max_label <= 255 {
            out.labels8 = Some(labels16.iter().map(|l| *l as u8).collect());
        } else {
            out.labels16 = labels16;
        }
        out.prepare_block_occupancy();
        Ok(out)
    }
    pub fn dimensions(&self) -> (i64, i64, i64) {
        (self.cols, self.rows, self.depth)
    }
    pub fn label_at(&self, x: i64, y: i64, z: i64) -> u16 {
        if x < 0 || y < 0 || z < 0 || x >= self.cols || y >= self.rows || z >= self.depth {
            return 0;
        }
        self.label(((z * self.rows + y) * self.cols + x) as usize)
    }
    fn label(&self, index: usize) -> u16 {
        match &self.labels8 {
            Some(l) => u16::from(l[index]),
            None => self.labels16[index],
        }
    }
    pub fn styles(&self) -> &[VrLabelStyle] {
        &self.styles
    }
    pub fn only_inside(&self) -> bool {
        self.only_inside
    }
    pub fn storage_bits(&self) -> u32 {
        if self.max_label <= 255 { 8 } else { 16 }
    }
    pub fn label_bytes(&self) -> Vec<u8> {
        match &self.labels8 {
            Some(l) => l.clone(),
            None => self.labels16.iter().flat_map(|l| l.to_le_bytes()).collect(),
        }
    }
    pub fn max_label(&self) -> u16 {
        self.max_label
    }
    pub(crate) fn matches_volume(&self, vol: &Volume) -> bool {
        self.cols == vol.cols && self.rows == vol.rows && self.depth == vol.depth
    }
    pub(crate) fn label_at_tex(&self, tex: Vec3) -> u16 {
        let nearest = |t: f64, n: i64| -> i64 {
            if n < 2 {
                return 0;
            }
            nearest_index(t * (n - 1) as f64).clamp(0, n - 1)
        };
        let x = nearest(tex.x, self.cols);
        let y = nearest(tex.y, self.rows);
        let z = nearest(tex.z, self.depth);
        self.label(((z * self.rows + y) * self.cols + x) as usize)
    }
    pub(crate) fn visible_style(&self, label: u16) -> Option<VrLabelStyle> {
        if label == 0 {
            return None;
        }
        self.styles
            .get(label as usize)
            .filter(|s| s.visible)
            .copied()
    }
    pub(crate) fn keeps(&self, tex: Vec3) -> bool {
        if !self.only_inside {
            return true;
        }
        self.visible_style(self.label_at_tex(tex)).is_some()
    }
    fn prepare_block_occupancy(&mut self) {
        let (nx, ny, nz) = (
            (self.cols - 1) / VR_BLOCK_SIZE + 1,
            (self.rows - 1) / VR_BLOCK_SIZE + 1,
            (self.depth - 1) / VR_BLOCK_SIZE + 1,
        );
        let mut block_flags = vec![0u8; (nx * ny * nz) as usize];
        for z in 0..self.depth {
            for y in 0..self.rows {
                for x in 0..self.cols {
                    let label = self.label(((z * self.rows + y) * self.cols + x) as usize);
                    let Some(style) = self.visible_style(label) else {
                        continue;
                    };
                    let mut flags = VR_BLOCK_VISIBLE_LABEL;
                    if style.blend > 0.0 && style.opacity > 0.0 {
                        flags |= VR_BLOCK_LABEL_OPACITY;
                    }
                    let (bx, by, bz) = (x / VR_BLOCK_SIZE, y / VR_BLOCK_SIZE, z / VR_BLOCK_SIZE);
                    let x0 = if x > 0 && x % VR_BLOCK_SIZE == 0 {
                        bx - 1
                    } else {
                        bx
                    };
                    let y0 = if y > 0 && y % VR_BLOCK_SIZE == 0 {
                        by - 1
                    } else {
                        by
                    };
                    let z0 = if z > 0 && z % VR_BLOCK_SIZE == 0 {
                        bz - 1
                    } else {
                        bz
                    };
                    for iz in z0..=bz {
                        for iy in y0..=by {
                            for ix in x0..=bx {
                                block_flags[((iz * ny + iy) * nx + ix) as usize] |= flags;
                            }
                        }
                    }
                }
            }
        }
        self.block_flags = block_flags;
    }
    pub(crate) fn block_flags(&self) -> &[u8] {
        &self.block_flags
    }
}
pub(crate) fn mix_vr_label(
    style: &VrLabelStyle,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) -> (f64, f64, f64, f64) {
    let f = style.blend;
    (
        r + (style.color.r - r) * f,
        g + (style.color.g - g) * f,
        b + (style.color.b - b) * f,
        a + (style.opacity - a) * f,
    )
}
