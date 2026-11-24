#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
use super::bitstream::BitWriter;
use super::loco::{
    self, BASIC_RESET, LocoState, Preset, RUN_J, context_index, neighbors, predict,
    write_limited_golomb,
};
use super::{Error, ErrorKind, Metadata, UID, UID_NEAR_LOSSLESS, normalized_photometric};
const MAX_FRAGMENT_LENGTH: u64 = (1 << 32) - 2;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InterleaveMode {
    #[default]
    None,
    Line,
    Sample,
}
impl InterleaveMode {
    fn ilv(self) -> i64 {
        match self {
            InterleaveMode::None => 0,
            InterleaveMode::Line => 1,
            InterleaveMode::Sample => 2,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EncoderOptions {
    pub near: i64,
    pub allow_lossy: bool,
    pub interleave: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderCapabilities {
    pub transfer_syntax_uid: &'static str,
    pub bits_allocated: Vec<u16>,
    pub pixel_representations: Vec<u16>,
    pub samples_per_pixel: Vec<u16>,
    pub photometric_interpretations: Vec<&'static str>,
    pub lossless: bool,
    pub lossy_method: &'static str,
    pub supports_multi_frame: bool,
    pub backend: &'static str,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Encoder {
    near: i64,
    interleave: InterleaveMode,
}
pub(crate) struct EncodeParams {
    pub(crate) bytes_per_sample: usize,
    pub(crate) maxval: i64,
    pub(crate) frame_length: u64,
    pub(crate) max_encoded: u64,
}
impl Encoder {
    pub fn new() -> Encoder {
        Encoder::default()
    }
    pub fn with_options(opts: EncoderOptions) -> Result<Encoder, Error> {
        if opts.near < 0 || opts.near > 255 {
            return Err(Error::encoder_metadata("NEAR"));
        }
        let interleave = match opts.interleave {
            0 => InterleaveMode::None,
            1 => InterleaveMode::Line,
            2 => InterleaveMode::Sample,
            _ => return Err(Error::encoder_metadata("ILV")),
        };
        if opts.near > 0 && !opts.allow_lossy {
            return Err(Error::bare(ErrorKind::LossyDisallowed));
        }
        Ok(Encoder {
            near: opts.near,
            interleave,
        })
    }
    pub fn capabilities(&self) -> EncoderCapabilities {
        let mut caps = EncoderCapabilities {
            transfer_syntax_uid: UID,
            bits_allocated: vec![8, 16],
            pixel_representations: vec![0, 1],
            samples_per_pixel: vec![1, 3],
            photometric_interpretations: vec!["MONOCHROME1", "MONOCHROME2", "PALETTE COLOR", "RGB"],
            lossless: true,
            lossy_method: "",
            supports_multi_frame: true,
            backend: "pure-rust",
        };
        if self.near > 0 {
            caps.transfer_syntax_uid = UID_NEAR_LOSSLESS;
            caps.pixel_representations = vec![0];
            caps.photometric_interpretations = vec!["MONOCHROME1", "MONOCHROME2", "RGB"];
            caps.lossless = false;
            caps.lossy_method = "ISO_14495_1";
        }
        caps
    }
    pub fn encode_frame(&self, frame: &[u8], metadata: &Metadata) -> Result<Vec<u8>, Error> {
        let params = validate_encoder_input(metadata)?;
        let near = self.near;
        if near > 0 {
            if near > 255.min(params.maxval / 2) {
                return Err(Error::encoder_metadata("NEAR"));
            }
            if metadata.pixel_representation != 0 {
                return Err(Error::encoder_metadata("PixelRepresentation"));
            }
            if metadata
                .photometric_interpretation
                .trim()
                .eq_ignore_ascii_case("PALETTE COLOR")
            {
                return Err(Error::encoder_metadata("PhotometricInterpretation"));
            }
        }
        if frame.len() as u64 != params.frame_length {
            return Err(Error::new(
                ErrorKind::PixelDataSizeMismatch,
                "native frame length",
            ));
        }
        let mut samples = native_to_samples(frame, metadata, &params);
        let ilv = if metadata.samples_per_pixel == 1 {
            InterleaveMode::None
        } else {
            self.interleave
        };
        let t = loco::default_near_thresholds(params.maxval, near);
        let preset = Preset {
            near,
            maxval: params.maxval,
            t1: t.0,
            t2: t.1,
            t3: t.2,
            reset: BASIC_RESET,
        };
        let encoded = encode_frame_with_preset(&mut samples, metadata, &params, &preset, ilv)?;
        if encoded.is_empty() {
            return Err(Error::new(ErrorKind::EncoderOutputInvalid, "empty frame"));
        }
        Ok(encoded)
    }
}
pub(crate) fn validate_encoder_input(metadata: &Metadata) -> Result<EncodeParams, Error> {
    if metadata.rows == 0 {
        return Err(Error::encoder_metadata("Rows"));
    }
    if metadata.columns == 0 {
        return Err(Error::encoder_metadata("Columns"));
    }
    if metadata.bits_allocated != 8 && metadata.bits_allocated != 16 {
        return Err(Error::encoder_metadata("BitsAllocated"));
    }
    if metadata.bits_stored < 2 || metadata.bits_stored > metadata.bits_allocated {
        return Err(Error::encoder_metadata("BitsStored"));
    }
    if metadata.high_bit != metadata.bits_stored - 1 {
        return Err(Error::encoder_metadata("HighBit"));
    }
    if metadata.pixel_representation > 1 {
        return Err(Error::encoder_metadata("PixelRepresentation"));
    }
    if metadata.samples_per_pixel != 1 && metadata.samples_per_pixel != 3 {
        return Err(Error::encoder_metadata("SamplesPerPixel"));
    }
    if metadata.number_of_frames < 1 {
        return Err(Error::encoder_metadata("NumberOfFrames"));
    }
    let photometric = normalized_photometric(metadata);
    if metadata.samples_per_pixel == 1 {
        if metadata.planar_configuration_present && metadata.planar_configuration != 0 {
            return Err(Error::encoder_metadata("PlanarConfiguration"));
        }
        if photometric != "MONOCHROME1"
            && photometric != "MONOCHROME2"
            && photometric != "PALETTE COLOR"
        {
            return Err(Error::encoder_metadata("PhotometricInterpretation"));
        }
        if photometric == "PALETTE COLOR" && metadata.pixel_representation != 0 {
            return Err(Error::encoder_metadata("PixelRepresentation"));
        }
    } else {
        if photometric != "RGB" {
            return Err(Error::encoder_metadata("PhotometricInterpretation"));
        }
        if metadata.pixel_representation != 0 {
            return Err(Error::encoder_metadata("PixelRepresentation"));
        }
        if !metadata.planar_configuration_present || metadata.planar_configuration != 0 {
            return Err(Error::encoder_metadata("PlanarConfiguration"));
        }
    }
    let bytes_per_sample = (metadata.bits_allocated / 8) as usize;
    let samples =
        metadata.rows as u64 * metadata.columns as u64 * metadata.samples_per_pixel as u64;
    let frame_length = samples * bytes_per_sample as u64;
    let limit_bits = 2 * (2.max(metadata.bits_stored as u64) + 16);
    let max_encoded =
        samples * (limit_bits.div_ceil(8) + 2) + 64 + metadata.samples_per_pixel as u64 * 16;
    if max_encoded > MAX_FRAGMENT_LENGTH {
        return Err(Error::new(
            ErrorKind::EncoderOutputInvalid,
            "encoded frame length",
        ));
    }
    Ok(EncodeParams {
        bytes_per_sample,
        maxval: (1i64 << metadata.bits_stored) - 1,
        frame_length,
        max_encoded,
    })
}
fn native_to_samples(frame: &[u8], metadata: &Metadata, params: &EncodeParams) -> Vec<i64> {
    let count =
        metadata.rows as usize * metadata.columns as usize * metadata.samples_per_pixel as usize;
    let mask = params.maxval;
    if params.bytes_per_sample == 1 {
        frame[..count].iter().map(|&b| b as i64 & mask).collect()
    } else {
        frame[..2 * count]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| (p[0] as i64 | (p[1] as i64) << 8) & mask)
            .collect()
    }
}
fn encode_frame_with_preset(
    samples: &mut [i64],
    metadata: &Metadata,
    params: &EncodeParams,
    preset: &Preset,
    ilv: InterleaveMode,
) -> Result<Vec<u8>, Error> {
    let near = preset.near;
    let rows = metadata.rows as usize;
    let cols = metadata.columns as usize;
    let comps = metadata.samples_per_pixel as usize;
    let capacity =
        ((samples.len() * params.bytes_per_sample) as u64 / 2 + 64).min(params.max_encoded);
    let mut w = BitWriter::with_capacity(capacity as usize);
    w.buf.extend_from_slice(&[0xff, 0xd8]);
    write_sof55(&mut w, rows, cols, comps, metadata.bits_stored as usize);
    if ilv != InterleaveMode::None && comps > 1 {
        write_interleaved_sos(&mut w, comps, near, ilv);
        let mut planes: Vec<Vec<i64>> = (0..comps)
            .map(|c| extract_component(samples, rows, cols, comps, c))
            .collect();
        encode_interleaved_scan(&mut w, &mut planes, cols, rows, ilv, preset);
        w.pad_to_byte();
        return finish_frame(w, params);
    }
    for c in 0..comps {
        write_sos(&mut w, c + 1);
        let n = w.buf.len();
        w.buf[n - 3] = near as u8;
        if comps == 1 {
            encode_scan(&mut w, samples, cols, rows, preset);
        } else {
            let mut plane = extract_component(samples, rows, cols, comps, c);
            encode_scan(&mut w, &mut plane, cols, rows, preset);
        }
        w.pad_to_byte();
    }
    finish_frame(w, params)
}
fn finish_frame(mut w: BitWriter, params: &EncodeParams) -> Result<Vec<u8>, Error> {
    w.buf.extend_from_slice(&[0xff, 0xd9]);
    if w.buf.len() as u64 > params.max_encoded {
        return Err(Error::new(
            ErrorKind::EncoderOutputInvalid,
            "encoded frame length",
        ));
    }
    Ok(w.buf)
}
fn write_sof55(w: &mut BitWriter, rows: usize, cols: usize, comps: usize, precision: usize) {
    let length = 8 + 3 * comps;
    w.buf.extend_from_slice(&[
        0xff,
        0xf7,
        (length >> 8) as u8,
        length as u8,
        precision as u8,
        (rows >> 8) as u8,
        rows as u8,
        (cols >> 8) as u8,
        cols as u8,
        comps as u8,
    ]);
    for c in 0..comps {
        w.buf.extend_from_slice(&[(c + 1) as u8, 0x11, 0x00]);
    }
}
fn write_sos(w: &mut BitWriter, component_id: usize) {
    w.buf.extend_from_slice(&[
        0xff,
        0xda,
        0x00,
        0x08,
        0x01,
        component_id as u8,
        0x00,
        0x00,
        0x00,
        0x00,
    ]);
}
fn write_interleaved_sos(w: &mut BitWriter, comps: usize, near: i64, ilv: InterleaveMode) {
    let length = 6 + 2 * comps;
    w.buf
        .extend_from_slice(&[0xff, 0xda, (length >> 8) as u8, length as u8, comps as u8]);
    for c in 0..comps {
        w.buf.extend_from_slice(&[(c + 1) as u8, 0x00]);
    }
    w.buf
        .extend_from_slice(&[near as u8, ilv.ilv() as u8, 0x00]);
}
fn extract_component(
    samples: &[i64],
    rows: usize,
    cols: usize,
    comps: usize,
    component: usize,
) -> Vec<i64> {
    (0..rows * cols)
        .map(|i| samples[i * comps + component])
        .collect()
}
fn encode_scan(w: &mut BitWriter, recon: &mut [i64], width: usize, height: usize, preset: &Preset) {
    let mut st = LocoState::new(preset);
    for y in 0..height {
        encode_line(w, &mut st, recon, width, y);
    }
}
fn encode_line(w: &mut BitWriter, st: &mut LocoState, recon: &mut [i64], width: usize, y: usize) {
    let mut x = 0;
    while x < width {
        let (ra, rb, rc, rd) = if x > 0 && y > 0 && x + 1 < width {
            let i = y * width + x;
            (
                recon[i - 1],
                recon[i - width],
                recon[i - width - 1],
                recon[i - width + 1],
            )
        } else {
            neighbors(recon, width, x, y)
        };
        let q1 = st.quantize(rd - rb);
        let q2 = st.quantize(rb - rc);
        let q3 = st.quantize(rc - ra);
        if q1 == 0 && q2 == 0 && q3 == 0 {
            x = encode_run(w, st, recon, width, x, y, ra);
            continue;
        }
        let i = y * width + x;
        recon[i] = encode_regular(w, st, recon[i], ra, rb, rc, q1, q2, q3);
        x += 1;
    }
}
fn encode_interleaved_scan(
    w: &mut BitWriter,
    planes: &mut [Vec<i64>],
    width: usize,
    height: usize,
    ilv: InterleaveMode,
    preset: &Preset,
) {
    let mut st = LocoState::new(preset);
    let mut run_indices = [0usize; 3];
    for y in 0..height {
        if ilv == InterleaveMode::Line {
            for (c, plane) in planes.iter_mut().enumerate() {
                st.run_idx = run_indices[c];
                encode_line(w, &mut st, plane, width, y);
                run_indices[c] = st.run_idx;
            }
            continue;
        }
        encode_sample_line(w, &mut st, planes, width, y);
    }
}
fn encode_sample_line(
    w: &mut BitWriter,
    st: &mut LocoState,
    planes: &mut [Vec<i64>],
    width: usize,
    y: usize,
) {
    let mut x = 0;
    while x < width {
        let mut a = [0i64; 3];
        let mut b = [0i64; 3];
        let mut c = [0i64; 3];
        let mut q1 = [0i64; 3];
        let mut q2 = [0i64; 3];
        let mut q3 = [0i64; 3];
        let mut run = true;
        for (component, plane) in planes.iter().enumerate() {
            let (ra, rb, rc, rd) = neighbors(plane, width, x, y);
            a[component] = ra;
            b[component] = rb;
            c[component] = rc;
            q1[component] = st.quantize(rd - rb);
            q2[component] = st.quantize(rb - rc);
            q3[component] = st.quantize(rc - ra);
            run = run && q1[component] == 0 && q2[component] == 0 && q3[component] == 0;
        }
        if run {
            x = encode_vector_run(w, st, planes, width, x, y, a);
            continue;
        }
        for (component, plane) in planes.iter_mut().enumerate() {
            let i = y * width + x;
            plane[i] = encode_regular(
                w,
                st,
                plane[i],
                a[component],
                b[component],
                c[component],
                q1[component],
                q2[component],
                q3[component],
            );
        }
        x += 1;
    }
}
fn encode_vector_run(
    w: &mut BitWriter,
    st: &mut LocoState,
    planes: &mut [Vec<i64>],
    width: usize,
    x: usize,
    y: usize,
    values: [i64; 3],
) -> usize {
    let mut xx = x;
    while xx < width {
        let within = planes
            .iter()
            .enumerate()
            .all(|(component, plane)| (plane[y * width + xx] - values[component]).abs() <= st.near);
        if !within {
            break;
        }
        for (component, plane) in planes.iter_mut().enumerate() {
            plane[y * width + xx] = values[component];
        }
        xx += 1;
    }
    let mut run = xx - x;
    while run >= 1 << RUN_J[st.run_idx] {
        w.write_bit(1);
        run -= 1 << RUN_J[st.run_idx];
        if st.run_idx < 31 {
            st.run_idx += 1;
        }
    }
    if xx == width {
        if run > 0 {
            w.write_bit(1);
        }
        return width;
    }
    w.write_bit(0);
    let bits = RUN_J[st.run_idx];
    if bits > 0 {
        w.write_bits(run as u32, bits);
    }
    for (component, plane) in planes.iter_mut().enumerate() {
        let (_, rb, _, _) = neighbors(plane, width, xx, y);
        let i = y * width + xx;
        let run_idx = st.run_idx;
        plane[i] = encode_run_interruption(w, st, plane[i], values[component], rb, run_idx, 0);
    }
    if st.run_idx > 0 {
        st.run_idx -= 1;
    }
    xx + 1
}
#[allow(clippy::too_many_arguments)]
fn encode_regular(
    w: &mut BitWriter,
    st: &mut LocoState,
    ix: i64,
    ra: i64,
    rb: i64,
    rc: i64,
    q1: i64,
    q2: i64,
    q3: i64,
) -> i64 {
    let (q, sign) = context_index(q1, q2, q3);
    let cx = st.ctx[q];
    let px = (predict(ra, rb, rc) + sign * cx.c).clamp(0, st.maxval);
    let mut err = quantize_prediction_error(sign * (ix - px), st.near);
    let rx = (px + sign * err * (2 * st.near + 1)).clamp(0, st.maxval);
    err = st.mod_range(err);
    let k = cx.k;
    let mapped = st.map_regular(err, q, k);
    write_limited_golomb(w, mapped, k, st.limit, st.qbpp);
    st.update_regular(q, err);
    rx
}
fn quantize_prediction_error(err: i64, near: i64) -> i64 {
    if near == 0 {
        return err;
    }
    if err < 0 {
        return -((near - err) / (2 * near + 1));
    }
    (err + near) / (2 * near + 1)
}
fn encode_run(
    w: &mut BitWriter,
    st: &mut LocoState,
    recon: &mut [i64],
    width: usize,
    x: usize,
    y: usize,
    ra: i64,
) -> usize {
    let run_val = ra;
    let mut run = 0usize;
    let mut xx = x;
    while xx < width && (recon[y * width + xx] - run_val).abs() <= st.near {
        recon[y * width + xx] = run_val;
        run += 1;
        xx += 1;
    }
    let interrupted = xx < width;
    while run >= 1 << RUN_J[st.run_idx] {
        w.write_bit(1);
        run -= 1 << RUN_J[st.run_idx];
        if st.run_idx < 31 {
            st.run_idx += 1;
        }
    }
    if interrupted {
        w.write_bit(0);
        let bits = RUN_J[st.run_idx];
        if bits > 0 {
            w.write_bits(run as u32, bits);
        }
        let glimit_index = st.run_idx;
        if st.run_idx > 0 {
            st.run_idx -= 1;
        }
        let ix = recon[y * width + xx];
        let (_, rb, _, _) = neighbors(recon, width, xx, y);
        let ri_type = if (ra - rb).abs() <= st.near { 1 } else { 0 };
        recon[y * width + xx] = encode_run_interruption(w, st, ix, ra, rb, glimit_index, ri_type);
        return xx + 1;
    }
    if run > 0 {
        w.write_bit(1);
    }
    width
}
fn encode_run_interruption(
    w: &mut BitWriter,
    st: &mut LocoState,
    ix: i64,
    ra: i64,
    rb: i64,
    run_index_before: usize,
    ri_type: usize,
) -> i64 {
    let px = if ri_type == 1 { ra } else { rb };
    let mut err = ix - px;
    let mut sign = 1;
    if ri_type == 0 && ra > rb {
        err = -err;
        sign = -1;
    }
    err = quantize_prediction_error(err, st.near);
    let rx = (px + sign * err * (2 * st.near + 1)).clamp(0, st.maxval);
    err = st.mod_range(err);
    let q = 365 + ri_type;
    let rt = ri_type as i64;
    let mut temp = st.ctx[q].a;
    if ri_type == 1 {
        temp = st.ctx[q].a + (st.ctx[q].n >> 1);
    }
    let k = loco::golomb_k(temp, st.ctx[q].n);
    let nn = st.nn[ri_type];
    let n = st.ctx[q].n;
    let map_flag =
        (k == 0 && err > 0 && 2 * nn < n) || (err < 0 && 2 * nn >= n) || (err < 0 && k != 0);
    let mapped = 2 * err.abs() - rt - map_flag as i64;
    let glimit = st.limit - RUN_J[run_index_before] as i64 - 1;
    write_limited_golomb(w, mapped, k, glimit, st.qbpp);
    if err < 0 {
        st.nn[ri_type] += 1;
    }
    st.ctx[q].a += (mapped + 1 - rt) >> 1;
    if st.ctx[q].n == st.reset {
        st.ctx[q].a >>= 1;
        st.ctx[q].n >>= 1;
        st.nn[ri_type] >>= 1;
    }
    st.ctx[q].n += 1;
    rx
}
