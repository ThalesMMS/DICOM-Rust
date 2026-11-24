use super::Metadata;
use super::encapsulated::{self, EncodedFrameSink, Stream, TableHeader};
use crate::object::error::{Error, Result};
use crate::object::parser::{EncapsulatedSink, FrameMetadata};
use crate::object::{Element, ElementHeader, Syntax, Value, Vr};
use std::io::Read;
fn sink_error(err: encapsulated::Error) -> Error {
    Error::msg(err.to_string())
}
fn table_header(header: &ElementHeader) -> TableHeader {
    TableHeader {
        group: header.tag.group,
        element: header.tag.element,
        vr_ov: header.vr == Vr::OV,
        length: header.length,
    }
}
impl<S: EncodedFrameSink> EncapsulatedSink for Stream<S> {
    fn context_error(&self) -> Option<Error> {
        self.is_closed()
            .then(|| Error::sentinel(crate::object::Kind::ContextCanceled))
    }
    fn check_extended_table(&mut self, header: &ElementHeader) -> Result<()> {
        Stream::check_extended_table(self, &table_header(header)).map_err(sink_error)
    }
    fn extended_table(&mut self, element: &Element) -> Result<()> {
        let value: &[u8] = match &element.value {
            Value::Raw(raw) => raw,
            _ => &[],
        };
        Stream::extended_table(self, &table_header(&element.header), value).map_err(sink_error)
    }
    fn start_pixel_data(&mut self, metadata: &FrameMetadata, syntax: &Syntax) -> Result<()> {
        let metadata = Metadata {
            rows: metadata.rows,
            columns: metadata.columns,
            samples_per_pixel: metadata.samples_per_pixel,
            bits_allocated: metadata.bits_allocated,
            bits_stored: metadata.bits_stored,
            high_bit: metadata.high_bit,
            pixel_representation: metadata.pixel_representation,
            planar_configuration: metadata.planar_configuration,
            planar_configuration_present: metadata.planar_configuration_present,
            number_of_frames: metadata.number_of_frames,
            photometric_interpretation: metadata.photometric_interpretation.clone(),
        };
        Stream::start_pixel_data(self, metadata, syntax.uid).map_err(sink_error)
    }
    fn item(&mut self, length: u32, basic: bool, value: &mut dyn Read) -> Result<()> {
        Stream::item(self, length, basic, value).map_err(sink_error)
    }
    fn end_pixel_data(&mut self) -> Result<()> {
        Stream::end_pixel_data(self).map_err(sink_error)
    }
    fn close(&mut self) -> Result<()> {
        Stream::close(self).map_err(sink_error)
    }
}
