use crate::harness::{Precision, Runner, Work};
use crate::inputs::{self, Inputs};
use crate::part10;
use dicom_rust::codec::jpeg2000::{
    self, CoreOptions, Registration, encoder as j2k_encoder, j2kcore,
};
use dicom_rust::codec::jpeglossless::{self, Metadata};
use dicom_rust::codec::{jpeg, jpegls, jpegxl, rle};
use dicom_rust::object::{ReadFileOptions, read_file_with_options};
use dicom_rust::pixel::pixeldata;
pub struct Img {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    pub md: Metadata,
}
impl Img {
    fn pixels(&self) -> u64 {
        u64::from(self.md.rows) * u64::from(self.md.columns)
    }
    fn rle_md(&self) -> rle::Metadata {
        pixel_md(&self.md)
    }
}
pub fn pixel_md(m: &Metadata) -> rle::Metadata {
    rle::Metadata {
        rows: m.rows,
        columns: m.columns,
        samples_per_pixel: m.samples_per_pixel,
        bits_allocated: m.bits_allocated,
        bits_stored: m.bits_stored,
        high_bit: m.high_bit,
        pixel_representation: m.pixel_representation,
        planar_configuration: m.planar_configuration,
        planar_configuration_present: m.planar_configuration_present,
        number_of_frames: m.number_of_frames,
        photometric_interpretation: m.photometric_interpretation.clone(),
    }
}
pub fn md(w: usize, h: usize, samples: u16, bits_stored: u16, photometric: &str) -> Metadata {
    Metadata {
        rows: h as u16,
        columns: w as u16,
        samples_per_pixel: samples,
        bits_allocated: if bits_stored > 8 { 16 } else { 8 },
        bits_stored,
        high_bit: bits_stored - 1,
        pixel_representation: 0,
        planar_configuration: 0,
        planar_configuration_present: samples > 1,
        number_of_frames: 1,
        photometric_interpretation: photometric.into(),
    }
}
pub fn images(inp: &Inputs) -> Vec<Img> {
    let ct = inp.ct_slice();
    let mr = inp.mr();
    let dx = inp.dx();
    let us = inp.us();
    let gray = |name: &'static str, g: &crate::synth::Gray16| Img {
        name,
        bytes: g.le_bytes(),
        md: md(g.width, g.height, 1, g.bits_stored, "MONOCHROME2"),
    };
    vec![
        gray("ct512", ct),
        gray("mr256", mr),
        gray("dx2048", dx),
        Img {
            name: "us640rgb",
            bytes: us.pixels.clone(),
            md: md(us.width, us.height, 3, 8, "RGB"),
        },
        Img {
            name: "ct512_8bit",
            bytes: ct.windowed8(1064.0, 400.0),
            md: md(ct.width, ct.height, 1, 8, "MONOCHROME2"),
        },
    ]
}
fn img<'a>(all: &'a [Img], name: &str) -> &'a Img {
    all.iter().find(|i| i.name == name).expect("image class")
}
fn note(cs: &[u8], i: &Img) -> String {
    format!(
        "{} bytes, ratio {:.2}:1",
        cs.len(),
        i.bytes.len() as f64 / cs.len().max(1) as f64
    )
}
fn j2k_options(uid: &str, color: bool, step: f64, lossy: bool) -> j2k_encoder::Options {
    j2k_encoder::Options {
        transfer_syntax_uid: uid.into(),
        color_transform: color,
        levels: 5,
        step_size: step,
        allow_lossy: lossy,
    }
}
fn j2k_encode(i: &Img, o: &j2k_encoder::Options) -> Vec<u8> {
    j2k_encoder::Encoder::new(o)
        .and_then(|e| e.encode_frame(&i.bytes, &i.md))
        .map(|f| f.data)
        .unwrap_or_else(|e| panic!("encode {}: {e}", i.name))
}
fn j2k_decode(cs: &[u8], i: &Img, uid: &str) -> Vec<u8> {
    let mut d = jpeg2000::decode_frames(
        &[cs],
        &i.md,
        uid,
        Registration::Default,
        CoreOptions::default(),
    )
    .unwrap_or_else(|e| panic!("decode {}: {e}", i.name));
    d.data.swap_remove(0)
}
pub fn names() -> Vec<String> {
    let mut v = Vec::new();
    let threads = all_threads();
    for i in ["ct512", "us640rgb"] {
        v.push(format!("codec/rle/encode/{i}"));
        v.push(format!("codec/rle/decode/{i}"));
    }
    for i in ["ct512_8bit", "us640rgb"] {
        v.push(format!("codec/jpeg_baseline_q90/encode/{i}"));
        v.push(format!("codec/jpeg_baseline_q90/decode/{i}"));
    }
    for i in ["ct512", "mr256", "dx2048"] {
        v.push(format!("codec/jpeg_lossless_sv1/encode/{i}"));
        v.push(format!("codec/jpeg_lossless_sv1/decode/{i}"));
    }
    for i in ["ct512", "mr256", "dx2048", "us640rgb"] {
        v.push(format!("codec/jpegls_lossless/encode/{i}"));
        v.push(format!("codec/jpegls_lossless/decode/{i}"));
    }
    v.push("codec/jpegls_near2/encode/ct512".into());
    v.push("codec/jpegls_near2/decode/ct512".into());
    for i in ["ct512", "mr256", "dx2048", "us640rgb"] {
        v.push(format!("codec/j2k_lossless/encode/{i}"));
        v.push(format!("codec/j2k_lossless/decode/{i}/t1"));
        v.push(format!("codec/j2k_lossless/decode/{i}/t{threads}"));
    }
    for i in ["ct512", "dx2048"] {
        v.push(format!("codec/j2k_lossy/encode/{i}"));
        v.push(format!("codec/j2k_lossy/decode/{i}/t1"));
        v.push(format!("codec/j2k_lossy/decode/{i}/t{threads}"));
        v.push(format!("codec/htj2k_lossless/encode/{i}"));
        v.push(format!("codec/htj2k_lossless/decode/{i}/t1"));
        v.push(format!("codec/htj2k_lossless/decode/{i}/t{threads}"));
    }
    v.push("codec/j2k_lossless/decode_file/ct512".into());
    for i in ["ct512", "mr256"] {
        v.push(format!("codec/jpegxl_lossless/encode/{i}"));
        v.push(format!("codec/jpegxl_lossless/decode/{i}"));
    }
    v
}
pub fn all_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}
pub const J2K_LOSSY_STEP: f64 = 8.0;
pub fn run(r: &mut Runner, inp: &Inputs) {
    let all_names = names();
    let refs: Vec<&str> = all_names.iter().map(String::as_str).collect();
    if !r.any_selected(&refs) {
        return;
    }
    let all = images(inp);
    let threads = all_threads();
    for name in ["ct512", "us640rgb"] {
        let i = img(&all, name);
        let m = i.rle_md();
        let cs = rle::encode_frame(&i.bytes, &m).expect("rle encode");
        let w = Work::Pixels(i.pixels());
        r.bench_with(
            &format!("codec/rle/encode/{name}"),
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || rle::encode_frame(&i.bytes, &m),
        );
        r.bench_with(
            &format!("codec/rle/decode/{name}"),
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || rle::decode(&[&cs], &[], &m, rle::Recovery::default()),
        );
    }
    for name in ["ct512_8bit", "us640rgb"] {
        let i = img(&all, name);
        let enc = jpeg::Encoder::with_options(jpeg::EncoderOptions {
            quality: 90,
            subsampling: jpeg::Subsampling::S420,
            optimize_huffman: false,
        })
        .expect("jpeg options");
        let f = enc.encode_frame(&i.bytes, &i.md).expect("jpeg encode");
        let mut dmd = i.md.clone();
        if !f.photometric_interpretation.is_empty() {
            dmd.photometric_interpretation = f.photometric_interpretation.clone();
        }
        let opts = jpeg::DecodeOptions {
            syntax: jpeg::Syntax::Baseline,
            ..Default::default()
        };
        let w = Work::Pixels(i.pixels());
        r.bench_with(
            &format!("codec/jpeg_baseline_q90/encode/{name}"),
            w,
            &note(&f.data, i),
            Precision::NORMAL,
            || enc.encode_frame(&i.bytes, &i.md),
        );
        r.bench_with(
            &format!("codec/jpeg_baseline_q90/decode/{name}"),
            w,
            &note(&f.data, i),
            Precision::NORMAL,
            || jpeg::decode_frames(&[&f.data], &dmd, opts),
        );
    }
    for name in ["ct512", "mr256", "dx2048"] {
        let i = img(&all, name);
        let enc = jpeglossless::Encoder::sv1();
        let cs = enc
            .encode_frame(&i.bytes, &i.md)
            .expect("jpeg lossless encode");
        let opts = jpeglossless::DecodeOptions {
            selection_value: 1,
            ..Default::default()
        };
        let w = Work::Pixels(i.pixels());
        r.bench_with(
            &format!("codec/jpeg_lossless_sv1/encode/{name}"),
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || enc.encode_frame(&i.bytes, &i.md),
        );
        r.bench_with(
            &format!("codec/jpeg_lossless_sv1/decode/{name}"),
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || jpeglossless::decode_frames(&[&cs], &i.md, opts),
        );
    }
    for name in ["ct512", "mr256", "dx2048", "us640rgb"] {
        let i = img(&all, name);
        let enc = jpegls::Encoder::with_options(jpegls::EncoderOptions {
            near: 0,
            allow_lossy: false,
            interleave: if i.md.samples_per_pixel == 3 { 1 } else { 0 },
        })
        .expect("jpegls options");
        let cs = enc.encode_frame(&i.bytes, &i.md).expect("jpegls encode");
        let codec = jpegls::Codec::lossless();
        let w = Work::Pixels(i.pixels());
        r.bench_with(
            &format!("codec/jpegls_lossless/encode/{name}"),
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || enc.encode_frame(&i.bytes, &i.md),
        );
        r.bench_with(
            &format!("codec/jpegls_lossless/decode/{name}"),
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || codec.decode_frames(&[&cs], &i.md, jpegls::DecodeOptions::default()),
        );
    }
    {
        let i = img(&all, "ct512");
        let enc = jpegls::Encoder::with_options(jpegls::EncoderOptions {
            near: 2,
            allow_lossy: true,
            interleave: 0,
        })
        .expect("jpegls options");
        let cs = enc.encode_frame(&i.bytes, &i.md).expect("jpegls encode");
        let codec = jpegls::Codec::near_lossless();
        let w = Work::Pixels(i.pixels());
        r.bench_with(
            "codec/jpegls_near2/encode/ct512",
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || enc.encode_frame(&i.bytes, &i.md),
        );
        r.bench_with(
            "codec/jpegls_near2/decode/ct512",
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || codec.decode_frames(&[&cs], &i.md, jpegls::DecodeOptions::default()),
        );
    }
    let mut j2k = Vec::new();
    for name in ["ct512", "mr256", "dx2048", "us640rgb"] {
        let color = img(&all, name).md.samples_per_pixel == 3;
        j2k.push((
            "j2k_lossless",
            jpeg2000::UID_LOSSLESS,
            name,
            j2k_options(jpeg2000::UID_LOSSLESS, color, 0.0, false),
        ));
    }
    for name in ["ct512", "dx2048"] {
        j2k.push((
            "j2k_lossy",
            jpeg2000::UID_LOSSY,
            name,
            j2k_options(jpeg2000::UID_LOSSY, false, J2K_LOSSY_STEP, true),
        ));
    }
    for name in ["ct512", "dx2048"] {
        j2k.push((
            "htj2k_lossless",
            jpeg2000::UID_HTJ2K_LOSSLESS,
            name,
            j2k_options(jpeg2000::UID_HTJ2K_LOSSLESS, false, 0.0, false),
        ));
    }
    for (codec, uid, name, opts) in &j2k {
        let i = img(&all, name);
        let cs = j2k_encode(i, opts);
        let w = Work::Pixels(i.pixels());
        let n = note(&cs, i);
        r.bench_with(
            &format!("codec/{codec}/encode/{name}"),
            w,
            &n,
            Precision::NORMAL,
            || j2k_encoder::Encoder::new(opts).and_then(|e| e.encode_frame(&i.bytes, &i.md)),
        );
        r.bench_with(
            &format!("codec/{codec}/decode/{name}/t1"),
            w,
            &n,
            Precision::HIGH,
            || j2k_decode(&cs, i, uid),
        );
        r.bench_with(
            &format!("codec/{codec}/decode/{name}/t{threads}"),
            w,
            &n,
            Precision::HIGH,
            || j2kcore::with_decode_threads(threads, || j2k_decode(&cs, i, uid)),
        );
    }
    if r.selected("codec/j2k_lossless/decode_file/ct512") {
        let i = img(&all, "ct512");
        let cs = j2k_encode(i, &j2k_options(jpeg2000::UID_LOSSLESS, false, 0.0, false));
        let z = inputs::CT_SLICES / 2;
        let file = part10::ct_encapsulated_file(
            inputs::ct_place(z),
            512,
            512,
            inputs::ct_z_mm(z),
            jpeg2000::UID_LOSSLESS,
            &cs,
        );
        r.bench_with(
            "codec/j2k_lossless/decode_file/ct512",
            Work::Pixels(i.pixels()),
            &note(&cs, i),
            Precision::HIGH,
            || decode_file(&file),
        );
    }
    for name in ["ct512", "mr256"] {
        let i = img(&all, name);
        let m = i.rle_md();
        let cs = jpegxl::dicom::encode_lossless_frame(&i.bytes, &m).expect("jpegxl encode");
        let w = Work::Pixels(i.pixels());
        r.bench_with(
            &format!("codec/jpegxl_lossless/encode/{name}"),
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || jpegxl::dicom::encode_lossless_frame(&i.bytes, &m),
        );
        r.bench_with(
            &format!("codec/jpegxl_lossless/decode/{name}"),
            w,
            &note(&cs, i),
            Precision::NORMAL,
            || jpegxl::decode(&cs),
        );
    }
}
pub fn decode_file(bytes: &[u8]) -> Vec<Vec<u8>> {
    let f = read_file_with_options(bytes, ReadFileOptions::default())
        .unwrap_or_else(|(_, e)| panic!("read: {e}"));
    let view = pixeldata::extract_view(&f.dataset).unwrap_or_else(|e| panic!("view: {e}"));
    let frames = pixeldata::decode_frames(f.transfer_syntax.uid, &view, &f.dataset)
        .unwrap_or_else(|e| panic!("decode: {e}"));
    frames.data
}
pub fn check(inp: &Inputs) -> Result<(), String> {
    let all = images(inp);
    let ct = img(&all, "ct512");
    let rgb = img(&all, "us640rgb");
    let same = |what: &str, a: &[u8], b: &[u8]| {
        if a == b {
            Ok(())
        } else {
            Err(format!("{what}: decoded pixels differ from the input"))
        }
    };
    for i in [ct, rgb] {
        let m = i.rle_md();
        let cs = rle::encode_frame(&i.bytes, &m).map_err(|e| e.to_string())?;
        let d =
            rle::decode(&[&cs], &[], &m, rle::Recovery::default()).map_err(|e| e.to_string())?;
        same(&format!("rle {}", i.name), &d.data[0], &i.bytes)?;
    }
    let cs = jpeglossless::Encoder::sv1()
        .encode_frame(&ct.bytes, &ct.md)
        .map_err(|e| e.to_string())?;
    let d = jpeglossless::decode_frames(
        &[&cs],
        &ct.md,
        jpeglossless::DecodeOptions {
            selection_value: 1,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    same("jpeg lossless", &d.data[0], &ct.bytes)?;
    for i in [ct, rgb] {
        let enc = jpegls::Encoder::with_options(jpegls::EncoderOptions {
            near: 0,
            allow_lossy: false,
            interleave: if i.md.samples_per_pixel == 3 { 1 } else { 0 },
        })
        .map_err(|e| e.to_string())?;
        let cs = enc
            .encode_frame(&i.bytes, &i.md)
            .map_err(|e| e.to_string())?;
        let d = jpegls::Codec::lossless()
            .decode_frames(&[&cs], &i.md, jpegls::DecodeOptions::default())
            .map_err(|e| e.to_string())?;
        same(&format!("jpegls {}", i.name), &d.data[0], &i.bytes)?;
    }
    for (uid, i) in [
        (jpeg2000::UID_LOSSLESS, ct),
        (jpeg2000::UID_LOSSLESS, rgb),
        (jpeg2000::UID_HTJ2K_LOSSLESS, ct),
    ] {
        let cs = j2k_encode(
            i,
            &j2k_options(uid, i.md.samples_per_pixel == 3, 0.0, false),
        );
        same(
            &format!("j2k {uid} {}", i.name),
            &j2k_decode(&cs, i, uid),
            &i.bytes,
        )?;
        let threaded = j2kcore::with_decode_threads(all_threads(), || j2k_decode(&cs, i, uid));
        same(
            &format!("j2k {uid} {} threaded", i.name),
            &threaded,
            &i.bytes,
        )?;
    }
    let cs = j2k_encode(
        ct,
        &j2k_options(jpeg2000::UID_LOSSY, false, J2K_LOSSY_STEP, true),
    );
    if j2k_decode(&cs, ct, jpeg2000::UID_LOSSY).len() != ct.bytes.len() {
        return Err("j2k lossy: wrong decoded size".into());
    }
    let z = inputs::CT_SLICES / 2;
    let lossless = j2k_encode(ct, &j2k_options(jpeg2000::UID_LOSSLESS, false, 0.0, false));
    let file = part10::ct_encapsulated_file(
        inputs::ct_place(z),
        512,
        512,
        inputs::ct_z_mm(z),
        jpeg2000::UID_LOSSLESS,
        &lossless,
    );
    same("j2k file", &decode_file(&file)[0], &ct.bytes)?;
    let m = ct.rle_md();
    let cs = jpegxl::dicom::encode_lossless_frame(&ct.bytes, &m).map_err(|e| e.to_string())?;
    let img = jpegxl::decode(&cs).map_err(|e| e.to_string())?;
    let back: Vec<u8> = img
        .samples
        .iter()
        .flat_map(|&v| (v as u16).to_le_bytes())
        .collect();
    same("jpegxl", &back, &ct.bytes)?;
    Ok(())
}
