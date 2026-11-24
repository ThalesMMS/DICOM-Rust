#![forbid(unsafe_code)]
pub(crate) mod common;
#[cfg(feature = "jpeg")]
pub mod jpeg;
#[cfg(feature = "jpeg2000")]
pub mod jpeg2000;
#[cfg(feature = "jpeg-lossless")]
pub mod jpeglossless;
#[cfg(feature = "jpeg-ls")]
pub mod jpegls;
#[cfg(feature = "jpeg-xl")]
pub mod jpegxl;
pub mod rle;
