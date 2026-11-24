#![forbid(unsafe_code)]
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_element_bytes: u64,
    pub max_pixel_data_bytes: u64,
    pub max_data_set_bytes: u64,
    pub max_inflated_bytes: u64,
    pub max_sequence_depth: u32,
    pub max_elements: u64,
    pub max_items: u64,
    pub max_fragments: u64,
    pub max_rows: u32,
    pub max_columns: u32,
    pub max_frames: u64,
    pub max_frame_bytes: u64,
    pub max_pdu_length: u32,
    pub max_association_pdu_length: u32,
    pub max_associations: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitExceeded {
    pub what: &'static str,
    pub got: u64,
    pub limit: u64,
}
impl fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "dicom: resource limit exceeded: {} is {}, limit {}",
            self.what, self.got, self.limit
        )
    }
}
impl std::error::Error for LimitExceeded {}
fn check(what: &'static str, got: u64, limit: u64) -> Result<(), LimitExceeded> {
    if limit != 0 && got > limit {
        return Err(LimitExceeded { what, got, limit });
    }
    Ok(())
}
fn as_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}
impl Limits {
    pub const DEFAULT: Limits = Limits {
        max_element_bytes: 0,
        max_pixel_data_bytes: 0,
        max_data_set_bytes: 0,
        max_inflated_bytes: 2 << 30,
        max_sequence_depth: 128,
        max_elements: 50_000_000,
        max_items: 10_000_000,
        max_fragments: 10_000_000,
        max_rows: 65_535,
        max_columns: 65_535,
        max_frames: 100_000,
        max_frame_bytes: 1 << 30,
        max_pdu_length: crate::net::ul::DEFAULT_MAX_PDU,
        max_association_pdu_length: crate::net::ul::DEFAULT_MAX_ASSOCIATION_PDU,
        max_associations: 64,
    };
    pub const NONE: Limits = Limits {
        max_element_bytes: 0,
        max_pixel_data_bytes: 0,
        max_data_set_bytes: 0,
        max_inflated_bytes: 0,
        max_sequence_depth: 0,
        max_elements: 0,
        max_items: 0,
        max_fragments: 0,
        max_rows: 0,
        max_columns: 0,
        max_frames: 0,
        max_frame_bytes: 0,
        max_pdu_length: 0,
        max_association_pdu_length: 0,
        max_associations: 0,
    };
    pub fn check_image(
        &self,
        rows: u64,
        columns: u64,
        frames: u64,
        frame_bytes: u64,
    ) -> Result<(), LimitExceeded> {
        check("rows", rows, u64::from(self.max_rows))?;
        check("columns", columns, u64::from(self.max_columns))?;
        check("frames", frames, self.max_frames)?;
        check(
            "decompressed frame bytes",
            frame_bytes,
            self.max_frame_bytes,
        )
    }
    pub fn decompress_limits(&self) -> crate::pixel::transcode::decompress::DecompressLimits {
        let mut l = crate::pixel::transcode::decompress::default_decompress_limits();
        if self.max_frames != 0 {
            l.max_frames = as_i64(self.max_frames);
        }
        if self.max_frame_bytes != 0 {
            l.max_pixels = l.max_pixels.min(as_i64(self.max_frame_bytes));
        }
        if self.max_pixel_data_bytes != 0 {
            l.max_input_bytes = l.max_input_bytes.min(as_i64(self.max_pixel_data_bytes));
        }
        l
    }
    pub fn guard_limits(&self) -> crate::pixel::guard::GuardLimits {
        let d = crate::pixel::guard::GuardLimits::DEFAULT;
        let pick = |v: u64, d: i64| if v == 0 { d } else { as_i64(v) };
        crate::pixel::guard::GuardLimits {
            max_side: pick(u64::from(self.max_rows.max(self.max_columns)), d.max_side),
            max_samples: pick(self.max_frame_bytes, d.max_samples),
            max_frames: pick(self.max_frames, d.max_frames),
            max_frame_bytes: pick(self.max_frame_bytes, d.max_frame_bytes),
            max_series_bytes: d.max_series_bytes,
        }
    }
    #[cfg(feature = "net")]
    pub fn apply_to_accept(&self, accept: &mut crate::net::ul::AcceptOptions) {
        if self.max_pdu_length != 0 {
            accept.max_pdu = self.max_pdu_length;
        }
        if self.max_association_pdu_length != 0 {
            accept.max_association_pdu = self.max_association_pdu_length;
        }
    }
    #[cfg(feature = "net")]
    pub fn apply_to_server(&self, server: &mut crate::net::ul::AssociationServerOptions) {
        self.apply_to_accept(&mut server.accept);
        if self.max_associations != 0 {
            server.max_concurrent_associations = i64::from(self.max_associations);
        }
    }
    pub(crate) fn effective(option: i64, limit: u64) -> i64 {
        if option != 0 { option } else { as_i64(limit) }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn none_is_all_zero_and_default_is_not() {
        assert_eq!(
            Limits::NONE.check_image(u64::MAX, u64::MAX, u64::MAX, u64::MAX),
            Ok(())
        );
        let d = Limits::default();
        assert!(d.max_sequence_depth > 0 && d.max_frames > 0 && d.max_associations > 0);
    }
    #[test]
    fn check_image_names_the_exceeded_limit() {
        let l = Limits {
            max_rows: 100,
            max_columns: 200,
            max_frames: 3,
            max_frame_bytes: 1000,
            ..Limits::NONE
        };
        assert_eq!(l.check_image(100, 200, 3, 1000), Ok(()));
        assert_eq!(l.check_image(101, 1, 1, 1).unwrap_err().what, "rows");
        assert_eq!(l.check_image(1, 201, 1, 1).unwrap_err().what, "columns");
        assert_eq!(l.check_image(1, 1, 4, 1).unwrap_err().what, "frames");
        let e = l.check_image(1, 1, 1, 1001).unwrap_err();
        assert_eq!((e.got, e.limit), (1001, 1000));
        assert!(e.to_string().contains("limit 1000"));
    }
    #[test]
    fn derived_limits_follow() {
        let l = Limits {
            max_frames: 7,
            max_frame_bytes: 4096,
            max_rows: 64,
            max_columns: 32,
            ..Limits::default()
        };
        let d = l.decompress_limits();
        assert_eq!((d.max_frames, d.max_pixels), (7, 4096));
        let g = l.guard_limits();
        assert_eq!((g.max_side, g.max_frames, g.max_frame_bytes), (64, 7, 4096));
    }
}
