#![forbid(unsafe_code)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::neg_cmp_op_on_partial_ord)]
mod gradient_descent;
mod lbfgs;
mod lbfgsb;
mod nelder_mead;
mod one_plus_one;
mod powell;
mod search;
pub use gradient_descent::{
    RegularStepGradientDescent, StepScaleEstimator, WindowConvergenceMonitor,
};
pub use lbfgs::Lbfgs;
pub use lbfgsb::{Lbfgsb, LbfgsbLimits};
pub use nelder_mead::NelderMead;
pub use one_plus_one::OnePlusOne;
pub use powell::Powell;
pub use search::{Exhaustive, LocalMinimizer, MultiStart, SearchFunction, SearchLimits};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
pub trait Function {
    fn value(&mut self, x: &[f64]) -> Result<f64, Error>;
    fn as_gradient(&mut self) -> Option<&mut dyn GradientFunction> {
        None
    }
}
pub trait GradientFunction: Function {
    fn value_and_gradient(&mut self, x: &[f64]) -> Result<(f64, Vec<f64>), Error>;
}
pub struct Func<F>(pub F);
impl<F: FnMut(&[f64]) -> Result<f64, Error>> Function for Func<F> {
    fn value(&mut self, x: &[f64]) -> Result<f64, Error> {
        (self.0)(x)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    MaximumIterations,
    MinimumStepLength,
    GradientMagnitude,
    ConvergenceWindow,
    ParameterTolerance,
    ValueTolerance,
    Radius,
    LineSearch,
    MaximumEvaluations,
    ProjectedGradient,
    ExhaustiveComplete,
    MultiStartComplete,
}
impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::MaximumIterations => "maximum-iterations",
            StopReason::MinimumStepLength => "minimum-step-length",
            StopReason::GradientMagnitude => "gradient-magnitude",
            StopReason::ConvergenceWindow => "convergence-window",
            StopReason::ParameterTolerance => "parameter-tolerance",
            StopReason::ValueTolerance => "value-tolerance",
            StopReason::Radius => "radius",
            StopReason::LineSearch => "line-search",
            StopReason::MaximumEvaluations => "maximum-evaluations",
            StopReason::ProjectedGradient => "projected-gradient",
            StopReason::ExhaustiveComplete => "exhaustive-complete",
            StopReason::MultiStartComplete => "multi-start-complete",
        }
    }
}
impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct OptimizeResult {
    pub x: Vec<f64>,
    pub value: f64,
    pub iterations: i64,
    pub evaluations: i64,
    pub stop: StopReason,
}
#[derive(Debug)]
pub enum Error {
    Options(String),
    SearchBudget,
    NoFiniteCandidate,
    LbfgsbResourceLimit,
    LbfgsbModel,
    Canceled,
    Callback(Box<dyn std::error::Error + Send + Sync>),
}
impl Error {
    pub fn callback(message: impl Into<String>) -> Self {
        Error::Callback(message.into().into())
    }
    pub fn is_options(&self) -> bool {
        matches!(self, Error::Options(_))
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Options(detail) => write!(f, "optimize: invalid options: {detail}"),
            Error::SearchBudget => f.write_str("optimize: search budget exceeded"),
            Error::NoFiniteCandidate => f.write_str("optimize: no finite search candidate"),
            Error::LbfgsbResourceLimit => f.write_str("optimize: L-BFGS-B resource limit"),
            Error::LbfgsbModel => f.write_str("optimize: unusable L-BFGS-B numerical model"),
            Error::Canceled => f.write_str("context canceled"),
            Error::Callback(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for Error {}
fn options(detail: impl Into<String>) -> Error {
    Error::Options(detail.into())
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Context<'a> {
    cancel: Option<&'a AtomicBool>,
}
impl<'a> Context<'a> {
    pub fn background() -> Self {
        Context { cancel: None }
    }
    pub fn with_cancel(flag: &'a AtomicBool) -> Self {
        Context { cancel: Some(flag) }
    }
    pub fn err(&self) -> Result<(), Error> {
        match self.cancel {
            Some(flag) if flag.load(Ordering::Relaxed) => Err(Error::Canceled),
            _ => Ok(()),
        }
    }
    fn tick(&self, i: usize) -> Result<(), Error> {
        if i & 1023 == 0 { self.err() } else { Ok(()) }
    }
}
const DEFAULT_VALUE_TOLERANCE: f64 = 2.220446049250313e-9;
fn scales_for(scales: Option<&[f64]>, n: usize) -> Result<Vec<f64>, Error> {
    let Some(scales) = scales else {
        return Ok(vec![1.0; n]);
    };
    if scales.len() != n {
        return Err(options(format!(
            "{} scales for {n} parameters",
            scales.len()
        )));
    }
    for &s in scales {
        if !(s > 0.0) || s.is_infinite() {
            return Err(options(format!("scale {}", crate::textfmt::float_v(s))));
        }
    }
    Ok(scales.to_vec())
}
struct Counter<'f> {
    f: &'f mut dyn Function,
    count: i64,
}
impl Counter<'_> {
    fn value(&mut self, x: &[f64]) -> Result<f64, Error> {
        self.count += 1;
        self.f.value(x)
    }
}
fn default_to(v: f64, fallback: f64) -> f64 {
    if v > 0.0 { v } else { fallback }
}
fn default_to_int(v: i64, fallback: i64) -> i64 {
    if v > 0 { v } else { fallback }
}
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut s = 0.0;
    for i in 0..a.len() {
        s += a[i] * b[i];
    }
    s
}
fn axpy(alpha: f64, x: &[f64], y: &mut [f64]) {
    for i in 0..y.len() {
        y[i] += alpha * x[i];
    }
}
fn max_abs(v: &[f64]) -> f64 {
    let mut m = 0.0;
    for &x in v {
        m = crate::fmath::max(m, x.abs());
    }
    m
}
fn sq(v: f64) -> f64 {
    v * v
}
