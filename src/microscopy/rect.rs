use std::fmt;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}
impl Rect {
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
        Rect {
            min_x: x0.min(x1),
            min_y: y0.min(y1),
            max_x: x0.max(x1),
            max_y: y0.max(y1),
        }
    }
    pub fn dx(&self) -> i64 {
        self.max_x.wrapping_sub(self.min_x)
    }
    pub fn dy(&self) -> i64 {
        self.max_y.wrapping_sub(self.min_y)
    }
    pub fn is_empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    pub fn intersect(&self, s: Rect) -> Rect {
        let r = Rect {
            min_x: self.min_x.max(s.min_x),
            min_y: self.min_y.max(s.min_y),
            max_x: self.max_x.min(s.max_x),
            max_y: self.max_y.min(s.max_y),
        };
        if r.is_empty() { Rect::default() } else { r }
    }
    pub fn overlaps(&self, s: Rect) -> bool {
        !self.is_empty()
            && !s.is_empty()
            && self.min_x < s.max_x
            && s.min_x < self.max_x
            && self.min_y < s.max_y
            && s.min_y < self.max_y
    }
    pub(super) fn center(&self) -> (i64, i64) {
        (
            self.min_x.wrapping_add(self.max_x) / 2,
            self.min_y.wrapping_add(self.max_y) / 2,
        )
    }
}
impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({},{})-({},{})",
            self.min_x, self.min_y, self.max_x, self.max_y
        )
    }
}
