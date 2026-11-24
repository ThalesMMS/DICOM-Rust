#![forbid(unsafe_code)]
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop)]
mod affine;
mod bspline;
mod coarse;
mod composite;
mod composite_metric;
mod demons;
mod dicom;
mod displacement;
mod error;
mod euler;
mod ffd;
pub mod hdf5;
mod image;
mod image_component;
mod itkfile;
mod itkhdf5;
mod landmark;
mod landmark_warp;
mod masked_fft;
mod matrix;
mod mattes;
mod method;
mod metric;
mod mincxfm;
mod motion;
mod neighborhood;
mod pointset;
mod principal;
mod resample;
mod scale_versor;
mod scales;
mod similarity;
mod syn;
mod transform;
mod transform_2d;
mod translation;
mod versor;
mod versor_rigid;
pub use affine::Affine;
pub use bspline::{
    BSPLINE_FIXED_PARAMETERS, BSpline, BSplineJacobianBound, transform_displacement_field,
};
pub use coarse::{CoarseSearchOptions, coarse_search_minimizer};
pub use composite::Composite;
pub use composite_metric::{
    CompositeMetric, MetricComponent, MetricContribution, MultiMetricResult, WeightedMetric,
};
pub use demons::{DemonsOptions, DemonsProgress, DemonsResult, demons};
pub use dicom::{
    DICOM_RIGID_TOLERANCE, dicom_forward_matrix, dicom_matrix, from_affine_registration,
    from_dicom_matrix,
};
pub use displacement::{
    DISPLACEMENT_FIELD_FIXED_PARAMETERS, DisplacementField, DisplacementFieldTransform,
};
pub use error::{RegError, Sentinel};
pub use euler::{Euler3D, EulerOrder};
pub use ffd::{
    FfdBoundedOptions, FfdJacobianBound, FfdLevel, FfdResult, FreeFormDeformation,
    default_ffd_levels,
};
pub use image::{Image, image_from_snapshot};
pub use image_component::{
    ImageComponentOptions, ImageMetricComponent, ImageMetricKind, MetricLimits,
};
pub use itkfile::read_itk_hdf5_2d;
pub use itkfile::{
    FileTransform2D, ITK_TEXT_HEADER, TransformFileReadOptions, read_itk_matlab,
    read_itk_matlab_2d, read_itk_text, read_itk_text_2d, read_transform_file,
    read_transform_file_2d, read_transform_file_context, write_itk_matlab, write_itk_matlab_2d,
    write_itk_text, write_itk_text_2d,
};
pub use itkhdf5::{
    DEFAULT_ITK_HDF5_MAX_BYTES, ItkHdf5WriteOptions, read_itk_hdf5, write_itk_hdf5,
    write_itk_hdf5_2d, write_itk_hdf5_2d_context, write_itk_hdf5_context,
};
pub use landmark::{
    IcpOptions, IcpResult, IcpStopReason, LandmarkMode, LandmarkResult, iterative_closest_point,
    register_landmarks,
};
pub use landmark_warp::{
    LandmarkWarp3D, LandmarkWarpDiagnostics, LandmarkWarpOptions, LandmarkWarpResult,
    LandmarkWarpSample, fit_landmark_warp_3d,
};
pub use masked_fft::{
    MaskedFftOptions, MaskedFftResult, MaskedFftSearch, estimate_masked_fft_translation,
    masked_fft_workspace,
};
pub use matrix::{Jacobian, Matrix3};
pub use mattes::{DEFAULT_MATTES_BINS, MattesMutualInformation};
pub use method::{
    CenteringMode, Level, LevelMinimizer, LevelResult, Method, MethodResult, NewMetric, Progress,
    Sampling, center_transform, default_levels, validate_sampling_percentage,
};
pub use metric::{Correlation, MeanSquares, Metric, MetricDomain, Objective};
pub use motion::{
    DEFAULT_MOTION_ITERATIONS, DEFAULT_MOTION_POINTS, DEFAULT_MOTION_SAMPLES, MotionMetric,
    MotionOptions, MotionProgress, MotionSink, MotionSource, MotionStage, PhaseCorrection,
    PhaseMotion, correct_motion,
};
pub use neighborhood::{NeighborhoodCorrelation, NeighborhoodCorrelationOptions};
pub use pointset::{GaussianPointSetMetric, GaussianPointSetOptions, PointSetMetric};
pub use principal::{
    PrincipalAxisOptions, PrincipalMoments, image_principal_moments, principal_axis_candidates,
};
pub use resample::{
    Grid, Interpolator, LABEL_GAUSSIAN_ALPHA, Mapping, ResampleOptions, Resampled, SliceRange,
    compose_mappings, field_mapping, resample, resample_affine, subtract, transform_mapping,
};
pub use scale_versor::ScaleVersor3D;
pub use scales::{PhysicalShiftScales, corner_points, maximum_step_size};
pub use similarity::Similarity3D;
pub use syn::{SyNLevel, SyNOptions, SyNProgress, SyNResult, syn};
pub use transform::{
    DEFAULT_ORTHOGONALITY_TOLERANCE, Error, ErrorKind, Kind, MatrixTransform, Transform,
};
pub use transform_2d::{
    Composite2D, Jacobian2D, Linear2D, Matrix2, MatrixTransform2D, Point2, Transform2D,
};
pub use translation::Translation3D;
pub use versor::Versor;
pub use versor_rigid::VersorRigid3D;
pub(crate) fn local_lookup() -> Option<mattes::LocalLookup> {
    Some(bspline::local_of)
}
