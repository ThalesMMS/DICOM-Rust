#[cfg(windows)]
extern crate dicom_rust_fuzz_sancov_sections as _;
pub mod extra;
pub mod seeds;
use dicom_rust::limits::Limits;
use dicom_rust::object::{self, ReadFileOptions, Syntax};
use dicom_rust::pixel::pixeldata::{self, CodecRecovery};
use std::rc::Rc;
pub type Target = (&'static str, fn(&[u8]));
pub const TARGETS: &[Target] = &[
    ("file_reader", file_reader),
    ("dataset_implicit_le", dataset_implicit_le),
    ("dataset_explicit_le", dataset_explicit_le),
    ("dataset_explicit_be", dataset_explicit_be),
    ("dataset_deflated", dataset_deflated),
    ("dataset_encapsulated", dataset_encapsulated),
    ("ul_pdu", ul_pdu),
    ("dimse_command", dimse_command),
    ("dicomdir", dicomdir),
    ("dicomjson", dicomjson),
    ("dicomxml", dicomxml),
    ("pixel_native", pixel_native),
    ("codec_rle", codec_rle),
    ("codec_jpeg", codec_jpeg),
    ("codec_jpeg_lossless", codec_jpeg_lossless),
    ("codec_jpegls", codec_jpegls),
    ("codec_jpeg2000", codec_jpeg2000),
    ("codec_htj2k", codec_htj2k),
    ("codec_jpegxl", codec_jpegxl),
    ("file_deferred", extra::file_deferred),
    ("frame_reader", extra::frame_reader),
    ("probe", extra::probe),
    ("encapsulated_layout", extra::encapsulated_layout),
    ("vendor_headers", extra::vendor_headers),
    ("dicomweb_response", extra::dicomweb_response),
    ("transcode", extra::transcode),
];
pub fn target(name: &str) -> Option<fn(&[u8])> {
    TARGETS.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}
pub const FUZZ_LIMITS: Limits = Limits {
    max_element_bytes: 8 << 20,
    max_pixel_data_bytes: 16 << 20,
    max_data_set_bytes: 0,
    max_inflated_bytes: 16 << 20,
    max_sequence_depth: 32,
    max_elements: 100_000,
    max_items: 20_000,
    max_fragments: 10_000,
    max_rows: 512,
    max_columns: 512,
    max_frames: 16,
    max_frame_bytes: 4 << 20,
    max_pdu_length: 1 << 16,
    max_association_pdu_length: 1 << 16,
    max_associations: 4,
};
fn read_options(variant: u8) -> ReadFileOptions {
    let mut o = ReadFileOptions {
        limits: FUZZ_LIMITS,
        ..ReadFileOptions::default()
    };
    match variant % 4 {
        0 => {}
        1 => {
            o.skip_pixel_data = true;
            o.allow_partial_read = true;
        }
        2 => {
            o.allow_missing_meta_element_group_length = true;
            o.allow_implicit_vr_file_meta = true;
            o.accept_odd_length = true;
            o.recover_container_overruns = true;
            o.recover_implicit_vr_elements = true;
            o.recover_delimiter_anomalies = true;
            o.allow_zlib_deflate = true;
        }
        _ => o.allow_incomplete_pixel_data = true,
    }
    o
}
fn touch(obj: &object::Object, depth: usize) {
    for e in obj.elements().iter().take(4096) {
        let tag = e.header.tag;
        let _ = obj.get_strings(tag);
        let _ = obj.get_ints(tag);
        let _ = obj.get_floats(tag);
        let _ = obj.get_person_names(tag);
        if depth < 8
            && let Some(items) = obj.get_sequence(tag)
        {
            for item in items.iter().take(64) {
                touch(item, depth + 1);
            }
        }
    }
    let _ = obj.character_set();
    let _ = pixeldata::extract_metadata(obj);
    let _ = pixeldata::extract_view(obj);
}
pub fn file_reader(data: &[u8]) {
    for variant in 0..4 {
        match object::read_file_with_options(data, read_options(variant)) {
            Ok(file) => {
                touch(&file.meta, 0);
                touch(&file.dataset, 0);
            }
            Err((Some(file), _)) => touch(&file.dataset, 0),
            Err((None, _)) => {}
        }
    }
}
fn data_set(data: &[u8], syntax: Syntax) {
    for variant in 0..3 {
        let source = Box::new(object::parser::source::MemFile::new(Rc::from(data)));
        if let Ok(obj) = object::read_data_set_with_options(source, syntax, read_options(variant)) {
            touch(&obj, 0);
        }
    }
}
fn syntax(uid: &str) -> Syntax {
    object::transfer::lookup(uid).unwrap_or(object::transfer::EXPLICIT_VR_LITTLE_ENDIAN)
}
pub fn dataset_implicit_le(data: &[u8]) {
    data_set(data, object::transfer::IMPLICIT_VR_LITTLE_ENDIAN);
}
pub fn dataset_explicit_le(data: &[u8]) {
    data_set(data, object::transfer::EXPLICIT_VR_LITTLE_ENDIAN);
}
pub fn dataset_explicit_be(data: &[u8]) {
    data_set(data, syntax("1.2.840.10008.1.2.2"));
}
pub fn dataset_deflated(data: &[u8]) {
    data_set(data, syntax("1.2.840.10008.1.2.1.99"));
}
pub fn dataset_encapsulated(data: &[u8]) {
    data_set(data, syntax("1.2.840.10008.1.2.4.90"));
}
pub fn ul_pdu(data: &[u8]) {
    use dicom_rust::net::ul;
    let mut stream: &[u8] = data;
    for _ in 0..64 {
        match ul::read_pdu_with_association_limit(
            &mut stream,
            FUZZ_LIMITS.max_pdu_length,
            FUZZ_LIMITS.max_association_pdu_length,
        ) {
            Ok(pdu) => {
                let _ = ul::encode_pdu(&pdu);
            }
            Err(_) => break,
        }
    }
    if let Some((&pdu_type, body)) = data.split_first() {
        let _ = ul::parse_pdu_body(pdu_type, body);
    }
}
pub fn dimse_command(data: &[u8]) {
    use dicom_rust::net::dimse;
    let Ok(command) = dimse::decode_command_set(data) else {
        return;
    };
    let _ = dimse::parse_c_echo_response(&command);
    let _ = dimse::parse_c_cancel_request(&command);
    let _ = dimse::parse_c_store_request(&command);
    let _ = dimse::parse_c_store_response(&command);
    let _ = dimse::parse_c_find_request(&command);
    let _ = dimse::parse_c_find_response(&command);
    let _ = dimse::parse_c_move_request(&command);
    let _ = dimse::parse_c_move_response(&command);
    let _ = dimse::parse_c_get_request(&command);
    let _ = dimse::parse_c_get_response(&command);
    for field in [0x0100u16, 0x0110, 0x0120, 0x0130, 0x0140, 0x0150] {
        let _ = dimse::parse_normalized_request(&command, field);
        let _ = dimse::parse_normalized_response(&command, field | 0x8000);
    }
}
pub fn dicomdir(data: &[u8]) {
    use dicom_rust::dicomdir;
    thread_local! {
        static DIR: std::path::PathBuf = {
            let dir = std::env::temp_dir().join(format!(
                "dicom-rust-fuzz-dicomdir-{}",
                std::process::id()
            ));
            let _ = std::fs::create_dir_all(&dir);
            dir
        };
    }
    DIR.with(|dir| {
        let path = dir.join("DICOMDIR");
        if std::fs::write(&path, data).is_err() {
            return;
        }
        let Some(path) = path.to_str() else { return };
        let _ = dicomdir::parse_file_set(path);
        let _ = dicomdir::referenced_paths(path);
    });
    if let Ok(file) = object::read_file_with_options(data, read_options(0)) {
        for reference in dicomdir::references(&file.dataset).iter().take(256) {
            let _ = dicomdir::resolve(".", reference);
        }
    }
}
pub fn dicomjson(data: &[u8]) {
    use dicom_rust::dicomjson;
    let opts = dicomjson::UnmarshalOptions::default();
    if let Ok(obj) = dicomjson::unmarshal(data, &opts) {
        touch(&obj, 0);
        let _ = dicomjson::marshal_compact(&obj);
    }
    let _ = dicomjson::decode_stream(data, &opts);
}
pub fn dicomxml(data: &[u8]) {
    use dicom_rust::dicomxml;
    let mut opts = dicomxml::UnmarshalOptions::default();
    opts.limits.max_xml_bytes = 1 << 20;
    if let Ok(obj) = dicomxml::unmarshal(data, &mut opts) {
        touch(&obj, 0);
        let _ = dicomxml::marshal(Some(&obj), &mut dicomxml::Options::default());
    }
}
pub struct ImageInput<'a> {
    pub syntax: u8,
    pub rows: u16,
    pub columns: u16,
    pub samples: u16,
    pub bits_allocated: u16,
    pub bits_stored: u16,
    pub signed: bool,
    pub planar: u16,
    pub photometric: &'static str,
    pub frames: u8,
    pub recover: bool,
    pub payload: &'a [u8],
}
pub const IMAGE_HEADER: usize = 10;
pub const PHOTOMETRICS: [&str; 8] = [
    "MONOCHROME2",
    "MONOCHROME1",
    "RGB",
    "YBR_FULL",
    "YBR_FULL_422",
    "YBR_RCT",
    "YBR_ICT",
    "PALETTE COLOR",
];
impl ImageInput<'_> {
    pub fn parse(data: &[u8]) -> Option<ImageInput<'_>> {
        if data.len() < IMAGE_HEADER {
            return None;
        }
        let (h, payload) = data.split_at(IMAGE_HEADER);
        let bits_allocated = [8u16, 16, 1, 32][usize::from(h[4] & 3)];
        Some(ImageInput {
            syntax: h[0],
            rows: u16::from(h[1]) + 1,
            columns: u16::from(h[2]) + 1,
            samples: [1u16, 3, 1, 4][usize::from(h[3] & 3)],
            bits_allocated,
            bits_stored: (u16::from(h[5]) % bits_allocated) + 1,
            signed: h[6] & 1 == 1,
            planar: u16::from(h[6] >> 1 & 1),
            photometric: PHOTOMETRICS[usize::from(h[7] & 7)],
            frames: h[8] % 4 + 1,
            recover: h[9] & 1 == 1,
            payload,
        })
    }
    pub fn file(&self, uid: &str, encapsulated: bool) -> Vec<u8> {
        let us = |v: u16| v.to_le_bytes().to_vec();
        let mut body = Vec::new();
        body.extend(seeds::element(0x0028, 0x0002, "US", &us(self.samples)));
        body.extend(seeds::element(
            0x0028,
            0x0004,
            "CS",
            self.photometric.as_bytes(),
        ));
        if self.samples > 1 {
            body.extend(seeds::element(0x0028, 0x0006, "US", &us(self.planar)));
        }
        body.extend(seeds::element(
            0x0028,
            0x0008,
            "IS",
            self.frames.to_string().as_bytes(),
        ));
        body.extend(seeds::element(0x0028, 0x0010, "US", &us(self.rows)));
        body.extend(seeds::element(0x0028, 0x0011, "US", &us(self.columns)));
        body.extend(seeds::element(
            0x0028,
            0x0100,
            "US",
            &us(self.bits_allocated),
        ));
        body.extend(seeds::element(0x0028, 0x0101, "US", &us(self.bits_stored)));
        body.extend(seeds::element(
            0x0028,
            0x0102,
            "US",
            &us(self.bits_stored - 1),
        ));
        body.extend(seeds::element(
            0x0028,
            0x0103,
            "US",
            &us(u16::from(self.signed)),
        ));
        if self.photometric == "PALETTE COLOR" {
            for e in 1..=3u16 {
                body.extend(seeds::element(
                    0x0028,
                    0x1100 + e,
                    "US",
                    &[0, 1, 0, 0, 8, 0],
                ));
            }
            for e in 1..=3u16 {
                body.extend(seeds::element(0x0028, 0x1200 + e, "OW", &[0u8; 256]));
            }
        }
        if encapsulated {
            let frames = usize::from(self.frames);
            let step = self.payload.len().div_ceil(frames).max(1);
            let fragments: Vec<&[u8]> = self.payload.chunks(step).collect();
            body.extend(seeds::encapsulated_pixel_data(&fragments));
        } else {
            body.extend(seeds::element(0x7FE0, 0x0010, "OW", self.payload));
        }
        seeds::part10(uid, &body)
    }
}
fn decode_image(input: &ImageInput<'_>, uid: &str, encapsulated: bool) {
    let bytes = input.file(uid, encapsulated);
    let Ok(file) = object::read_file_with_options(&bytes, read_options(0)) else {
        return;
    };
    let Ok(pixel) = pixeldata::extract_view(&file.dataset) else {
        return;
    };
    let recovery = if input.recover {
        CodecRecovery {
            ignore_inconsistent_basic_offset_table: true,
            jpeg_lossless_encoder_quirks: true,
            jpeg_lossless_predictor6_overflow: true,
            rle_segment_overrun: true,
            rle_reversed_byte_segments: true,
            jpeg_sequential_scan_quirks: true,
            jpegls_jai_thresholds: true,
            jpegls_hp_color_transforms: true,
            ..CodecRecovery::default()
        }
    } else {
        CodecRecovery::default()
    };
    let _ =
        pixeldata::decode_frames_with_limits(uid, &pixel, &file.dataset, &recovery, &FUZZ_LIMITS);
    let _ = dicom_rust::pixel::guard::guard_file(
        &file,
        &dicom_rust::pixel::guard::GuardOptions {
            limits: FUZZ_LIMITS.guard_limits(),
            allow_trailing: input.recover,
            frame: None,
        },
    );
}
fn codec(data: &[u8], uids: &[&str]) {
    if let Some(input) = ImageInput::parse(data) {
        decode_image(&input, uids[usize::from(input.syntax) % uids.len()], true);
    }
}
pub const NATIVE_UIDS: &[&str] = &[
    "1.2.840.10008.1.2.1",
    "1.2.840.10008.1.2.2",
    "1.2.840.10008.1.2.1.98",
];
pub const RLE_UIDS: &[&str] = &["1.2.840.10008.1.2.5"];
pub const JPEG_UIDS: &[&str] = &[
    "1.2.840.10008.1.2.4.50",
    "1.2.840.10008.1.2.4.51",
    "1.2.840.10008.1.2.4.53",
    "1.2.840.10008.1.2.4.55",
];
pub const JPEG_LOSSLESS_UIDS: &[&str] = &["1.2.840.10008.1.2.4.57", "1.2.840.10008.1.2.4.70"];
pub const JPEGLS_UIDS: &[&str] = &["1.2.840.10008.1.2.4.80", "1.2.840.10008.1.2.4.81"];
pub const JPEG2000_UIDS: &[&str] = &[
    "1.2.840.10008.1.2.4.90",
    "1.2.840.10008.1.2.4.91",
    "1.2.840.10008.1.2.4.92",
    "1.2.840.10008.1.2.4.93",
];
pub const HTJ2K_UIDS: &[&str] = &[
    "1.2.840.10008.1.2.4.201",
    "1.2.840.10008.1.2.4.202",
    "1.2.840.10008.1.2.4.203",
];
pub const JPEGXL_UIDS: &[&str] = &[
    "1.2.840.10008.1.2.4.110",
    "1.2.840.10008.1.2.4.111",
    "1.2.840.10008.1.2.4.112",
];
pub fn pixel_native(data: &[u8]) {
    if let Some(input) = ImageInput::parse(data) {
        let uid = NATIVE_UIDS[usize::from(input.syntax) % NATIVE_UIDS.len()];
        decode_image(&input, uid, uid.ends_with(".98"));
    }
}
pub fn codec_rle(data: &[u8]) {
    codec(data, RLE_UIDS);
}
pub fn codec_jpeg(data: &[u8]) {
    codec(data, JPEG_UIDS);
}
pub fn codec_jpeg_lossless(data: &[u8]) {
    codec(data, JPEG_LOSSLESS_UIDS);
}
pub fn codec_jpegls(data: &[u8]) {
    codec(data, JPEGLS_UIDS);
}
pub fn codec_jpeg2000(data: &[u8]) {
    codec(data, JPEG2000_UIDS);
}
pub fn codec_htj2k(data: &[u8]) {
    codec(data, HTJ2K_UIDS);
}
pub fn codec_jpegxl(data: &[u8]) {
    codec(data, JPEGXL_UIDS);
}
