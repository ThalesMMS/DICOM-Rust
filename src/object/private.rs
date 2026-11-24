#![forbid(unsafe_code)]
use super::error::Error;
use super::obj::Object;
use super::types::Tag;
use crate::dictionary::reservations::{
    PrivateError, PrivateReservations, is_private_creator_tag, object_reservations,
};
pub const ERR_PRIVATE_DEFINITION_MISSING: &str = "dicom: private attribute definition missing";
const MAX_PRIVATE_ISSUES: usize = 128;
fn join(result: Option<String>, err: String) -> String {
    match result {
        Some(r) => format!("{r}\n{err}"),
        None => err,
    }
}
pub fn validate_private_reservations(o: &Object) -> Result<(), Error> {
    let mut probe = PrivateReservations::new(o.len());
    let mut result: Option<String> = None;
    let mut issues = 0;
    for e in o.elements() {
        if !is_private_creator_tag(e.tag()) {
            continue;
        }
        match probe.observe(e) {
            Ok(()) => {}
            Err(PrivateError::ResourceLimit) => {
                return Err(Error::msg(PrivateError::ResourceLimit.to_string()));
            }
            Err(err) => {
                if issues < MAX_PRIVATE_ISSUES {
                    issues += 1;
                    result = Some(join(result, format!("{}: {err}", e.tag())));
                }
            }
        }
    }
    match object_reservations(o) {
        Err(err) => return Err(Error::msg(join(result, err.to_string()))),
        Ok(scope) => {
            for e in o.elements() {
                let tag = e.tag();
                if is_private_creator_tag(tag)
                    && scope.creator(Tag::new(tag.group, tag.element << 8))
                        == Err(PrivateError::Conflict)
                {
                    return Err(Error::msg(join(result, PrivateError::Conflict.to_string())));
                }
            }
        }
    }
    match result {
        Some(r) => Err(Error::msg(r)),
        None => Ok(()),
    }
}
pub fn resolve_private_entry(o: &Object, tag: Tag) -> Result<(), Error> {
    let scope = object_reservations(o).map_err(|e| Error::msg(e.to_string()))?;
    scope.creator(tag).map_err(|e| Error::msg(e.to_string()))?;
    Err(Error::msg(ERR_PRIVATE_DEFINITION_MISSING))
}
pub use super::parser::{PrivateDiagnostic, PrivateDictionaryOptions, PrivateIssue};
impl Object {
    pub fn parsed_private_diagnostics(&self) -> (&[PrivateDiagnostic], bool) {
        (
            &self.private_diagnostics,
            self.private_diagnostics_truncated,
        )
    }
}
#[doc(hidden)]
pub fn read_file_with_private_dictionary(
    data: &[u8],
    opts: super::file::ReadFileOptions,
    private: &PrivateDictionaryOptions,
) -> super::file::ReadResult {
    super::file::read_file_core(data, opts, None, &mut None, true, Some(private))
}
#[doc(hidden)]
pub fn read_data_set_with_private_dictionary(
    data: &[u8],
    syntax: super::transfer::Syntax,
    opts: super::file::ReadFileOptions,
    private: &PrivateDictionaryOptions,
) -> Result<Object, Error> {
    let source: std::rc::Rc<[u8]> = std::rc::Rc::from(data);
    if syntax.deflated {
        return super::file::read_data_set_stream_core(
            Box::new(super::parser::source::MemFile::new(source)),
            syntax,
            opts,
            &mut None,
            Some(private),
        );
    }
    super::file::read_data_set_seekable_core(
        source,
        syntax,
        opts,
        &super::deferred::DeferOptions::default(),
        &mut None,
        Some(private),
    )
}
