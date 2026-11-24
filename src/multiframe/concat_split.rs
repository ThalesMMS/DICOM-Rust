use super::ConcatenationPart;
use super::{Error, Kind, SplitOptions, plan_concatenation};
use crate::object::Vr;
use crate::object::frame_writer::{FrameSource, FrameWriteOptions, write_file_frames};
use crate::pixel::frame_reader::FrameReader;
use crate::pixel::pixeldata::frame_size;
#[derive(Clone, Copy, Debug, Default)]
pub struct SplitWriteOptions {
    pub offset_table: u8,
    pub max_fragment_bytes: i64,
    pub max_fragments: i64,
}
pub fn split_concatenation(
    frames: &FrameReader<'_>,
    opts: &SplitOptions,
    write: SplitWriteOptions,
) -> Result<Vec<(ConcatenationPart, Vec<u8>)>, Error> {
    let metadata = frames.metadata();
    let syntax = frames.transfer_syntax();
    let source = frames.file();
    let mut packed_bits = 0i64;
    if !syntax.encapsulated && metadata.bits_allocated == 1 {
        packed_bits = i64::from(metadata.rows)
            * i64::from(metadata.columns)
            * i64::from(metadata.samples_per_pixel);
    }
    let pixel_vr = source
        .dataset
        .get(crate::object::Tag::PIXEL_DATA)
        .map(|e| e.vr());
    let decoded = packed_bits > 0
        || (!syntax.encapsulated
            && metadata.bits_allocated == 8
            && syntax.big_endian
            && pixel_vr == Some(Vr::OW));
    let count = metadata.number_of_frames;
    let mut lengths = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count {
        if decoded {
            lengths.push(frame_size(metadata));
            continue;
        }
        let length = frames
            .raw_frame_length(i)
            .map_err(|e| Error::wrap(Kind::ConcatenationSplit, format!("frame {}: {e}", i + 1)))?;
        lengths.push(length);
    }
    let parts = plan_concatenation(Some(source), &lengths, opts)?;
    let frame_opts = FrameWriteOptions {
        write: opts.write.clone(),
        offset_table: write.offset_table,
        max_fragment_bytes: write.max_fragment_bytes,
        max_fragments: write.max_fragments,
    };
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let number = part.number;
        let instance =
            |e: &dyn std::fmt::Display| Error::with_text(format!("instance {number}: {e}"));
        let mut data = Vec::with_capacity(part.frames.max(0) as usize);
        for i in part.first_frame..part.first_frame + part.frames {
            let frame = if decoded {
                frames.frame(i).map(|(f, _)| f)
            } else {
                frames.raw_frame(i)
            };
            data.push(frame.map_err(|e| instance(&e))?);
        }
        let bits = vec![packed_bits; data.len()];
        let source = FrameSource {
            frames: &data,
            packed_bits: (packed_bits > 0).then_some(bits.as_slice()),
        };
        let bytes = write_file_frames(&part.file, source, &frame_opts).map_err(|e| instance(&e))?;
        out.push((part, bytes));
    }
    Ok(out)
}
