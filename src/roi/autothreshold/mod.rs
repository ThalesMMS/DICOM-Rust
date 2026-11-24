pub mod histogram;
pub mod methods;
pub use histogram::{
    DEFAULT_BINS, ERR_NO_SAMPLES, ERR_NOT_BIMODAL, ERR_SINGLE_VALUE, ERR_TOO_MANY_BINS, Histogram,
    HistogramOptions, MAX_BINS, Values, grid_values, mask_values, new_histogram,
    new_histogram_from_counts, segmentation_values, slice_values,
};
pub use methods::{Method, Threshold, class_of, compute, methods, parse_method};
