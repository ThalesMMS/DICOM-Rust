#[allow(unused_imports)]
use super::support::*;
use dicom_rust::dicomdir::{self, ErrorKind, Reference};
use dicom_rust::object;
use dicom_rust::textfmt::quote;
use std::io::Read;
fn quote_list(values: &[String]) -> String {
    let parts: Vec<String> = values.iter().map(|v| quote(v)).collect();
    format!("[{}]", parts.join(" "))
}
fn classes(err: &dicomdir::Error) -> String {
    if err.is(ErrorKind::InvalidFileId) {
        "fileid".to_string()
    } else {
        String::new()
    }
}
fn render_id(r: Result<dicomdir::FileId, dicomdir::Error>) -> String {
    match r {
        Ok(id) => format!(
            "ok {} n={} {}",
            quote(&id.to_string()),
            id.0.len(),
            quote_list(&id.0)
        ),
        Err(e) => format!("err {} is={} nil=true", quote(&e.to_string()), classes(&e)),
    }
}
pub fn new_id(components: String) -> Result<String, String> {
    let parts: Vec<String> = if components == "<nil>" {
        Vec::new()
    } else {
        components.split('|').map(str::to_string).collect()
    };
    Ok(render_id(dicomdir::new_file_id(&parts)))
}
pub fn parse_id(value: String) -> Result<String, String> {
    Ok(render_id(dicomdir::parse_file_id(&value)))
}
pub fn refs(data: Vec<i64>) -> Result<String, String> {
    if data.len() > 32 << 20 {
        return Ok("too large".to_string());
    }
    let bytes: Vec<u8> = data.iter().map(|&b| b as u8).collect();
    let Ok(file) = object::read_file(&bytes) else {
        return Ok("readerr".to_string());
    };
    let mut b = String::new();
    for r in dicomdir::references(&file.dataset) {
        b.push_str(&format!(
            "ref {} {}\n",
            quote_list(&r.file_id),
            quote(&r.typ)
        ));
    }
    Ok(b)
}
struct Tmp {
    dir: String,
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Tmp {
    fn norm(&self, text: &str) -> String {
        let mut text = text.replace(&self.dir, "<dir>");
        if text.starts_with("dicomdir: open referenced file ")
            && let Some(i) = text.find("\": ")
        {
            text = format!("{}<os>", &text[..i + 3]);
        }
        text.replace('\\', "/")
    }
    fn path(&self, rel: &str) -> String {
        let rel = rel.replace('/', std::path::MAIN_SEPARATOR_STR);
        format!("{}{}{}", self.dir, std::path::MAIN_SEPARATOR, rel)
    }
}
fn temp_dir(prefix: &str) -> Result<String, String> {
    let base = std::env::temp_dir();
    for i in 0..1000u32 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let p = base.join(format!("{prefix}{}{nanos}{i}", std::process::id()));
        if std::fs::create_dir(&p).is_ok() {
            let abs = std::path::absolute(&p).map_err(|e| e.to_string())?;
            return Ok(abs.to_string_lossy().into_owned());
        }
    }
    Err("harness: no temp dir".to_string())
}
fn build(tree: &str) -> Result<Tmp, String> {
    let t = Tmp {
        dir: temp_dir("ddref")?,
    };
    let mut total = 0usize;
    for entry in tree.split(';') {
        let (name, value) = match entry.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (entry, None),
        };
        if name.is_empty() {
            continue;
        }
        let p = t.path(name);
        let Some(value) = value else {
            if name.ends_with('/') {
                let _ = std::fs::create_dir_all(&p);
            }
            continue;
        };
        let Ok(data) = hex::decode(value) else {
            continue;
        };
        total += data.len();
        if total > 64 << 10 {
            return Err("harness: input too large".to_string());
        }
        if let Some(parent) = std::path::Path::new(&p).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&p, data);
    }
    Ok(t)
}
pub fn resolve_in(
    tree: String,
    root_rel: String,
    file_id: String,
    typ: String,
) -> Result<String, String> {
    let t = build(&tree)?;
    let root = if root_rel.is_empty() {
        t.dir.clone()
    } else {
        t.path(&root_rel)
    };
    let values: Vec<String> = if file_id == "<nil>" {
        Vec::new()
    } else {
        file_id.split('|').map(str::to_string).collect()
    };
    let reference = Reference {
        file_id: values,
        typ,
    };
    let mut b = format!(
        "resolve {}\n",
        quote(&t.norm(&dicomdir::resolve(&root, &reference)))
    );
    match dicomdir::open_referenced_file(&root, &reference) {
        Err(e) => b.push_str(&format!("open err {}\n", quote(&t.norm(&e.to_string())))),
        Ok((file, path)) => {
            let mut data = Vec::new();
            let _ = file.take(1 << 20).read_to_end(&mut data);
            b.push_str(&format!(
                "open ok {} len={}\n",
                quote(&t.norm(&path)),
                data.len()
            ));
        }
    }
    Ok(b)
}
pub fn paths_in(tree: String, rel: String) -> Result<String, String> {
    let t = build(&tree)?;
    match dicomdir::referenced_paths(&t.path(&rel)) {
        Err(e) => Ok(format!("err {} nil=true\n", quote(&t.norm(&e.to_string())))),
        Ok(paths) => {
            let mut b = String::new();
            for p in paths {
                b.push_str(&format!("path {}\n", quote(&t.norm(&p))));
            }
            Ok(b)
        }
    }
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "NewID" => {
            let out = new_id(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ParseID" => {
            let out = parse_id(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Refs" => {
            let out = refs(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ResolveIn" => {
            let out = resolve_in(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?, arg(args, 3)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "PathsIn" => {
            let out = paths_in(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
