#![forbid(unsafe_code)]
use crate::dicomjson::jsonwire::{Decoder, as_raw_array, as_raw_map, as_string, kind};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
pub const MANIFEST_NAME: &str = "THIRD_PARTY_PROVENANCE.json";
pub const NOTICES_NAME: &str = "THIRD_PARTY_NOTICES.md";
pub const UPSTREAM_DATA_DIR: &str = "testdata/upstream";
pub const DISTRIBUTION_TEST_DATA: &str = "test-data";
pub const DISTRIBUTION_EMBEDDED: &str = "embedded";
pub const DISTRIBUTION_OPTIONAL_MODULE: &str = "optional-module";
pub const REFERENCE_CHECKOUTS: [(&str, &str); 4] = [
    ("https://github.com/DCMTK/dcmtk", "dcmtk"),
    ("https://github.com/malaterre/GDCM", "GDCM"),
    ("https://github.com/Kitware/VTK", "VTK"),
    ("https://github.com/InsightSoftwareConsortium/ITK", "ITK"),
];
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub schema: i64,
    pub upstreams: HashMap<String, Upstream>,
    pub artifacts: Vec<Artifact>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Upstream {
    pub project: String,
    pub repository: String,
    pub commit: String,
    pub license: String,
    pub license_file: String,
    pub license_sha256: String,
    pub license_source: String,
    pub license_url: String,
    pub notice_heading: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Artifact {
    pub path: String,
    pub upstream: String,
    pub sha256: String,
    pub sha256_policy: String,
    pub attribution: String,
    pub distribution: String,
    pub transformation: String,
    pub sources: Vec<Source>,
    pub generated_by: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    pub file: String,
    pub lines: String,
    pub sha256: String,
    pub url: String,
    pub unverified_reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    Read(String),
    Decode(String),
}
impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Read(t) | LoadError::Decode(t) => f.write_str(t),
        }
    }
}
impl std::error::Error for LoadError {}
fn os_error(op: &str, path: &Path, e: &std::io::Error) -> String {
    let text = e.to_string();
    let text = match text.rfind(" (os error ") {
        Some(i) => &text[..i],
        None => &text,
    };
    format!("{op} {}: {text}", path.display())
}
type Fields<'a> = Vec<(String, &'a [u8])>;
fn take<'a>(fields: &Fields<'a>, names: &[&str]) -> Result<Vec<Option<&'a [u8]>>, String> {
    let mut out = vec![None; names.len()];
    for (key, value) in fields {
        let i = names
            .iter()
            .position(|n| n == key)
            .or_else(|| names.iter().position(|n| n.eq_ignore_ascii_case(key)))
            .ok_or_else(|| format!("json: unknown field {}", crate::textfmt::quote(key)))?;
        out[i] = Some(*value);
    }
    Ok(out)
}
fn text(v: Option<&[u8]>) -> Result<String, String> {
    v.map_or(Ok(String::new()), |raw| {
        as_string(raw).map_err(|e| e.to_string())
    })
}
fn object(raw: &[u8]) -> Result<Fields<'_>, String> {
    as_raw_map(raw).map_err(|e| e.to_string())
}
fn source(raw: &[u8]) -> Result<Source, String> {
    let f = take(
        &object(raw)?,
        &["file", "lines", "sha256", "url", "unverified_reason"],
    )?;
    Ok(Source {
        file: text(f[0])?,
        lines: text(f[1])?,
        sha256: text(f[2])?,
        url: text(f[3])?,
        unverified_reason: text(f[4])?,
    })
}
fn artifact(raw: &[u8]) -> Result<Artifact, String> {
    let names = [
        "path",
        "upstream",
        "sha256",
        "sha256_policy",
        "attribution",
        "distribution",
        "transformation",
        "sources",
        "generated_by",
    ];
    let f = take(&object(raw)?, &names)?;
    let mut sources = Vec::new();
    if let Some(list) = f[7] {
        for item in as_raw_array(list).map_err(|e| e.to_string())? {
            sources.push(source(item)?);
        }
    }
    Ok(Artifact {
        path: text(f[0])?,
        upstream: text(f[1])?,
        sha256: text(f[2])?,
        sha256_policy: text(f[3])?,
        attribution: text(f[4])?,
        distribution: text(f[5])?,
        transformation: text(f[6])?,
        sources,
        generated_by: text(f[8])?,
    })
}
fn upstream(raw: &[u8]) -> Result<Upstream, String> {
    let names = [
        "project",
        "repository",
        "commit",
        "license",
        "license_file",
        "license_sha256",
        "license_source",
        "license_url",
        "notice_heading",
    ];
    let f = take(&object(raw)?, &names)?;
    Ok(Upstream {
        project: text(f[0])?,
        repository: text(f[1])?,
        commit: text(f[2])?,
        license: text(f[3])?,
        license_file: text(f[4])?,
        license_sha256: text(f[5])?,
        license_source: text(f[6])?,
        license_url: text(f[7])?,
        notice_heading: text(f[8])?,
    })
}
fn integer(raw: &[u8]) -> Result<i64, String> {
    if raw == b"null" {
        return Ok(0);
    }
    let s = std::str::from_utf8(raw).map_err(|e| e.to_string())?;
    if kind(raw) != "number" || s.contains(['.', 'e', 'E']) {
        return Err(format!("json: cannot unmarshal {} into int", kind(raw)));
    }
    s.parse::<i64>().map_err(|e| e.to_string())
}
fn decode(data: &[u8]) -> Result<Manifest, String> {
    let raw = Decoder::new(data).decode_raw().map_err(|e| e.to_string())?;
    let f = take(&object(raw)?, &["schema", "upstreams", "artifacts"])?;
    let mut m = Manifest {
        schema: f[0].map_or(Ok(0), integer)?,
        ..Manifest::default()
    };
    if let Some(ups) = f[1] {
        for (id, value) in object(ups)? {
            m.upstreams.insert(id, upstream(value)?);
        }
    }
    if let Some(list) = f[2] {
        for item in as_raw_array(list).map_err(|e| e.to_string())? {
            m.artifacts.push(artifact(item)?);
        }
    }
    Ok(m)
}
pub fn load(file: &Path) -> Result<Manifest, LoadError> {
    let data = std::fs::read(file).map_err(|e| LoadError::Read(os_error("open", file, &e)))?;
    decode(&data).map_err(|e| LoadError::Decode(format!("{}: {e}", file.display())))
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub reference_root: Option<PathBuf>,
}
fn hex_of(len: usize, s: &str) -> bool {
    s.len() == len
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn sum(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn module_path(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}
fn read_module_file(root: &Path, rel: &str) -> Result<Vec<u8>, String> {
    let p = module_path(root, rel);
    std::fs::read(&p).map_err(|e| os_error("open", &p, &e))
}
fn is_clean(p: &str) -> bool {
    if p == "/" || p == "." {
        return true;
    }
    let rel = p.strip_prefix('/').unwrap_or(p);
    if rel.is_empty() || rel.ends_with('/') {
        return false;
    }
    let mut leading = !p.starts_with('/');
    for part in rel.split('/') {
        match part {
            "" | "." => return false,
            ".." if leading => {}
            ".." => return false,
            _ => leading = false,
        }
    }
    true
}
fn atoi(s: &str) -> i64 {
    s.parse::<i64>().unwrap_or(i64::MAX)
}
fn line_range(s: &str) -> Option<(&str, &str)> {
    let (a, b) = s.split_once('-')?;
    let ok = |x: &str| {
        x.bytes().next().is_some_and(|c| (b'1'..=b'9').contains(&c))
            && x.bytes().all(|c| c.is_ascii_digit())
    };
    (ok(a) && ok(b)).then_some((a, b))
}
pub fn verify(root: &Path, m: &Manifest, opt: &Options) -> Vec<String> {
    let mut errs: Vec<String> = Vec::new();
    let q = crate::textfmt::quote;
    if m.schema != 1 {
        errs.push(format!("manifest schema {}, want 1", m.schema));
    }
    let notices_path = root.join(NOTICES_NAME);
    let notices = std::fs::read(&notices_path).unwrap_or_else(|e| {
        errs.push(format!(
            "{NOTICES_NAME}: {}",
            os_error("open", &notices_path, &e)
        ));
        Vec::new()
    });
    let notices = String::from_utf8_lossy(&notices).replace("\r\n", "\n");
    let headings: HashSet<&str> = notices
        .split('\n')
        .filter(|l| l.starts_with('#'))
        .map(|l| l.trim())
        .collect();
    let mut license_files = HashSet::new();
    for (id, u) in &m.upstreams {
        if u.project.is_empty() || u.license.is_empty() || !u.repository.starts_with("https://") {
            errs.push(format!(
                "upstream {id}: project, license and https repository are required"
            ));
        }
        if !hex_of(40, &u.commit) {
            errs.push(format!(
                "upstream {id}: commit {} is not a full 40-hex SHA-1",
                q(&u.commit)
            ));
        }
        if !u.license_file.is_empty() {
            license_files.insert(u.license_file.clone());
            match read_module_file(root, &u.license_file) {
                Err(e) => errs.push(format!("upstream {id}: license file: {e}")),
                Ok(data) if data.trim_ascii().is_empty() => errs.push(format!(
                    "upstream {id}: license file {} is empty",
                    u.license_file
                )),
                Ok(_) if !hex_of(64, &u.license_sha256) => errs.push(format!(
                    "upstream {id}: license_sha256 is required with license_file"
                )),
                Ok(data) => {
                    let got = sum(&data);
                    if got != u.license_sha256 {
                        errs.push(format!(
                            "upstream {id}: license file {} SHA-256 {got}, manifest {}",
                            u.license_file, u.license_sha256
                        ));
                    }
                }
            }
        } else if !u.license_url.is_empty() {
            if !u.license_url.starts_with("https://") {
                errs.push(format!("upstream {id}: license_url must be https"));
            }
        } else {
            errs.push(format!(
                "upstream {id}: license_file or license_url is required"
            ));
        }
        if u.notice_heading.is_empty() || !headings.contains(u.notice_heading.as_str()) {
            errs.push(format!(
                "upstream {id}: notice heading {} missing from {NOTICES_NAME}",
                q(&u.notice_heading)
            ));
        }
    }
    let mut listed = HashSet::new();
    let mut used = HashSet::new();
    for a in &m.artifacts {
        verify_artifact(root, m, opt, a, &mut listed, &mut used, &mut errs);
    }
    for (id, u) in &m.upstreams {
        if !used.contains(id.as_str())
            && !u.license_file.starts_with(&format!("{UPSTREAM_DATA_DIR}/"))
        {
            errs.push(format!("upstream {id}: no artifact references it"));
        }
    }
    let dir = module_path(root, UPSTREAM_DATA_DIR);
    if std::fs::metadata(&dir).is_ok() {
        walk(root, &dir, &listed, &license_files, &mut errs);
    }
    errs.sort();
    errs
}
#[allow(clippy::too_many_arguments)]
fn verify_artifact<'m>(
    root: &Path,
    m: &'m Manifest,
    opt: &Options,
    a: &'m Artifact,
    listed: &mut HashSet<String>,
    used: &mut HashSet<&'m str>,
    errs: &mut Vec<String>,
) {
    let q = crate::textfmt::quote;
    let w = format!("artifact {}", a.path);
    if !is_clean(&a.path) || a.path.starts_with("../") || a.path.starts_with('/') {
        errs.push(format!("{w}: path must be clean and module-relative"));
        return;
    }
    if !listed.insert(a.path.clone()) {
        errs.push(format!("{w}: listed twice"));
    }
    let upstream = m.upstreams.get(&a.upstream);
    if upstream.is_none() {
        errs.push(format!("{w}: unknown upstream {}", q(&a.upstream)));
    }
    used.insert(&a.upstream);
    let in_testdata = a.path.starts_with("testdata/") || a.path.contains("/testdata/");
    match a.distribution.as_str() {
        DISTRIBUTION_TEST_DATA if !in_testdata => errs.push(format!(
            "{w}: test-data artifacts must live under a testdata directory"
        )),
        DISTRIBUTION_EMBEDDED | DISTRIBUTION_OPTIONAL_MODULE if in_testdata => errs.push(format!(
            "{w}: {} artifact must not live under testdata",
            a.distribution
        )),
        DISTRIBUTION_TEST_DATA | DISTRIBUTION_EMBEDDED | DISTRIBUTION_OPTIONAL_MODULE => {}
        other => errs.push(format!(
            "{w}: distribution {} is not test-data, embedded or optional-module",
            q(other)
        )),
    }
    if a.transformation.trim().is_empty() {
        errs.push(format!("{w}: transformation is required"));
    }
    match read_module_file(root, &a.path) {
        Err(e) => errs.push(format!("{w}: {e}")),
        Ok(data) => match a.sha256_policy.as_str() {
            "" | "pinned" if !hex_of(64, &a.sha256) => {
                errs.push(format!("{w}: sha256 is required"))
            }
            "" | "pinned" => {
                let got = sum(&data);
                if got != a.sha256 {
                    errs.push(format!(
                        "{w}: SHA-256 {got}, manifest {} (regenerate or update the manifest)",
                        a.sha256
                    ));
                }
            }
            "modified" => {
                let present = !a.attribution.is_empty()
                    && data
                        .windows(a.attribution.len())
                        .any(|win| win == a.attribution.as_bytes());
                if !present {
                    errs.push(format!(
                        "{w}: modified source must contain attribution {}",
                        q(&a.attribution)
                    ));
                }
            }
            other => errs.push(format!(
                "{w}: sha256_policy {} is not pinned or modified",
                q(other)
            )),
        },
    }
    if a.sources.is_empty() {
        errs.push(format!("{w}: at least one source is required"));
    }
    for s in &a.sources {
        if s.file.is_empty() {
            errs.push(format!("{w}: source file is required"));
        }
        if s.sha256.is_empty() && s.unverified_reason.is_empty() {
            errs.push(format!(
                "{w}: source {} needs sha256 or unverified_reason",
                s.file
            ));
        }
        if !s.sha256.is_empty() && !hex_of(64, &s.sha256) {
            errs.push(format!("{w}: source {} sha256 is malformed", s.file));
        }
        if !s.lines.is_empty() {
            match line_range(&s.lines) {
                None => errs.push(format!(
                    "{w}: source {} lines {} is not first-last",
                    s.file,
                    q(&s.lines)
                )),
                Some((first, last)) if atoi(first) > atoi(last) => errs.push(format!(
                    "{w}: source {} lines {} are reversed",
                    s.file,
                    q(&s.lines)
                )),
                Some(_) => {}
            }
        }
        if let (Some(reference), Some(u)) = (&opt.reference_root, upstream)
            && !s.sha256.is_empty()
            && let Err(e) = verify_source(reference, u, s)
        {
            errs.push(format!("{w}: {e}"));
        }
    }
}
fn walk(
    root: &Path,
    dir: &Path,
    listed: &HashSet<String>,
    licenses: &HashSet<String>,
    errs: &mut Vec<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            errs.push(format!("{}: {}", dir.display(), os_error("open", dir, &e)));
            return;
        }
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            walk(root, &p, listed, licenses, errs);
            continue;
        }
        let rel = p
            .strip_prefix(root)
            .map(|r| {
                r.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();
        let base = rel.rsplit('/').next().unwrap_or("");
        if !listed.contains(&rel) && !licenses.contains(&rel) && base != "README.md" {
            errs.push(format!(
                "{rel}: file has no provenance entry in {MANIFEST_NAME}"
            ));
        }
    }
}
fn verify_source(reference_root: &Path, u: &Upstream, s: &Source) -> Result<(), String> {
    let Some((_, name)) = REFERENCE_CHECKOUTS
        .iter()
        .find(|(repo, _)| *repo == u.repository)
    else {
        return Ok(());
    };
    let dir = reference_root.join(name);
    if let Err(e) = std::fs::metadata(&dir) {
        let op = if cfg!(windows) {
            "GetFileAttributesEx"
        } else {
            "stat"
        };
        return Err(format!(
            "reference checkout for {}: {}",
            u.repository,
            os_error(op, &dir, &e)
        ));
    }
    let commit = if u.commit.len() > 8 {
        &u.commit[..8]
    } else {
        &u.commit
    };
    let out = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .arg("show")
        .arg(format!("{}:{}", u.commit, s.file))
        .output()
        .map_err(|e| format!("source {} at {commit}: {e}", s.file))?;
    if !out.status.success() {
        return Err(format!("source {} at {commit}: {}", s.file, out.status));
    }
    let got = sum(&out.stdout);
    if got != s.sha256 {
        return Err(format!(
            "source {} at {commit}: SHA-256 {got}, manifest {}",
            s.file, s.sha256
        ));
    }
    Ok(())
}
