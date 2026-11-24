use super::{
    BasicProfileOptions, Error, Kind, Options, ProfileReport, Report, UidRemapper,
    anonymize_object_with_report, rederive_file_meta,
};
use crate::object::deferred::{DeferOptions, read_file_deferred};
use crate::object::{self, File, ReadFileOptions};
use std::path::{Path, PathBuf};
fn os_error(e: std::io::Error) -> String {
    let text = e.to_string();
    match text.rfind(" (os error ") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}
fn object_error(text: impl Into<String>) -> Error {
    Error::with_text(Kind::Object, text)
}
fn absolute(path: &str) -> Result<PathBuf, Error> {
    std::path::absolute(path).map_err(|e| object_error(os_error(e)))
}
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
fn open_path_source(path: &Path, defer_pixels: bool) -> Result<File, Error> {
    let display = path.to_string_lossy().into_owned();
    let data = std::fs::read(path)
        .map_err(|e| object_error(format!("open {display}: {}", os_error(e))))?;
    if defer_pixels {
        let defer = DeferOptions {
            defer_pixel_data: true,
            ..DeferOptions::default()
        };
        match read_file_deferred(&data, true, ReadFileOptions::default(), &defer) {
            Ok(f) => return Ok(f),
            Err((_, e)) if e.to_string().contains("deflated datasets cannot preserve") => {}
            Err((_, e)) => return Err(e.into()),
        }
    }
    object::read_file(&data).map_err(|(_, e)| e.into())
}
pub(super) fn deidentify_path(
    source: &str,
    destination: &str,
    defer_pixels: bool,
    apply: &mut dyn FnMut(&mut File) -> Result<(), Error>,
) -> Result<(), Error> {
    let source = absolute(source)?;
    let destination = absolute(destination)?;
    if source.exists() && destination.exists() && same_file(&source, &destination) {
        return Err(Error::bare(Kind::SamePath));
    }
    let mut file = open_path_source(&source, defer_pixels)?;
    apply(&mut file)?;
    let derived = rederive_file_meta(file)?;
    let bytes = object::writer::write_file(&derived)
        .map_err(|e| object_error(format!("deid: write de-identified copy: {e}")))?;
    let dir = destination.parent().unwrap_or(Path::new("."));
    let temp = temp_path(dir);
    std::fs::write(&temp, &bytes).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        object_error(format!("open {}: {}", temp.to_string_lossy(), os_error(e)))
    })?;
    if let Err(e) = std::fs::rename(&temp, &destination) {
        let _ = std::fs::remove_file(&temp);
        return Err(object_error(format!(
            "rename {} {}: {}",
            temp.to_string_lossy(),
            destination.to_string_lossy(),
            os_error(e)
        )));
    }
    Ok(())
}
fn temp_path(dir: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    loop {
        let n = seed
            .wrapping_add(NEXT.fetch_add(1, Ordering::Relaxed))
            .wrapping_mul(6364136223846793005)
            % 1_000_000_000;
        let candidate = dir.join(format!(".dicom-deid-{n}"));
        if !candidate.exists() {
            return candidate;
        }
    }
}
pub fn anonymize_path(
    source: impl AsRef<std::path::Path>,
    destination: impl AsRef<std::path::Path>,
    opts: &Options,
    uids: &mut UidRemapper,
) -> (Report, Option<Error>) {
    let source = source.as_ref().to_string_lossy();
    let source: &str = &source;
    let destination = destination.as_ref().to_string_lossy();
    let destination: &str = &destination;
    let mut report = Report::default();
    let defer = opts.burned_in_pixel_policy.is_none();
    let result = deidentify_path(source, destination, defer, &mut |file| {
        let (r, err) = anonymize_object_with_report(&mut file.dataset, opts, uids);
        report = r;
        err.map_or(Ok(()), Err)
    });
    (report, result.err())
}
pub fn basic_profile_path(
    source: impl AsRef<std::path::Path>,
    destination: impl AsRef<std::path::Path>,
    options: &mut BasicProfileOptions,
    uids: &mut UidRemapper,
) -> (ProfileReport, Option<Error>) {
    let source = source.as_ref().to_string_lossy();
    let source: &str = &source;
    let destination = destination.as_ref().to_string_lossy();
    let destination: &str = &destination;
    let (limits, selected) = match super::profile::normalize_profile_options(options) {
        Ok(v) => v,
        Err(e) => return (ProfileReport::default(), Some(e)),
    };
    let mut report = ProfileReport::default();
    let defer = !selected.contains(&super::ProfileOption::CleanPixelData)
        && options.pixel_redaction.is_none();
    let result = deidentify_path(source, destination, defer, &mut |file| {
        super::profile::validate_profile_budget(&file.dataset.to_data_set(), &limits)?;
        let pre_redacted = match &options.pixel_redaction {
            Some(r) => Some(super::pixel_redaction::redact_file_for_profile(
                file,
                r,
                limits.max_pixel_masks,
            )?),
            None => None,
        };
        let mut plan = super::profile::plan_basic_profile_redacted(
            &file.dataset,
            options,
            uids,
            pre_redacted.as_ref(),
        )?;
        report = plan.apply(&mut file.dataset)?;
        Ok(())
    });
    (report, result.err())
}
pub type MetadataCallback<'a> = dyn FnMut(&File) -> Result<(), crate::BoxError> + 'a;
pub type MetadataOpener<'a> = dyn FnMut() -> Result<Option<std::fs::File>, crate::BoxError> + 'a;
pub fn with_metadata_path(
    path: impl AsRef<Path>,
    use_file: Option<&mut MetadataCallback<'_>>,
) -> Result<(), crate::BoxError> {
    let Some(use_file) = use_file else {
        return Err("deid: nil metadata callback".into());
    };
    let file = open_path_source(path.as_ref(), true).map_err(|e| e.to_string())?;
    use_file(&file)
}
pub fn with_metadata_source(
    open: Option<&mut MetadataOpener<'_>>,
    use_file: Option<&mut MetadataCallback<'_>>,
) -> Result<(), crate::BoxError> {
    use std::io::{Read, Seek, SeekFrom};
    let Some(use_file) = use_file else {
        return Err("deid: nil metadata callback".into());
    };
    let Some(open) = open else {
        return Err("deid: nil metadata opener".into());
    };
    let Some(mut source) = open()? else {
        return Err("deid: nil metadata source".into());
    };
    let mut data = Vec::new();
    source.read_to_end(&mut data).map_err(os_error)?;
    let defer = DeferOptions {
        defer_pixel_data: true,
        ..DeferOptions::default()
    };
    let file = match read_file_deferred(&data, true, ReadFileOptions::default(), &defer) {
        Ok(f) => f,
        Err((_, e)) if e.to_string().contains("deflated datasets cannot preserve") => {
            source.seek(SeekFrom::Start(0)).map_err(os_error)?;
            data.clear();
            source.read_to_end(&mut data).map_err(os_error)?;
            open_deflated_source(&data)?
        }
        Err((_, e)) => return Err(e.to_string().into()),
    };
    drop(source);
    use_file(&file)
}
fn open_deflated_source(data: &[u8]) -> Result<File, String> {
    use crate::object::parser::source::MemFile;
    let header = object::read_part10_header_mem(
        &MemFile::new(std::rc::Rc::from(data)),
        &ReadFileOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    if header.transfer_syntax.uid != "1.2.840.10008.1.2.1.99" {
        return Err(object::deferred::ERR_DEFERRED_VALUE_REQUIRES_SEEKABLE.to_string());
    }
    let compressed = usize::try_from(header.data_set_offset)
        .ok()
        .and_then(|at| data.get(at..))
        .unwrap_or_default();
    if compressed.is_empty() {
        return Err("unexpected EOF".into());
    }
    let (inflated, err, _) = object::inflate::inflate_raw(compressed);
    if !err.is(object::Kind::Eof) {
        return Err(format!("deid: inflate dataset: {err}"));
    }
    let defer = DeferOptions {
        defer_pixel_data: true,
        ..DeferOptions::default()
    };
    let opts = ReadFileOptions {
        max_total_bytes: inflated.len() as i64 + 1,
        max_element_bytes: 128 << 20,
        max_pixel_data_bytes: 2 << 30,
        max_elements: 100_000,
        ..ReadFileOptions::default()
    };
    let dataset = object::deferred::read_data_set_deferred(
        &inflated,
        true,
        object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        opts,
        &defer,
    )
    .map_err(|e| format!("deid: read inflated dataset: {e}"))?;
    Ok(File {
        preamble: header.preamble,
        meta: header.meta,
        dataset,
        transfer_syntax: header.transfer_syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    })
}
