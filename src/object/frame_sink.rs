#![forbid(unsafe_code)]
use super::deferred::{self, DeferOptions};
use super::error::{Error, Kind, Result};
use super::file::{self, ReadFileOptions, ReadResult};
use super::obj::Object;
use super::parser::Reader;
use super::parser::source::MemFile;
pub use super::parser::{EncapsulatedSink, Frame, FrameMetadata, FrameSink, FrameSinkFn};
use super::transfer::{self, Syntax};
use std::rc::Rc;
#[derive(Default)]
pub struct ReadSinks {
    pub frame_sink: Option<Box<dyn FrameSink>>,
    pub encapsulated_sink: Option<Box<dyn EncapsulatedSink>>,
}
impl ReadSinks {
    pub fn is_empty(&self) -> bool {
        self.frame_sink.is_none() && self.encapsulated_sink.is_none()
    }
    pub(super) fn install(self, reader: &mut Reader) {
        if let Some(sink) = self.frame_sink {
            reader.set_frame_sink(sink);
        }
        if let Some(sink) = self.encapsulated_sink {
            reader.set_encapsulated_sink(sink);
        }
    }
    fn close(self, err: Option<Error>) -> Option<Error> {
        let mut err = err;
        if let Some(mut sink) = self.encapsulated_sink {
            err = Error::join(err, sink.close().err());
        }
        if let Some(mut sink) = self.frame_sink {
            err = Error::join(err, sink.close().err());
        }
        err
    }
}
#[doc(hidden)]
pub fn read_file_with_sinks(
    data: &[u8],
    seekable: bool,
    opts: ReadFileOptions,
    defer: &DeferOptions,
    sinks: ReadSinks,
) -> ReadResult {
    if opts.allow_incomplete_pixel_data && !sinks.is_empty() {
        return Err((None, Error::sentinel(Kind::IncompletePixelDataOptions)));
    }
    if let Err(err) = file::incomplete_file_options(&opts) {
        return Err((None, err));
    }
    let mut held = Some(sinks);
    let mut defer = defer.clone();
    let result = if !seekable && deferred::requires_seekable(&defer) {
        Err((
            None,
            Error::msg(deferred::ERR_DEFERRED_VALUE_REQUIRES_SEEKABLE),
        ))
    } else {
        if !seekable {
            defer.inline_value_bytes_threshold = 0;
        }
        file::read_file_core(data, opts, Some(&defer), &mut held, seekable, None)
    };
    match (result, held.take()) {
        (Err((file, err)), Some(sinks)) => match sinks.close(Some(err.clone())) {
            Some(joined) => Err((file, joined)),
            None => Err((file, err)),
        },
        (result, _) => result,
    }
}
#[doc(hidden)]
pub fn read_data_set_with_sinks(
    data: &[u8],
    seekable: bool,
    syntax: Syntax,
    opts: ReadFileOptions,
    defer: &DeferOptions,
    sinks: ReadSinks,
) -> Result<Object> {
    if opts.allow_incomplete_pixel_data && !sinks.is_empty() {
        return Err(Error::sentinel(Kind::IncompletePixelDataOptions));
    }
    file::incomplete_file_options(&opts)?;
    let had_sinks = !sinks.is_empty();
    let mut held = Some(sinks);
    let result = (|| {
        if deferred::requires_seekable(defer) && !seekable {
            return Err(Error::msg(deferred::ERR_DEFERRED_VALUE_REQUIRES_SEEKABLE));
        }
        if had_sinks && transfer::is_legacy(syntax.uid) {
            return Err(Error::sentinel(Kind::LegacySyntaxFrameSink));
        }
        if deferred::requires_seekable(defer) && syntax.deflated {
            return Err(deferred::deflated_deferred_value_error());
        }
        let source: Rc<[u8]> = Rc::from(data);
        if syntax.deflated || !seekable {
            return file::read_data_set_stream_core(
                Box::new(MemFile::new(source)),
                syntax,
                opts,
                &mut held,
                None,
            );
        }
        file::read_data_set_seekable_core(source, syntax, opts, defer, &mut held, None)
    })();
    match (result, held.take()) {
        (Err(err), Some(sinks)) => match sinks.close(Some(err.clone())) {
            Some(joined) => Err(joined),
            None => Err(err),
        },
        (result, _) => result,
    }
}
