#[path = "support/build.rs"]
mod build;
use build::*;
use dicom_rust::object::{self, Object, Tag, Vr, writer};
use dicom_rust::pixel::transcode::{
    self as tc, DecoderRegistry, DecompressOptions, EncoderRegistry, FrameEncoder, TranscodeOptions,
};
struct Image {
    name: &'static str,
    rows: usize,
    cols: usize,
    spp: usize,
    allocated: u32,
    bits: u32,
    frames: usize,
}
const MONO8: Image = Image {
    name: "mono8",
    rows: 13,
    cols: 17,
    spp: 1,
    allocated: 8,
    bits: 8,
    frames: 1,
};
const MONO12: Image = Image {
    name: "mono12",
    rows: 16,
    cols: 16,
    spp: 1,
    allocated: 16,
    bits: 12,
    frames: 1,
};
const RGB8: Image = Image {
    name: "rgb8",
    rows: 9,
    cols: 15,
    spp: 3,
    allocated: 8,
    bits: 8,
    frames: 1,
};
const MONO8_2F: Image = Image {
    name: "mono8x2",
    rows: 11,
    cols: 8,
    spp: 1,
    allocated: 8,
    bits: 8,
    frames: 2,
};
fn data_set(im: &Image) -> (Object, Vec<u16>) {
    let one = samples(im.rows, im.cols, im.spp, im.bits);
    let mut all = one.clone();
    for f in 1..im.frames {
        all.extend(
            one.iter()
                .rev()
                .map(|&s| ((s as u32 + f as u32 * 17) % (1 << im.bits)) as u16),
        );
    }
    let mut ds = image(im.rows, im.cols, im.spp, im.allocated, im.bits);
    let vr = if im.allocated == 8 { Vr::OB } else { Vr::OW };
    let mut bytes = sample_bytes(&all, im.allocated);
    if bytes.len() % 2 == 1 {
        bytes.push(0);
    }
    ds.put(raw(0x7FE0, 0x0010, vr, bytes));
    if im.frames > 1 {
        ds.put(text(0x0028, 0x0008, Vr::IS, &im.frames.to_string()));
    }
    (ds, all)
}
fn native_samples(f: &object::File, im: &Image) -> Vec<u16> {
    let bytes = pixel_bytes(f);
    let n = im.rows * im.cols * im.spp * im.frames;
    if im.allocated == 8 {
        bytes[..n].iter().map(|&b| b as u16).collect()
    } else {
        bytes[..2 * n]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect()
    }
}
fn round_trip(
    im: &Image,
    encoder: FrameEncoder,
    lossy: bool,
) -> Result<(Vec<u16>, Vec<u16>, usize), String> {
    let (ds, original) = data_set(im);
    let source = file(ds, EXPLICIT_LE);
    let mut encoders = EncoderRegistry::new();
    let target = encoder.capabilities().transfer_syntax_uid;
    encoders.register_own(encoder).map_err(|e| e.to_string())?;
    let options = TranscodeOptions {
        decoder: Some(DecoderRegistry::Builtin),
        encoders: Some(encoders),
        allow_lossy: lossy,
        ..TranscodeOptions::default()
    };
    let (encoded, report) =
        tc::transcode_file(&source, &target, &options).map_err(|e| e.to_string())?;
    assert_eq!(encoded.transfer_syntax.uid, target);
    assert_eq!(report.frames, im.frames as i64, "{}", im.name);
    assert_eq!(report.lossy, lossy, "{} to {target}", im.name);
    let bytes = writer::write_file(&encoded).map_err(|e| e.to_string())?;
    let read = object::read_file(&bytes).map_err(|(_, e)| e.to_string())?;
    assert_eq!(read.transfer_syntax.uid, target);
    if read.transfer_syntax.encapsulated {
        assert!(
            read.dataset.get_raw(Tag::PIXEL_DATA).is_none(),
            "{target}: encapsulated"
        );
    }
    let decode = DecompressOptions {
        registry: Some(DecoderRegistry::BuiltinWithJpegXl),
        ..DecompressOptions::default()
    };
    let decoded = tc::decompress_file(&read, &decode).map_err(|e| e.to_string())?;
    assert_eq!(decoded.transfer_syntax.uid, EXPLICIT_LE);
    Ok((original, native_samples(&decoded, im), bytes.len()))
}
fn lossless(encoder: impl Fn() -> FrameEncoder, images: &[&Image]) {
    for im in images {
        let what = format!(
            "{} with {:?}",
            im.name,
            encoder().capabilities().transfer_syntax_uid
        );
        let (want, got, _) =
            round_trip(im, encoder(), false).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(got, want, "{what}");
    }
}
#[allow(dead_code)]
fn error(want: &[u16], got: &[u16]) -> (u32, f64) {
    assert_eq!(want.len(), got.len());
    let diffs: Vec<u32> = want
        .iter()
        .zip(got)
        .map(|(&a, &b)| (a as i32 - b as i32).unsigned_abs())
        .collect();
    let max = diffs.iter().copied().max().unwrap_or(0);
    (
        max,
        diffs.iter().map(|&d| d as f64).sum::<f64>() / diffs.len() as f64,
    )
}
#[allow(dead_code)]
fn lossy(encoder: impl Fn() -> FrameEncoder, images: &[&Image], max_error: u32, mean_error: f64) {
    for im in images {
        let what = format!(
            "{} with {:?}",
            im.name,
            encoder().capabilities().transfer_syntax_uid
        );
        let (want, got, _) =
            round_trip(im, encoder(), true).unwrap_or_else(|e| panic!("{what}: {e}"));
        let (max, mean) = error(&want, &got);
        assert!(
            max <= max_error && mean <= mean_error,
            "{what}: max {max} mean {mean}"
        );
    }
}
#[test]
fn rle_is_lossless() {
    lossless(|| FrameEncoder::Rle, &[&MONO8, &MONO12, &RGB8, &MONO8_2F]);
}
#[test]
fn deflate_frames_are_lossless() {
    lossless(
        || FrameEncoder::Deflate(6),
        &[&MONO8, &MONO12, &RGB8, &MONO8_2F],
    );
}
#[cfg(feature = "jpeg-lossless")]
#[test]
fn jpeg_lossless_is_lossless() {
    use dicom_rust::codec::jpeglossless::Encoder;
    lossless(
        || FrameEncoder::JpegLossless(Encoder::sv1()),
        &[&MONO8, &MONO12, &RGB8, &MONO8_2F],
    );
    for predictor in 1..=7 {
        lossless(
            || FrameEncoder::JpegLossless(Encoder::process14(predictor).expect("predictor")),
            &[&MONO8, &MONO12],
        );
    }
}
#[cfg(feature = "jpeg-ls")]
#[test]
fn jpeg_ls_lossless_and_near_lossless() {
    use dicom_rust::codec::jpegls::{Encoder, EncoderOptions};
    let jls = |near: i64, interleave: i64| {
        move || {
            FrameEncoder::JpegLs(
                Encoder::with_options(EncoderOptions {
                    near,
                    allow_lossy: near > 0,
                    interleave,
                })
                .expect("JPEG-LS options"),
            )
        }
    };
    lossless(jls(0, 0), &[&MONO8, &MONO12, &RGB8, &MONO8_2F]);
    lossless(jls(0, 1), &[&RGB8]);
    lossless(jls(0, 2), &[&RGB8]);
    lossy(jls(2, 0), &[&MONO8, &MONO12], 2, 2.0);
}
#[cfg(feature = "jpeg2000")]
#[test]
fn jpeg_2000_and_htj2k() {
    use dicom_rust::codec::jpeg2000::encoder::{Encoder, Options};
    let j2k = |uid: &'static str, levels: i64, step: f64, mct: bool| {
        move || {
            FrameEncoder::Jpeg2000(
                Encoder::new(&Options {
                    transfer_syntax_uid: uid.to_string(),
                    color_transform: mct,
                    levels,
                    step_size: step,
                    allow_lossy: step > 0.0,
                })
                .expect("JPEG 2000 options"),
            )
        }
    };
    lossless(
        j2k("1.2.840.10008.1.2.4.90", 0, 0.0, false),
        &[&MONO8, &MONO12, &RGB8, &MONO8_2F],
    );
    lossless(j2k("1.2.840.10008.1.2.4.90", 3, 0.0, true), &[&RGB8]);
    lossless(
        j2k("1.2.840.10008.1.2.4.201", 2, 0.0, false),
        &[&MONO8, &MONO12, &RGB8],
    );
    lossy(
        j2k("1.2.840.10008.1.2.4.91", 3, 0.02, false),
        &[&MONO8],
        40,
        4.0,
    );
}
#[cfg(feature = "jpeg")]
#[test]
fn jpeg_baseline_and_extended_are_close() {
    use dicom_rust::codec::jpeg::{
        Encoder, EncoderOptions, ExtendedEncoder, ExtendedEncoderOptions, Subsampling,
    };
    let baseline = |quality: i64| {
        move || {
            FrameEncoder::JpegBaseline(
                Encoder::with_options(EncoderOptions {
                    quality,
                    subsampling: Subsampling(0),
                    optimize_huffman: quality > 90,
                })
                .expect("JPEG options"),
            )
        }
    };
    lossy(baseline(95), &[&MONO8, &RGB8, &MONO8_2F], 48, 4.0);
    lossy(baseline(60), &[&MONO8], 96, 10.0);
    let extended = || {
        FrameEncoder::JpegExtended(
            ExtendedEncoder::with_options(ExtendedEncoderOptions {
                quality: 95,
                ..Default::default()
            })
            .expect("extended options"),
        )
    };
    lossy(extended, &[&MONO12], 4095 / 6, 4095.0 / 60.0);
}
#[cfg(feature = "jpeg-xl")]
#[test]
fn jpeg_xl_lossless_is_lossless() {
    lossless(|| FrameEncoder::JpegXlLossless, &[&MONO8, &MONO12, &RGB8]);
    let (ds, _) = data_set(&MONO8);
    let mut encoders = EncoderRegistry::new();
    encoders.register_own(FrameEncoder::JpegXlLossless).unwrap();
    let options = TranscodeOptions {
        decoder: Some(DecoderRegistry::Builtin),
        encoders: Some(encoders),
        ..TranscodeOptions::default()
    };
    let (encoded, _) =
        tc::transcode_file(&file(ds, EXPLICIT_LE), "1.2.840.10008.1.2.4.110", &options).unwrap();
    assert!(tc::decompress_file(&encoded, &DecompressOptions::default()).is_err());
}
#[test]
fn lossy_targets_need_consent() {
    #[cfg(feature = "jpeg-ls")]
    {
        use dicom_rust::codec::jpegls::{Encoder, EncoderOptions};
        let near = FrameEncoder::JpegLs(
            Encoder::with_options(EncoderOptions {
                near: 3,
                allow_lossy: true,
                interleave: 0,
            })
            .unwrap(),
        );
        assert!(round_trip(&MONO8, near, false).is_err());
    }
    let (ds, _) = data_set(&MONO8);
    let source = file(ds, EXPLICIT_LE);
    let options = TranscodeOptions {
        decoder: Some(DecoderRegistry::Builtin),
        encoders: Some(EncoderRegistry::new()),
        ..TranscodeOptions::default()
    };
    assert!(tc::transcode_file(&source, "1.2.840.10008.1.2.5", &options).is_err());
}
#[test]
fn native_syntaxes_convert_without_encoders() {
    let (ds, want) = data_set(&MONO12);
    let source = file(ds, EXPLICIT_LE);
    let options = TranscodeOptions {
        decoder: Some(DecoderRegistry::Builtin),
        ..TranscodeOptions::default()
    };
    for target in [IMPLICIT_LE, EXPLICIT_BE, EXPLICIT_LE] {
        let (out, _) = tc::transcode_file(&source, target, &options)
            .unwrap_or_else(|e| panic!("{target}: {e}"));
        let bytes = writer::write_file(&out).unwrap();
        let back = object::read_file(&bytes).map_err(|(_, e)| e).unwrap();
        assert_eq!(back.transfer_syntax.uid, target);
        let native = tc::decompress_file(&back, &DecompressOptions::default()).unwrap();
        assert_eq!(native_samples(&native, &MONO12), want, "{target}");
    }
}
