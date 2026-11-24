use super::extract::{Info, copy};
use super::mux::{MuxInfo, MuxOptions, mux_fragmented_mp4};
use super::ts::demux_transport_stream;
use super::{CODEC_H264, CODEC_HEVC, CONTAINER_MPEG_TRANSPORT, Error, Kind};
use crate::object::File;
use std::io::Write;
pub const CONTAINER_ANNEX_B: &str = "Annex B";
fn is_annex_b(data: &[u8]) -> bool {
    data.starts_with(&[0, 0, 1]) || data.starts_with(&[0, 0, 0, 1])
}
pub fn export_fragmented_mp4(
    file: &File,
    dst: &mut dyn Write,
    mut opts: MuxOptions,
) -> (Info, Result<MuxInfo, Error>) {
    let mut media: Vec<u8> = Vec::new();
    let (mut info, copied) = copy(file, &mut media, opts.limits);
    if let Err(err) = &copied
        && !err.is(Kind::UnsupportedContainer)
    {
        return (info, Err(err.clone()));
    }
    if info.codec != CODEC_H264 && info.codec != CODEC_HEVC {
        let err = Error::new(
            Kind::UnsupportedContainer,
            format!("MP4 export supports H.264 and HEVC, not {}", info.codec),
        );
        return (info, Err(err));
    }
    match copied {
        Ok(()) if info.container == CONTAINER_MPEG_TRANSPORT => {
            match demux_transport_stream(&media, info.codec) {
                Ok(stream) => media = stream,
                Err(err) => return (info, Err(err)),
            }
        }
        Ok(()) => {
            let err = Error::new(
                Kind::UnsupportedContainer,
                format!("the object already holds {}; use Copy", info.container),
            );
            return (info, Err(err));
        }
        Err(err) if !is_annex_b(&media) => return (info, Err(err)),
        Err(_) => {
            info.container = CONTAINER_ANNEX_B;
            (info.extension, info.mime_type) = if info.codec == CODEC_HEVC {
                (".h265", "video/h265")
            } else {
                (".h264", "video/h264")
            };
        }
    }
    opts.codec = info.codec;
    if opts.frame_rate <= 0.0 {
        opts.frame_rate = info.frame_rate;
    }
    let muxed = mux_fragmented_mp4(dst, &media, opts);
    (info, muxed)
}
