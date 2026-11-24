use crate::{FUZZ_LIMITS, read_options, touch};
use dicom_rust::object::{self, Tag};
use dicom_rust::pixel::encapsulated as enc;
pub fn file_deferred(data: &[u8]) {
    let defer = object::deferred::DeferOptions {
        defer_pixel_data: true,
        defer_waveform_data: true,
        inline_value_bytes_threshold: 64,
        defer_nested_values: true,
        ..Default::default()
    };
    if let Ok(file) = object::deferred::read_file_deferred(data, true, read_options(0), &defer) {
        touch(&file.dataset, 0);
        for e in file.dataset.elements().iter().take(256) {
            let mut out = Vec::new();
            let _ = file.dataset.copy_value_to(e.header.tag, &mut out);
            let _ = file.value_locations(e.header.tag);
        }
    }
    let prefix = object::read_ext::ReadExtras {
        stop_before_tag: Some(Tag::PIXEL_DATA),
        ..Default::default()
    };
    if let Ok(read) = object::read_ext::read_file_extended(data, read_options(1), &prefix) {
        touch(&read.file.dataset, 0);
    }
    let projection = object::read_ext::ReadExtras {
        data_set_tags: Some(vec![Tag::new(0x0010, 0x0010), Tag::new(0x0008, 0x0018)]),
        seek_unselected_sequences: true,
        ..Default::default()
    };
    if let Ok(read) = object::read_ext::read_file_extended(data, read_options(0), &projection) {
        touch(&read.file.dataset, 0);
    }
}
pub fn frame_reader(data: &[u8]) {
    use dicom_rust::pixel::frame_reader::{FrameReaderOptions, FrameSource};
    let Ok(source) = FrameSource::from_bytes(data.to_vec()) else {
        return;
    };
    let options = FrameReaderOptions {
        limits: FUZZ_LIMITS.decompress_limits(),
        max_frames: 16,
        max_fragments: 1024,
        ..Default::default()
    };
    let Ok(reader) = source.reader(options) else {
        return;
    };
    for i in 0..4 {
        let _ = reader.raw_frame_length(i);
        let _ = reader.raw_frame(i);
        if reader.frame(i).is_err() {
            break;
        }
    }
}
pub fn probe(data: &[u8]) {
    use dicom_rust::object::probe::{ProbeOptions, probe_transfer_syntax};
    let options = ProbeOptions {
        max_probe_bytes: 1 << 20,
        max_candidate_bytes: 1 << 20,
        max_elements: 10_000,
        max_sequence_depth: 32,
        max_fragments: 1_000,
        max_tokens: 20_000,
        max_duration: Some(std::time::Duration::from_secs(2)),
        max_candidate_duration: Some(std::time::Duration::from_millis(500)),
        ..Default::default()
    };
    let (report, _) = probe_transfer_syntax(data, &options);
    let _ = report.render();
}
pub fn encapsulated_layout(data: &[u8]) {
    let Some((&h, rest)) = data.split_first() else {
        return;
    };
    let format = [
        enc::Format::JPEG,
        enc::Format::JPEGLS,
        enc::Format::JPEG2000,
    ][usize::from(h % 3)];
    let frames = i64::from(h >> 2 & 7) + 1;
    let _ = enc::validate_frame(rest, format);
    let _ = enc::jpeg_scan_selection_values(rest);
    let _ = enc::parse_j2k_layout(rest);
    let _ = enc::trailing_pad(rest);
    let mut chunks: Vec<&[u8]> = Vec::new();
    let mut at = rest;
    while let [lo, hi, tail @ ..] = at {
        let n = usize::from(u16::from_le_bytes([*lo, *hi])).min(tail.len());
        let (chunk, next) = tail.split_at(n);
        chunks.push(chunk);
        at = next;
        if chunks.len() > 256 {
            break;
        }
    }
    let Some((&basic, fragments)) = chunks.split_first() else {
        return;
    };
    let table = (h & 0x20 != 0).then_some(enc::TableAttribute {
        vr_ov: true,
        value: enc::TableValue::Raw(basic),
    });
    let limits = enc::Limits {
        max_frames: 16,
        max_fragments: 1024,
        max_fragments_per_frame: 1024,
        max_bytes: 4 << 20,
        max_frame_bytes: 4 << 20,
    };
    let recovery = enc::Recovery {
        ignore_inconsistent_basic_offset_table: h & 0x40 != 0,
    };
    let _ = enc::read_tables(basic, table, table, frames);
    let eot = enc::EotAttributes {
        extended: table,
        lengths: table,
    };
    if let Ok(plan) = enc::from_fragments(basic, fragments, eot, frames, format, limits, recovery) {
        for i in 0..plan.len().min(16) {
            let _ = plan.frame_length(i);
            let _ = plan.frame(i);
        }
    }
}
pub fn vendor_headers(data: &[u8]) {
    use dicom_rust::siemens;
    let limits = siemens::CsaLimits {
        max_bytes: 4 << 20,
        max_elements: 4096,
        max_items: 4096,
    };
    if let Ok(header) = siemens::parse_csa_with_limits(data, limits) {
        let _ = siemens::encode_csa(&header.elements);
    }
    let _ = siemens::scrub_csa(data, limits);
    let _ = siemens::parse_medcom_history(data, siemens::MedComLimits::default());
    let _ = siemens::parse_ascconv(&String::from_utf8_lossy(data));
}
pub fn dicomweb_response(data: &[u8]) {
    use dicom_rust::net::dicomweb::{mimeheader, response};
    let _ = mimeheader::read_mime_header(data);
    let _ = mimeheader::parse_media_type(&String::from_utf8_lossy(data));
    let _ = response::dataset_from_json(data);
    let _ = response::datasets_from_dicom_json(data);
    let _ = response::store_result_from_dicom_json(data);
}
pub fn transcode_uids() -> Vec<&'static str> {
    [
        crate::RLE_UIDS,
        crate::JPEG_UIDS,
        crate::JPEG_LOSSLESS_UIDS,
        crate::JPEGLS_UIDS,
        crate::JPEG2000_UIDS,
        crate::HTJ2K_UIDS,
        crate::JPEGXL_UIDS,
    ]
    .concat()
}
pub fn transcode(data: &[u8]) {
    use dicom_rust::pixel::transcode;
    let Some(input) = crate::ImageInput::parse(data) else {
        return;
    };
    let uids = transcode_uids();
    let uid = uids[usize::from(input.syntax) % uids.len()];
    let bytes = input.file(uid, true);
    let Ok(file) = object::read_file_with_options(&bytes, read_options(0)) else {
        return;
    };
    let _ = transcode::transcode_file(&file, "1.2.840.10008.1.2.1", &transcode_options());
}
fn transcode_options() -> dicom_rust::pixel::transcode::TranscodeOptions {
    use dicom_rust::pixel::transcode::{DecoderRegistry, TranscodeLimits, TranscodeOptions};
    TranscodeOptions {
        decoder: Some(DecoderRegistry::BuiltinWithJpegXl),
        limits: TranscodeLimits {
            max_frames: 16,
            max_pixels: 1 << 20,
            max_input_bytes: 4 << 20,
            max_output_bytes: 16 << 20,
            max_fragments: 1024,
            max_elements: 100_000,
            max_depth: 32,
            max_expansion_ratio: 1024,
            max_duration_nanos: 0,
        },
        ..Default::default()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::seeds::seeds;
    #[test]
    fn file_deferred_seeds_read() {
        let defer = object::deferred::DeferOptions {
            defer_pixel_data: true,
            ..Default::default()
        };
        let read = seeds("file_deferred")
            .iter()
            .filter(|s| {
                object::deferred::read_file_deferred(s, true, read_options(0), &defer).is_ok()
            })
            .count();
        assert!(read >= 5, "{read} seeds read");
    }
    #[test]
    fn frame_reader_seeds_decode() {
        use dicom_rust::pixel::frame_reader::{FrameReaderOptions, FrameSource};
        let decoded = seeds("frame_reader")
            .into_iter()
            .filter(|s| {
                FrameSource::from_bytes(s.clone()).is_ok_and(|source| {
                    source
                        .reader(FrameReaderOptions::default())
                        .is_ok_and(|r| r.frame(0).is_ok())
                })
            })
            .count();
        assert!(decoded >= 10, "{decoded} seeds decoded");
    }
    #[test]
    fn probe_seeds_select_a_syntax() {
        use dicom_rust::object::probe::{ProbeOptions, probe_transfer_syntax};
        for seed in seeds("probe") {
            let (report, err) = probe_transfer_syntax(&seed, &ProbeOptions::default());
            assert!(err.is_none() && report.selected_candidate().is_some());
        }
    }
    #[test]
    fn encapsulated_layout_seeds_plan() {
        let mut planned = 0;
        for seed in seeds("encapsulated_layout") {
            let format = [
                enc::Format::JPEG,
                enc::Format::JPEGLS,
                enc::Format::JPEG2000,
            ][usize::from(seed[0] % 3)];
            let frames = i64::from(seed[0] >> 2 & 7) + 1;
            let mut chunks = Vec::new();
            let mut at = &seed[1..];
            while let [lo, hi, tail @ ..] = at {
                let n = usize::from(u16::from_le_bytes([*lo, *hi])).min(tail.len());
                chunks.push(&tail[..n]);
                at = &tail[n..];
            }
            let plan = enc::from_fragments(
                chunks[0],
                &chunks[1..],
                enc::EotAttributes::default(),
                frames,
                format,
                enc::Limits::default(),
                enc::Recovery::default(),
            );
            if plan.is_ok_and(|p| p.len() == frames as usize && p.frame(0).is_ok()) {
                planned += 1;
            }
        }
        assert!(planned >= 10, "{planned} seeds planned");
    }
    #[test]
    fn vendor_header_seeds_parse() {
        let all = seeds("vendor_headers");
        assert!(dicom_rust::siemens::parse_csa(&all[0]).is_ok_and(|h| h.elements.len() == 2));
        let text = String::from_utf8_lossy(&all[1]);
        assert!(dicom_rust::siemens::parse_ascconv(&text).is_ok());
    }
    #[test]
    fn transcode_seeds_decompress() {
        use dicom_rust::pixel::transcode;
        let uids = transcode_uids();
        let all = seeds("transcode");
        let mut done = 0;
        for input in all.iter().filter_map(|s| crate::ImageInput::parse(s)) {
            let bytes = input.file(uids[usize::from(input.syntax) % uids.len()], true);
            let file = object::read_file_with_options(&bytes, read_options(0)).map_err(|e| e.1);
            match file.and_then(|f| {
                transcode::transcode_file(&f, "1.2.840.10008.1.2.1", &transcode_options())
                    .map_err(|e| object::Error::msg(e.to_string()))
            }) {
                Ok(_) => done += 1,
                Err(e) => eprintln!(
                    "seed in {}: {e}",
                    uids[usize::from(input.syntax) % uids.len()]
                ),
            }
        }
        assert!(done >= 10, "{done} seeds transcoded");
    }
    #[test]
    fn dicomweb_response_seeds_decode() {
        use dicom_rust::net::dicomweb::{mimeheader, response};
        let all = seeds("dicomweb_response");
        assert!(all.iter().any(|s| response::dataset_from_json(s).is_ok()));
        assert!(all.iter().any(|s| mimeheader::read_mime_header(s).is_ok()));
        assert!(
            all.iter()
                .any(|s| response::store_result_from_dicom_json(s).is_ok())
        );
    }
}
