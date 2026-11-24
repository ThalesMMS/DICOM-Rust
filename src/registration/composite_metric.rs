use super::error::{RegError, Sentinel};
use super::image_component::{ComponentInfo, ImageMetricComponent, MetricLimits, multi};
use super::matrix::finite_slice;
use super::metric::Metric;
use super::pointset::{GaussianPointSetMetric, PointSetMetric};
use super::transform::Transform;
use crate::render::parallel::{CancelToken, Cancellation, ctx_err};
use std::collections::HashSet;
#[derive(Clone, Copy)]
pub enum MetricComponent<'a> {
    Image(&'a ImageMetricComponent),
    PointSet(&'a PointSetMetric),
    GaussianPointSet(&'a GaussianPointSetMetric),
}
impl MetricComponent<'_> {
    fn info(&self) -> &ComponentInfo {
        match self {
            MetricComponent::Image(m) => &m.info,
            MetricComponent::PointSet(m) => &m.info,
            MetricComponent::GaussianPointSet(m) => &m.info,
        }
    }
    fn evaluate(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
        derivatives: bool,
    ) -> Result<(f64, Vec<f64>, i64), (RegError, i64)> {
        match self {
            MetricComponent::Image(m) => m.evaluate_component(ctx, t, derivatives),
            MetricComponent::PointSet(m) => m.evaluate_component(ctx, t, derivatives),
            MetricComponent::GaussianPointSet(m) => m.evaluate_component(ctx, t, derivatives),
        }
    }
}
#[derive(Clone)]
pub struct WeightedMetric<'a> {
    pub label: String,
    pub metric: Option<MetricComponent<'a>>,
    pub weight: f64,
    pub scale: f64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MetricContribution {
    pub label: String,
    pub frame_of_reference: String,
    pub units: String,
    pub weight: f64,
    pub scale: f64,
    pub value: f64,
    pub weighted_value: f64,
    pub points: i64,
    pub overlap: i64,
    pub excluded: bool,
    pub error: Option<RegError>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MultiMetricResult {
    pub value: f64,
    pub derivative: Option<Vec<f64>>,
    pub contributions: Vec<MetricContribution>,
}
pub struct CompositeMetric<'a> {
    components: Vec<WeightedMetric<'a>>,
    prototype: Box<dyn Transform>,
    _limits: MetricLimits,
}
impl<'a> CompositeMetric<'a> {
    pub fn new(
        prototype: Option<&dyn Transform>,
        components: &[WeightedMetric<'a>],
        limits: MetricLimits,
    ) -> Result<CompositeMetric<'a>, RegError> {
        let l = limits.checked()?;
        let Some(prototype) = prototype else {
            return Err(multi());
        };
        let np = prototype.number_of_parameters();
        let params = prototype.parameters();
        let fixed = prototype.fixed_parameters();
        if np < 1
            || np as i64 > l.parameters
            || components.is_empty()
            || components.len() as i64 > l.components
            || fixed.len() > 4096
            || params.len() != np
            || !finite_slice(&params)
            || !finite_slice(&fixed)
        {
            return Err(multi());
        }
        let mut used =
            8192u64 + components.len() as u64 * 2048 + np as u64 * 128 + fixed.len() as u64 * 8;
        if used > l.bytes {
            return Err(multi());
        }
        let (mut points, mut channels) = (0i64, 0i64);
        let mut frame = String::new();
        let mut labels = HashSet::new();
        let mut active = 0;
        for c in components {
            if c.label.is_empty()
                || c.label.len() > 1024
                || labels.contains(&c.label)
                || !finite_slice(&[c.weight, c.scale])
                || c.weight < 0.0
                || c.scale < 0.0
            {
                return Err(multi());
            }
            labels.insert(c.label.clone());
            if c.weight == 0.0 {
                continue;
            }
            let Some(m) = &c.metric else {
                return Err(multi());
            };
            let d = m.info();
            if d.frame.is_empty() || d.points < 1 || d.channels < 0 || np as i64 > d.parameters {
                return Err(multi());
            }
            if frame.is_empty() {
                frame = d.frame.clone();
            }
            if frame != d.frame {
                return Err(RegError::new(
                    Sentinel::MultiMetric,
                    format!("component {} frame", c.label),
                ));
            }
            if d.points > l.points - points
                || d.channels > l.channels - channels
                || used > l.bytes
                || d.bytes > l.bytes - used
            {
                return Err(RegError::new(Sentinel::MultiMetric, "aggregate payload"));
            }
            points += d.points;
            channels += d.channels;
            used += d.bytes;
            active += 1;
        }
        if active == 0 || used > l.bytes {
            return Err(multi());
        }
        let out = components
            .iter()
            .map(|c| {
                let mut c = c.clone();
                if c.scale == 0.0 {
                    c.scale = 1.0;
                }
                if c.weight == 0.0 {
                    c.metric = None;
                }
                c
            })
            .collect();
        Ok(CompositeMetric {
            components: out,
            prototype: prototype.clone_box(),
            _limits: l,
        })
    }
    fn compatible(&self, t: &dyn Transform) -> bool {
        t.as_any().type_id() == self.prototype.as_any().type_id()
            && t.kind() == self.prototype.kind()
            && t.number_of_parameters() == self.prototype.number_of_parameters()
            && floats_equal(&t.fixed_parameters(), &self.prototype.fixed_parameters())
            && finite_slice(&t.parameters())
    }
    pub fn evaluate(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: Option<&dyn Transform>,
        derivatives: bool,
    ) -> (MultiMetricResult, Option<RegError>) {
        let mut r = MultiMetricResult::default();
        let (Some(c), Some(t)) = (ctx, t) else {
            return (r, Some(multi()));
        };
        if !self.compatible(t) {
            return (r, Some(multi()));
        }
        if let Some(e) = ctx_err(Some(c)) {
            return (r, Some(e.into()));
        }
        if derivatives {
            r.derivative = Some(vec![0.0; t.number_of_parameters()]);
        }
        for comp in &self.components {
            let mut d = MetricContribution {
                label: comp.label.clone(),
                weight: comp.weight,
                scale: comp.scale,
                excluded: comp.weight == 0.0,
                ..Default::default()
            };
            let Some(m) = comp.metric.filter(|_| !d.excluded) else {
                r.contributions.push(d);
                continue;
            };
            let info = m.info();
            d.frame_of_reference = info.frame.clone();
            d.units = info.units.clone();
            d.points = info.points;
            let (v, g, n, mut err) = match m.evaluate(ctx, t, derivatives) {
                Ok((v, g, n)) => (v, g, n, None),
                Err((e, n)) => (0.0, Vec::new(), n, Some(e)),
            };
            d.value = v;
            d.overlap = n;
            let want = r.derivative.as_ref().map_or(0, Vec::len);
            if err.is_none()
                && (!finite_slice(&[v]) || derivatives && (g.len() != want || !finite_slice(&g)))
            {
                err = Some(multi());
            }
            if err.is_none() {
                let factor = comp.weight / comp.scale;
                d.weighted_value = factor * v;
                r.value += d.weighted_value;
                if let Some(deriv) = r.derivative.as_mut() {
                    for (j, gj) in g.iter().enumerate() {
                        deriv[j] += factor * gj;
                    }
                }
                if !finite_slice(&[r.value, d.weighted_value])
                    || !finite_slice(r.derivative.as_deref().unwrap_or(&[]))
                {
                    err = Some(multi());
                }
            }
            d.error = err.clone();
            r.contributions.push(d);
            if let Some(e) = err {
                r.value = 0.0;
                r.derivative = None;
                return (r, Some(e.prefixed(format!("component {}", comp.label))));
            }
        }
        if let Some(e) = ctx_err(Some(c)) {
            r.value = 0.0;
            r.derivative = None;
            return (r, Some(e.into()));
        }
        (r, None)
    }
    pub fn value_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Result<f64, RegError> {
        match self.evaluate(ctx, Some(t), false) {
            (r, None) => Ok(r.value),
            (_, Some(e)) => Err(e),
        }
    }
    pub fn value_and_derivative_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Result<(f64, Vec<f64>), RegError> {
        match self.evaluate(ctx, Some(t), true) {
            (r, None) => Ok((r.value, r.derivative.unwrap_or_default())),
            (_, Some(e)) => Err(e),
        }
    }
}
fn floats_equal(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}
impl Metric for CompositeMetric<'_> {
    fn value(&self, t: &dyn Transform) -> Result<f64, RegError> {
        let token = CancelToken::new();
        self.value_context(Some(&token), t)
    }
    fn value_and_derivative(&self, t: &dyn Transform) -> Result<(f64, Vec<f64>), RegError> {
        let token = CancelToken::new();
        self.value_and_derivative_context(Some(&token), t)
    }
    fn context_value(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Option<Result<f64, RegError>> {
        Some(self.value_context(ctx, t))
    }
    fn context_value_and_derivative(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Option<Result<(f64, Vec<f64>), RegError>> {
        Some(self.value_and_derivative_context(ctx, t))
    }
}
