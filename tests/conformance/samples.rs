use crate::support::{self, Outcome, Plane, Report};
use dicom_rust::codec::jpegls;
use dicom_rust::limits::Limits;
use dicom_rust::object::{self, Tag, Value};
use dicom_rust::pixel::{frame_reader, pixeldata};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
const IMAGES: &[&str] = &[
    "CT1", "CT2", "MG1", "MR1", "MR2", "MR3", "MR4", "NM1", "RG1", "RG2", "RG3", "SC1", "US1",
    "VL1", "VL2", "VL3", "VL4", "VL5", "VL6", "XA1",
];
const LOSSLESS: &[(&str, &str)] = &[
    ("IMAGES", "J2KR"),
    ("IMAGES", "JLSL"),
    ("IMAGES", "JPLL"),
    ("rle/IMAGES", "RLE"),
];
const SEQUENTIAL_SCAN: &str = "the scan header of a sequential DCT frame gives the end of     spectral selection as 0 where ITU-T T.81 B.2.3 requires 63: refused by default, decoded     with the opt-in recovery `jpeg_sequential_scan_quirks` (next line)";
const LOSSY: &[(&str, &str, &[&str], i64)] = &[
    ("JLSN", "dcmdjpls", &[], 0),
    ("JPLY", "dcmdjpeg", &[], 1),
    ("J2KI", "gdcmconv", &["--raw"], 2),
];
const LEAST_PSNR: f64 = 25.0;
struct Decoded {
    planes: Vec<Plane>,
    written: Vec<Plane>,
    bytes: Vec<u8>,
    bits: u32,
}
fn decode(path: &Path) -> Result<Decoded, String> {
    let file = object::open_file(path).map_err(|(_, e)| e.to_string())?;
    let (m, frames, _) =
        frame_reader::decode_file_frames_view(&file, None).map_err(|e| e.to_string())?;
    if frames.len() != 1 {
        return Err(format!("{} frames", frames.len()));
    }
    let (width, height, channels) = (
        usize::from(m.columns),
        usize::from(m.rows),
        usize::from(m.samples_per_pixel),
    );
    let bytes = usize::from(m.bits_allocated / 8);
    let signed = m.pixel_representation == 1;
    let bits = u32::from(m.bits_stored);
    let frame = &frames[0];
    let planes = if channels > 1 && m.planar_configuration == 1 {
        frame
            .chunks_exact(width * height * bytes)
            .flat_map(|plane| {
                support::planes_from_interleaved(plane, width, height, 1, bytes, signed, bits)
            })
            .collect()
    } else {
        support::planes_from_interleaved(frame, width, height, channels, bytes, signed, bits)
    };
    let wide = if signed {
        bits
    } else {
        u32::from(m.bits_allocated)
    };
    let written = if channels > 1 && m.planar_configuration == 1 {
        planes.clone()
    } else {
        support::planes_from_interleaved(frame, width, height, channels, bytes, signed, wide)
    };
    Ok(Decoded {
        planes,
        written,
        bytes: frames.into_iter().next().unwrap_or_default(),
        bits,
    })
}
fn decode_recovered(path: &Path, reference: &Decoded) -> Result<Decoded, String> {
    let file = object::open_file(path).map_err(|(_, e)| e.to_string())?;
    let pixel = pixeldata::extract_view(&file.dataset).map_err(|e| e.to_string())?;
    let recovery = pixeldata::CodecRecovery {
        jpeg_sequential_scan_quirks: true,
        ..Default::default()
    };
    let frames = pixeldata::decode_frames_with_limits(
        file.transfer_syntax.uid,
        &pixel,
        &file.dataset,
        &recovery,
        &Limits::DEFAULT,
    )
    .map_err(|e| e.to_string())?;
    let frame = frames.data.into_iter().next().ok_or("no frame")?;
    let samples = (frames.rows * frames.columns).max(1);
    let bytes = frame.len() / samples;
    let (width, height) = (frames.columns, frames.rows);
    let read =
        |bits| support::planes_from_interleaved(&frame, width, height, 1, bytes, false, bits);
    let (planes, written) = (read(reference.bits), read(8 * bytes as u32));
    Ok(Decoded {
        planes,
        written,
        bytes: frame,
        bits: reference.bits,
    })
}
fn directory(dir: &Path) -> (usize, Vec<PathBuf>) {
    let dicomdir = object::open_file(dir.join("DICOMDIR"))
        .unwrap_or_else(|(_, e)| panic!("{}: DICOMDIR: {e}", dir.display()));
    let records = dicomdir
        .dataset
        .get_sequence(Tag::new(0x0004, 0x1220))
        .expect("Directory Record Sequence");
    let files = records
        .iter()
        .filter_map(|r| r.get_strings(Tag::new(0x0004, 0x1500)))
        .map(|parts| parts.iter().fold(dir.to_path_buf(), |p, part| p.join(part)))
        .collect();
    (records.len(), files)
}
fn psnr(got: &[Plane], want: &[Plane], bits: u32) -> Result<(i64, f64), String> {
    let (mut peak, mut sum) = (0i64, 0f64);
    for (g, w) in got.iter().zip(want) {
        let (p, m) = support::errors(g, w)?;
        peak = peak.max(p);
        sum += m;
    }
    let mse = sum / got.len().max(1) as f64;
    let range = ((1u64 << bits) - 1) as f64;
    Ok((
        peak,
        if mse == 0.0 {
            f64::INFINITY
        } else {
            10.0 * (range * range / mse).log10()
        },
    ))
}
fn near(path: &Path) -> Result<i64, String> {
    let file = object::open_file(path).map_err(|(_, e)| e.to_string())?;
    let Some(Value::Fragments { fragments, .. }) =
        file.dataset.get(Tag::PIXEL_DATA).map(|e| &e.value)
    else {
        return Err("Pixel Data is not encapsulated".into());
    };
    let mut stream: Vec<u8> = fragments.concat();
    while !stream.is_empty() && !stream.ends_with(&[0xFF, 0xD9]) {
        stream.pop();
    }
    jpegls::inspect_codestream(&stream, 0, Default::default())
        .map(|info| info.near)
        .map_err(|e| e.to_string())
}
fn program(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let path = match std::env::var_os("DCMTK_BIN") {
        Some(dir) if name.starts_with("dcm") => Path::new(&dir).join(exe),
        _ => PathBuf::from(exe),
    };
    Command::new(&path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
        .then_some(path)
}
fn independent(
    program: &Path,
    args: &[&str],
    input: &Path,
    scratch: &Path,
) -> Result<Decoded, String> {
    let output = scratch.join("independent.dcm");
    let _ = std::fs::remove_file(&output);
    let out = Command::new(program)
        .args(args)
        .arg(input)
        .arg(&output)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() || !output.is_file() {
        return Err(format!(
            "{} failed: {}",
            program.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    decode(&output)
}
#[test]
#[ignore = "needs the conformance cache: run scripts/ci.sh conformance"]
fn nema_wg04_samples() {
    let Some(dir) = support::set_dir("nema-wg04") else {
        return;
    };
    let images = dir.join("IMAGES");
    let scratch = std::env::temp_dir().join(format!(
        "dicom-rust-conformance-{}-samples",
        std::process::id()
    ));
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let mut report = Report::new("nema-wg04 (DICOM compression samples)");
    let mut referenced = Vec::new();
    for (label, at) in [("DICOMDIR", dir.clone()), ("rle/DICOMDIR", dir.join("rle"))] {
        let (records, files) = directory(&at);
        let missing = files.iter().filter(|p| !p.is_file()).count();
        report.case(
            label,
            if missing == 0 && !files.is_empty() {
                Outcome::Pass(format!(
                    "{records} records, {} files, all present",
                    files.len()
                ))
            } else {
                Outcome::Differs(format!("{missing} of {} files missing", files.len()))
            },
            None,
        );
        referenced.extend(files);
    }
    let tools: Vec<Option<PathBuf>> = LOSSY.iter().map(|l| program(l.1)).collect();
    for (lossy, tool) in LOSSY.iter().zip(&tools) {
        if tool.is_none() {
            report.note(format!(
                "{} is not installed: {} is compared with the reference only",
                lossy.1, lossy.0
            ));
        }
    }
    let mut files = 0;
    for image in IMAGES {
        let name = format!("{image}_UNC");
        let reference = match decode(&images.join("REF").join(&name)) {
            Ok(r) => r,
            Err(e) => {
                report.case(&name, Outcome::Refused(e), None);
                continue;
            }
        };
        files += 1;
        report.case(
            &name,
            Outcome::Pass(format!(
                "{}x{}x{}, {} bits",
                reference.planes[0].width,
                reference.planes[0].height,
                reference.planes.len(),
                reference.bits
            )),
            None,
        );
        for (folder, kind) in LOSSLESS {
            let name = format!("{image}_{kind}");
            let path = dir.join(folder).join(kind).join(&name);
            if !path.is_file() {
                continue;
            }
            files += 1;
            let outcome = match decode(&path) {
                Err(e) => Outcome::Refused(e),
                Ok(got) if got.bytes == reference.bytes => Outcome::Pass("exact".into()),
                Ok(got) if got.planes == reference.planes => {
                    Outcome::Pass("exact in the stored bits".into())
                }
                Ok(got) => match psnr(&got.planes, &reference.planes, reference.bits) {
                    Ok((peak, _)) => Outcome::Differs(format!("peak {peak}")),
                    Err(e) => Outcome::Differs(e),
                },
            };
            report.case(&name, outcome, None);
        }
        for ((kind, _, args, allowed), tool) in LOSSY.iter().zip(&tools) {
            let name = format!("{image}_{kind}");
            let path = images.join(kind).join(&name);
            if !path.is_file() {
                continue;
            }
            files += 1;
            let (name, got) = match decode(&path) {
                Ok(got) => (name, got),
                Err(e) if *kind == "JPLY" => {
                    report.case(&name, Outcome::Refused(e), Some(SEQUENTIAL_SCAN));
                    let name = format!("{name} with the recovery");
                    match decode_recovered(&path, &reference) {
                        Ok(got) => (name, got),
                        Err(e) => {
                            report.case(&name, Outcome::Refused(e), None);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    report.case(&name, Outcome::Refused(e), None);
                    continue;
                }
            };
            let mut ok = true;
            let mut detail = Vec::new();
            match psnr(&got.planes, &reference.planes, reference.bits) {
                Err(e) => {
                    ok = false;
                    detail.push(e);
                }
                Ok((peak, db)) => {
                    if *kind == "JLSN" {
                        match near(&path) {
                            Ok(near) => {
                                ok &= peak <= near;
                                detail.push(format!("peak {peak}, NEAR {near}"));
                            }
                            Err(e) => {
                                ok = false;
                                detail.push(e);
                            }
                        }
                    } else {
                        ok &= db >= LEAST_PSNR;
                        detail.push(format!("{db:.1} dB to the reference"));
                    }
                }
            }
            if let Some(tool) = tool {
                match independent(tool, args, &path, &scratch) {
                    Err(e) => {
                        ok = false;
                        detail.push(e);
                    }
                    Ok(other) => match psnr(&got.written, &other.written, reference.bits) {
                        Err(e) => {
                            ok = false;
                            detail.push(e);
                        }
                        Ok((peak, _)) => {
                            ok &= peak <= *allowed;
                            detail.push(format!(
                                "peak {peak}/{allowed} to {}",
                                tool.file_stem().unwrap_or_default().to_string_lossy()
                            ));
                        }
                    },
                }
            }
            let detail = detail.join(", ");
            report.case(
                &name,
                if ok {
                    Outcome::Pass(detail)
                } else {
                    Outcome::Differs(detail)
                },
                None,
            );
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    let listed = referenced.len();
    report.case(
        "files",
        if files == listed {
            Outcome::Pass(format!("{files} read, as many as the DICOMDIR lists"))
        } else {
            Outcome::Differs(format!("{files} read, the DICOMDIR lists {listed}"))
        },
        None,
    );
    report.finish();
}
