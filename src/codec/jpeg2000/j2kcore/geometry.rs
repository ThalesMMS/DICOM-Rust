use super::codestream::{Codestream, ceil_div_u32};
use super::{CoreError, Result};
pub const BAND_LL: i64 = 0;
pub const BAND_HL: i64 = 1;
pub const BAND_LH: i64 = 2;
pub const BAND_HH: i64 = 3;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodeBlock {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub index: i64,
}
impl CodeBlock {
    pub fn width(&self) -> i64 {
        self.x1.saturating_sub(self.x0)
    }
    pub fn height(&self) -> i64 {
        self.y1.saturating_sub(self.y0)
    }
}
const MAX_PRECINCTS_PER_RESOLUTION: i64 = 1 << 20;
#[derive(Debug, Clone, Default)]
pub struct Band {
    pub kind: i64,
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub level: i64,
    pub precincts: Vec<Precinct>,
}
impl Band {
    pub fn width(&self) -> i64 {
        self.x1.saturating_sub(self.x0)
    }
    pub fn height(&self) -> i64 {
        self.y1.saturating_sub(self.y0)
    }
}
#[derive(Debug, Clone, Default)]
pub struct Precinct {
    pub blocks_wide: i64,
    pub blocks_high: i64,
    pub blocks: Vec<CodeBlock>,
}
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    pub index: i64,
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub bands: Vec<Band>,
    pub ppx: i64,
    pub ppy: i64,
    pub precincts_wide: i64,
    pub precincts_high: i64,
    pub code_block_w: i64,
    pub code_block_h: i64,
}
impl Resolution {
    #[doc(hidden)]
    pub fn precincts(&self) -> i64 {
        self.precincts_wide.wrapping_mul(self.precincts_high)
    }
}
#[derive(Debug, Clone, Default)]
pub struct TileComponent {
    pub tile: i64,
    pub component: i64,
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub resolutions: Vec<Resolution>,
}
impl TileComponent {
    pub fn width(&self) -> i64 {
        self.x1.saturating_sub(self.x0)
    }
    pub fn height(&self) -> i64 {
        self.y1.saturating_sub(self.y0)
    }
}
pub(super) fn shl(a: i64, s: i64) -> i64 {
    if s >= 64 { 0 } else { a.wrapping_shl(s as u32) }
}
fn shr(a: i64, s: i64) -> i64 {
    if s >= 64 {
        if a < 0 { -1 } else { 0 }
    } else {
        a >> s
    }
}
impl Codestream {
    pub(crate) fn tile_bounds(&self, tile: i64) -> (i64, i64, i64, i64) {
        let across = ceil_div_u32(self.width.wrapping_sub(self.tile_x_offset), self.tile_width);
        if across == 0 {
            return (0, 0, 0, 0);
        }
        let p = tile.checked_rem(across).unwrap_or(0);
        let q = tile.checked_div(across).unwrap_or(0);
        let (tw, th) = (self.tile_width as i64, self.tile_height as i64);
        let (tx, ty) = (self.tile_x_offset as i64, self.tile_y_offset as i64);
        let at = |t: i64, n: i64, size: i64| t.saturating_add(n.saturating_mul(size));
        let x0 = at(tx, p, tw).max(self.x_offset as i64);
        let y0 = at(ty, q, th).max(self.y_offset as i64);
        let x1 = at(tx, p.saturating_add(1), tw).min(self.width as i64);
        let y1 = at(ty, q.saturating_add(1), th).min(self.height as i64);
        (x0, y0, x1, y1)
    }
    #[doc(hidden)]
    pub fn tile_component_for(&self, tile: i64, component: i64) -> Result<TileComponent> {
        let mut tc = self.tile_component_geometry(tile, component)?;
        for (r, res) in tc.resolutions.iter_mut().enumerate() {
            let (band_ppx, band_ppy) = band_exponents(r > 0, res.ppx, res.ppy);
            let origin_x = shl(floor_div_pow2(res.x0, res.ppx), band_ppx);
            let origin_y = shl(floor_div_pow2(res.y0, res.ppy), band_ppy);
            let count = res.precincts();
            let wide = res.precincts_wide;
            let (cbw, cbh) = (res.code_block_w, res.code_block_h);
            for band in res.bands.iter_mut() {
                let mut precincts = Vec::with_capacity(count.max(0) as usize);
                for k in 0..count {
                    let px =
                        origin_x.saturating_add(shl(k.checked_rem(wide).unwrap_or(0), band_ppx));
                    let py =
                        origin_y.saturating_add(shl(k.checked_div(wide).unwrap_or(0), band_ppy));
                    precincts.push(precinct_of(
                        band,
                        px,
                        py,
                        px.saturating_add(shl(1, band_ppx)),
                        py.saturating_add(shl(1, band_ppy)),
                        cbw,
                        cbh,
                    ));
                }
                band.precincts = precincts;
            }
        }
        Ok(tc)
    }
    pub(super) fn tile_component_geometry(
        &self,
        tile: i64,
        component: i64,
    ) -> Result<TileComponent> {
        if component < 0 || component >= self.components.len() as i64 {
            return Err(CoreError::Plain(format!(
                "jpeg2000: component {component} of {}",
                self.components.len()
            )));
        }
        if tile < 0 || tile >= self.num_tiles() {
            return Err(CoreError::Plain(format!(
                "jpeg2000: tile {tile} of {}",
                self.num_tiles()
            )));
        }
        let (tx0, ty0, tx1, ty1) = self.tile_bounds(tile);
        let Some(&comp) = self.components.get(component as usize) else {
            return Err(CoreError::Plain(format!(
                "jpeg2000: component {component} of {}",
                self.components.len()
            )));
        };
        let mut tc = TileComponent {
            tile,
            component,
            x0: ceil_div_int(tx0, comp.dx),
            y0: ceil_div_int(ty0, comp.dy),
            x1: ceil_div_int(tx1, comp.dx),
            y1: ceil_div_int(ty1, comp.dy),
            resolutions: Vec::new(),
        };
        let style = self.coding_for_tile(tile, component);
        let levels = style.levels;
        for r in 0..=levels {
            let shift = levels.saturating_sub(r);
            let mut res = Resolution {
                index: r,
                x0: ceil_div_pow2(tc.x0, shift),
                y0: ceil_div_pow2(tc.y0, shift),
                x1: ceil_div_pow2(tc.x1, shift),
                y1: ceil_div_pow2(tc.y1, shift),
                ..Resolution::default()
            };
            (res.ppx, res.ppy) = style.precinct_exponents(r);
            let (xcb, ycb) = (log2(style.code_block_width), log2(style.code_block_height));
            let (band_ppx, band_ppy) = band_exponents(r > 0, res.ppx, res.ppy);
            res.code_block_w = xcb.min(band_ppx);
            res.code_block_h = ycb.min(band_ppy);
            if res.x1 > res.x0 && res.y1 > res.y0 {
                res.precincts_wide =
                    ceil_div_pow2(res.x1, res.ppx).saturating_sub(floor_div_pow2(res.x0, res.ppx));
                res.precincts_high =
                    ceil_div_pow2(res.y1, res.ppy).saturating_sub(floor_div_pow2(res.y0, res.ppy));
            }
            let (count, ok) = mul_int64(&[res.precincts_wide, res.precincts_high]);
            if !ok || count > MAX_PRECINCTS_PER_RESOLUTION {
                return Err(CoreError::Limit {
                    resource: "precincts per resolution",
                    value: count,
                    limit: MAX_PRECINCTS_PER_RESOLUTION,
                });
            }
            res.bands = bands_of(&tc, r, levels);
            tc.resolutions.push(res);
        }
        Ok(tc)
    }
}
fn band_exponents(above_zero: bool, ppx: i64, ppy: i64) -> (i64, i64) {
    if above_zero {
        (ppx.saturating_sub(1), ppy.saturating_sub(1))
    } else {
        (ppx, ppy)
    }
}
fn precinct_of(
    band: &Band,
    px0: i64,
    py0: i64,
    px1: i64,
    py1: i64,
    xcb: i64,
    ycb: i64,
) -> Precinct {
    let (x0, y0) = (band.x0.max(px0), band.y0.max(py0));
    let (x1, y1) = (band.x1.min(px1), band.y1.min(py1));
    if x1 <= x0 || y1 <= y0 {
        return Precinct::default();
    }
    let (w, h) = (shl(1, xcb), shl(1, ycb));
    let mut p = Precinct::default();
    let mut y = floor_to(y0, h);
    while y < y1 {
        p.blocks_high = p.blocks_high.saturating_add(1);
        p.blocks_wide = 0;
        let mut x = floor_to(x0, w);
        while x < x1 {
            p.blocks_wide = p.blocks_wide.saturating_add(1);
            let index = p.blocks.len() as i64;
            p.blocks.push(CodeBlock {
                x0: x.max(x0),
                y0: y.max(y0),
                x1: x.saturating_add(w).min(x1),
                y1: y.saturating_add(h).min(y1),
                index,
            });
            x = x.saturating_add(w);
        }
        y = y.saturating_add(h);
    }
    p
}
fn bands_of(tc: &TileComponent, r: i64, levels: i64) -> Vec<Band> {
    if r == 0 {
        let nb = levels;
        return vec![Band {
            kind: BAND_LL,
            level: nb,
            x0: band_coord(tc.x0, nb, 0),
            y0: band_coord(tc.y0, nb, 0),
            x1: band_coord(tc.x1, nb, 0),
            y1: band_coord(tc.y1, nb, 0),
            precincts: Vec::new(),
        }];
    }
    let nb = levels.saturating_sub(r).saturating_add(1);
    [(BAND_HL, 1, 0), (BAND_LH, 0, 1), (BAND_HH, 1, 1)]
        .into_iter()
        .map(|(kind, xob, yob)| Band {
            kind,
            level: nb,
            x0: band_coord(tc.x0, nb, xob),
            y0: band_coord(tc.y0, nb, yob),
            x1: band_coord(tc.x1, nb, xob),
            y1: band_coord(tc.y1, nb, yob),
            precincts: Vec::new(),
        })
        .collect()
}
fn band_coord(x: i64, nb: i64, o: i64) -> i64 {
    if nb == 0 {
        return x;
    }
    ceil_div_int(
        x.saturating_sub(shl(1, nb.saturating_sub(1)).saturating_mul(o)),
        shl(1, nb),
    )
}
pub(super) fn ceil_div_int(a: i64, b: i64) -> i64 {
    if b == 0 {
        return 0;
    }
    if a >= 0 {
        return a
            .saturating_add(b)
            .saturating_sub(1)
            .checked_div(b)
            .unwrap_or(0);
    }
    a.saturating_neg()
        .checked_div(b)
        .unwrap_or(0)
        .saturating_neg()
}
pub(super) fn ceil_div_pow2(a: i64, shift: i64) -> i64 {
    ceil_div_int(a, shl(1, shift))
}
pub(super) fn floor_div_pow2(a: i64, shift: i64) -> i64 {
    if a >= 0 {
        return shr(a, shift);
    }
    shr(
        a.saturating_neg()
            .saturating_add(shl(1, shift))
            .saturating_sub(1),
        shift,
    )
    .saturating_neg()
}
fn floor_to(x: i64, size: i64) -> i64 {
    if x >= 0 {
        return x.saturating_sub(x.checked_rem(size).unwrap_or(0));
    }
    x.saturating_neg()
        .saturating_add(size)
        .saturating_sub(1)
        .checked_div(size)
        .unwrap_or(0)
        .saturating_neg()
        .saturating_mul(size)
}
pub(super) fn log2(v: i64) -> i64 {
    if v > 1 { i64::from(v.ilog2()) } else { 0 }
}
pub(super) fn mul_int64(values: &[i64]) -> (i64, bool) {
    let mut product = 1i64;
    for &v in values {
        if v < 0 {
            return (i64::MAX, false);
        }
        let Some(next) = product.checked_mul(v) else {
            return (i64::MAX, false);
        };
        product = next;
    }
    (product, true)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_components: i64,
    pub max_tiles: i64,
    pub max_resolutions: i64,
    pub max_samples: i64,
    pub max_tile_work_bytes: i64,
    pub max_code_blocks_per_tile: i64,
}
impl Limits {
    pub fn defaults() -> Limits {
        Limits {
            max_components: 4,
            max_tiles: 4096,
            max_resolutions: 33,
            max_samples: 1 << 28,
            max_tile_work_bytes: 2 << 30,
            max_code_blocks_per_tile: 1 << 20,
        }
    }
    pub(super) fn normalized(mut self) -> Limits {
        let d = Limits::defaults();
        if self.max_components <= 0 {
            self.max_components = d.max_components;
        }
        if self.max_tiles <= 0 {
            self.max_tiles = d.max_tiles;
        }
        if self.max_resolutions <= 0 {
            self.max_resolutions = d.max_resolutions;
        }
        if self.max_samples <= 0 {
            self.max_samples = d.max_samples;
        }
        if self.max_tile_work_bytes <= 0 {
            self.max_tile_work_bytes = d.max_tile_work_bytes;
        }
        if self.max_code_blocks_per_tile <= 0 {
            self.max_code_blocks_per_tile = d.max_code_blocks_per_tile;
        }
        self
    }
    pub(super) fn check(self, c: &Codestream) -> Result<()> {
        let l = self.normalized();
        let limit = |resource: &'static str, value: i64, limit: i64| CoreError::Limit {
            resource,
            value,
            limit,
        };
        let components = c.components.len() as i64;
        if components == 0 || components > l.max_components {
            return Err(limit("components", components, l.max_components));
        }
        let tiles = c.num_tiles();
        if tiles <= 0 || tiles > l.max_tiles {
            return Err(limit("tiles", tiles, l.max_tiles));
        }
        let width = (c.width as i64).saturating_sub(c.x_offset as i64);
        let height = (c.height as i64).saturating_sub(c.y_offset as i64);
        let (samples, ok) = mul_int64(&[width, height, components]);
        if !ok || samples > l.max_samples {
            return Err(limit("output samples", samples, l.max_samples));
        }
        for tile in 0..tiles {
            let (x0, y0, x1, y1) = c.tile_bounds(tile);
            let (area, ok) = mul_int64(&[x1.saturating_sub(x0), y1.saturating_sub(y0)]);
            if !ok {
                return Err(limit("tile area", i64::MAX, l.max_tile_work_bytes));
            }
            let (work, ok) = mul_int64(&[area, components, 8]);
            if !ok || work > l.max_tile_work_bytes {
                return Err(limit("tile work bytes", work, l.max_tile_work_bytes));
            }
            let mut blocks = 0i64;
            for comp in 0..components {
                let levels = c.coding_for_tile(tile, comp).levels;
                let resolutions = levels.saturating_add(1);
                if resolutions > l.max_resolutions {
                    return Err(limit("resolutions", resolutions, l.max_resolutions));
                }
                let tc = c.tile_component_geometry(tile, comp)?;
                for res in &tc.resolutions {
                    for band in &res.bands {
                        if band.width() <= 0 || band.height() <= 0 {
                            continue;
                        }
                        let wide = ceil_div_pow2(band.x1, res.code_block_w)
                            .saturating_sub(floor_div_pow2(band.x0, res.code_block_w));
                        let high = ceil_div_pow2(band.y1, res.code_block_h)
                            .saturating_sub(floor_div_pow2(band.y0, res.code_block_h));
                        let (count, ok) = mul_int64(&[wide, high]);
                        let Some(sum) = blocks.checked_add(count).filter(|_| ok) else {
                            return Err(limit(
                                "code-blocks per tile",
                                i64::MAX,
                                l.max_code_blocks_per_tile,
                            ));
                        };
                        blocks = sum;
                        if blocks > l.max_code_blocks_per_tile {
                            return Err(limit(
                                "code-blocks per tile",
                                blocks,
                                l.max_code_blocks_per_tile,
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
