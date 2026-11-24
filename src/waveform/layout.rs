use super::{
    Annotation, CalibrationStatus, ChannelEnvelope, ChannelInfo, Duration, GroupInfo, SECOND,
    Sample,
};
use crate::fmath;
use crate::fmath::f64_to_i64;
use crate::numtext::format_float;
use std::collections::BTreeSet;
const MILLISECOND: Duration = 1_000_000;
const SMALLEST_NONZERO: f64 = 5e-324;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RasterPoint {
    pub x: i64,
    pub y: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RasterLine {
    pub from: RasterPoint,
    pub to: RasterPoint,
}
fn line(x0: i64, y0: i64, x1: i64, y1: i64) -> RasterLine {
    RasterLine {
        from: RasterPoint { x: x0, y: y0 },
        to: RasterPoint { x: x1, y: y1 },
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AmplitudeScale {
    pub min: f64,
    pub max: f64,
    pub center: f64,
    pub half_span: f64,
    pub valid: bool,
}
const UNIT_SCALE: AmplitudeScale = AmplitudeScale {
    min: -1.0,
    max: 1.0,
    center: 0.0,
    half_span: 1.0,
    valid: false,
};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraceLayout {
    pub channel_index: i64,
    pub position: i64,
    pub lines: Vec<RasterLine>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridLineKind {
    Minor = 0,
    Major = 1,
    ChannelSeparator = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridLine {
    pub kind: GridLineKind,
    pub line: RasterLine,
    pub channel_index: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridTextKind {
    TimeLabel = 0,
    ValueLabel = 1,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridText {
    pub kind: GridTextKind,
    pub at: RasterPoint,
    pub text: String,
    pub channel_index: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RasterLayout {
    pub width: i64,
    pub height: i64,
    pub grid_lines: Vec<GridLine>,
    pub grid_labels: Vec<GridText>,
    pub traces: Vec<TraceLayout>,
    pub annotation_lines: Vec<RasterLine>,
    pub cursor: Option<RasterLine>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutOptions {
    pub width: i64,
    pub height: i64,
    pub group: GroupInfo,
    pub channel_order: Vec<i64>,
    pub envelopes: Vec<ChannelEnvelope>,
    pub annotations: Vec<Annotation>,
    pub scales: Vec<AmplitudeScale>,
    pub start: Duration,
    pub duration: Duration,
    pub amplitude: f64,
    pub grid: bool,
    pub cursor: Duration,
}
pub fn build_raster_layout(
    options: &LayoutOptions,
) -> (RasterLayout, Result<(), crate::MessageError>) {
    let mut layout = RasterLayout {
        width: options.width,
        height: options.height,
        ..RasterLayout::default()
    };
    if let Err(err) = validate_layout_options(options) {
        return (layout, Err(err.into()));
    }
    if options.grid {
        layout_grid(&mut layout, options);
    }
    if !options.group.supported || options.channel_order.is_empty() {
        return (layout, Ok(()));
    }
    if !options.envelopes.is_empty() {
        layout.traces = layout_traces(options);
    }
    layout.annotation_lines = layout_annotations(options);
    layout.cursor = raster_cursor_line(
        options.width,
        options.height,
        options.start,
        options.duration,
        options.cursor,
    );
    (layout, Ok(()))
}
pub fn raster_cursor_line(
    width: i64,
    height: i64,
    start: Duration,
    duration: Duration,
    at: Duration,
) -> Option<RasterLine> {
    if width <= 0 || height <= 0 || duration <= 0 || start > i64::MAX - duration {
        return None;
    }
    if at < start || at > start + duration {
        return None;
    }
    let x = time_x(width, at, start, duration);
    Some(line(x, 0, x, height - 1))
}
fn validate_layout_options(options: &LayoutOptions) -> Result<(), String> {
    if options.width <= 0 || options.height <= 0 {
        return Err(format!(
            "waveform: raster dimensions must be positive, got {}x{}",
            options.width, options.height
        ));
    }
    if options.duration <= 0 {
        return Err("waveform: layout duration must be positive".to_string());
    }
    if options.start > i64::MAX - options.duration {
        return Err("waveform: layout interval overflows duration".to_string());
    }
    if options.amplitude <= 0.0 || !options.amplitude.is_finite() {
        return Err("waveform: layout amplitude must be positive and finite".to_string());
    }
    let mut seen = BTreeSet::new();
    for &channel_index in &options.channel_order {
        if channel_index < 0 || channel_index >= options.group.channels.len() as i64 {
            return Err(format!(
                "waveform: channel index {channel_index} out of range"
            ));
        }
        if !seen.insert(channel_index) {
            return Err(format!("waveform: duplicate channel index {channel_index}"));
        }
    }
    if !options.envelopes.is_empty() {
        if options.envelopes.len() != options.channel_order.len() {
            return Err(format!(
                "waveform: {} envelopes do not match {} visible channels",
                options.envelopes.len(),
                options.channel_order.len()
            ));
        }
        for (position, envelope) in options.envelopes.iter().enumerate() {
            let want = options.channel_order[position];
            if envelope.channel.index != want {
                return Err(format!(
                    "waveform: envelope {position} is channel {}, want channel {want}",
                    envelope.channel.index
                ));
            }
        }
    }
    Ok(())
}
fn time_x(width: i64, at: Duration, start: Duration, duration: Duration) -> i64 {
    if width <= 1 || duration <= 0 {
        return 0;
    }
    saturated_round((at as f64 - start as f64) / duration as f64 * (width - 1) as f64)
}
fn saturated_round(value: f64) -> i64 {
    if value.is_nan() {
        return 0;
    }
    if value >= i64::MAX as f64 {
        return i64::MAX;
    }
    if value <= i64::MIN as f64 {
        return i64::MIN;
    }
    f64_to_i64(value.round())
}
fn saturating_duration_add(left: Duration, right: Duration) -> Duration {
    left.saturating_add(right)
}
fn duration_from_positive_seconds(seconds: f64) -> Duration {
    if seconds <= 0.0 || seconds.is_nan() {
        return 0;
    }
    let maximum_seconds = i64::MAX as f64 / SECOND as f64;
    if seconds >= maximum_seconds {
        return i64::MAX;
    }
    f64_to_i64(seconds * SECOND as f64)
}
fn seconds(d: Duration) -> f64 {
    (d / SECOND) as f64 + (d % SECOND) as f64 / 1e9
}
pub fn amplitude_scale_from_envelope(envelope: &ChannelEnvelope) -> AmplitudeScale {
    let mut scale = AmplitudeScale::default();
    for bucket in &envelope.buckets {
        if !bucket.valid || !bucket.min.valid || !bucket.max.valid {
            continue;
        }
        for value in [bucket.min.value, bucket.max.value] {
            if !value.is_finite() {
                continue;
            }
            if !scale.valid {
                (scale.min, scale.max, scale.valid) = (value, value, true);
                continue;
            }
            scale.min = fmath::min(scale.min, value);
            scale.max = fmath::max(scale.max, value);
        }
    }
    if !scale.valid {
        return UNIT_SCALE;
    }
    scale.center = scale.min / 2.0 + scale.max / 2.0;
    scale.half_span = scale.max / 2.0 - scale.min / 2.0;
    if !scale.center.is_finite() {
        return UNIT_SCALE;
    }
    if scale.half_span <= 0.0 || !scale.half_span.is_finite() {
        scale.half_span = fmath::max(scale.center.abs(), 1.0);
    }
    scale
}
fn channel_shift(channel: &ChannelInfo) -> Duration {
    saturating_duration_add(channel.time_skew, channel.channel_offset)
}
pub fn group_sample_bounds(group: &GroupInfo) -> (Duration, Duration) {
    if group.sample_count <= 0 || group.sampling_frequency_hz <= 0.0 {
        return (0, 0);
    }
    let last_base = duration_from_positive_seconds(
        (group.sample_count - 1) as f64 / group.sampling_frequency_hz,
    );
    let Some(first) = group.channels.first() else {
        return (0, last_base);
    };
    let first_shift = channel_shift(first);
    let (mut start, mut end) = (first_shift, saturating_duration_add(last_base, first_shift));
    for channel in &group.channels[1..] {
        let shift = channel_shift(channel);
        start = start.min(shift);
        end = end.max(saturating_duration_add(last_base, shift));
    }
    (start, end)
}
pub fn group_display_bounds(group: &GroupInfo) -> (Duration, Duration) {
    let (mut start, _) = group_sample_bounds(group);
    let mut end = group.duration;
    for channel in &group.channels {
        end = end.max(saturating_duration_add(
            group.duration,
            channel_shift(channel),
        ));
    }
    if end <= start {
        if start == i64::MAX {
            start -= 1;
        }
        end = start + 1;
    }
    (start, end)
}
pub fn cursor_bounds(
    group: &GroupInfo,
    start: Duration,
    duration: Duration,
) -> (Duration, Duration) {
    let (mut sample_start, mut sample_end) = group_sample_bounds(group);
    let view_end = saturating_duration_add(start, duration);
    sample_start = sample_start.max(start);
    sample_end = sample_end.min(view_end);
    sample_end = sample_end.max(sample_start);
    (sample_start, sample_end)
}
pub fn cursor_step(group: &GroupInfo, start: Duration, duration: Duration) -> f64 {
    let (cursor_start, cursor_end) = cursor_bounds(group, start, duration);
    if group.sample_count <= 1 || group.sampling_frequency_hz <= 0.0 || cursor_end <= cursor_start {
        return 1.0;
    }
    let interval_count =
        (cursor_end as f64 - cursor_start as f64) / SECOND as f64 * group.sampling_frequency_hz;
    if !interval_count.is_finite() || interval_count >= i64::MAX as f64 {
        return SMALLEST_NONZERO;
    }
    let intervals = f64_to_i64(interval_count.round());
    if intervals < 1 {
        return 1.0;
    }
    1.0 / intervals as f64
}
pub fn channel_unit(channel: &ChannelInfo) -> String {
    if channel.calibration.status != CalibrationStatus::Complete {
        return "raw counts".to_string();
    }
    let units = &channel.calibration.units;
    for text in [units.meaning.trim(), units.value.trim()] {
        if !text.is_empty() {
            return text.to_string();
        }
    }
    "physical units".to_string()
}
fn scale_at(scales: &[AmplitudeScale], channel_index: i64) -> AmplitudeScale {
    if let Some(scale) = usize::try_from(channel_index)
        .ok()
        .and_then(|i| scales.get(i))
        && scale.half_span > 0.0
        && scale.half_span.is_finite()
        && scale.center.is_finite()
    {
        return *scale;
    }
    AmplitudeScale {
        min: -1.0,
        max: 1.0,
        center: 0.0,
        half_span: 1.0,
        valid: false,
    }
}
fn layout_grid(layout: &mut RasterLayout, options: &LayoutOptions) {
    let major_division = nice_duration(options.duration, options.width);
    let mut minor_division = major_division / 5;
    if minor_division <= 0 {
        minor_division = major_division;
    }
    let mut first_minor = options.start - options.start % minor_division;
    if first_minor < options.start {
        first_minor = first_minor.wrapping_add(minor_division);
    }
    let view_end = options.start + options.duration;
    let mut at = first_minor;
    while at <= view_end {
        let x = time_x(options.width, at, options.start, options.duration);
        let mut kind = GridLineKind::Minor;
        if at % major_division == 0 {
            kind = GridLineKind::Major;
            layout.grid_labels.push(GridText {
                kind: GridTextKind::TimeLabel,
                at: RasterPoint { x: x + 3, y: 13 },
                text: format_grid_time(at),
                channel_index: -1,
            });
        }
        layout.grid_lines.push(GridLine {
            kind,
            channel_index: -1,
            line: line(x, 0, x, options.height - 1),
        });
        if at > view_end.wrapping_sub(minor_division) {
            break;
        }
        at += minor_division;
    }
    let count = options.channel_order.len() as i64;
    if count == 0 {
        return;
    }
    let band_height = options.height as f64 / count as f64;
    for (position, &channel_index) in options.channel_order.iter().enumerate() {
        let position = position as i64;
        let band = channel_raster_band(position, count, options.height);
        let scale = scale_at(&options.scales, channel_index);
        let visible_half_span = scale.half_span / fmath::max(options.amplitude, SMALLEST_NONZERO);
        let division = nice_number(visible_half_span / 2.0);
        if division > 0.0 {
            let minimum = scale.center - visible_half_span;
            let maximum = scale.center + visible_half_span;
            let unit = channel_unit(&options.group.channels[channel_index as usize]);
            let mut value = (minimum / division).ceil() * division;
            while value.is_finite() && value <= maximum + division * 1e-9 {
                let y = value_y(&band, band_height, value, &scale, options.amplitude);
                layout.grid_lines.push(GridLine {
                    kind: GridLineKind::Minor,
                    channel_index,
                    line: line(0, y, options.width - 1, y),
                });
                layout.grid_labels.push(GridText {
                    kind: GridTextKind::ValueLabel,
                    at: RasterPoint { x: 3, y: y - 2 },
                    channel_index,
                    text: format!("{} {unit}", format_float(value, 'g', 4)),
                });
                let next = value + division;
                if next <= value || !next.is_finite() {
                    break;
                }
                value = next;
            }
        }
        if position > 0 {
            let y =
                f64_to_i64((position as f64 * band_height).round()).clamp(0, options.height - 1);
            layout.grid_lines.push(GridLine {
                kind: GridLineKind::ChannelSeparator,
                channel_index,
                line: line(0, y, options.width - 1, y),
            });
        }
    }
}
fn value_y(
    band: &RasterBand,
    band_height: f64,
    value: f64,
    scale: &AmplitudeScale,
    amplitude: f64,
) -> i64 {
    if band.collapsed {
        return band.center;
    }
    let center = band.trace_center;
    if scale.half_span <= 0.0 {
        return saturated_round(center).clamp(band.top, band.bottom);
    }
    saturated_round(
        center - (value - scale.center) * 0.43 * band_height / scale.half_span * amplitude,
    )
    .clamp(band.top, band.bottom)
}
fn nice_duration(duration: Duration, width: i64) -> Duration {
    if duration <= 0 {
        return 1;
    }
    let target_divisions = fmath::max(1.0, width as f64 / 100.0);
    let division =
        duration_from_positive_seconds(nice_number(seconds(duration) / target_divisions));
    if division <= 0 { 1 } else { division }
}
fn nice_number(value: f64) -> f64 {
    if value <= 0.0 || !value.is_finite() {
        return 0.0;
    }
    let power = fmath::pow(10.0, fmath::log10(value).floor());
    let fraction = value / power;
    if fraction <= 1.0 {
        power
    } else if fraction <= 2.0 {
        2.0 * power
    } else if fraction <= 5.0 {
        5.0 * power
    } else {
        10.0 * power
    }
}
fn format_grid_time(at: Duration) -> String {
    if at < SECOND {
        return format!(
            "{} ms",
            format_float(at as f64 / MILLISECOND as f64, 'f', 0)
        );
    }
    format!("{} s", format_float(seconds(at), 'g', 3))
}
fn layout_traces(options: &LayoutOptions) -> Vec<TraceLayout> {
    let count = options.channel_order.len() as i64;
    let band_height = options.height as f64 / count as f64;
    let mut traces = Vec::with_capacity(options.envelopes.len());
    for (position, envelope) in options.envelopes.iter().enumerate() {
        let channel_index = options.channel_order[position];
        let position = position as i64;
        let scale = scale_at(&options.scales, channel_index);
        let mut trace = TraceLayout {
            channel_index,
            position,
            lines: Vec::new(),
        };
        let band = channel_raster_band(position, count, options.height);
        let clip = (0, band.top, options.width - 1, band.bottom);
        let mut previous: Option<RasterPoint> = None;
        for bucket in &envelope.buckets {
            if !bucket.valid
                || !bucket.min.valid
                || !bucket.max.valid
                || !bucket.min.value.is_finite()
                || !bucket.max.value.is_finite()
            {
                previous = None;
                continue;
            }
            let (mut first, mut second) = (bucket.min, bucket.max);
            if second.time < first.time {
                std::mem::swap(&mut first, &mut second);
            }
            let first_point = sample_point(options, &band, band_height, &first, &scale);
            let second_point = sample_point(options, &band, band_height, &second, &scale);
            if let Some(previous) = previous {
                append_clipped_line(
                    &mut trace.lines,
                    RasterLine {
                        from: previous,
                        to: first_point,
                    },
                    clip,
                );
            }
            append_clipped_line(
                &mut trace.lines,
                RasterLine {
                    from: first_point,
                    to: second_point,
                },
                clip,
            );
            previous = Some(second_point);
        }
        traces.push(trace);
    }
    traces
}
fn append_clipped_line(
    lines: &mut Vec<RasterLine>,
    line: RasterLine,
    bounds: (i64, i64, i64, i64),
) {
    if let Some(clipped) = clip_raster_line(line, bounds) {
        lines.push(clipped);
    }
}
fn clip_raster_line(
    line: RasterLine,
    (min_x, min_y, max_x, max_y): (i64, i64, i64, i64),
) -> Option<RasterLine> {
    if min_x > max_x || min_y > max_y {
        return None;
    }
    let inside = |p: RasterPoint| p.x >= min_x && p.x <= max_x && p.y >= min_y && p.y <= max_y;
    if inside(line.from) && inside(line.to) {
        return Some(line);
    }
    let (x0, y0) = (line.from.x as f64, line.from.y as f64);
    let dx = line.to.x as f64 - x0;
    let dy = line.to.y as f64 - y0;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let mut clip = |p: f64, q: f64| -> bool {
        if p == 0.0 {
            return q >= 0.0;
        }
        let ratio = q / p;
        if p < 0.0 {
            if ratio > t1 {
                return false;
            }
            if ratio > t0 {
                t0 = ratio;
            }
            return true;
        }
        if ratio < t0 {
            return false;
        }
        if ratio < t1 {
            t1 = ratio;
        }
        true
    };
    if !clip(-dx, x0 - min_x as f64)
        || !clip(dx, max_x as f64 - x0)
        || !clip(-dy, y0 - min_y as f64)
        || !clip(dy, max_y as f64 - y0)
    {
        return None;
    }
    let point_at = |t: f64| RasterPoint {
        x: saturated_round(x0 + t * dx).clamp(min_x, max_x),
        y: saturated_round(y0 + t * dy).clamp(min_y, max_y),
    };
    Some(RasterLine {
        from: point_at(t0),
        to: point_at(t1),
    })
}
fn sample_point(
    options: &LayoutOptions,
    band: &RasterBand,
    band_height: f64,
    sample: &Sample,
    scale: &AmplitudeScale,
) -> RasterPoint {
    let x = saturated_round(
        (sample.time as f64 - options.start as f64) / options.duration as f64
            * (options.width - 1) as f64,
    );
    if band.collapsed {
        return RasterPoint { x, y: band.center };
    }
    let y_scale = 0.43 * band_height / scale.half_span * options.amplitude;
    let y = saturated_round(band.trace_center - (sample.value - scale.center) * y_scale);
    RasterPoint { x, y }
}
struct RasterBand {
    top: i64,
    bottom: i64,
    center: i64,
    trace_center: f64,
    collapsed: bool,
}
fn channel_raster_band(position: i64, channel_count: i64, height: i64) -> RasterBand {
    let top = position * height / channel_count;
    let bottom = (position + 1) * height / channel_count - 1;
    let mut continuous_center = (position as f64 + 0.5) * height as f64 / channel_count as f64;
    if bottom < top {
        let pixel = saturated_round(continuous_center - 0.5).clamp(0, height - 1);
        return RasterBand {
            top: pixel,
            bottom: pixel,
            center: pixel,
            trace_center: pixel as f64,
            collapsed: true,
        };
    }
    let rounded_center = saturated_round(continuous_center);
    if rounded_center < top || rounded_center > bottom {
        continuous_center = rounded_center.clamp(top, bottom) as f64;
    }
    RasterBand {
        top,
        bottom,
        center: (top + bottom) / 2,
        trace_center: continuous_center,
        collapsed: false,
    }
}
pub fn annotation_applies_to_channels(
    annotation: &Annotation,
    group_number: i64,
    order: &[i64],
) -> bool {
    !annotation_channel_positions(annotation, group_number, order).is_empty()
}
fn layout_annotations(options: &LayoutOptions) -> Vec<RasterLine> {
    let mut lines = Vec::new();
    for annotation in &options.annotations {
        annotation_lines(annotation, options, &mut lines);
    }
    lines
}
fn annotation_lines(annotation: &Annotation, options: &LayoutOptions, lines: &mut Vec<RasterLine>) {
    let order = &options.channel_order;
    let (start, duration, width, height) = (
        options.start,
        options.duration,
        options.width,
        options.height,
    );
    let channel_positions =
        annotation_channel_positions(annotation, options.group.index + 1, order);
    if channel_positions.is_empty() {
        return;
    }
    let times = annotation_times(annotation, options.group.sampling_frequency_hz);
    if times.is_empty() {
        return;
    }
    let view_end = start + duration;
    let x_at = |at: Duration| time_x(width, at.clamp(start, view_end), start, duration);
    let band = |position: i64| {
        let band = channel_raster_band(position, order.len() as i64, height);
        (band.top, band.bottom, band.center)
    };
    let mut add_marker = |position: i64, at: Duration| {
        if at < start || at > view_end {
            return;
        }
        let (top, bottom, _) = band(position);
        if bottom < top {
            return;
        }
        let x = x_at(at);
        lines.push(line(x, top, x, bottom));
    };
    let mut segments = Vec::new();
    let mut add_segment = |position: i64, mut from: Duration, mut to: Duration| {
        if to < from {
            std::mem::swap(&mut from, &mut to);
        }
        if to < start || from > view_end {
            return;
        }
        let (top, bottom, center) = band(position);
        if bottom < top {
            return;
        }
        let (x0, x1) = (x_at(from), x_at(to));
        segments.push(line(x0, center, x1, center));
        segments.push(line(x0, top, x0, bottom));
        segments.push(line(x1, top, x1, bottom));
    };
    let range_type = crate::textfmt::to_upper(annotation.temporal_range_type.trim());
    for &position in &channel_positions {
        match range_type.as_str() {
            "POINT" => add_marker(position, times[0]),
            "SEGMENT" => {
                if times.len() >= 2 {
                    add_segment(position, times[0], times[1]);
                } else {
                    add_marker(position, times[0]);
                }
            }
            "MULTISEGMENT" => {
                for pair in times.as_chunks::<2>().0 {
                    add_segment(position, pair[0], pair[1]);
                }
            }
            "BEGIN" => add_segment(position, times[0], view_end),
            "END" => add_segment(position, start, times[0]),
            _ => {
                for &at in &times {
                    add_marker(position, at);
                }
            }
        }
    }
    lines.extend(segments);
}
fn annotation_times(annotation: &Annotation, frequency: f64) -> Vec<Duration> {
    let mut times = annotation.referenced_time_offsets.clone();
    for &position in &annotation.referenced_sample_positions {
        if position > 0 && frequency > 0.0 {
            times.push(duration_from_positive_seconds(
                (position - 1) as f64 / frequency,
            ));
        }
    }
    times
}
fn annotation_channel_positions(
    annotation: &Annotation,
    group_number: i64,
    order: &[i64],
) -> Vec<i64> {
    let all = || (0..order.len() as i64).collect::<Vec<_>>();
    if annotation.channels.is_empty() {
        return all();
    }
    let mut selected = BTreeSet::new();
    for reference in &annotation.channels {
        if reference.group_number != group_number {
            continue;
        }
        if reference.channel_number == 0 {
            return all();
        }
        let channel_index = reference.channel_number - 1;
        for (position, &visible) in order.iter().enumerate() {
            if visible == channel_index {
                selected.insert(position as i64);
            }
        }
    }
    selected.into_iter().collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nice_numbers() {
        assert_eq!(nice_number(0.0), 0.0);
        assert_eq!(nice_number(1.0), 1.0);
        assert_eq!(nice_number(3.0), 5.0);
        assert_eq!(nice_number(70.0), 100.0);
        assert_eq!(nice_duration(SECOND, 200), SECOND / 2);
        assert_eq!(format_grid_time(250 * MILLISECOND), "250 ms");
        assert_eq!(format_grid_time(1500 * MILLISECOND), "1.5 s");
    }
    #[test]
    fn bands_and_clipping() {
        let band = channel_raster_band(1, 2, 100);
        assert_eq!((band.top, band.bottom, band.center), (50, 99, 74));
        let collapsed = channel_raster_band(3, 10, 4);
        assert!(collapsed.collapsed);
        let clipped = clip_raster_line(line(-10, 5, 10, 5), (0, 0, 9, 9)).unwrap();
        assert_eq!(clipped, line(0, 5, 9, 5));
        assert!(clip_raster_line(line(-5, -5, -1, -1), (0, 0, 9, 9)).is_none());
        assert_eq!(raster_cursor_line(11, 5, 0, 10, 5), Some(line(5, 0, 5, 4)));
    }
}
