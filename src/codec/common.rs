#![forbid(unsafe_code)]
#![cfg_attr(
    not(any(
        feature = "jpeg",
        feature = "jpeg-lossless",
        feature = "jpeg-ls",
        feature = "jpeg2000"
    )),
    allow(dead_code)
)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub rows: u16,
    pub columns: u16,
    pub samples_per_pixel: u16,
    pub bits_allocated: u16,
    pub bits_stored: u16,
    pub high_bit: u16,
    pub pixel_representation: u16,
    pub planar_configuration: u16,
    pub planar_configuration_present: bool,
    pub number_of_frames: i64,
    pub photometric_interpretation: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodedFrames {
    pub rows: usize,
    pub columns: usize,
    pub data: Vec<Vec<u8>>,
    pub photometric_interpretation: String,
    pub diagnostics: Vec<&'static str>,
}
impl DecodedFrames {
    #[cfg_attr(not(feature = "jpeg-lossless"), allow(dead_code))]
    pub(crate) fn add_diagnostic(&mut self, code: &'static str) {
        if !self.diagnostics.contains(&code) {
            self.diagnostics.push(code);
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorConversion {
    #[default]
    Guess,
    Photometric,
    Always,
    Never,
}
