#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
pub mod attrpath;
pub mod charset;
#[cfg(feature = "render")]
pub mod clinicalrelation;
pub mod clinicalresult;
pub mod codec;
#[cfg(feature = "codes")]
pub mod codes;
pub mod datasettime;
pub mod dcmtime;
#[doc(hidden)]
pub mod deflater;
pub mod deid;
pub mod derivation;
#[doc(hidden)]
pub mod derivedio;
#[doc(hidden)]
pub mod dicomcolor;
pub mod dicomdir;
pub mod dicominspect;
pub mod dicomjson;
pub mod dicomxml;
pub mod dictionary;
pub mod display;
pub mod dsa;
#[cfg(feature = "dsig")]
pub mod dsig;
#[cfg(feature = "render")]
pub mod dynamic;
pub mod encapdoc;
pub mod encapimage;
#[cfg(feature = "render")]
pub mod fitmap;
#[doc(hidden)]
pub mod fmath;
pub mod gsps;
pub mod hangingprotocol;
pub mod hl7;
#[cfg(feature = "net")]
pub mod ian;
pub mod icc;
pub mod iconimage;
pub mod imagebuild;
#[cfg(feature = "render")]
pub mod imagefilter;
pub mod index;
#[cfg(feature = "dicomweb")]
pub mod jpip;
pub mod limits;
#[cfg(feature = "render")]
pub mod mesh;
mod message_error;
#[cfg(feature = "render")]
pub mod metaimage;
#[cfg(all(feature = "render", feature = "dicomweb"))]
pub mod microscopy;
#[cfg(feature = "net")]
pub mod mpps;
#[cfg(feature = "render")]
pub mod mrdiffusion;
pub mod multiframe;
pub mod net;
#[cfg(feature = "render")]
pub mod nifti;
#[cfg(feature = "render")]
pub mod nrrd;
#[doc(hidden)]
#[allow(clippy::needless_range_loop)]
pub mod numeric;
#[doc(hidden)]
pub mod numtext;
pub mod object;
pub mod optimize;
pub mod papyrus;
#[cfg(feature = "render")]
pub mod parametricmap;
pub mod pathtemplate;
pub mod personname;
pub mod pet;
pub mod pixel;
#[cfg(feature = "net")]
pub mod printmgmt;
#[doc(hidden)]
pub mod provenance;
pub mod qrmatch;
pub mod realworld;
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub mod recordstore;
#[cfg(feature = "render")]
pub mod registration;
#[cfg(feature = "render")]
pub mod render;
#[cfg(feature = "render")]
pub mod roi;
#[cfg(feature = "render")]
pub mod rtdose;
pub mod rtplan;
#[cfg(feature = "render")]
pub mod rtstruct;
#[cfg(feature = "render")]
pub mod seg;
pub mod seriesderive;
pub mod siemens;
#[cfg(feature = "render")]
pub mod spatialreg;
pub mod spectroscopy;
#[cfg(feature = "codes")]
pub mod sr;
#[cfg(feature = "render")]
pub mod surfaceseg;
#[doc(hidden)]
pub mod textfmt;
#[cfg(feature = "render")]
pub mod tractography;
pub mod transfer;
#[cfg(feature = "codes")]
pub mod ultrasound;
#[cfg(feature = "dicomweb")]
pub mod ups;
pub mod validation;
#[doc(hidden)]
pub mod valueencode;
#[cfg(feature = "render")]
pub mod vendors;
pub mod video;
#[cfg(feature = "render")]
pub mod vps;
pub mod waveform;
pub use message_error::MessageError;
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
#[cfg(all(doctest, feature = "net", feature = "render"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
