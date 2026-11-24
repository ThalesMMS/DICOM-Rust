use super::plan::{
    ExportPlan, FramePlan, LinearTransform, Plan, TAG_FRAME_OF_REFERENCE_UID,
    TAG_SERIES_INSTANCE_UID, affine_close, duration_seconds, frame_linear_transform,
    integer_datatype, parametric_payload_bytes, parametric_working_set_bytes, plan_frame_geometry,
};
use super::source::SourceReadOptions;
use super::{
    COMPRESSION_GZIP, COMPRESSION_NONE, Cause, Error, ErrorCode, ExportSentinel as S,
    GEOMETRY_RESAMPLE_CUBIC_BSPLINE, Report, WARNING_PARAMETRIC_INFINITE_PRESERVED,
    WARNING_PARAMETRIC_MAPPING_OVERFLOW_AS_NAN, WARNING_PARAMETRIC_NAN_PRESERVED,
    WARNING_PARAMETRIC_OUTSIDE_MAPPING_AS_NAN, WARNING_PARAMETRIC_PADDING_AS_NAN, bare,
    checked_add, checked_mul, datatype_name, export_error, scaling_policy_name,
};
use crate::display::StoredSampleReader;
use crate::nifti::gzip::{Container, DeflateWriter};
use crate::nifti::header::{HeaderSpec, build_header};
use crate::nifti::sidecar::{CodedUnits, Sidecar};
use crate::nifti::{
    DATATYPE_FLOAT32, DATATYPE_FLOAT64, DATATYPE_INT8, DATATYPE_INT16, DATATYPE_INT32,
    DATATYPE_UINT8, DATATYPE_UINT16, DATATYPE_UINT32, datatype_bytes, finite,
};
use crate::object::File;
use crate::parametricmap;
use crate::pixel::Metadata;
use crate::pixel::pixeldata::PixelErrorKind;
use crate::pixel::pixeldata::{self, PixelData};
use crate::render::volume::{RegularGridInterpolation, Stack, build_volume_with_tolerances};
use crate::render::{self, GeometryAffine, SliceGeometry, Vec3};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
pub(crate) struct Spool {
    pub(crate) file: fs::File,
    path: PathBuf,
}
impl Drop for Spool {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
fn create_spool(dir: &str) -> std::io::Result<Spool> {
    spool_probe(dir, "dicom-nifti-spool-")
}
pub(crate) fn spool_probe(dir: &str, prefix: &str) -> std::io::Result<Spool> {
    let dir = if dir.is_empty() {
        std::env::temp_dir()
    } else {
        PathBuf::from(dir)
    };
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for attempt in 0..10_000u32 {
        let name = format!(
            "{prefix}{}",
            (seed as u64)
                .wrapping_add(u64::from(std::process::id()) << 20)
                .wrapping_add(u64::from(attempt))
                % 10_000_000_000
        );
        let path = dir.join(name);
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok(Spool { file, path }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("cannot create spool file"))
}
struct Counting<'a> {
    destination: &'a mut dyn Write,
    count: i64,
}
impl Write for Counting<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.destination.write(buf)?;
        self.count += n as i64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.destination.flush()
    }
}
enum Output<'a, 'b> {
    Plain(&'a mut Counting<'b>),
    Gzip(&'a mut DeflateWriter),
}
impl Output<'_, '_> {
    fn write_exact(&mut self, payload: &[u8]) -> std::io::Result<()> {
        match self {
            Output::Plain(w) => write_exact(*w, payload),
            Output::Gzip(z) => {
                z.write(payload);
                Ok(())
            }
        }
    }
}
fn write_exact(destination: &mut dyn Write, payload: &[u8]) -> std::io::Result<()> {
    if payload.is_empty() {
        return destination.write(payload).map(|_| ());
    }
    let written = destination.write(payload)?;
    if written != payload.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "short write",
        ));
    }
    Ok(())
}
fn write_error(e: impl std::fmt::Display) -> Error {
    export_error(
        ErrorCode::Write,
        -1,
        -1,
        None,
        Some(Cause::text(e.to_string())),
    )
}
impl Plan<'_> {
    pub fn write_voxels(&self, destination: &mut dyn Write) -> Result<Report, Error> {
        self.data_pass(destination, true)
    }
    pub fn write(&self, destination: &mut dyn Write) -> Result<Report, Error> {
        self.data_pass(destination, false)
    }
    fn data_pass(&self, destination: &mut dyn Write, voxels_only: bool) -> Result<Report, Error> {
        let mut working = self.value.clone();
        let mut spool = create_spool(&self.options.temp_dir).map_err(|e| {
            export_error(
                ErrorCode::Write,
                -1,
                -1,
                Some(S::InvalidSource),
                Some(Cause::text(e.to_string())),
            )
        })?;
        self.spool_frames(&mut working, &mut spool.file)?;
        spool.file.seek(SeekFrom::Start(0)).map_err(|e| {
            export_error(
                ErrorCode::Write,
                -1,
                -1,
                Some(S::InvalidSource),
                Some(Cause::text(e.to_string())),
            )
        })?;
        self.write_from_spool(&working, destination, &mut spool.file, voxels_only)
    }
    fn spool_frames(&self, plan: &mut ExportPlan, spool: &mut fs::File) -> Result<(), Error> {
        let mut by_source: Vec<Vec<usize>> = vec![Vec::new(); plan.sources as usize];
        for (index, frame) in plan.frames.iter().enumerate() {
            by_source[frame.source_index as usize].push(index);
        }
        let mut spool_bytes = 0u64;
        for (source_index, expected) in by_source.iter().enumerate() {
            let source_index = source_index as i64;
            if expected.is_empty() {
                return Err(bare(ErrorCode::Source, source_index, -1, S::InvalidSource));
            }
            let first = plan.frames[expected[0]].clone();
            let limits = &self.options.limits;
            let mut read_options = SourceReadOptions::default();
            if first.parametric {
                let frame_buffers = parametric_working_set_bytes(
                    0,
                    i64::from(first.metadata.rows),
                    i64::from(first.metadata.columns),
                );
                match frame_buffers {
                    Some(b) if b < limits.max_in_memory_source_bytes => {
                        let remaining = limits.max_in_memory_source_bytes - b;
                        if remaining <= i64::MAX as u64 {
                            read_options.max_total_bytes = remaining as i64;
                        }
                    }
                    _ => {
                        return Err(bare(ErrorCode::Limit, source_index, -1, S::LimitExceeded));
                    }
                }
            } else if plan.source_encapsulated[source_index as usize]
                && limits.max_in_memory_source_bytes <= i64::MAX as u64
            {
                read_options.max_total_bytes = limits.max_in_memory_source_bytes as i64;
            }
            let opened = self.source.open(source_index, &read_options).map_err(|e| {
                if e.limit {
                    let mut cause = Cause::text(e.text.clone());
                    cause.sentinels.push(S::LimitExceeded);
                    export_error(
                        ErrorCode::Limit,
                        source_index,
                        -1,
                        Some(S::LimitExceeded),
                        Some(cause),
                    )
                } else {
                    export_error(
                        ErrorCode::Source,
                        source_index,
                        -1,
                        Some(S::InvalidSource),
                        Some(Cause::text(e.text.clone())),
                    )
                }
            })?;
            let file = opened.file();
            if !same_identity(file, &plan.series_uid, &plan.frame_of_reference_uid)
                || file.transfer_syntax.encapsulated
                    != plan.source_encapsulated[source_index as usize]
            {
                return Err(bare(
                    ErrorCode::Identity,
                    source_index,
                    -1,
                    S::MixedIdentity,
                ));
            }
            if first.parametric {
                self.spool_parametric(plan, spool, file, expected, &mut spool_bytes)?;
            } else {
                self.spool_images(plan, spool, file, expected, &mut spool_bytes)?;
            }
        }
        Ok(())
    }
    fn spool_images(
        &self,
        plan: &mut ExportPlan,
        spool: &mut fs::File,
        file: &File,
        expected: &[usize],
        spool_bytes: &mut u64,
    ) -> Result<(), Error> {
        validate_source_frames(plan, file, expected)?;
        let first = plan.frames[expected[0]].clone();
        let pixels = |cause: Option<Cause>| {
            export_error(
                ErrorCode::Pixels,
                first.source_index,
                -1,
                Some(S::UnsupportedPixels),
                cause,
            )
        };
        match pixeldata::extract_native_frames(&file.dataset) {
            Ok(native) => {
                if native.data.len() != expected.len() {
                    return Err(pixels(None));
                }
                for (frame_index, raw) in native.data.iter().enumerate() {
                    self.spool_image_frame(
                        plan,
                        spool,
                        expected[frame_index],
                        raw,
                        &native.metadata,
                        file.transfer_syntax.big_endian,
                        spool_bytes,
                    )?;
                }
                return Ok(());
            }
            Err(e) if e.kind != PixelErrorKind::EncapsulatedPixelData => {
                return Err(pixels(Some(Cause::text(e.to_string()))));
            }
            Err(_) => {}
        }
        let view = match pixeldata::extract_view(&file.dataset) {
            Ok(v @ PixelData::Encapsulated { .. }) => v,
            Ok(_) => return Err(pixels(None)),
            Err(e) => return Err(pixels(Some(Cause::text(e.to_string())))),
        };
        let limit = || bare(ErrorCode::Limit, first.source_index, -1, S::LimitExceeded);
        let encoded_bytes = encapsulated_resident_bytes(&view).ok_or_else(limit)?;
        let m = &first.metadata;
        let decoded = checked_mul(expected.len() as u64, pixeldata::frame_size(m) as u64);
        let frame_output = checked_mul(
            u64::from(m.rows) * u64::from(m.columns),
            (plan.spool_bitpix / 8) as u64,
        );
        let working = decoded
            .zip(frame_output)
            .and_then(|(d, o)| checked_add(encoded_bytes, d).and_then(|w| checked_add(w, o)));
        match working {
            Some(w) if w <= self.options.limits.max_in_memory_source_bytes => {}
            _ => return Err(limit()),
        }
        let frames = crate::pixel::extract::extract_frames(file)
            .map_err(|e| pixels(Some(Cause::text(e.to_string()))))?;
        if frames.len() != expected.len() {
            return Err(pixels(None));
        }
        for (frame_index, wrapped) in frames.iter().enumerate() {
            let expect = &plan.frames[expected[frame_index]];
            let frame_error = |e: String| {
                export_error(
                    ErrorCode::Pixels,
                    expect.source_index,
                    frame_index as i64,
                    Some(S::UnsupportedPixels),
                    Some(Cause::text(e)),
                )
            };
            let (raw, metadata, big_endian) = if wrapped.is_encapsulated() {
                let decoded = wrapped
                    .encapsulated_frame()
                    .map_err(|e| frame_error(e.to_string()))?;
                (
                    decoded.data.clone(),
                    decoded.metadata.clone(),
                    decoded.big_endian,
                )
            } else {
                let native = wrapped
                    .native_frame()
                    .map_err(|e| frame_error(e.to_string()))?;
                (
                    native.data.clone(),
                    native.metadata.clone(),
                    native.big_endian,
                )
            };
            self.spool_image_frame(
                plan,
                spool,
                expected[frame_index],
                &raw,
                &metadata,
                big_endian,
                spool_bytes,
            )?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn spool_image_frame(
        &self,
        plan: &mut ExportPlan,
        spool: &mut fs::File,
        index: usize,
        raw: &[u8],
        metadata: &Metadata,
        big_endian: bool,
        spool_bytes: &mut u64,
    ) -> Result<(), Error> {
        let expect = plan.frames[index].clone();
        if !compatible_pixel_metadata(metadata, &expect.metadata) {
            return Err(bare(
                ErrorCode::Pixels,
                expect.source_index,
                expect.frame_index,
                S::UnsupportedPixels,
            ));
        }
        let (mut datatype, mut transform) = (plan.spool_datatype, expect.rescale);
        if plan.resampled {
            datatype = integer_datatype(&expect.metadata).map_or(0, |d| d.0);
            transform = LinearTransform::default();
        }
        let encoded = encode_stored_frame(raw, metadata, big_endian, transform, datatype).map_err(
            |sentinel| {
                export_error(
                    ErrorCode::Pixels,
                    expect.source_index,
                    expect.frame_index,
                    Some(S::UnsupportedPixels),
                    Some(Cause::sentinel(sentinel)),
                )
            },
        )?;
        self.append_spool(plan, spool, index, &encoded, spool_bytes)
    }
    fn spool_parametric(
        &self,
        plan: &mut ExportPlan,
        spool: &mut fs::File,
        file: &File,
        expected: &[usize],
        spool_bytes: &mut u64,
    ) -> Result<(), Error> {
        let first = plan.frames[expected[0]].clone();
        let working = parametric_payload_bytes(&file.dataset).and_then(|p| {
            parametric_working_set_bytes(
                p,
                i64::from(first.metadata.rows),
                i64::from(first.metadata.columns),
            )
        });
        match working {
            Some(w) if w <= self.options.limits.max_in_memory_source_bytes => {}
            _ => {
                return Err(bare(
                    ErrorCode::Limit,
                    first.source_index,
                    -1,
                    S::LimitExceeded,
                ));
            }
        }
        let pixels = |cause: Option<Cause>| {
            export_error(
                ErrorCode::Pixels,
                first.source_index,
                -1,
                Some(S::UnsupportedPixels),
                cause,
            )
        };
        let pm = parametricmap::read(file).map_err(|e| pixels(Some(Cause::text(e.to_string()))))?;
        let trim = |s: &str| s.trim_matches(char::is_whitespace).to_string();
        if pm.number_of_frames != expected.len() as i64
            || trim(&pm.units.value) != plan.unit_code
            || trim(&pm.units.scheme) != plan.unit_scheme
            || trim(&pm.quantity.value) != plan.quantity_code
            || trim(&pm.quantity.scheme) != plan.quantity_scheme
        {
            return Err(pixels(None));
        }
        let count = (pm.rows * pm.columns) as usize;
        let mut values = vec![0f64; count];
        let mut encoded = vec![0u8; count * 8];
        for (frame_index, &index) in expected.iter().enumerate() {
            let expect = plan.frames[index].clone();
            if !slice_geometry_close(&pm.frames[frame_index].geometry, &expect.geometry) {
                return Err(bare(
                    ErrorCode::Source,
                    expect.source_index,
                    frame_index as i64,
                    S::InvalidSource,
                ));
            }
            let counts = pm
                .decode_frame_into(frame_index as i64, &mut values, None)
                .map_err(|e| {
                    export_error(
                        ErrorCode::Pixels,
                        expect.source_index,
                        frame_index as i64,
                        Some(S::UnsupportedPixels),
                        Some(Cause::text(e.to_string())),
                    )
                })?;
            plan.unmeasured.padding += counts.padding;
            plan.unmeasured.nan += counts.nan;
            plan.unmeasured.infinite += counts.infinite;
            plan.unmeasured.outside_mapping += counts.outside_mapping;
            plan.unmeasured.mapping_overflow += counts.mapping_overflow;
            for (i, v) in values.iter().enumerate() {
                encoded[i * 8..i * 8 + 8].copy_from_slice(&v.to_bits().to_le_bytes());
            }
            self.append_spool(plan, spool, index, &encoded, spool_bytes)?;
        }
        Ok(())
    }
    fn append_spool(
        &self,
        plan: &mut ExportPlan,
        spool: &mut fs::File,
        index: usize,
        encoded: &[u8],
        total: &mut u64,
    ) -> Result<(), Error> {
        let frame = &plan.frames[index];
        let (s, f) = (frame.source_index, frame.frame_index);
        let next = match checked_add(*total, encoded.len() as u64) {
            Some(n) if n <= self.options.limits.max_spool_bytes => n,
            _ => return Err(bare(ErrorCode::Limit, s, f, S::LimitExceeded)),
        };
        let io = |e: std::io::Error| {
            export_error(
                ErrorCode::Write,
                s,
                f,
                Some(S::InvalidSource),
                Some(Cause::text(e.to_string())),
            )
        };
        let offset = spool.stream_position().map_err(io)?;
        spool.write_all(encoded).map_err(io)?;
        let frame = &mut plan.frames[index];
        frame.spool_offset = offset as i64;
        frame.spool_length = encoded.len() as i64;
        *total = next;
        Ok(())
    }
    fn write_from_spool(
        &self,
        plan: &ExportPlan,
        destination: &mut dyn Write,
        spool: &mut fs::File,
        voxels_only: bool,
    ) -> Result<Report, Error> {
        let mut affine_ras = crate::nifti::header::affine_lps2ras(plan.affine_lps);
        if plan.image_grid.used {
            let s = plan.spacing;
            affine_ras = GeometryAffine([
                s[0], 0.0, 0.0, 0.0, 0.0, s[1], 0.0, 0.0, 0.0, 0.0, s[2], 0.0, 0.0, 0.0, 0.0, 1.0,
            ]);
        }
        let header = build_header(&HeaderSpec {
            dimensions: plan.dimensions,
            datatype: plan.datatype,
            bit_pix: plan.bitpix,
            spacing: plan.spacing,
            voxel_offset: 352,
            slope: plan.header_slope,
            intercept: plan.header_intercept,
            time_offset: 0.0,
            temporal_unit: 0,
            affine_ras,
            no_spatial_transform: plan.image_grid.used,
        })
        .map_err(Error::Header)?;
        let mut counter = Counting {
            destination,
            count: 0,
        };
        let (mut compression, mut voxel_offset) = (self.options.compression, 352i64);
        if voxels_only {
            (compression, voxel_offset) = (COMPRESSION_NONE, 0);
        }
        let result = if compression == COMPRESSION_GZIP {
            let mut zipper = DeflateWriter::new(Container::Gzip);
            let mut output = Output::Gzip(&mut zipper);
            let r = self.write_body(plan, &mut output, spool, &header, voxels_only);
            let compressed = zipper.finish();
            r.and(write_exact(&mut counter, &compressed).map_err(write_error))
        } else {
            let mut output = Output::Plain(&mut counter);
            self.write_body(plan, &mut output, spool, &header, voxels_only)
        };
        result?;
        let mut warnings = plan.warnings.clone();
        let u = plan.unmeasured;
        for (count, warning) in [
            (u.padding, WARNING_PARAMETRIC_PADDING_AS_NAN),
            (u.nan, WARNING_PARAMETRIC_NAN_PRESERVED),
            (u.infinite, WARNING_PARAMETRIC_INFINITE_PRESERVED),
            (u.outside_mapping, WARNING_PARAMETRIC_OUTSIDE_MAPPING_AS_NAN),
            (
                u.mapping_overflow,
                WARNING_PARAMETRIC_MAPPING_OVERFLOW_AS_NAN,
            ),
        ] {
            if count > 0 {
                warnings.push(warning.into());
            }
        }
        let mut report = Report {
            dimensions: plan.dimensions,
            datatype: plan.datatype,
            bit_pix: plan.bitpix,
            voxel_offset,
            index_to_ras: affine_ras,
            scaling_policy: plan.scaling_policy,
            scaling_slope: plan.header_slope,
            scaling_intercept: plan.header_intercept,
            compression,
            input_reordered: plan.reordered,
            resampled: plan.resampled,
            interpolation: String::new(),
            warnings: (!warnings.is_empty()).then_some(warnings),
            bytes_written: counter.count,
            sidecar: Sidecar::default(),
            unmeasured_voxels: plan.unmeasured,
            single_slice: plan.single_slice,
            slice_spacing_source: plan.slice_spacing_source,
            no_spatial_transform: plan.image_grid.used,
            pixel_spacing_semantic: plan.image_grid.semantic,
        };
        if plan.resampled {
            report.interpolation = self.interpolation().1.into();
        }
        report.sidecar = report_sidecar(&report, plan);
        Ok(report)
    }
    fn interpolation(&self) -> (RegularGridInterpolation, &'static str) {
        if self.options.geometry == GEOMETRY_RESAMPLE_CUBIC_BSPLINE {
            (RegularGridInterpolation::CubicBSpline, "cubic-bspline")
        } else {
            (RegularGridInterpolation::Linear, "linear")
        }
    }
    fn write_body(
        &self,
        plan: &ExportPlan,
        output: &mut Output<'_, '_>,
        spool: &mut fs::File,
        header: &[u8],
        voxels_only: bool,
    ) -> Result<(), Error> {
        if !voxels_only {
            output.write_exact(header).map_err(write_error)?;
            output.write_exact(&[0, 0, 0, 0]).map_err(write_error)?;
        }
        if plan.resampled {
            self.write_resampled(plan, output, spool)
        } else {
            write_ordered_frames(plan, output, spool).map_err(write_error)
        }
    }
    fn write_resampled(
        &self,
        plan: &ExportPlan,
        output: &mut Output<'_, '_>,
        spool: &mut fs::File,
    ) -> Result<(), Error> {
        let dims = [plan.dimensions[0], plan.dimensions[1], plan.dimensions[2]];
        for (time_index, point) in plan.time_points.iter().enumerate() {
            let resample_error =
                |e: String| write_error(format!("resample time point {}: {e}", time_index + 1));
            let mut stack = Stack::default();
            for &index in &point.frames {
                let frame = &plan.frames[index];
                let mut pixels = vec![0u8; frame.spool_length as usize];
                spool
                    .seek(SeekFrom::Start(frame.spool_offset as u64))
                    .and_then(|_| spool.read_exact(&mut pixels))
                    .map_err(write_error)?;
                let mut metadata = frame.metadata.clone();
                metadata.bits_stored = metadata.bits_allocated;
                metadata.high_bit = metadata.bits_allocated - 1;
                stack
                    .frames
                    .push(Some(Arc::new(render_frame(frame, metadata, pixels))));
            }
            let volume = build_volume_with_tolerances(&stack, self.options.tolerances)
                .map_err(|e| resample_error(e.to_string()))?;
            let (interpolation, _) = self.interpolation();
            if interpolation != RegularGridInterpolation::Linear {
                let (values, grid) = volume
                    .resample_regular_grid_context(None, interpolation)
                    .map_err(|e| resample_error(e.to_string()))?;
                if grid.dimensions != dims
                    || !affine_close(&grid.voxel_to_patient_affine, &plan.affine_lps, 1e-5)
                {
                    return Err(resample_error(S::InvalidGeometry.text().into())
                        .with_sentinel(S::InvalidGeometry));
                }
                let cols = grid.dimensions[0] as usize;
                let mut row = vec![0u8; 4 * cols];
                for chunk in values.chunks(cols.max(1)) {
                    for (x, v) in chunk.iter().enumerate() {
                        row[4 * x..4 * x + 4].copy_from_slice(&v.to_bits().to_le_bytes());
                    }
                    output.write_exact(&row).map_err(write_error)?;
                }
                continue;
            }
            let snapshot = volume
                .acquire_snapshot_context(None)
                .map_err(|e| write_error(e.to_string()))?;
            let descriptor = snapshot.descriptor();
            if descriptor.dimensions != [dims[0] as u32, dims[1] as u32, dims[2] as u32]
                || !affine_close(&descriptor.index_to_patient_lps, &plan.affine_lps, 1e-5)
            {
                return Err(
                    write_error(S::InvalidGeometry.text()).with_sentinel(S::InvalidGeometry)
                );
            }
            let [nx, ny, nz] = descriptor.dimensions;
            let mut slice = vec![0u8; 4 * nx as usize * ny as usize];
            for z in 0..nz {
                for y in 0..ny {
                    for x in 0..nx {
                        let value = snapshot.modality_at(x, y, z).unwrap_or(0.0) as f32;
                        let at = 4 * (y as usize * nx as usize + x as usize);
                        slice[at..at + 4].copy_from_slice(&value.to_bits().to_le_bytes());
                    }
                }
                output.write_exact(&slice).map_err(write_error)?;
            }
        }
        Ok(())
    }
}
impl Error {
    fn with_sentinel(mut self, sentinel: S) -> Error {
        if let Error::Export(e) = &mut self
            && !e.sentinels.contains(&sentinel)
        {
            e.sentinels.push(sentinel);
        }
        self
    }
}
fn write_ordered_frames(
    plan: &ExportPlan,
    output: &mut Output<'_, '_>,
    spool: &mut fs::File,
) -> std::io::Result<()> {
    const BUFFER: usize = 128 << 10;
    let mut buffer = vec![0u8; BUFFER];
    for point in &plan.time_points {
        for &index in &point.frames {
            let frame = &plan.frames[index];
            spool.seek(SeekFrom::Start(frame.spool_offset as u64))?;
            let mut remaining = frame.spool_length as usize;
            while remaining > 0 {
                let chunk = remaining.min(BUFFER);
                spool.read_exact(&mut buffer[..chunk])?;
                output.write_exact(&buffer[..chunk])?;
                remaining -= chunk;
            }
        }
    }
    Ok(())
}
fn render_frame(frame: &FramePlan, metadata: Metadata, pixels: Vec<u8>) -> render::frame::Frame {
    let g = &frame.geometry;
    render::frame::Frame {
        frame_index: frame.frame_index,
        metadata,
        big_endian: false,
        pixel_bytes: Arc::new(pixels),
        rescale: render::Rescale {
            slope: frame.rescale.slope,
            intercept: frame.rescale.intercept,
        },
        image_position: vec![g.origin.x, g.origin.y, g.origin.z],
        image_orientation: vec![
            g.row_dir.x,
            g.row_dir.y,
            g.row_dir.z,
            g.col_dir.x,
            g.col_dir.y,
            g.col_dir.z,
        ],
        pixel_spacing: vec![g.row_spacing, g.col_spacing],
        temporal: frame.temporal.clone(),
        ..render::frame::Frame::default()
    }
}
fn report_sidecar(report: &Report, plan: &ExportPlan) -> Sidecar {
    let n = plan.time_points.len();
    let mut all_offsets = n > 1;
    let mut all_durations = n > 1;
    for point in &plan.time_points {
        all_offsets = all_offsets && point.timing.has_offset;
        all_durations = all_durations && point.timing.has_duration;
    }
    let offsets: Vec<f64> = plan
        .time_points
        .iter()
        .map(|p| duration_seconds(p.timing.offset))
        .collect();
    let durations: Vec<f64> = plan
        .time_points
        .iter()
        .map(|p| duration_seconds(p.timing.duration))
        .collect();
    Sidecar {
        dimensions: report.dimensions,
        datatype: datatype_name(report.datatype).into(),
        scaling_policy: scaling_policy_name(report.scaling_policy).into(),
        reordered: report.input_reordered,
        resampled: report.resampled,
        interpolation: report.interpolation.clone(),
        units: CodedUnits {
            code: plan.unit_code.clone(),
            scheme: plan.unit_scheme.clone(),
        },
        quantity: CodedUnits {
            code: plan.quantity_code.clone(),
            scheme: plan.quantity_scheme.clone(),
        },
        temporal_offsets_seconds: all_offsets.then_some(offsets),
        temporal_durations_seconds: all_durations.then_some(durations),
        warnings: report.warnings.clone(),
        slice_spacing_source: report.slice_spacing_source.into(),
        pixel_spacing_semantic: report.pixel_spacing_semantic.into(),
    }
}
fn same_identity(file: &File, series: &str, frame_of_reference: &str) -> bool {
    file.dataset
        .get_uid(TAG_SERIES_INSTANCE_UID)
        .unwrap_or_default()
        == series
        && file
            .dataset
            .get_uid(TAG_FRAME_OF_REFERENCE_UID)
            .unwrap_or_default()
            == frame_of_reference
}
fn compatible_pixel_metadata(actual: &Metadata, expected: &Metadata) -> bool {
    actual.rows == expected.rows
        && actual.columns == expected.columns
        && actual.samples_per_pixel == expected.samples_per_pixel
        && actual.bits_allocated == expected.bits_allocated
        && actual.bits_stored == expected.bits_stored
        && actual.high_bit == expected.high_bit
        && actual.pixel_representation == expected.pixel_representation
}
fn validate_source_frames(plan: &ExportPlan, file: &File, expected: &[usize]) -> Result<(), Error> {
    for (frame_index, &index) in expected.iter().enumerate() {
        let expect = &plan.frames[index];
        let frame_index = frame_index as i64;
        let geometry = plan_frame_geometry(
            &file.dataset,
            &file.dataset.frame_geometry_at(frame_index),
            frame_index,
            i64::from(expect.metadata.rows),
            i64::from(expect.metadata.columns),
            expect.grid.used,
        );
        let rescale = frame_linear_transform(&file.dataset, frame_index);
        let geometry_ok = geometry
            .as_ref()
            .is_some_and(|(g, _)| slice_geometry_close(g, &expect.geometry));
        if !geometry_ok || rescale != Some(expect.rescale) {
            let mut cause = Cause::default();
            if geometry.is_none() {
                cause.sentinels.push(S::InvalidGeometry);
            }
            if rescale.is_none() {
                cause.sentinels.push(S::UnsupportedScaling);
            }
            return Err(export_error(
                ErrorCode::Source,
                expect.source_index,
                frame_index,
                Some(S::InvalidSource),
                Some(cause),
            ));
        }
    }
    Ok(())
}
fn slice_geometry_close(a: &SliceGeometry, b: &SliceGeometry) -> bool {
    a.rows == b.rows
        && a.columns == b.columns
        && (a.row_spacing - b.row_spacing).abs() <= 1e-9
        && (a.col_spacing - b.col_spacing).abs() <= 1e-9
        && vec_close(a.origin, b.origin, 1e-9)
        && vec_close(a.row_dir, b.row_dir, 1e-9)
        && vec_close(a.col_dir, b.col_dir, 1e-9)
}
fn vec_close(a: Vec3, b: Vec3, tolerance: f64) -> bool {
    (a.x - b.x).abs() <= tolerance
        && (a.y - b.y).abs() <= tolerance
        && (a.z - b.z).abs() <= tolerance
}
fn encapsulated_resident_bytes(view: &PixelData<'_>) -> Option<u64> {
    let PixelData::Encapsulated {
        offset_table,
        fragments,
    } = view
    else {
        return Some(0);
    };
    let mut total = offset_table.len() as u64;
    for fragment in fragments.iter() {
        total = checked_add(total, fragment.len() as u64)?;
    }
    Some(total)
}
fn encode_stored_frame(
    raw: &[u8],
    metadata: &Metadata,
    big_endian: bool,
    transform: LinearTransform,
    datatype: i16,
) -> Result<Vec<u8>, S> {
    let reader = StoredSampleReader::new(
        i64::from(metadata.bits_allocated),
        i64::from(metadata.bits_stored),
        i64::from(metadata.high_bit),
        metadata.pixel_representation == 1,
        big_endian,
    )
    .map_err(|_| S::UnsupportedPixels)?;
    let width = reader.width();
    let count64 = checked_mul(u64::from(metadata.rows), u64::from(metadata.columns))
        .filter(|&c| c <= i64::MAX as u64 && width > 0)
        .ok_or(S::UnsupportedPixels)?;
    let count = count64 as usize;
    match checked_mul(count64, width as u64) {
        Some(want) if want == raw.len() as u64 => {}
        _ => return Err(S::UnsupportedPixels),
    }
    let out_width = datatype_bytes(datatype).ok_or(S::UnsupportedPixels)?;
    let mut out = vec![0u8; count * out_width];
    for index in 0..count {
        let signed = reader.value(&raw[index * width..]);
        let unsigned = signed as u64;
        let d = &mut out[index * out_width..];
        match datatype {
            DATATYPE_UINT8 => d[0] = unsigned as u8,
            DATATYPE_INT8 => d[0] = signed as i8 as u8,
            DATATYPE_UINT16 => d[..2].copy_from_slice(&(unsigned as u16).to_le_bytes()),
            DATATYPE_INT16 => d[..2].copy_from_slice(&(signed as i16).to_le_bytes()),
            DATATYPE_UINT32 => d[..4].copy_from_slice(&(unsigned as u32).to_le_bytes()),
            DATATYPE_INT32 => d[..4].copy_from_slice(&(signed as i32).to_le_bytes()),
            DATATYPE_FLOAT32 => {
                let value = signed as f64 * transform.slope + transform.intercept;
                let converted = value as f32;
                if !finite(value) || converted.is_nan() || converted.is_infinite() {
                    return Err(S::UnsupportedScaling);
                }
                d[..4].copy_from_slice(&converted.to_bits().to_le_bytes());
            }
            DATATYPE_FLOAT64 => {
                let value = signed as f64 * transform.slope + transform.intercept;
                if !finite(value) {
                    return Err(S::UnsupportedScaling);
                }
                d[..8].copy_from_slice(&value.to_bits().to_le_bytes());
            }
            _ => return Err(S::UnsupportedPixels),
        }
    }
    Ok(out)
}
