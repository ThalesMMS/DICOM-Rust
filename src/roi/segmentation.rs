use super::roimask::RasterMask;
use crate::render::VolumeGeometry;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Default)]
pub struct Segmentation3D {
    pub geometry: VolumeGeometry,
    pub columns: i64,
    pub rows: i64,
    pub(crate) masks: BTreeMap<i64, RasterMask>,
}
impl Segmentation3D {
    pub fn new(geometry: VolumeGeometry, columns: i64, rows: i64) -> Segmentation3D {
        Segmentation3D {
            geometry,
            columns,
            rows,
            masks: BTreeMap::new(),
        }
    }
    pub fn set_mask(&mut self, slice: i64, mask: Option<RasterMask>) {
        match mask {
            Some(m) if !m.is_empty() => {
                self.masks.insert(slice, m);
            }
            _ => {
                self.masks.remove(&slice);
            }
        }
    }
    pub fn mask(&mut self, slice: i64) -> &mut RasterMask {
        let (c, r) = (self.columns, self.rows);
        self.masks
            .entry(slice)
            .or_insert_with(|| RasterMask::new(c, r))
    }
    pub fn mask_at(&self, slice: i64) -> Option<&RasterMask> {
        self.masks.get(&slice)
    }
    #[doc(hidden)]
    pub fn mask_at_mut(&mut self, slice: i64) -> Option<&mut RasterMask> {
        self.masks.get_mut(&slice)
    }
    pub fn set_voxel(&mut self, x: i64, y: i64, slice: i64, on: bool) {
        self.mask(slice).set(x, y, on);
    }
    pub fn voxel(&self, x: i64, y: i64, slice: i64) -> bool {
        self.masks.get(&slice).is_some_and(|m| m.get(x, y))
    }
    pub fn voxel_count(&self) -> i64 {
        let mut n = 0i64;
        for m in self.masks.values() {
            n += m.count();
        }
        n
    }
    pub fn slices(&self) -> Vec<i64> {
        self.masks
            .iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|(&z, _)| z)
            .collect()
    }
    pub fn prune(&mut self) {
        self.masks.retain(|_, m| !m.is_empty());
    }
    #[doc(hidden)]
    pub fn tracked_slices(&self) -> Vec<i64> {
        self.masks.keys().copied().collect()
    }
}
