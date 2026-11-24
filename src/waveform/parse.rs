use super::audio::{audio_profile_error, is_audio_storage_sop_class, validate_audio_profile};
use super::{
    Annotation, Calibration, CalibrationStatus, ChannelInfo, ChannelReference, CodedConcept,
    Duration, Error, Group, GroupInfo, NormalizedOptions, Options, RawFallback, Recording, SECOND,
    Source, SourceFactory,
};
use crate::fmath::f64_to_i64;
use crate::numtext;
use crate::object::{Element, File, Object, Tag, UNDEFINED_LENGTH, Value, Vr};
use crate::textfmt::{quote, to_upper};
const UID_TWELVE_LEAD_ECG: &str = "1.2.840.10008.5.1.4.1.1.9.1.1";
const UID_GENERAL_ECG: &str = "1.2.840.10008.5.1.4.1.1.9.1.2";
const UID_AMBULATORY_ECG: &str = "1.2.840.10008.5.1.4.1.1.9.1.3";
const UID_GENERAL_32BIT_ECG: &str = "1.2.840.10008.5.1.4.1.1.9.1.4";
const UID_HEMODYNAMIC: &str = "1.2.840.10008.5.1.4.1.1.9.2.1";
const UID_CARDIAC_ELECTROPHYSIOLOGY: &str = "1.2.840.10008.5.1.4.1.1.9.3.1";
const UID_ARTERIAL_PULSE: &str = "1.2.840.10008.5.1.4.1.1.9.5.1";
const UID_RESPIRATORY: &str = "1.2.840.10008.5.1.4.1.1.9.6.1";
const UID_MULTICHANNEL_RESPIRATORY: &str = "1.2.840.10008.5.1.4.1.1.9.6.2";
const UID_ROUTINE_SCALP_EEG: &str = "1.2.840.10008.5.1.4.1.1.9.7.1";
const UID_ELECTROMYOGRAM: &str = "1.2.840.10008.5.1.4.1.1.9.7.2";
const UID_ELECTROOCULOGRAM: &str = "1.2.840.10008.5.1.4.1.1.9.7.3";
const UID_SLEEP_EEG: &str = "1.2.840.10008.5.1.4.1.1.9.7.4";
const UID_BODY_POSITION: &str = "1.2.840.10008.5.1.4.1.1.9.8.1";
const UID_WAVEFORM_TRIAL: &str = "1.2.840.10008.5.1.4.1.1.9.1";
pub(crate) const UID_BASIC_VOICE_AUDIO: &str = "1.2.840.10008.5.1.4.1.1.9.4.1";
pub(crate) const UID_GENERAL_AUDIO: &str = "1.2.840.10008.5.1.4.1.1.9.4.2";
const UID_ULTRASOUND_WAVEFORM: &str = "1.2.840.10008.5.1.4.1.1.601.5";
const SUPPORTED: &[(&str, &[&str])] = &[
    (UID_BASIC_VOICE_AUDIO, &["AB", "MB", "UB"]),
    (UID_GENERAL_AUDIO, &["SB", "SS"]),
    (UID_TWELVE_LEAD_ECG, &["SS"]),
    (UID_GENERAL_ECG, &["SS"]),
    (UID_AMBULATORY_ECG, &["SB", "SS"]),
    (UID_GENERAL_32BIT_ECG, &["SL", "SS"]),
    (UID_HEMODYNAMIC, &["SS"]),
    (UID_CARDIAC_ELECTROPHYSIOLOGY, &["SS"]),
    (UID_ARTERIAL_PULSE, &["SB", "SS"]),
    (UID_RESPIRATORY, &["SB", "SS"]),
    (UID_MULTICHANNEL_RESPIRATORY, &["SL", "SS"]),
    (UID_ROUTINE_SCALP_EEG, &["SL", "SS"]),
    (UID_ELECTROMYOGRAM, &["SL", "SS"]),
    (UID_ELECTROOCULOGRAM, &["SL", "SS"]),
    (UID_SLEEP_EEG, &["SL", "SS"]),
    (UID_BODY_POSITION, &["SS", "UB"]),
];
fn unsupported_reason(uid: &str) -> Option<&'static str> {
    match uid {
        UID_WAVEFORM_TRIAL => Some("retired trial waveform storage is not decoded"),
        UID_ULTRASOUND_WAVEFORM => Some(
            "ultrasound waveform storage is not in the calibrated physiologic waveform profile",
        ),
        _ => None,
    }
}
pub(crate) const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0100);
const TAG_CODING_SCHEME_DESIGNATOR: Tag = Tag::new(0x0008, 0x0102);
const TAG_CODING_SCHEME_VERSION: Tag = Tag::new(0x0008, 0x0103);
const TAG_CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
const TAG_MULTIPLEX_GROUP_TIME_OFFSET: Tag = Tag::new(0x0018, 0x1068);
const TAG_WAVEFORM_ORIGINALITY: Tag = Tag::new(0x003A, 0x0004);
pub(crate) const TAG_NUMBER_OF_WAVEFORM_CHANNELS: Tag = Tag::new(0x003A, 0x0005);
const TAG_NUMBER_OF_WAVEFORM_SAMPLES: Tag = Tag::new(0x003A, 0x0010);
pub(crate) const TAG_SAMPLING_FREQUENCY: Tag = Tag::new(0x003A, 0x001A);
const TAG_MULTIPLEX_GROUP_LABEL: Tag = Tag::new(0x003A, 0x0020);
pub(crate) const TAG_CHANNEL_DEFINITION_SEQUENCE: Tag = Tag::new(0x003A, 0x0200);
const TAG_WAVEFORM_CHANNEL_NUMBER: Tag = Tag::new(0x003A, 0x0202);
const TAG_CHANNEL_LABEL: Tag = Tag::new(0x003A, 0x0203);
const TAG_CHANNEL_STATUS: Tag = Tag::new(0x003A, 0x0205);
const TAG_CHANNEL_SOURCE_SEQUENCE: Tag = Tag::new(0x003A, 0x0208);
const TAG_CHANNEL_SENSITIVITY: Tag = Tag::new(0x003A, 0x0210);
const TAG_CHANNEL_SENSITIVITY_UNITS: Tag = Tag::new(0x003A, 0x0211);
const TAG_CHANNEL_SENSITIVITY_CORRECTION: Tag = Tag::new(0x003A, 0x0212);
const TAG_CHANNEL_BASELINE: Tag = Tag::new(0x003A, 0x0213);
pub(crate) const TAG_CHANNEL_TIME_SKEW: Tag = Tag::new(0x003A, 0x0214);
pub(crate) const TAG_CHANNEL_SAMPLE_SKEW: Tag = Tag::new(0x003A, 0x0215);
pub(crate) const TAG_CHANNEL_OFFSET: Tag = Tag::new(0x003A, 0x0218);
pub(crate) const TAG_WAVEFORM_BITS_STORED: Tag = Tag::new(0x003A, 0x021A);
const TAG_FILTER_LOW_FREQUENCY: Tag = Tag::new(0x003A, 0x0220);
const TAG_FILTER_HIGH_FREQUENCY: Tag = Tag::new(0x003A, 0x0221);
const TAG_NOTCH_FILTER_FREQUENCY: Tag = Tag::new(0x003A, 0x0222);
const TAG_MULTIPLEX_GROUP_UID: Tag = Tag::new(0x003A, 0x0310);
const TAG_REFERENCED_WAVEFORM_CHANNELS: Tag = Tag::new(0x0040, 0xA0B0);
const TAG_MEASUREMENT_UNITS_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0x08EA);
const TAG_TEMPORAL_RANGE_TYPE: Tag = Tag::new(0x0040, 0xA130);
const TAG_REFERENCED_SAMPLE_POSITIONS: Tag = Tag::new(0x0040, 0xA132);
const TAG_REFERENCED_TIME_OFFSETS: Tag = Tag::new(0x0040, 0xA138);
const TAG_REFERENCED_DATE_TIME: Tag = Tag::new(0x0040, 0xA13A);
const TAG_LEGACY_TEXT_VALUE: Tag = Tag::new(0x0040, 0xA160);
const TAG_CONCEPT_NAME_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0xA043);
const TAG_CONCEPT_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0xA168);
const TAG_ANNOTATION_GROUP_NUMBER: Tag = Tag::new(0x0040, 0xA180);
const TAG_NUMERIC_VALUE: Tag = Tag::new(0x0040, 0xA30A);
const TAG_WAVEFORM_ANNOTATION_SEQUENCE: Tag = Tag::new(0x0040, 0xB020);
const TAG_UNFORMATTED_TEXT_VALUE: Tag = Tag::new(0x0070, 0x0006);
const TAG_WAVEFORM_SEQUENCE: Tag = Tag::new(0x5400, 0x0100);
const TAG_WAVEFORM_BITS_ALLOCATED: Tag = Tag::new(0x5400, 0x1004);
pub(crate) const TAG_WAVEFORM_SAMPLE_INTERPRETATION: Tag = Tag::new(0x5400, 0x1006);
pub(crate) const TAG_WAVEFORM_PADDING_VALUE: Tag = Tag::new(0x5400, 0x100A);
const TAG_WAVEFORM_DATA: Tag = Tag::new(0x5400, 0x1010);
const DEFAULT_MAX_INDEX_ENTRIES: i64 = 1_000_000;
const DEFAULT_MAX_WIDTH: i64 = 16_384;
const DEFAULT_MAX_GROUPS: i64 = 256;
const DEFAULT_MAX_CHANNELS: i64 = 1_024;
pub(crate) fn trim_uid(uid: &str) -> &str {
    uid.trim_end_matches('\0').trim()
}
pub fn is_storage_sop_class(uid: &str) -> bool {
    let uid = trim_uid(uid);
    SUPPORTED.iter().any(|(known, _)| *known == uid)
}
pub fn is_waveform_sop_class(uid: &str) -> bool {
    let uid = trim_uid(uid);
    is_storage_sop_class(uid) || unsupported_reason(uid).is_some()
}
pub fn supported_storage_sop_class_uids() -> Vec<&'static str> {
    SUPPORTED.iter().map(|(uid, _)| *uid).collect()
}
pub fn open(file: &File, opts: Options) -> Result<Recording, Error> {
    open_sources(file, opts, None)
}
pub fn open_with_source_factory(
    file: &File,
    opts: Options,
    factory: &SourceFactory<'_>,
) -> Result<Recording, Error> {
    open_sources(file, opts, Some(factory))
}
fn open_sources(
    file: &File,
    opts: Options,
    factory: Option<&SourceFactory<'_>>,
) -> Result<Recording, Error> {
    let dataset = &file.dataset;
    let normalized = normalize_options(opts)?;
    let Some(sop_class_uid) = dataset.get_uid(TAG_SOP_CLASS_UID) else {
        return Err(Error::not_waveform("missing SOP Class UID"));
    };
    if !is_storage_sop_class(&sop_class_uid) {
        if let Some(reason) = unsupported_reason(&sop_class_uid) {
            return Err(Error::unsupported(RawFallback {
                group_index: -1,
                raw_bytes: raw_waveform_byte_count(dataset),
                reason: reason.to_string(),
                sop_class_uid,
                encoding: String::new(),
            }));
        }
        return Err(Error::not_waveform(&sop_class_uid));
    }
    let audio = is_audio_storage_sop_class(&sop_class_uid);
    let items = dataset
        .get_sequence(TAG_WAVEFORM_SEQUENCE)
        .unwrap_or_default();
    if items.is_empty() {
        if audio {
            return Err(audio_profile_error(
                dataset,
                &sop_class_uid,
                "missing Waveform Sequence",
            ));
        }
        return Err(Error::other(
            "waveform: supported SOP class has no Waveform Sequence",
        ));
    }
    if audio {
        validate_audio_profile(dataset, &sop_class_uid, &items)?;
    }
    if items.len() as i64 > normalized.max_groups {
        return Err(Error::other(format!(
            "waveform: Waveform Sequence has {} groups, limit {}",
            items.len(),
            normalized.max_groups
        )));
    }
    let mut groups = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let parsed = match parse_group(&sop_class_uid, i as i64, item, normalized, factory) {
            Ok(parsed) => parsed,
            Err(err) if audio => {
                return Err(audio_profile_error(
                    dataset,
                    &sop_class_uid,
                    &err.to_string(),
                ));
            }
            Err(err) => {
                return Err(err.prefixed(format!("waveform: parse multiplex group {}", i + 1)));
            }
        };
        if audio && !parsed.info.supported {
            return Err(audio_profile_error(
                dataset,
                &sop_class_uid,
                &parsed.info.fallback_reason,
            ));
        }
        groups.push(parsed);
    }
    assign_index_budgets(&mut groups, normalized.max_index_entries)?;
    let annotations = parse_annotations(dataset, &groups)
        .map_err(|err| err.prefixed("waveform: parse annotations"))?;
    Ok(Recording {
        closed: false,
        sop_class_uid,
        groups,
        annotations,
        options: normalized,
    })
}
fn normalize_options(opts: Options) -> Result<NormalizedOptions, Error> {
    for (value, name) in [
        (opts.max_index_entries, "MaxIndexEntries"),
        (opts.max_envelope_width, "MaxEnvelopeWidth"),
        (opts.max_groups, "MaxGroups"),
        (opts.max_channels_per_group, "MaxChannelsPerGroup"),
    ] {
        if value < 0 {
            return Err(Error::other(format!(
                "waveform: {name} must not be negative"
            )));
        }
    }
    let or = |value: i64, default: i64| if value == 0 { default } else { value };
    Ok(NormalizedOptions {
        max_index_entries: or(opts.max_index_entries, DEFAULT_MAX_INDEX_ENTRIES),
        max_width: or(opts.max_envelope_width, DEFAULT_MAX_WIDTH),
        max_groups: or(opts.max_groups, DEFAULT_MAX_GROUPS),
        max_channels: or(opts.max_channels_per_group, DEFAULT_MAX_CHANNELS),
    })
}
fn assign_index_budgets(groups: &mut [Group], max_entries: i64) -> Result<(), Error> {
    let mut minimum = 0i64;
    let mut total_weight = 0i64;
    let mut supported = Vec::new();
    let mut weights = Vec::new();
    for (position, group) in groups.iter_mut().enumerate() {
        if !group.info.supported {
            continue;
        }
        let channels = group.info.channels.len() as i64;
        if channels > max_entries - minimum {
            return Err(Error::other(format!(
                "waveform: MaxIndexEntries {max_entries} cannot retain one bucket for each channel across all groups"
            )));
        }
        minimum += channels;
        let weight = multiply_size(&[group.info.sample_count, channels])
            .unwrap_or(i64::MAX)
            .max(1);
        total_weight = total_weight.saturating_add(weight);
        supported.push(position);
        weights.push(weight);
        group.index_budget = channels;
    }
    let mut remaining_budget = max_entries - minimum;
    let mut remaining_weight = total_weight;
    for (i, &position) in supported.iter().enumerate() {
        if remaining_budget == 0 {
            break;
        }
        let mut extra = remaining_budget;
        if i != supported.len() - 1 && remaining_weight > 0 {
            extra =
                f64_to_i64(remaining_budget as f64 * (weights[i] as f64 / remaining_weight as f64))
                    .clamp(0, remaining_budget);
        }
        groups[position].index_budget += extra;
        remaining_budget -= extra;
        remaining_weight = if weights[i] < remaining_weight {
            remaining_weight - weights[i]
        } else {
            0
        };
    }
    Ok(())
}
fn validate_waveform_data_vr(vr: Vr, bits_allocated: i64) -> Result<(), Error> {
    if vr != Vr::OB && vr != Vr::OW {
        return Err(Error::other(format!(
            "Waveform Data has VR {vr}, want OB or OW"
        )));
    }
    if bits_allocated != 8 && vr != Vr::OW {
        return Err(Error::other(format!(
            "Waveform Data with {bits_allocated} bits allocated requires OW, got {vr}"
        )));
    }
    Ok(())
}
pub(crate) fn validate_sop_sample_interpretation(
    sop_class_uid: &str,
    interpretation: &str,
) -> Result<(), Error> {
    let profile: &[&str] = SUPPORTED
        .iter()
        .find(|(uid, _)| *uid == sop_class_uid)
        .map_or(&[], |(_, terms)| *terms);
    if profile.contains(&interpretation) {
        return Ok(());
    }
    Err(Error::other(format!(
        "SOP Class {sop_class_uid} permits sample interpretation {}, got {interpretation}",
        profile.join(" or ")
    )))
}
fn parse_group(
    sop_class_uid: &str,
    index: i64,
    item: &Object,
    opts: NormalizedOptions,
    factory: Option<&SourceFactory<'_>>,
) -> Result<Group, Error> {
    let channel_count = required_positive_int(
        item,
        TAG_NUMBER_OF_WAVEFORM_CHANNELS,
        "Number of Waveform Channels",
    )?;
    if channel_count > opts.max_channels {
        return Err(Error::other(format!(
            "Number of Waveform Channels {channel_count} exceeds limit {}",
            opts.max_channels
        )));
    }
    let sample_count = required_positive_int(
        item,
        TAG_NUMBER_OF_WAVEFORM_SAMPLES,
        "Number of Waveform Samples",
    )?;
    let frequency = required_positive_float(item, TAG_SAMPLING_FREQUENCY, "Sampling Frequency")?;
    let bits_allocated =
        required_positive_int(item, TAG_WAVEFORM_BITS_ALLOCATED, "Waveform Bits Allocated")?;
    let interpretation = item
        .get_string(TAG_WAVEFORM_SAMPLE_INTERPRETATION)
        .map(|s| to_upper(s.trim()))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::other("missing Waveform Sample Interpretation"))?;
    let channel_items = item
        .get_sequence(TAG_CHANNEL_DEFINITION_SEQUENCE)
        .unwrap_or_default();
    if channel_items.len() as i64 != channel_count {
        return Err(Error::other(format!(
            "Channel Definition Sequence has {} items, want {channel_count}",
            channel_items.len()
        )));
    }
    let mut channels = Vec::with_capacity(channel_items.len());
    for (i, channel_item) in channel_items.iter().enumerate() {
        let channel = parse_channel(i as i64, channel_item, frequency, bits_allocated)
            .map_err(|err| err.prefixed(format!("channel {}", i + 1)))?;
        channels.push(channel);
    }
    let Some(data_element) = item.get(TAG_WAVEFORM_DATA) else {
        return Err(Error::other("missing Waveform Data"));
    };
    validate_waveform_data_vr(data_element.vr(), bits_allocated)?;
    let source = match factory {
        Some(factory) => Source::External(
            factory(index, data_element)
                .map_err(|e| Error::other(format!("open Waveform Data source: {e}")))?,
        ),
        None => Source::Bytes(
            data_element
                .raw()
                .ok_or_else(|| {
                    Error::other(
                        "open Waveform Data source: Waveform Data is deferred; configure Options.SourceFactory",
                    )
                })?
                .to_vec(),
        ),
    };
    let source_size = source.size();
    let decoder = SampleDecoder::new(item.value_big_endian(), bits_allocated, &interpretation)
        .and_then(|decoder| {
            validate_sop_sample_interpretation(sop_class_uid, &interpretation).map(|()| decoder)
        });
    let info = GroupInfo {
        index,
        label: optional_string(item, TAG_MULTIPLEX_GROUP_LABEL),
        uid: optional_string(item, TAG_MULTIPLEX_GROUP_UID),
        originality: optional_string(item, TAG_WAVEFORM_ORIGINALITY),
        time_offset: duration_from_float(
            optional_float(item, TAG_MULTIPLEX_GROUP_TIME_OFFSET) / 1000.0,
        ),
        sampling_frequency_hz: frequency,
        sample_count,
        duration: duration_from_float(sample_count as f64 / frequency),
        bits_allocated,
        sample_interpretation: interpretation,
        channels,
        supported: decoder.is_ok(),
        fallback_reason: String::new(),
        raw_data_bytes: source_size,
    };
    let mut parsed = Group {
        info,
        source: Some(source),
        decoder: SampleDecoder::default(),
        padding_bits: Vec::new(),
        has_padding: false,
        index: None,
        index_err: None,
        index_budget: 0,
    };
    let mut padding = None;
    if let Some(padding_element) = item.get(TAG_WAVEFORM_PADDING_VALUE) {
        if padding_element.vr() != data_element.vr() {
            return Err(Error::other(format!(
                "Waveform Padding Value VR {} differs from Waveform Data VR {}",
                padding_element.vr(),
                data_element.vr()
            )));
        }
        let Some(raw) = padding_element.raw() else {
            return Err(Error::other("Waveform Padding Value is not a raw sample"));
        };
        let width = bits_allocated / 8;
        let max_length = if width == 1 { 2 } else { width };
        if width <= 0 || (raw.len() as i64) < width || raw.len() as i64 > max_length {
            return Err(Error::other(format!(
                "Waveform Padding Value has {} bytes, want {width}",
                raw.len()
            )));
        }
        padding = Some(raw);
    }
    let decoder = match decoder {
        Ok(decoder) => decoder,
        Err(err) => {
            parsed.info.fallback_reason = err.to_string();
            return Ok(parsed);
        }
    };
    let Some(expected) = multiply_size(&[sample_count, channel_count, decoder.width as i64]) else {
        return Err(Error::other("Waveform Data size overflows int64"));
    };
    if source_size < expected || source_size > expected + expected % 2 {
        return Err(Error::other(format!(
            "Waveform Data has {source_size} bytes, want {expected} data bytes plus optional value padding"
        )));
    }
    if let Some(raw) = padding {
        for (channel_index, channel) in parsed.info.channels.iter().enumerate() {
            let (_, bits) = decoder
                .decode_bytes(&raw[..decoder.width], channel.bits_stored)
                .map_err(|err| {
                    err.prefixed(format!(
                        "Waveform Padding Value for channel {}",
                        channel_index + 1
                    ))
                })?;
            parsed.padding_bits.push(bits);
        }
        parsed.has_padding = true;
    }
    parsed.decoder = decoder;
    Ok(parsed)
}
fn parse_channel(
    index: i64,
    item: &Object,
    frequency: f64,
    bits_allocated: i64,
) -> Result<ChannelInfo, Error> {
    let mut number = index + 1;
    let encoded = optional_int(item, TAG_WAVEFORM_CHANNEL_NUMBER)?;
    if encoded != 0 {
        number = encoded;
    }
    let bits_stored =
        required_positive_int(item, TAG_WAVEFORM_BITS_STORED, "Waveform Bits Stored")?;
    if bits_stored > bits_allocated {
        return Err(Error::other(format!(
            "Waveform Bits Stored {bits_stored} exceeds Bits Allocated {bits_allocated}"
        )));
    }
    let mut calibration = Calibration {
        correction_factor: 1.0,
        ..Calibration::default()
    };
    if let Some(sensitivity) = lookup_optional_float(item, TAG_CHANNEL_SENSITIVITY)? {
        calibration.sensitivity = sensitivity;
        calibration.has_sensitivity = true;
    }
    if let Some(correction) = lookup_optional_float(item, TAG_CHANNEL_SENSITIVITY_CORRECTION)? {
        calibration.correction_factor = correction;
        calibration.has_correction_factor = true;
    }
    if let Some(baseline) = lookup_optional_float(item, TAG_CHANNEL_BASELINE)? {
        calibration.baseline = baseline;
        calibration.has_baseline = true;
    }
    if let Some(units) = single_code(item, TAG_CHANNEL_SENSITIVITY_UNITS)? {
        calibration.units = units;
    }
    let units = &calibration.units;
    calibration.status = if !calibration.has_sensitivity {
        CalibrationStatus::MissingSensitivity
    } else if units.value.is_empty() || units.scheme.is_empty() || units.meaning.is_empty() {
        CalibrationStatus::MissingUnits
    } else if !calibration.has_correction_factor || !calibration.has_baseline {
        CalibrationStatus::MissingParameters
    } else {
        CalibrationStatus::Complete
    };
    let time_skew = lookup_optional_float(item, TAG_CHANNEL_TIME_SKEW)?;
    let sample_skew = lookup_optional_float(item, TAG_CHANNEL_SAMPLE_SKEW)?.unwrap_or(0.0);
    let time_skew = time_skew.unwrap_or(sample_skew / frequency);
    let channel_offset = lookup_optional_float(item, TAG_CHANNEL_OFFSET)?.unwrap_or(0.0);
    Ok(ChannelInfo {
        index,
        number,
        label: optional_string(item, TAG_CHANNEL_LABEL),
        status: item.get_strings(TAG_CHANNEL_STATUS).unwrap_or_default(),
        source: first_code(item, TAG_CHANNEL_SOURCE_SEQUENCE).unwrap_or_default(),
        calibration,
        time_skew: duration_from_float(time_skew),
        sample_skew,
        channel_offset: duration_from_float(channel_offset),
        bits_stored,
        low_frequency: optional_float(item, TAG_FILTER_LOW_FREQUENCY),
        high_frequency: optional_float(item, TAG_FILTER_HIGH_FREQUENCY),
        notch_frequency: optional_float(item, TAG_NOTCH_FILTER_FREQUENCY),
    })
}
fn parse_annotations(dataset: &Object, groups: &[Group]) -> Result<Vec<Annotation>, Error> {
    let Some(items) = dataset.get_sequence(TAG_WAVEFORM_ANNOTATION_SEQUENCE) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let mut annotation = Annotation {
            temporal_range_type: to_upper(&optional_string(item, TAG_TEMPORAL_RANGE_TYPE)),
            text: optional_string(item, TAG_UNFORMATTED_TEXT_VALUE),
            ..Annotation::default()
        };
        if annotation.text.is_empty() {
            annotation.text = optional_string(item, TAG_LEGACY_TEXT_VALUE);
        }
        annotation.annotation_group_number = optional_int(item, TAG_ANNOTATION_GROUP_NUMBER)
            .map_err(|err| err.prefixed(format!("annotation {n} group number")))?;
        if let Some(refs) = optional_uint_values(item, TAG_REFERENCED_WAVEFORM_CHANNELS)
            .map_err(|err| err.prefixed(format!("annotation {n} channel references")))?
        {
            if refs.len() % 2 != 0 {
                return Err(Error::other(format!(
                    "annotation {n} has odd Referenced Waveform Channels multiplicity"
                )));
            }
            for pair in refs.as_chunks::<2>().0 {
                annotation.channels.push(ChannelReference {
                    group_number: pair[0] as i64,
                    channel_number: pair[1] as i64,
                });
            }
        }
        if let Some(positions) = optional_uint_values(item, TAG_REFERENCED_SAMPLE_POSITIONS)
            .map_err(|err| err.prefixed(format!("annotation {n} sample positions")))?
        {
            annotation.referenced_sample_positions = positions;
        }
        for encoded in item
            .get_strings(TAG_REFERENCED_TIME_OFFSETS)
            .unwrap_or_default()
        {
            match numtext::parse_float(&encoded) {
                Ok(seconds) if seconds.is_finite() => annotation
                    .referenced_time_offsets
                    .push(duration_from_float(seconds)),
                _ => {
                    return Err(Error::other(format!(
                        "annotation {n} invalid time offset {}",
                        quote(&encoded)
                    )));
                }
            }
        }
        annotation.referenced_date_times = item
            .get_strings(TAG_REFERENCED_DATE_TIME)
            .unwrap_or_default();
        annotation.concept_name =
            first_code(item, TAG_CONCEPT_NAME_CODE_SEQUENCE).unwrap_or_default();
        annotation.concept_value = first_code(item, TAG_CONCEPT_CODE_SEQUENCE).unwrap_or_default();
        annotation.numeric_units =
            first_code(item, TAG_MEASUREMENT_UNITS_CODE_SEQUENCE).unwrap_or_default();
        for encoded in item.get_strings(TAG_NUMERIC_VALUE).unwrap_or_default() {
            match numtext::parse_float(&encoded) {
                Ok(value) if value.is_finite() => annotation.numeric_values.push(value),
                _ => {
                    return Err(Error::other(format!(
                        "annotation {n} invalid numeric value {}",
                        quote(&encoded)
                    )));
                }
            }
        }
        validate_annotation(n, &annotation, groups)?;
        out.push(annotation);
    }
    Ok(out)
}
fn validate_annotation(n: usize, annotation: &Annotation, groups: &[Group]) -> Result<(), Error> {
    if annotation.channels.is_empty() {
        return Err(Error::other(format!(
            "annotation {n} has no Referenced Waveform Channels"
        )));
    }
    let mut referenced_groups = Vec::new();
    for reference in &annotation.channels {
        if reference.group_number < 1 || reference.group_number > groups.len() as i64 {
            return Err(Error::other(format!(
                "annotation {n} references multiplex group {}, valid range is [1,{}]",
                reference.group_number,
                groups.len()
            )));
        }
        let channel_count = groups[(reference.group_number - 1) as usize]
            .info
            .channels
            .len() as i64;
        if reference.channel_number < 0 || reference.channel_number > channel_count {
            return Err(Error::other(format!(
                "annotation {n} references channel {} in group {}, valid range is [0,{channel_count}]",
                reference.channel_number, reference.group_number
            )));
        }
        if !referenced_groups.contains(&reference.group_number) {
            referenced_groups.push(reference.group_number);
        }
    }
    let position_count = annotation.referenced_sample_positions.len();
    let offset_count = annotation.referenced_time_offsets.len();
    let date_time_count = annotation.referenced_date_times.len();
    let coordinate_kinds = [position_count, offset_count, date_time_count]
        .iter()
        .filter(|&&count| count > 0)
        .count();
    let range_type = annotation.temporal_range_type.as_str();
    if range_type.is_empty() {
        if coordinate_kinds != 0 {
            return Err(Error::other(format!(
                "annotation {n} has temporal coordinates without Temporal Range Type"
            )));
        }
        return Ok(());
    }
    if coordinate_kinds != 1 {
        return Err(Error::other(format!(
            "annotation {n} Temporal Range Type requires exactly one temporal coordinate kind"
        )));
    }
    let count = position_count + offset_count + date_time_count;
    let valid_count = match range_type {
        "POINT" | "BEGIN" | "END" => count == 1,
        "SEGMENT" => count == 2,
        "MULTIPOINT" => count >= 2,
        "MULTISEGMENT" => count >= 4 && count.is_multiple_of(2),
        _ => {
            return Err(Error::other(format!(
                "annotation {n} has unsupported Temporal Range Type {}",
                quote(range_type)
            )));
        }
    };
    if !valid_count {
        return Err(Error::other(format!(
            "annotation {n} Temporal Range Type {range_type} has invalid coordinate cardinality {count}"
        )));
    }
    if position_count > 0 {
        if referenced_groups.len() != 1 {
            return Err(Error::other(format!(
                "annotation {n} sample positions reference more than one multiplex group"
            )));
        }
        let group_number = annotation.channels[0].group_number;
        let sample_count = groups[(group_number - 1) as usize].info.sample_count as u64;
        for &position in &annotation.referenced_sample_positions {
            if position < 1 || position > sample_count {
                return Err(Error::other(format!(
                    "annotation {n} sample position {position} is outside [1,{sample_count}]"
                )));
            }
        }
    }
    if annotation.referenced_time_offsets.iter().any(|&o| o < 0) {
        return Err(Error::other(format!(
            "annotation {n} has negative referenced time offset"
        )));
    }
    Ok(())
}
fn code_of(item: &Object) -> CodedConcept {
    CodedConcept {
        value: optional_string(item, TAG_CODE_VALUE),
        scheme: optional_string(item, TAG_CODING_SCHEME_DESIGNATOR),
        version: optional_string(item, TAG_CODING_SCHEME_VERSION),
        meaning: optional_string(item, TAG_CODE_MEANING),
    }
}
fn first_code(obj: &Object, tag: Tag) -> Option<CodedConcept> {
    obj.get_sequence(tag)?.first().map(code_of)
}
fn single_code(obj: &Object, tag: Tag) -> Result<Option<CodedConcept>, Error> {
    let Some(items) = obj.get_sequence(tag) else {
        return Ok(None);
    };
    if items.len() != 1 {
        return Err(Error::other(format!(
            "{tag} must contain exactly one coded item"
        )));
    }
    Ok(Some(code_of(&items[0])))
}
pub(crate) fn required_positive_int(obj: &Object, tag: Tag, name: &str) -> Result<i64, Error> {
    match optional_uint_values(obj, tag)?.as_deref() {
        Some(&[value]) if value != 0 && value <= i64::MAX as u64 => Ok(value as i64),
        _ => Err(Error::other(format!(
            "{name} must contain one positive integer"
        ))),
    }
}
pub(crate) fn required_positive_float(obj: &Object, tag: Tag, name: &str) -> Result<f64, Error> {
    match lookup_optional_float(obj, tag)? {
        Some(value) if value > 0.0 => Ok(value),
        _ => Err(Error::other(format!(
            "{name} must contain one positive finite number"
        ))),
    }
}
fn optional_int(obj: &Object, tag: Tag) -> Result<i64, Error> {
    let Some(element) = obj.get(tag) else {
        return Ok(0);
    };
    if element.vr() == Vr::IS {
        return obj
            .get_int(tag)
            .map_err(|err| Error::other(err.to_string()));
    }
    let values = optional_uint_values(obj, tag)?.unwrap_or_default();
    Ok(values.first().map_or(0, |&v| v as i64))
}
pub(crate) fn optional_string(obj: &Object, tag: Tag) -> String {
    obj.get_string(tag).unwrap_or_default()
}
fn optional_float(obj: &Object, tag: Tag) -> f64 {
    lookup_optional_float(obj, tag)
        .ok()
        .flatten()
        .unwrap_or(0.0)
}
fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Nil => "<nil>",
        Value::Raw(_) => "core.RawValue",
        Value::Strings(_) => "core.StringValue",
        Value::Sequence(_) => "core.SequenceValue",
        Value::Fragments { .. } => "core.FragmentSequence",
        Value::BulkData(_) => "core.BulkDataValue",
    }
}
pub(crate) fn lookup_optional_float(obj: &Object, tag: Tag) -> Result<Option<f64>, Error> {
    if obj.get(tag).is_none() {
        return Ok(None);
    }
    let encoded = obj
        .lookup_string(tag)
        .map_err(|err| Error::other(err.to_string()))?;
    let value = numtext::parse_float(&encoded)
        .map_err(|err| Error::other(format!("parse {tag} value {}: {err}", quote(&encoded))))?;
    if !value.is_finite() {
        return Err(Error::other(format!("{tag} must be finite")));
    }
    Ok(Some(value))
}
fn optional_uint_values(obj: &Object, tag: Tag) -> Result<Option<Vec<u64>>, Error> {
    let Some(element) = obj.get(tag) else {
        return Ok(None);
    };
    let Value::Raw(raw) = &element.value else {
        return Err(Error::other(format!(
            "{tag} has unsupported value type {}",
            value_type_name(&element.value)
        )));
    };
    let width = match element.vr() {
        Vr::US => 2,
        Vr::UL => 4,
        Vr::UV => 8,
        vr => {
            return Err(Error::other(format!(
                "{tag} has unsupported integer VR {vr}"
            )));
        }
    };
    if raw.len() % width != 0 {
        return Err(Error::other(format!(
            "{tag} byte length {} is not divisible by {width}",
            raw.len()
        )));
    }
    let big_endian = obj.value_big_endian();
    Ok(Some(
        raw.chunks_exact(width)
            .map(|chunk| read_uint(chunk, big_endian))
            .collect(),
    ))
}
fn read_uint(raw: &[u8], big_endian: bool) -> u64 {
    let mut value = 0u64;
    if big_endian {
        for &b in raw {
            value = value << 8 | u64::from(b);
        }
    } else {
        for &b in raw.iter().rev() {
            value = value << 8 | u64::from(b);
        }
    }
    value
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SampleDecoder {
    pub big_endian: bool,
    pub width: usize,
    pub bits: i64,
    pub signed: bool,
    pub law: &'static str,
}
impl SampleDecoder {
    fn new(big_endian: bool, bits: i64, interpretation: &str) -> Result<SampleDecoder, Error> {
        let (want_bits, signed, law) = match interpretation {
            "SB" => (8, true, ""),
            "UB" => (8, false, ""),
            "SS" => (16, true, ""),
            "US" => (16, false, ""),
            "SL" => (32, true, ""),
            "UL" => (32, false, ""),
            "SV" => (64, true, ""),
            "UV" => (64, false, ""),
            "MB" => (8, false, "MB"),
            "AB" => (8, false, "AB"),
            _ => {
                return Err(Error::other(format!(
                    "sample interpretation {} is not a supported integer encoding",
                    quote(interpretation)
                )));
            }
        };
        if bits != want_bits {
            return Err(Error::other(format!(
                "{interpretation} requires {want_bits} bits allocated, got {bits}"
            )));
        }
        Ok(SampleDecoder {
            big_endian,
            width: (bits / 8) as usize,
            bits,
            signed,
            law,
        })
    }
    pub fn decode_bytes(&self, raw: &[u8], bits_stored: i64) -> Result<(f64, u64), Error> {
        if bits_stored <= 0 || bits_stored > self.bits {
            return Err(Error::other(format!(
                "invalid bits stored {bits_stored} for {}-bit sample",
                self.bits
            )));
        }
        if !self.law.is_empty() {
            if bits_stored != 8 {
                return Err(Error::other("companded audio requires 8 bits stored"));
            }
            let value = super::audio::decode_dicom_law(raw[0], self.law);
            return Ok((f64::from(value), u64::from(raw[0])));
        }
        let mut bits = read_uint(&raw[..self.width], self.big_endian);
        if bits_stored < 64 {
            let mask = (1u64 << bits_stored) - 1;
            bits &= mask;
            if self.signed && bits & (1u64 << (bits_stored - 1)) != 0 {
                bits |= !mask;
            }
        }
        if self.signed {
            Ok((bits as i64 as f64, bits))
        } else {
            Ok((bits as f64, bits))
        }
    }
}
pub(crate) fn raw_waveform_byte_count(dataset: &Object) -> i64 {
    let mut total = 0i64;
    for item in dataset
        .get_sequence(TAG_WAVEFORM_SEQUENCE)
        .unwrap_or_default()
    {
        let Some(element) = item.get(TAG_WAVEFORM_DATA) else {
            continue;
        };
        total += encoded_bytes(element);
    }
    total
}
fn encoded_bytes(element: &Element) -> i64 {
    if let Some(raw) = element.raw() {
        raw.len() as i64
    } else if element.header.length_set && element.header.length != UNDEFINED_LENGTH {
        i64::from(element.header.length)
    } else {
        0
    }
}
pub(crate) fn multiply_size(values: &[i64]) -> Option<i64> {
    let mut result = 1i64;
    for &value in values {
        if value < 0 || (value != 0 && result > i64::MAX / value) {
            return None;
        }
        result *= value;
    }
    Some(result)
}
pub(crate) fn duration_from_float(seconds: f64) -> Duration {
    if !seconds.is_finite() {
        return 0;
    }
    let scaled = seconds * SECOND as f64;
    if scaled > i64::MAX as f64 {
        return i64::MAX;
    }
    if scaled < i64::MIN as f64 {
        return i64::MIN;
    }
    f64_to_i64(scaled.round())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn predicates() {
        assert!(is_storage_sop_class(" 1.2.840.10008.5.1.4.1.1.9.1.2\0"));
        assert!(!is_storage_sop_class(UID_WAVEFORM_TRIAL));
        assert!(is_waveform_sop_class(UID_ULTRASOUND_WAVEFORM));
        assert_eq!(supported_storage_sop_class_uids().len(), 16);
    }
    #[test]
    fn decodes_samples() {
        let d = SampleDecoder::new(false, 16, "SS").unwrap();
        assert_eq!(d.decode_bytes(&[0xFF, 0xFF], 16).unwrap(), (-1.0, u64::MAX));
        assert_eq!(d.decode_bytes(&[0x00, 0x08], 12).unwrap().0, -2048.0);
        let be = SampleDecoder::new(true, 16, "US").unwrap();
        assert_eq!(be.decode_bytes(&[0x01, 0x02], 16).unwrap(), (258.0, 258));
        assert!(SampleDecoder::new(false, 8, "SS").is_err());
    }
    #[test]
    fn durations() {
        assert_eq!(duration_from_float(1.5), 1_500_000_000);
        assert_eq!(duration_from_float(f64::NAN), 0);
        assert_eq!(duration_from_float(1e30), i64::MAX);
        assert_eq!(multiply_size(&[i64::MAX, 2]), None);
    }
}
