use super::{Accumulator, Error, Metrics, ctx_err};
use crate::display::{self, ColorFrame, Frame, LutOptions, PixelFormat, VoiLut};
use crate::net::ul::Ctx;
use crate::object::File;
use crate::pixel::pixeldata::{NativeFrames, extract_native_frames};
use crate::pixel::transcode::DecoderRegistry;
use crate::pixel::transcode::decompress::{DecompressOptions, decompress_file};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Domain {
    #[default]
    Modality,
    Stored,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilesOptions {
    pub domain: Domain,
    pub registry: Option<DecoderRegistry>,
}
pub fn files(
    ctx: &Ctx,
    reference: Option<&File>,
    test: Option<&File>,
    options: &FilesOptions,
) -> Result<Metrics, Error> {
    let (Some(reference), Some(test)) = (reference, test) else {
        return Err(Error::Mismatch(Some("nil file")));
    };
    let registry = options
        .registry
        .unwrap_or(DecoderRegistry::BuiltinWithoutJpeg2000);
    let wrap = |context: &str, err: Error| Error::Wrapped(context.to_string(), Box::new(err));
    let r = Decoded::new(ctx, reference, registry).map_err(|e| wrap("reference", e))?;
    let t = Decoded::new(ctx, test, registry).map_err(|e| wrap("test", e))?;
    let (a, b) = (&r.frames.metadata, &t.frames.metadata);
    if a.rows != b.rows
        || a.columns != b.columns
        || a.samples_per_pixel != b.samples_per_pixel
        || r.frames.data.len() != t.frames.data.len()
        || r.monochrome() != t.monochrome()
    {
        return Err(Error::MismatchDetail(format!(
            "{}x{}x{} with {} sample(s) {} against {}x{}x{} with {} sample(s) {}",
            a.columns,
            a.rows,
            r.frames.data.len(),
            a.samples_per_pixel,
            a.photometric_interpretation,
            b.columns,
            b.rows,
            t.frames.data.len(),
            b.samples_per_pixel,
            b.photometric_interpretation
        )));
    }
    let mut acc = Accumulator::default();
    for frame in 0..r.frames.data.len() {
        ctx_err(ctx)?;
        let rv = r
            .values(ctx, frame, options.domain)
            .map_err(|e| wrap(&format!("reference frame {}", frame.saturating_add(1)), e))?;
        let tv = t
            .values(ctx, frame, options.domain)
            .map_err(|e| wrap(&format!("test frame {}", frame.saturating_add(1)), e))?;
        if rv.len() != tv.len() {
            return Err(Error::MismatchDetail(format!(
                "frame {} has {} and {} samples",
                frame.saturating_add(1),
                rv.len(),
                tv.len()
            )));
        }
        for (reference, test) in rv.iter().zip(&tv) {
            acc.add(*reference, *test);
        }
    }
    let mut m = acc.metrics();
    m.frames = r.frames.data.len() as i64;
    Ok(m)
}
struct Decoded<'a> {
    original: &'a File,
    decoded: Option<File>,
    frames: NativeFrames,
    big_endian: bool,
}
fn other(err: impl std::fmt::Display) -> Error {
    Error::Other(err.to_string())
}
impl<'a> Decoded<'a> {
    fn new(ctx: &Ctx, file: &'a File, registry: DecoderRegistry) -> Result<Decoded<'a>, Error> {
        let syntax = crate::transfer::default_registry().get(file.transfer_syntax.uid);
        let needs_codec = file.transfer_syntax.encapsulated
            || syntax.as_ref().is_some_and(|s| s.requires_codec());
        let decoded = if needs_codec {
            ctx_err(ctx)?;
            let options = DecompressOptions {
                registry: Some(registry),
                ..DecompressOptions::default()
            };
            Some(decompress_file(file, &options).map_err(other)?)
        } else {
            None
        };
        let native = decoded.as_ref().unwrap_or(file);
        let frames = extract_native_frames(&native.dataset).map_err(other)?;
        let big_endian = native.transfer_syntax.big_endian;
        Ok(Decoded {
            original: file,
            decoded,
            frames,
            big_endian,
        })
    }
    fn dataset(&self) -> &crate::object::Object {
        &self.decoded.as_ref().unwrap_or(self.original).dataset
    }
    fn photometric(&self) -> String {
        self.frames
            .metadata
            .photometric_interpretation
            .trim()
            .to_uppercase()
    }
    fn monochrome(&self) -> bool {
        self.photometric().starts_with("MONOCHROME")
    }
    fn values(&self, ctx: &Ctx, frame: usize, domain: Domain) -> Result<Vec<f64>, Error> {
        let m = &self.frames.metadata;
        let Some(data) = self.frames.data.get(frame) else {
            return Err(Error::MismatchDetail(format!(
                "frame {frame} of {}",
                self.frames.data.len()
            )));
        };
        let count = i64::from(m.rows)
            .saturating_mul(i64::from(m.columns))
            .saturating_mul(i64::from(m.samples_per_pixel));
        if domain == Domain::Stored {
            return stored_samples(ctx, data, m, self.big_endian, count);
        }
        let format = PixelFormat {
            bits_allocated: i64::from(m.bits_allocated),
            bits_stored: i64::from(m.bits_stored),
            high_bit: i64::from(m.high_bit),
            signed: m.pixel_representation == 1,
            big_endian: self.big_endian,
        };
        let dataset = self.dataset();
        if self.monochrome() {
            let (modality, err) =
                display::modality_lut_at(dataset, frame as i64, LutOptions::default());
            if let Some(err) = err {
                return Err(other(err));
            }
            return display::decode_modality(&Frame {
                rows: i64::from(m.rows),
                columns: i64::from(m.columns),
                pixels: data,
                format,
                modality,
                voi: VoiLut::default(),
            })
            .map_err(other);
        }
        let photometric = self.photometric();
        let palette = if photometric == "PALETTE COLOR" {
            display::palette_from_object(dataset, LutOptions::default()).map_err(other)?
        } else {
            None
        };
        let rgba = display::render_color(&ColorFrame {
            rows: i64::from(m.rows),
            columns: i64::from(m.columns),
            pixels: data,
            photometric,
            samples_per_pixel: i64::from(m.samples_per_pixel),
            planar_configuration: i64::from(m.planar_configuration),
            format,
            palette,
        })
        .map_err(other)?;
        Ok(rgba
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
            .collect())
    }
}
fn stored_samples(
    ctx: &Ctx,
    src: &[u8],
    m: &crate::pixel::Metadata,
    big_endian: bool,
    max_samples: i64,
) -> Result<Vec<f64>, Error> {
    let format = crate::pixel::sample_format(m, big_endian);
    let samples = crate::pixel::samples::stored_samples::<i64>(ctx, src, &format, max_samples)
        .map_err(|e| match e {
            crate::pixel::samples::SampleError::Context(kind) => Error::Context(kind),
            other => Error::Other(other.to_string()),
        })?;
    Ok(samples.into_iter().map(|v| v as f64).collect())
}
