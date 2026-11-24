#![forbid(unsafe_code)]
#![allow(clippy::neg_cmp_op_on_partial_ord)]
#![allow(
    clippy::needless_range_loop,
    clippy::manual_range_contains,
    clippy::manual_div_ceil,
    clippy::int_plus_one,
    clippy::result_large_err
)]
pub mod canny;
pub mod clahe;
pub mod deconvolution;
pub mod edge_preserving;
pub mod histogram_matching;
pub mod hough;
pub mod local_chain;
pub mod masked_rank;
pub mod median;
pub mod n4;
pub mod nlm;
pub mod phase2d;
pub mod poisson_denoising;
pub mod recursive_gaussian;
pub mod recursive_gaussian_plan;
pub mod regional;
pub mod slab;
pub mod tensor_diffusion;
pub mod vesselness;
pub mod volume;
pub use canny::{
    CannyLimits, CannyOptions, CannyParameters, CannyResult, canny, canny_gaussian_scratch_bytes,
    canny_working_bytes, default_canny_parameters,
};
pub use clahe::{
    CLAHEOptions, DEFAULT_CLAHE_BINS, DEFAULT_CLAHE_BIT_DEPTH, DEFAULT_CLAHE_CLIP_LIMIT,
    DEFAULT_CLAHE_TILES, clahe, clahe_gray, clahe_gray16, clahe_in_place,
};
pub use deconvolution::{
    PSF_CIRCULAR_BOUNDARY, PSF_CLAMP_NEGATIVE, PSF_NORMALIZE_MASS, PSF_REJECT_NEGATIVE,
    PSF_REQUIRE_UNIT_MASS, PSF_ZERO_BOUNDARY, PSFDeconvolutionLimits, PSFDeconvolutionOptions,
    PSFDeconvolutionResult, PSFDerivedMetadata, convolve_psf, richardson_lucy,
};
pub use edge_preserving::{
    AnisotropicDiffusionOptions, BilateralOptions, UnsharpMaskOptions, bilateral,
    curvature_anisotropic_diffusion, gradient_anisotropic_diffusion, unsharp_mask,
};
pub use histogram_matching::{
    DEFAULT_HISTOGRAM_MATCH_LEVELS, DEFAULT_HISTOGRAM_MATCH_POINTS, HISTOGRAM_MATCH_NONE,
    HistogramLandmark, HistogramMatch, HistogramMatchOptions, HistogramOptions, IntensityHistogram,
    MAX_HISTOGRAM_MATCH_LEVELS, MAX_HISTOGRAM_MATCH_POINTS, match_histograms, new_histogram_match,
    new_histogram_match_from_samples, new_intensity_histogram,
};
pub use hough::{
    Hough2DOptions, HoughCircle, HoughCircleOptions, HoughCircleResult, HoughLimits, HoughLine,
    HoughLineOptions, HoughLineResult, HoughReport, Rect, hough_circles_2d, hough_lines_2d,
};
pub use local_chain::{
    LocalChain, LocalChainIdentity, LocalChainOptions, LocalChainPlan, LocalChainResult,
    LocalChainSource, LocalChainStagePlan, LocalChainStats, new_local_chain,
};
pub use masked_rank::{
    ANY_VALID_NEIGHBORHOOD, CLIP_BOUNDARY, EXCLUDE_NON_FINITE, MaskedRankOptions, MaskedRankResult,
    PHYSICAL_BOX, PHYSICAL_ELLIPSOID, REJECT_NON_FINITE, REPLICATE_BOUNDARY, REQUIRE_VALID_CENTER,
    RankLimits, RankRegion, default_masked_rank_options, masked_rank, masked_rank_region,
};
pub use median::{MedianOptions, median, median_radius_samples, median_radius_samples_partial};
pub use n4::{N4Level, N4Options, N4Progress, N4Result, n4_bias_field_correction};
pub use nlm::{NonLocalMeansOptions, non_local_means};
pub use phase2d::{
    PHASE2D_BOTH, PHASE2D_BRIGHT, PHASE2D_DARK, Phase2DLimits, PhaseSymmetry2DOptions,
    PhaseSymmetry2DResult, WAVELET_PERIODIC, WAVELET_REFLECT, WAVELET2D_FAMILY, Wavelet2DMetadata,
    Wavelet2DOptions, Wavelet2DResult, decompose_wavelet_2d, phase_symmetry_2d,
    reconstruct_wavelet_2d,
};
pub use poisson_denoising::{
    PoissonDenoisingLimits, PoissonDenoisingMetadata, PoissonDenoisingOptions,
    PoissonDenoisingResult, denoise_poisson,
};
pub use recursive_gaussian::{
    Complex128, Hessian, RecursiveGaussianOptions, gradient_magnitude_recursive_gaussian,
    gradient_recursive_gaussian, hessian_recursive_gaussian, isotropic_sigma,
    laplacian_recursive_gaussian, recursive_gaussian, smooth_recursive_gaussian,
};
pub use recursive_gaussian_plan::{
    RecursiveGaussianKernel, RecursiveGaussianPole, VESSELNESS_SLAB_MARGIN_SIGMAS,
    plan_recursive_gaussian,
};
pub use regional::{
    Region, RegionReader, RegionWriter, RegionalFilter, RegionalIo, RegionalOptions,
    RegionalResult, filter_regions, plan_regions,
};
pub use slab::{
    DEFAULT_SLAB_WORKING_BYTES, SlabFilter, SlabKernel, SlabOptions, SlabResult, SlabView,
    SliceReader, SliceWriter, filter_slabs,
};
pub use tensor_diffusion::{
    COHERENCE_ENHANCING, EDGE_ENHANCING, TensorDiffusionLimits, TensorDiffusionMetadata,
    TensorDiffusionOptions, TensorDiffusionResult, tensor_anisotropic_diffusion,
};
pub use vesselness::{
    DEFAULT_VESSELNESS_SIGMA_STEPS, DEFAULT_VESSELNESS_WORKING_BYTES, HessianStructure,
    VesselnessMeasure, VesselnessOptions, VesselnessResult, plan_vesselness_slab_slices,
    vesselness, vesselness_sigmas,
};
pub use volume::{
    Ctx, ErrorKind, FilterError, ModalityF32Writer, ModalitySink, Volume, volume_from_mask,
    volume_from_modality_f32,
};
