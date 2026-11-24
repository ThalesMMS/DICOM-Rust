use super::{Cause, Error, InstanceRef, Kind, Tile, TileImage};
use crate::derivedio;
use crate::net::dicomweb::{self, FramePart, RetrieveOptions};
use crate::object::{File, Object, Tag};
use crate::pixel::Metadata;
use crate::pixel::frame::{FrameOptions, new_encapsulated_frame, new_native_frame};
use crate::pixel::pixeldata::{self, PixelData, PixelErrorKind};
use crate::transfer::{default_registry, normalize_uid};
use std::collections::HashMap;
pub trait TileSource {
    fn fetch_tile(&self, tile: &Tile) -> Result<Option<TileImage>, Error>;
}
pub struct TileSourceFn<F>(pub F);
impl<F: Fn(&Tile) -> Result<Option<TileImage>, Error>> TileSource for TileSourceFn<F> {
    fn fetch_tile(&self, tile: &Tile) -> Result<Option<TileImage>, Error> {
        (self.0)(tile)
    }
}
impl<T: TileSource + ?Sized> TileSource for &T {
    fn fetch_tile(&self, tile: &Tile) -> Result<Option<TileImage>, Error> {
        (**self).fetch_tile(tile)
    }
}
impl<T: TileSource + ?Sized> TileSource for Box<T> {
    fn fetch_tile(&self, tile: &Tile) -> Result<Option<TileImage>, Error> {
        (**self).fetch_tile(tile)
    }
}
impl<T: TileSource + ?Sized> TileSource for std::sync::Arc<T> {
    fn fetch_tile(&self, tile: &Tile) -> Result<Option<TileImage>, Error> {
        (**self).fetch_tile(tile)
    }
}
pub type FileOpener = dyn Fn(&InstanceRef) -> Result<Option<File>, Error> + Send + Sync;
#[derive(Default)]
pub struct LocalTileSource {
    pub open: Option<Box<FileOpener>>,
}
impl TileSource for LocalTileSource {
    fn fetch_tile(&self, tile: &Tile) -> Result<Option<TileImage>, Error> {
        let Some(open) = &self.open else {
            return Err(Error::other(
                "dicom/microscopy: local tile source has no opener",
            ));
        };
        let Some(file) = open(&tile.source)? else {
            return Err(Error::other("dicom/microscopy: opener returned no dataset"));
        };
        frame_from_file(&file, tile.frame_number).map(Some)
    }
}
fn pixel_error(err: impl std::fmt::Display) -> Error {
    Error::with_cause(Kind::Other, Some(Cause::Pixel), err.to_string())
}
fn out_of_range(frame: i64, count: impl std::fmt::Display) -> Error {
    Error::other(format!(
        "dicom/microscopy: frame {frame} out of range [1,{count}]"
    ))
}
fn native_image(
    index: i64,
    data: Vec<u8>,
    metadata: Metadata,
    big_endian: bool,
) -> Result<TileImage, Error> {
    let options = FrameOptions {
        big_endian,
        ..FrameOptions::default()
    };
    new_native_frame(index, data, metadata, options)
        .image()
        .map(TileImage::from_frame)
        .map_err(pixel_error)
}
pub(crate) fn frame_from_file(file: &File, frame_number: i64) -> Result<TileImage, Error> {
    if frame_number <= 0 {
        return Err(Error::other(
            "dicom/microscopy: frame numbers are one-based",
        ));
    }
    let ds = &file.dataset;
    let metadata = pixeldata::extract_metadata(ds).map_err(pixel_error)?;
    let native_err = match pixeldata::extract_native_frames(ds) {
        Ok(mut native) => {
            if frame_number as u64 > native.data.len() as u64 {
                return Err(out_of_range(frame_number, native.data.len()));
            }
            let data = std::mem::take(&mut native.data[frame_number as usize - 1]);
            return native_image(
                frame_number - 1,
                data,
                native.metadata,
                file.transfer_syntax.big_endian,
            );
        }
        Err(err) => err,
    };
    let uid = file.transfer_syntax.uid.to_string();
    if let Some(syntax) = default_registry().get(normalize_uid(&uid))
        && !syntax.encapsulated
    {
        return deferred_native_frame(ds, metadata, frame_number, syntax.big_endian);
    }
    if native_err.kind != PixelErrorKind::EncapsulatedPixelData {
        return Err(pixel_error(native_err));
    }
    let fragments = match pixeldata::extract_view(ds).map_err(pixel_error)? {
        PixelData::Encapsulated { fragments, .. } => fragments,
        PixelData::Native(_) => &[],
    };
    if metadata.number_of_frames <= 0 || fragments.len() as i64 != metadata.number_of_frames {
        return Err(Error::with_cause(
            Kind::Other,
            Some(Cause::UnsupportedFragmentLayout),
            format!(
                "dicom/microscopy: compressed frame layout is not independently addressable: {} fragments for {} frames",
                fragments.len(),
                metadata.number_of_frames
            ),
        ));
    }
    if frame_number as u64 > fragments.len() as u64 {
        return Err(out_of_range(frame_number, fragments.len()));
    }
    decode_encapsulated_frame(&fragments[frame_number as usize - 1], &uid, metadata)
}
fn deferred_native_frame(
    ds: &Object,
    metadata: Metadata,
    frame_number: i64,
    big_endian: bool,
) -> Result<TileImage, Error> {
    if metadata.number_of_frames <= 0 || frame_number > metadata.number_of_frames {
        return Err(out_of_range(frame_number, metadata.number_of_frames));
    }
    let frame_bytes = pixeldata::frame_size(&metadata);
    if frame_bytes <= 0 {
        return Err(Error::other(format!(
            "dicom/microscopy: invalid native frame size {frame_bytes}"
        )));
    }
    let tag = derivedio::TAG_PIXEL_DATA;
    let Some(element) = ds.get(tag) else {
        return Err(Error::other(format!("dicom: missing element {tag}")));
    };
    let Some(raw) = element.raw() else {
        return Err(Error::other(format!(
            "dicom: element {tag} value is not a raw byte payload"
        )));
    };
    let start = (frame_number - 1) * frame_bytes;
    let end = frame_number * frame_bytes;
    let mut data = vec![0u8; frame_bytes as usize];
    let copy_start = start.max(0);
    let copy_end = end.min(raw.len() as i64);
    let mut copied = 0;
    if copy_start < copy_end {
        let count = (copy_end - copy_start) as usize;
        data[..count].copy_from_slice(&raw[copy_start as usize..copy_end as usize]);
        copied = count as i64;
    }
    if copied != frame_bytes {
        return Err(Error::other(format!(
            "dicom/microscopy: copied {copied} of {frame_bytes} native frame bytes"
        )));
    }
    native_image(frame_number - 1, data, metadata, big_endian)
}
pub trait DicomwebFrameClient {
    fn retrieve_frames(
        &self,
        instance: &dicomweb::InstanceRef,
        frames: &[i64],
        options: &RetrieveOptions,
    ) -> Result<Vec<FramePart>, Error>;
}
pub struct DicomwebTileSource<C> {
    pub client: Option<C>,
    pub metadata: HashMap<String, Metadata>,
    pub options: RetrieveOptions,
}
impl<C: DicomwebFrameClient> TileSource for DicomwebTileSource<C> {
    fn fetch_tile(&self, tile: &Tile) -> Result<Option<TileImage>, Error> {
        let Some(client) = &self.client else {
            return Err(Error::other(
                "dicom/microscopy: DICOMweb tile source has no client",
            ));
        };
        let Some(metadata) = self.metadata.get(&tile.source.sop_instance_uid) else {
            return Err(Error::other(format!(
                "dicom/microscopy: no pixel metadata for instance {}",
                tile.source.sop_instance_uid
            )));
        };
        let instance = dicomweb::InstanceRef {
            study_instance_uid: tile.source.study_instance_uid.clone(),
            series_instance_uid: tile.source.series_instance_uid.clone(),
            sop_instance_uid: tile.source.sop_instance_uid.clone(),
        };
        let parts = client.retrieve_frames(&instance, &[tile.frame_number], &self.options)?;
        if parts.len() != 1 || parts[0].frame_number != tile.frame_number {
            return Err(Error::other(format!(
                "dicom/microscopy: WADO-RS returned no matching frame {}",
                tile.frame_number
            )));
        }
        let part = &parts[0];
        let mut syntax_uid = part.transfer_syntax_uid.as_str();
        if syntax_uid.is_empty() && self.options.transfer_syntax_uids.len() == 1 {
            syntax_uid = &self.options.transfer_syntax_uids[0];
        }
        if let Some(syntax) = default_registry().get(normalize_uid(syntax_uid))
            && !syntax.encapsulated
        {
            return native_image(
                tile.frame_number - 1,
                part.data.clone(),
                metadata.clone(),
                syntax.big_endian,
            )
            .map(Some);
        }
        decode_encapsulated_frame(&part.data, syntax_uid, metadata.clone()).map(Some)
    }
}
fn decode_encapsulated_frame(
    payload: &[u8],
    syntax_uid: &str,
    mut metadata: Metadata,
) -> Result<TileImage, Error> {
    metadata.number_of_frames = 1;
    let template = metadata_object(&metadata);
    let fragments = [payload.to_vec()];
    let pixels = PixelData::Encapsulated {
        offset_table: &[0; 4],
        fragments: &fragments,
    };
    let decoded = pixeldata::decode_frames(syntax_uid, &pixels, &template).map_err(pixel_error)?;
    if decoded.data.len() != 1 {
        return Err(Error::other(format!(
            "dicom/microscopy: codec returned {} frames for one tile",
            decoded.data.len()
        )));
    }
    let data = decoded.data.into_iter().next().unwrap_or_default();
    new_encapsulated_frame(0, data, metadata, FrameOptions::default())
        .image()
        .map(TileImage::from_frame)
        .map_err(pixel_error)
}
fn metadata_object(m: &Metadata) -> Object {
    let t = |element| Tag::new(0x0028, element);
    let mut elements = vec![
        derivedio::us(t(0x0010), &[m.rows]),
        derivedio::us(t(0x0011), &[m.columns]),
        derivedio::us(t(0x0002), &[m.samples_per_pixel]),
        derivedio::cs(t(0x0004), &m.photometric_interpretation),
        derivedio::is(t(0x0008), &[1]),
        derivedio::us(t(0x0100), &[m.bits_allocated]),
        derivedio::us(t(0x0101), &[m.bits_stored]),
        derivedio::us(t(0x0102), &[m.high_bit]),
        derivedio::us(t(0x0103), &[m.pixel_representation]),
    ];
    if m.planar_configuration_present {
        elements.push(derivedio::us(t(0x0006), &[m.planar_configuration]));
    }
    let mut obj = derivedio::object(elements);
    obj.set_value_big_endian(false);
    obj
}
