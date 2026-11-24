use super::parse::{duration_from_float, multiply_size};
use super::{
    CalibrationStatus, ChannelEnvelope, Duration, EnvelopeBucket, Error, Group, RawFallback,
    Recording, SECOND, Sample,
};
use crate::fmath::f64_to_i64;
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RawPoint {
    sample: i64,
    raw: f64,
    bits: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RawBucket {
    min: RawPoint,
    max: RawPoint,
    valid: bool,
}
pub(crate) struct IndexLevel {
    sample_span: i64,
    channels: Vec<Vec<RawBucket>>,
}
pub(crate) struct MultiIndex {
    levels: Vec<IndexLevel>,
}
impl Recording {
    pub fn envelope(
        &mut self,
        group_index: i64,
        channel_indices: Option<&[i64]>,
        start: Duration,
        duration: Duration,
        width: i64,
    ) -> Result<Vec<ChannelEnvelope>, Error> {
        if self.closed {
            return Err(Error::closed());
        }
        if group_index < 0 || group_index >= self.groups.len() as i64 {
            return Err(Error::other(format!(
                "waveform: group index {group_index} out of range"
            )));
        }
        if width <= 0 || width > self.options.max_width {
            return Err(Error::other(format!(
                "waveform: envelope width {width} must be in [1,{}]",
                self.options.max_width
            )));
        }
        if duration <= 0 {
            return Err(Error::other("waveform: duration must be positive"));
        }
        let group = &mut self.groups[group_index as usize];
        if !group.info.supported {
            return Err(group.unsupported_error(&self.sop_class_uid));
        }
        let channels = selected_channels(channel_indices, group.info.channels.len())?;
        group.ensure_index()?;
        let mut result = Vec::with_capacity(channels.len());
        for channel_index in channels {
            let mut envelope = ChannelEnvelope {
                channel: group.info.channels[channel_index].clone(),
                buckets: Vec::new(),
            };
            if let Some((start_sample, end_sample)) =
                group.channel_sample_range(channel_index, start, duration)?
            {
                envelope.buckets =
                    group.envelope_buckets(channel_index, start_sample, end_sample, width)?;
            }
            result.push(envelope);
        }
        Ok(result)
    }
    pub fn value_at(
        &self,
        group_index: i64,
        channel_index: i64,
        at: Duration,
    ) -> Result<Sample, Error> {
        if self.closed {
            return Err(Error::closed());
        }
        if group_index < 0 || group_index >= self.groups.len() as i64 {
            return Err(Error::other(format!(
                "waveform: group index {group_index} out of range"
            )));
        }
        let group = &self.groups[group_index as usize];
        if !group.info.supported {
            return Err(group.unsupported_error(&self.sop_class_uid));
        }
        if channel_index < 0 || channel_index >= group.info.channels.len() as i64 {
            return Err(Error::other(format!(
                "waveform: channel index {channel_index} out of range"
            )));
        }
        let channel_index = channel_index as usize;
        let channel = &group.info.channels[channel_index];
        let adjusted = at
            .wrapping_sub(channel.time_skew)
            .wrapping_sub(channel.channel_offset);
        if adjusted < 0 {
            return Err(Error::other("waveform: cursor time precedes channel start"));
        }
        let position = adjusted as f64 / SECOND as f64 * group.info.sampling_frequency_hz;
        let sample_index = f64_to_i64((position + 0.5).floor());
        if sample_index < 0 || sample_index >= group.info.sample_count {
            return Err(Error::other(
                "waveform: cursor time is outside group duration",
            ));
        }
        let point = group.read_sample(channel_index, sample_index)?;
        Ok(group.public_sample(channel_index, point))
    }
}
impl Group {
    pub(crate) fn ensure_index(&mut self) -> Result<(), Error> {
        if self.index.is_some() {
            return Ok(());
        }
        if let Some(err) = &self.index_err {
            return Err(err.clone());
        }
        match self.build_index(self.index_budget) {
            Ok(index) => {
                self.index = Some(index);
                Ok(())
            }
            Err(err) => {
                self.index_err = Some(err.clone());
                Err(err)
            }
        }
    }
    fn is_padding(&self, channel_index: usize, bits: u64) -> bool {
        self.has_padding && bits == self.padding_bits[channel_index]
    }
    fn build_index(&self, max_entries: i64) -> Result<MultiIndex, Error> {
        let channel_count = self.info.channels.len();
        let base_span =
            bounded_base_span(self.info.sample_count, channel_count as i64, max_entries)?;
        let leaf_count = divide_round_up(self.info.sample_count, base_span) as usize;
        let mut leaf = IndexLevel {
            sample_span: base_span,
            channels: vec![vec![RawBucket::default(); leaf_count]; channel_count],
        };
        let all: Vec<usize> = (0..channel_count).collect();
        self.scan_samples(0, self.info.sample_count, &all, |point, channel_index| {
            if self.is_padding(channel_index, point.bits) {
                return;
            }
            let bucket = (point.sample / base_span) as usize;
            update_raw_bucket(&mut leaf.channels[channel_index][bucket], point);
        })?;
        let mut levels = vec![leaf];
        loop {
            let previous = &levels[levels.len() - 1];
            if previous.channels[0].len() <= 1 {
                break;
            }
            let next = IndexLevel {
                sample_span: previous.sample_span * 2,
                channels: previous
                    .channels
                    .iter()
                    .map(|buckets| {
                        buckets
                            .chunks(2)
                            .map(|pair| match pair {
                                [left, right] => merge_raw_buckets(*left, *right),
                                _ => pair[0],
                            })
                            .collect()
                    })
                    .collect(),
            };
            levels.push(next);
        }
        Ok(MultiIndex { levels })
    }
    fn scan_samples(
        &self,
        start_sample: i64,
        end_sample: i64,
        channels: &[usize],
        mut visit: impl FnMut(RawPoint, usize),
    ) -> Result<(), Error> {
        if start_sample < 0 || end_sample < start_sample || end_sample > self.info.sample_count {
            return Err(Error::other(format!(
                "waveform: invalid sample scan [{start_sample},{end_sample})"
            )));
        }
        if start_sample == end_sample {
            return Ok(());
        }
        let source = self.source.as_ref().ok_or_else(Error::closed)?;
        let width = self.decoder.width;
        let stride = self.info.channels.len() * width;
        let samples_per_block = ((64usize << 10) / stride.max(1)).max(1) as i64;
        let mut block_start = start_sample;
        while block_start < end_sample {
            let block_end = (block_start + samples_per_block).min(end_sample);
            let samples_in_block = (block_end - block_start) as usize;
            let offset = multiply_size(&[block_start, stride as i64])
                .ok_or_else(|| Error::other("waveform: sample offset overflow"))?;
            let block = source
                .read_at(offset, samples_in_block * stride)
                .map_err(|e| {
                    Error::other(format!(
                        "read waveform samples [{block_start},{block_end}) at byte {offset}: {}",
                        e.unwrap_or_else(|| "unexpected EOF".into())
                    ))
                })?;
            for i in 0..samples_in_block {
                let sample = block_start + i as i64;
                for &channel_index in channels {
                    let at = i * stride + channel_index * width;
                    let (raw, bits) = self.decoder.decode_bytes(
                        &block[at..at + width],
                        self.info.channels[channel_index].bits_stored,
                    )?;
                    visit(RawPoint { sample, raw, bits }, channel_index);
                }
            }
            block_start = block_end;
        }
        Ok(())
    }
    fn envelope_buckets(
        &self,
        channel_index: usize,
        start_sample: i64,
        end_sample: i64,
        width: i64,
    ) -> Result<Vec<EnvelopeBucket>, Error> {
        let Some(index) = &self.index else {
            return Ok(Vec::new());
        };
        let level = choose_level(&index.levels, start_sample, end_sample, width);
        let first_bucket = start_sample / level.sample_span;
        let last_bucket = (end_sample - 1) / level.sample_span;
        let indexed = &level.channels[channel_index];
        let mut buckets = Vec::with_capacity((last_bucket - first_bucket + 1) as usize);
        for bucket_index in first_bucket..=last_bucket {
            let aligned_start = bucket_index * level.sample_span;
            let aligned_end = (aligned_start + level.sample_span).min(self.info.sample_count);
            let bucket_start = aligned_start.max(start_sample);
            let bucket_end = aligned_end.min(end_sample);
            let mut raw = indexed[bucket_index as usize];
            if bucket_start != aligned_start || bucket_end != aligned_end {
                raw = self.scan_raw_bucket(channel_index, bucket_start, bucket_end)?;
            }
            let mut bucket = EnvelopeBucket {
                start_sample: bucket_start,
                end_sample: bucket_end,
                valid: raw.valid,
                ..EnvelopeBucket::default()
            };
            if raw.valid {
                bucket.min = self.public_sample(channel_index, raw.min);
                bucket.max = self.public_sample(channel_index, raw.max);
                if bucket.min.value > bucket.max.value {
                    std::mem::swap(&mut bucket.min, &mut bucket.max);
                }
            }
            buckets.push(bucket);
        }
        Ok(buckets)
    }
    fn scan_raw_bucket(
        &self,
        channel_index: usize,
        start_sample: i64,
        end_sample: i64,
    ) -> Result<RawBucket, Error> {
        let mut bucket = RawBucket::default();
        self.scan_samples(start_sample, end_sample, &[channel_index], |point, _| {
            if !self.is_padding(channel_index, point.bits) {
                update_raw_bucket(&mut bucket, point);
            }
        })?;
        Ok(bucket)
    }
    fn read_sample(&self, channel_index: usize, sample_index: i64) -> Result<RawPoint, Error> {
        let overflow = || Error::other("waveform: sample offset overflow");
        let channel_count = self.info.channels.len() as i64;
        let ordinal = multiply_size(&[sample_index, channel_count])
            .and_then(|ordinal| ordinal.checked_add(channel_index as i64))
            .ok_or_else(overflow)?;
        let width = self.decoder.width;
        let offset = multiply_size(&[ordinal, width as i64]).ok_or_else(overflow)?;
        let source = self.source.as_ref().ok_or_else(Error::closed)?;
        let bytes = match source.read_at(offset, width) {
            Ok(bytes) => bytes,
            Err(None) => std::borrow::Cow::Owned(vec![0u8; width]),
            Err(Some(e)) => {
                return Err(Error::other(format!("read sample at byte {offset}: {e}")));
            }
        };
        let (raw, bits) = self
            .decoder
            .decode_bytes(&bytes, self.info.channels[channel_index].bits_stored)?;
        Ok(RawPoint {
            sample: sample_index,
            raw,
            bits,
        })
    }
    fn public_sample(&self, channel_index: usize, point: RawPoint) -> Sample {
        let channel = &self.info.channels[channel_index];
        let calibration = &channel.calibration;
        let calibrated = calibration.status == CalibrationStatus::Complete;
        let value = if calibrated {
            point.raw * calibration.sensitivity * calibration.correction_factor
                + calibration.baseline
        } else {
            point.raw
        };
        Sample {
            sample_index: point.sample,
            time: duration_from_float(point.sample as f64 / self.info.sampling_frequency_hz)
                .wrapping_add(channel.time_skew)
                .wrapping_add(channel.channel_offset),
            raw: point.raw,
            value,
            valid: !self.is_padding(channel_index, point.bits),
            calibrated,
        }
    }
    fn channel_sample_range(
        &self,
        channel_index: usize,
        start: Duration,
        duration: Duration,
    ) -> Result<Option<(i64, i64)>, Error> {
        let end = start.wrapping_add(duration);
        if end < start {
            return Err(Error::other(
                "waveform: requested interval overflows duration",
            ));
        }
        let channel = &self.info.channels[channel_index];
        let shift = channel.time_skew.wrapping_add(channel.channel_offset);
        let frequency = self.info.sampling_frequency_hz;
        let sample_count = self.info.sample_count;
        let start_float = (start as f64 - shift as f64) / SECOND as f64 * frequency;
        let end_float = (end as f64 - shift as f64) / SECOND as f64 * frequency;
        if start_float >= sample_count as f64 || end_float <= 0.0 {
            return Ok(None);
        }
        let mut start_sample = 0i64;
        if start_float > 0.0 {
            start_sample = f64_to_i64(start_float.ceil());
        }
        let mut end_sample = sample_count;
        if end_float < sample_count as f64 {
            end_sample = f64_to_i64(end_float.ceil());
        }
        start_sample = start_sample.max(0);
        end_sample = end_sample.min(sample_count);
        if end_sample <= start_sample {
            return Ok(None);
        }
        Ok(Some((start_sample, end_sample)))
    }
    pub(crate) fn unsupported_error(&self, sop_class_uid: &str) -> Error {
        Error::unsupported(RawFallback {
            sop_class_uid: sop_class_uid.to_string(),
            group_index: self.info.index,
            encoding: format!(
                "{}/{}-bit",
                self.info.sample_interpretation, self.info.bits_allocated
            ),
            raw_bytes: self.info.raw_data_bytes,
            reason: self.info.fallback_reason.clone(),
        })
    }
}
fn selected_channels(selected: Option<&[i64]>, count: usize) -> Result<Vec<usize>, Error> {
    let Some(selected) = selected else {
        return Ok((0..count).collect());
    };
    let mut out = Vec::with_capacity(selected.len());
    for &index in selected {
        if index < 0 || index >= count as i64 {
            return Err(Error::other(format!(
                "waveform: channel index {index} out of range"
            )));
        }
        if !out.contains(&(index as usize)) {
            out.push(index as usize);
        }
    }
    Ok(out)
}
fn choose_level(
    levels: &[IndexLevel],
    start_sample: i64,
    end_sample: i64,
    width: i64,
) -> &IndexLevel {
    levels
        .iter()
        .find(|level| {
            (end_sample - 1) / level.sample_span - start_sample / level.sample_span < width
        })
        .unwrap_or(&levels[levels.len() - 1])
}
fn bounded_base_span(
    sample_count: i64,
    channel_count: i64,
    max_entries: i64,
) -> Result<i64, Error> {
    if max_entries < channel_count {
        return Err(Error::other(format!(
            "waveform: MaxIndexEntries {max_entries} cannot retain one bucket for each of {channel_count} channels"
        )));
    }
    let (mut low, mut high) = (1i64, sample_count);
    while low < high {
        let middle = low + (high - low) / 2;
        if index_entry_count(sample_count, middle, channel_count, max_entries) <= max_entries {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    Ok(low)
}
fn index_entry_count(
    sample_count: i64,
    base_span: i64,
    channel_count: i64,
    stop_after: i64,
) -> i64 {
    let mut count = divide_round_up(sample_count, base_span);
    let mut total = 0i64;
    loop {
        if count > (stop_after - total) / channel_count {
            return stop_after + 1;
        }
        total += count * channel_count;
        if count == 1 {
            return total;
        }
        count = divide_round_up(count, 2);
    }
}
fn divide_round_up(value: i64, divisor: i64) -> i64 {
    value / divisor + i64::from(value % divisor != 0)
}
fn update_raw_bucket(bucket: &mut RawBucket, point: RawPoint) {
    if !bucket.valid {
        *bucket = RawBucket {
            min: point,
            max: point,
            valid: true,
        };
        return;
    }
    if point.raw < bucket.min.raw
        || (point.raw == bucket.min.raw && point.sample < bucket.min.sample)
    {
        bucket.min = point;
    }
    if point.raw > bucket.max.raw
        || (point.raw == bucket.max.raw && point.sample < bucket.max.sample)
    {
        bucket.max = point;
    }
}
fn merge_raw_buckets(left: RawBucket, right: RawBucket) -> RawBucket {
    if !left.valid {
        return right;
    }
    if !right.valid {
        return left;
    }
    let mut merged = left;
    update_raw_bucket(&mut merged, right.min);
    update_raw_bucket(&mut merged, right.max);
    merged
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spans() {
        assert_eq!(divide_round_up(10, 3), 4);
        assert_eq!(index_entry_count(10, 1, 1, 100), 21);
        assert_eq!(bounded_base_span(10, 1, 21).unwrap(), 1);
        assert_eq!(bounded_base_span(10, 1, 20).unwrap(), 2);
        assert_eq!(bounded_base_span(10, 1, 1).unwrap(), 10);
        assert!(bounded_base_span(10, 2, 1).is_err());
    }
    #[test]
    fn buckets_keep_first_extrema() {
        let mut b = RawBucket::default();
        for (sample, raw) in [(0, 1.0), (1, 5.0), (2, 5.0), (3, -2.0), (4, -2.0)] {
            update_raw_bucket(
                &mut b,
                RawPoint {
                    sample,
                    raw,
                    bits: 0,
                },
            );
        }
        assert_eq!((b.min.sample, b.max.sample), (3, 1));
    }
}
