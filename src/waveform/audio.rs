use super::parse::{
    TAG_CHANNEL_DEFINITION_SEQUENCE, TAG_CHANNEL_OFFSET, TAG_CHANNEL_SAMPLE_SKEW,
    TAG_CHANNEL_TIME_SKEW, TAG_NUMBER_OF_WAVEFORM_CHANNELS, TAG_SAMPLING_FREQUENCY,
    TAG_WAVEFORM_BITS_STORED, TAG_WAVEFORM_PADDING_VALUE, TAG_WAVEFORM_SAMPLE_INTERPRETATION,
    UID_BASIC_VOICE_AUDIO, UID_GENERAL_AUDIO, lookup_optional_float, optional_string,
    raw_waveform_byte_count, required_positive_float, required_positive_int, trim_uid,
    validate_sop_sample_interpretation,
};
use super::{Error, RawFallback, Recording};
use crate::fmath::f64_to_i64;
use crate::object::{Object, Tag};
use crate::textfmt::to_upper;
pub const MAX_AUDIO_RANGE_FRAMES: i64 = 32768;
pub fn is_audio_storage_sop_class(uid: &str) -> bool {
    let uid = trim_uid(uid);
    uid == UID_BASIC_VOICE_AUDIO || uid == UID_GENERAL_AUDIO
}
pub(crate) fn audio_profile_error(ds: &Object, uid: &str, reason: &str) -> Error {
    Error::unsupported(RawFallback {
        sop_class_uid: uid.to_string(),
        group_index: 0,
        encoding: String::new(),
        raw_bytes: raw_waveform_byte_count(ds),
        reason: format!("audio profile: {reason}"),
    })
}
pub(crate) fn validate_audio_profile(
    ds: &Object,
    uid: &str,
    items: &[Object],
) -> Result<(), Error> {
    let fail = |reason: &str| Err(audio_profile_error(ds, uid, reason));
    if optional_string(ds, Tag::new(0x0008, 0x0060)) != "AU" {
        return fail("Modality must be AU");
    }
    let [item] = items else {
        return fail("exactly one multiplex group is required");
    };
    let n = match required_positive_int(item, TAG_NUMBER_OF_WAVEFORM_CHANNELS, "channels") {
        Ok(n) if n <= 2 => n,
        _ => return fail("one or two audio channels are required"),
    };
    match required_positive_float(item, TAG_SAMPLING_FREQUENCY, "frequency") {
        Ok(f) if f <= 44100.0 && (uid != UID_BASIC_VOICE_AUDIO || f == 8000.0) => {}
        _ => {
            return fail(
                "Basic Voice requires 8000 Hz; General Audio requires positive frequency at most 44100 Hz",
            );
        }
    }
    let interpretation = to_upper(&optional_string(item, TAG_WAVEFORM_SAMPLE_INTERPRETATION));
    if let Err(err) = validate_sop_sample_interpretation(uid, &interpretation) {
        return fail(&err.to_string());
    }
    if item.get(TAG_WAVEFORM_PADDING_VALUE).is_some() {
        return fail("padding samples are outside the audio playback profile");
    }
    let channels = item
        .get_sequence(TAG_CHANNEL_DEFINITION_SEQUENCE)
        .unwrap_or_default();
    if item.get_sequence(TAG_CHANNEL_DEFINITION_SEQUENCE).is_none() || channels.len() as i64 != n {
        return fail("channel definitions must match the encoded channel count");
    }
    for channel in &channels {
        if interpretation == "AB" || interpretation == "MB" {
            let bits =
                required_positive_int(channel, TAG_WAVEFORM_BITS_STORED, "companded bits stored");
            if bits != Ok(8) {
                return fail("G.711 audio requires 8 bits stored");
            }
        }
        for tag in [
            TAG_CHANNEL_TIME_SKEW,
            TAG_CHANNEL_SAMPLE_SKEW,
            TAG_CHANNEL_OFFSET,
        ] {
            if channel.get(tag).is_some() && lookup_optional_float(channel, tag) != Ok(Some(0.0)) {
                return fail("channel skew/offset must be zero for aligned audio");
            }
        }
    }
    Ok(())
}
impl Recording {
    pub fn audio_pcm(
        &self,
        start: i64,
        frames: i64,
        channels: Option<&[i64]>,
    ) -> Result<Vec<i16>, Error> {
        if self.closed {
            return Err(Error::closed());
        }
        if !is_audio_storage_sop_class(&self.sop_class_uid) || self.groups.len() != 1 {
            return Err(Error::unsupported(RawFallback {
                sop_class_uid: self.sop_class_uid.clone(),
                reason: "recording is not a supported audio IOD".to_string(),
                ..RawFallback::default()
            }));
        }
        let g = &self.groups[0];
        let channel_count = g.info.channels.len() as i64;
        let default: &[i64] = if channel_count == 2 { &[0, 1] } else { &[0] };
        let channels = channels.unwrap_or(default);
        if start < 0
            || start > g.info.sample_count
            || !(0..=MAX_AUDIO_RANGE_FRAMES).contains(&frames)
            || frames > g.info.sample_count - start
            || channels.is_empty()
            || channels.len() > 2
        {
            return Err(Error::audio_range());
        }
        for (j, &ch) in channels.iter().enumerate() {
            if ch < 0 || ch >= channel_count || (j > 0 && ch == channels[0]) {
                return Err(Error::audio_range());
            }
        }
        let width = g.decoder.width;
        let stride = g.info.channels.len() * width;
        let frames = frames as usize;
        let source = g.source.as_ref().ok_or_else(Error::closed)?;
        let first = start * stride as i64;
        let raw = if frames * stride == 0 {
            std::borrow::Cow::Borrowed(&[][..])
        } else {
            source.read_at(first, frames * stride).map_err(|e| {
                Error::other(match e {
                    Some(e) => format!("audio range source: unexpected EOF\n{e}"),
                    None => "audio range source: unexpected EOF".to_string(),
                })
            })?
        };
        let mut pcm = Vec::with_capacity(frames * channels.len());
        for frame in 0..frames {
            for &ch in channels {
                let ch = ch as usize;
                let at = frame * stride + ch * width;
                let bits = g.info.channels[ch].bits_stored;
                let (mut value, _) = g.decoder.decode_bytes(&raw[at..at + width], bits)?;
                if g.decoder.law.is_empty() {
                    if !g.decoder.signed {
                        value -= (1u64 << (bits - 1)) as f64;
                    }
                    value *= (1u64 << (16 - bits)) as f64;
                }
                pcm.push(f64_to_i64(value) as i16);
            }
        }
        Ok(pcm)
    }
}
pub(crate) fn decode_dicom_law(code: u8, law: &str) -> i16 {
    if law == "AB" {
        let segment = i32::from(code >> 4) & 7;
        let mut magnitude = i32::from(code & 15) * 16 + 8;
        if segment > 0 {
            magnitude += 256;
            magnitude <<= segment - 1;
        }
        if code & 128 == 0 {
            magnitude = -magnitude;
        }
        return magnitude as i16;
    }
    let inverted = !code;
    let segment = i32::from(inverted >> 4) & 7;
    let mut magnitude = ((i32::from(inverted & 15) * 8 + 132) << segment) - 132;
    if inverted & 128 != 0 {
        magnitude = -magnitude;
    }
    magnitude as i16
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn g711() {
        assert_eq!(decode_dicom_law(0xFF, "MB"), 0);
        assert_eq!(decode_dicom_law(0x00, "MB"), -32124);
        assert_eq!(decode_dicom_law(0x80, "AB"), 8);
        assert_eq!(decode_dicom_law(0x00, "AB"), -8);
        assert!(is_audio_storage_sop_class(
            " 1.2.840.10008.5.1.4.1.1.9.4.2\0"
        ));
    }
}
