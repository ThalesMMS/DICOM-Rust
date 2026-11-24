use super::{ErrorKind, Mesh, MeshError, cross3, dot3, sub3};
use crate::fmath;
use crate::numtext;
use crate::textfmt;
use std::collections::HashMap;
const INVALID_MODEL: &str = "mesh: invalid model input";
const MODEL_LIMIT: &str = "mesh: model read limit exceeded";
const UNSUPPORTED_MODEL: &str = "mesh: unsupported model feature";
fn model_error(kind: ErrorKind, base: &str, detail: &str) -> MeshError {
    MeshError {
        kind,
        message: if detail.is_empty() {
            base.to_string()
        } else {
            format!("{base}: {detail}")
        },
        budget: None,
    }
}
fn invalid(detail: impl AsRef<str>) -> MeshError {
    model_error(ErrorKind::InvalidModel, INVALID_MODEL, detail.as_ref())
}
fn limit(detail: &str) -> MeshError {
    model_error(ErrorKind::ModelLimit, MODEL_LIMIT, detail)
}
fn unsupported(detail: impl AsRef<str>) -> MeshError {
    model_error(
        ErrorKind::UnsupportedModel,
        UNSUPPORTED_MODEL,
        detail.as_ref(),
    )
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReadOptions {
    pub millimetres_per_unit: f64,
    pub max_input_bytes: i64,
    pub max_output_bytes: i64,
    pub max_vertices: i64,
    pub max_triangles: i64,
    pub max_materials: i64,
    pub max_line_bytes: i64,
    pub max_name_bytes: i64,
    pub max_face_vertices: i64,
}
impl ReadOptions {
    pub fn defaults() -> ReadOptions {
        ReadOptions {
            millimetres_per_unit: 0.0,
            max_input_bytes: 64 << 20,
            max_output_bytes: 128 << 20,
            max_vertices: 1_000_000,
            max_triangles: 2_000_000,
            max_materials: 1024,
            max_line_bytes: 65536,
            max_name_bytes: 256,
            max_face_vertices: 256,
        }
    }
    fn normalized(mut self, geometry: bool) -> Result<ReadOptions, MeshError> {
        let d = ReadOptions::defaults();
        let fill = |v: &mut i64, d: i64| {
            if *v == 0 {
                *v = d;
            }
        };
        fill(&mut self.max_input_bytes, d.max_input_bytes);
        fill(&mut self.max_output_bytes, d.max_output_bytes);
        fill(&mut self.max_vertices, d.max_vertices);
        fill(&mut self.max_triangles, d.max_triangles);
        fill(&mut self.max_materials, d.max_materials);
        fill(&mut self.max_line_bytes, d.max_line_bytes);
        fill(&mut self.max_name_bytes, d.max_name_bytes);
        fill(&mut self.max_face_vertices, d.max_face_vertices);
        let o = &self;
        if !(1..=1 << 30).contains(&o.max_input_bytes)
            || !(1..=1 << 30).contains(&o.max_output_bytes)
            || !(1..=16_000_000).contains(&o.max_vertices)
            || !(1..=16_000_000).contains(&o.max_triangles)
            || !(1..=65536).contains(&o.max_materials)
            || !(1..=1 << 20).contains(&o.max_line_bytes)
            || !(1..=65536).contains(&o.max_name_bytes)
            || !(3..=4096).contains(&o.max_face_vertices)
        {
            return Err(invalid("invalid read limits"));
        }
        if geometry && (!o.millimetres_per_unit.is_finite() || o.millimetres_per_unit <= 0.0) {
            return Err(invalid("explicit millimetres-per-unit scale required"));
        }
        Ok(self)
    }
}
fn model_input<'a>(data: &'a [u8], o: &ReadOptions) -> Result<&'a [u8], MeshError> {
    if data.len() as i64 > o.max_input_bytes {
        return Err(limit(""));
    }
    Ok(data)
}
struct Lines<'a> {
    data: &'a [u8],
    max: usize,
    failed: bool,
}
impl<'a> Lines<'a> {
    fn new(data: &'a [u8], o: &ReadOptions) -> Lines<'a> {
        Lines {
            data,
            max: o.max_line_bytes as usize + 2,
            failed: false,
        }
    }
}
impl<'a> Iterator for Lines<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        if self.failed || self.data.is_empty() {
            return None;
        }
        let window = &self.data[..self.data.len().min(self.max)];
        let line = match window.iter().position(|b| *b == b'\n') {
            Some(i) => {
                let line = &self.data[..i];
                self.data = &self.data[i + 1..];
                line
            }
            None if self.data.len() < self.max => {
                let line = self.data;
                self.data = &[];
                line
            }
            None => {
                self.failed = true;
                return None;
            }
        };
        Some(line.strip_suffix(b"\r").unwrap_or(line))
    }
}
fn model_number(raw: &str) -> Result<f64, MeshError> {
    match numtext::parse_float(raw) {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(invalid(format!(
            "non-finite/invalid number {}",
            textfmt::quote(raw)
        ))),
    }
}
fn model_point(raw: &[&str], scale: f64) -> Result<[f32; 3], MeshError> {
    let mut p = [0f32; 3];
    if raw.len() != 3 {
        return Err(invalid("expected 3 coordinates"));
    }
    for (i, slot) in p.iter_mut().enumerate() {
        let v = model_number(raw[i])? * scale;
        if !v.is_finite() || v.abs() > f64::from(f32::MAX) {
            return Err(invalid("coordinate outside finite float32 millimetres"));
        }
        *slot = v as f32;
    }
    Ok(p)
}
fn model_name(raw: &str, o: &ReadOptions) -> Result<String, MeshError> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() as i64 > o.max_name_bytes || raw.contains(['\0', '\r', '\n']) {
        return Err(invalid("invalid/oversized name"));
    }
    Ok(raw.to_string())
}
fn model_triangle(m: &mut Mesh, t: [u32; 3], o: &ReadOptions) -> Result<(), MeshError> {
    if m.triangles.len() as i64 >= o.max_triangles
        || m.vertices.len() as i64 * 12 + (m.triangles.len() as i64 + 1) * 16 > o.max_output_bytes
    {
        return Err(limit(""));
    }
    if t.iter().any(|i| *i as usize >= m.vertices.len()) {
        return Err(invalid("face indexes missing vertex"));
    }
    let (a, b, c) = (m.point(t[0]), m.point(t[1]), m.point(t[2]));
    let normal = cross3(sub3(b, a), sub3(c, a));
    if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] || dot3(normal, normal) == 0.0 {
        return Err(invalid("degenerate triangle"));
    }
    m.triangles.push(t);
    Ok(())
}
fn model_vertex(m: &mut Mesh, p: [f32; 3], o: &ReadOptions) -> Result<(), MeshError> {
    if m.vertices.len() as i64 >= o.max_vertices
        || (m.vertices.len() as i64 + 1) * 12 + m.triangles.len() as i64 * 16 > o.max_output_bytes
    {
        return Err(limit(""));
    }
    m.vertices.push(p);
    Ok(())
}
fn model_text<'a>(data: &'a [u8], what: &str) -> Result<&'a str, MeshError> {
    match std::str::from_utf8(data) {
        Ok(text) if !data.contains(&0) => Ok(text),
        _ => Err(invalid(what)),
    }
}
pub fn read_stl(data: &[u8], options: ReadOptions) -> Result<Mesh, MeshError> {
    let o = options.normalized(true)?;
    let data = model_input(data, &o)?;
    if data.len() >= 84 {
        let count = u64::from(u32::from_le_bytes([data[80], data[81], data[82], data[83]]));
        if data.len() as u64 == 84 + 50 * count {
            return read_binary_stl(data, &o);
        }
    }
    read_ascii_stl(data, &o)
}
fn le_f32(b: &[u8]) -> f32 {
    f32::from_bits(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn read_binary_stl(data: &[u8], o: &ReadOptions) -> Result<Mesh, MeshError> {
    let count = i64::from(u32::from_le_bytes([data[80], data[81], data[82], data[83]]));
    if count < 1 {
        return Err(invalid("empty STL"));
    }
    if count > o.max_triangles || count * 3 > o.max_vertices || count * 52 > o.max_output_bytes {
        return Err(limit(""));
    }
    let mut m = Mesh::default();
    for facet in data[84..].as_chunks::<50>().0 {
        for axis in 0..3 {
            if !le_f32(&facet[axis * 4..]).is_finite() {
                return Err(invalid("non-finite STL normal"));
            }
        }
        let base = m.vertices.len() as u32;
        for vertex in 0..3 {
            let mut point = [0f32; 3];
            for (axis, slot) in point.iter_mut().enumerate() {
                let v = f64::from(le_f32(&facet[12 + vertex * 12 + axis * 4..]))
                    * o.millimetres_per_unit;
                if !v.is_finite() || v.abs() > f64::from(f32::MAX) {
                    return Err(invalid("non-finite STL coordinate"));
                }
                *slot = v as f32;
            }
            model_vertex(&mut m, point, o)?;
        }
        model_triangle(&mut m, [base, base + 1, base + 2], o)?;
    }
    Ok(m)
}
fn read_ascii_stl(data: &[u8], o: &ReadOptions) -> Result<Mesh, MeshError> {
    model_text(data, "invalid STL bytes/length")?;
    let mut lines = Lines::new(data, o);
    let mut m = Mesh::default();
    let mut state = 0;
    let mut ended = false;
    let mut corners = [[0f32; 3]; 3];
    let mut vertex = 0;
    for raw in lines.by_ref() {
        if raw.len() as i64 > o.max_line_bytes {
            return Err(limit(""));
        }
        let line = std::str::from_utf8(raw).unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        match state {
            0 => {
                if f[0] != "solid" {
                    return Err(invalid("missing STL solid"));
                }
                if line.len() as i64 > o.max_name_bytes + 6 {
                    return Err(limit(""));
                }
                state = 1;
            }
            1 => {
                if f[0] == "endsolid" {
                    ended = true;
                    state = 6;
                    continue;
                }
                if f.len() != 5 || f[0] != "facet" || f[1] != "normal" {
                    return Err(invalid("expected facet normal"));
                }
                model_point(&f[2..], 1.0)?;
                state = 2;
            }
            2 => {
                if f.len() != 2 || f[0] != "outer" || f[1] != "loop" {
                    return Err(invalid("expected outer loop"));
                }
                vertex = 0;
                state = 3;
            }
            3 => {
                if f.len() != 4 || f[0] != "vertex" {
                    return Err(invalid("expected vertex"));
                }
                corners[vertex] = model_point(&f[1..], o.millimetres_per_unit)?;
                vertex += 1;
                if vertex == 3 {
                    state = 4;
                }
            }
            4 => {
                if f.len() != 1 || f[0] != "endloop" {
                    return Err(invalid("expected endloop"));
                }
                state = 5;
            }
            5 => {
                if f.len() != 1 || f[0] != "endfacet" {
                    return Err(invalid("expected endfacet"));
                }
                let base = m.vertices.len() as u32;
                for point in corners {
                    model_vertex(&mut m, point, o)?;
                }
                model_triangle(&mut m, [base, base + 1, base + 2], o)?;
                state = 1;
            }
            _ => return Err(invalid("trailing STL content")),
        }
    }
    if lines.failed {
        return Err(limit("STL line exceeds capacity"));
    }
    if !ended || m.triangles.is_empty() {
        return Err(invalid("truncated/empty STL"));
    }
    Ok(m)
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Material {
    pub name: String,
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    pub emissive: [f32; 3],
    pub opacity: f64,
    pub shininess: f64,
    pub optical_density: f64,
    pub illumination: i64,
    pub texture_references: Vec<MaterialResource>,
    pub declared: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterialResource {
    pub directive: String,
    pub arguments: String,
}
fn strip_comment(raw: &[u8]) -> &str {
    let line = std::str::from_utf8(raw).unwrap_or("").trim();
    match line.find('#') {
        Some(pos) => line[..pos].trim(),
        None => line,
    }
}
pub fn read_mtl(data: &[u8], options: ReadOptions) -> Result<Vec<Material>, MeshError> {
    let o = options.normalized(false)?;
    let data = model_input(data, &o)?;
    model_text(data, "MTL must be UTF-8 text")?;
    let mut lines = Lines::new(data, &o);
    let mut out: Vec<Material> = Vec::new();
    let mut strings_bytes: i64 = 0;
    for raw in lines.by_ref() {
        if raw.len() as i64 > o.max_line_bytes {
            return Err(limit(""));
        }
        let line = strip_comment(raw);
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        if f[0] == "newmtl" {
            let name = model_name(line.strip_prefix("newmtl").unwrap_or(line), &o)?;
            if out.iter().any(|m| m.name == name) {
                return Err(invalid(format!(
                    "duplicate material {}",
                    textfmt::quote(&name)
                )));
            }
            if out.len() as i64 >= o.max_materials {
                return Err(limit(""));
            }
            strings_bytes += name.len() as i64;
            out.push(Material {
                name,
                opacity: 1.0,
                optical_density: 1.0,
                ..Material::default()
            });
        } else {
            let Some(current) = out.last_mut() else {
                return Err(invalid("material properties before newmtl"));
            };
            for key in &current.declared {
                if key == f[0] || (key == "d" && f[0] == "Tr") || (key == "Tr" && f[0] == "d") {
                    return Err(invalid(format!("duplicate MTL property {}", f[0])));
                }
            }
            current.declared.push(f[0].to_string());
            strings_bytes += f[0].len() as i64;
            match f[0] {
                "Ka" | "Kd" | "Ks" | "Ke" => {
                    let rgb = model_point(&f[1..], 1.0)?;
                    if rgb.iter().any(|v| !(0.0..=1.0).contains(v)) {
                        return Err(invalid("RGB material channel outside [0,1]"));
                    }
                    match f[0] {
                        "Ka" => current.ambient = rgb,
                        "Kd" => current.diffuse = rgb,
                        "Ks" => current.specular = rgb,
                        _ => current.emissive = rgb,
                    }
                }
                "d" | "Tr" | "Ns" | "Ni" => {
                    if f.len() != 2 {
                        return Err(unsupported("unsupported MTL scalar syntax"));
                    }
                    let mut value = model_number(f[1])?;
                    match f[0] {
                        "d" | "Tr" => {
                            if !(0.0..=1.0).contains(&value) {
                                return Err(invalid(""));
                            }
                            if f[0] == "Tr" {
                                value = 1.0 - value;
                            }
                            current.opacity = value;
                        }
                        "Ns" => {
                            if !(0.0..=1000.0).contains(&value) {
                                return Err(invalid(""));
                            }
                            current.shininess = value;
                        }
                        _ => {
                            if value <= 0.0 || value > 10.0 {
                                return Err(invalid(""));
                            }
                            current.optical_density = value;
                        }
                    }
                }
                "illum" => {
                    if f.len() != 2 {
                        return Err(invalid(""));
                    }
                    match numtext::parse_int(f[1]) {
                        Ok(value) if (0..=10).contains(&value) => current.illumination = value,
                        _ => return Err(invalid("")),
                    }
                }
                "map_Ka" | "map_Kd" | "map_Ks" | "map_Ke" | "map_Ns" | "map_d" | "bump"
                | "map_bump" | "disp" | "decal" | "refl" => {
                    let args = model_name(line.strip_prefix(f[0]).unwrap_or(line), &o)?;
                    strings_bytes += args.len() as i64 + 32;
                    current.texture_references.push(MaterialResource {
                        directive: f[0].to_string(),
                        arguments: args,
                    });
                }
                other => return Err(unsupported(format!("MTL directive {other}"))),
            }
        }
        if out.len() as i64 * 256 + strings_bytes > o.max_output_bytes {
            return Err(limit(""));
        }
    }
    if lines.failed {
        return Err(limit("MTL line exceeds capacity"));
    }
    if out.is_empty() {
        return Err(invalid("empty material library"));
    }
    Ok(out)
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjModel {
    pub mesh: Mesh,
    pub normals: Vec<[f32; 3]>,
    pub texture_coordinates: Vec<[f32; 3]>,
    pub triangle_normals: Vec<[i32; 3]>,
    pub triangle_textures: Vec<[i32; 3]>,
    pub triangle_materials: Vec<i32>,
    pub material_names: Vec<String>,
    pub material_libraries: Vec<String>,
}
#[derive(Clone, Copy)]
struct ObjCorner {
    vertex: u32,
    normal: i32,
    texture: i32,
}
fn obj_index(text: &str, count: usize) -> Result<i32, MeshError> {
    let n = match numtext::parse_int(text) {
        Ok(n) if n != 0 && i32::try_from(n).is_ok() => n,
        _ => return Err(invalid("invalid OBJ index")),
    };
    let n = if n < 0 { count as i64 + n } else { n - 1 };
    if n < 0 || n >= count as i64 {
        return Err(invalid("OBJ index outside preceding vertices"));
    }
    Ok(n as i32)
}
pub fn read_obj(data: &[u8], options: ReadOptions) -> Result<ObjModel, MeshError> {
    let o = options.normalized(true)?;
    let data = model_input(data, &o)?;
    model_text(data, "OBJ must be UTF-8 text")?;
    let mut lines = Lines::new(data, &o);
    let mut out = ObjModel::default();
    let mut material: i32 = -1;
    let mut names: HashMap<String, i32> = HashMap::new();
    let mut continuation = String::new();
    for (index, raw) in lines.by_ref().enumerate() {
        if raw.len() as i64 > o.max_line_bytes {
            return Err(limit(""));
        }
        let line_number = index + 1;
        let piece = strip_comment(raw);
        if (continuation.len() + piece.len()) as i64 > o.max_line_bytes {
            return Err(limit(""));
        }
        let line = format!("{continuation}{piece}");
        continuation.clear();
        if let Some(head) = line.strip_suffix('\\') {
            continuation = format!("{head} ");
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        let bad = || invalid(format!("OBJ line {line_number} ({})", f[0]));
        match f[0] {
            "v" => {
                if f.len() != 4 && f.len() != 5 {
                    return Err(bad());
                }
                let mut scale = o.millimetres_per_unit;
                if f.len() == 5 {
                    match model_number(f[4]) {
                        Ok(weight) if weight != 0.0 => scale /= weight,
                        _ => return Err(bad()),
                    }
                }
                let point = model_point(&f[1..4], scale)?;
                model_vertex(&mut out.mesh, point, &o)?;
            }
            "vn" => {
                let point = model_point(&f[1..], 1.0)?;
                if point == [0.0; 3] {
                    return Err(bad());
                }
                if out.normals.len() as i64 >= o.max_vertices {
                    return Err(limit(""));
                }
                out.normals.push(point);
            }
            "vt" => {
                if f.len() < 2 || f.len() > 4 {
                    return Err(bad());
                }
                let mut raw = ["0", "0", "0"];
                raw[..f.len() - 1].copy_from_slice(&f[1..]);
                let point = model_point(&raw, 1.0)?;
                if out.texture_coordinates.len() as i64 >= o.max_vertices {
                    return Err(limit(""));
                }
                out.texture_coordinates.push(point);
            }
            "f" => {
                if f.len() < 4 || f.len() as i64 - 1 > o.max_face_vertices {
                    return Err(bad());
                }
                let mut corners = Vec::with_capacity(f.len() - 1);
                for token in &f[1..] {
                    let parts: Vec<&str> = token.split('/').collect();
                    if parts.len() > 3 || parts[0].is_empty() {
                        return Err(bad());
                    }
                    let vertex = obj_index(parts[0], out.mesh.vertices.len())?;
                    let mut c = ObjCorner {
                        vertex: vertex as u32,
                        normal: -1,
                        texture: -1,
                    };
                    if parts.len() > 1 && !parts[1].is_empty() {
                        c.texture = obj_index(parts[1], out.texture_coordinates.len())?;
                    }
                    if parts.len() > 2 {
                        if parts[2].is_empty() {
                            return Err(bad());
                        }
                        c.normal = obj_index(parts[2], out.normals.len())?;
                    }
                    corners.push(c);
                }
                for indices in triangulate_obj(&out.mesh, &corners)? {
                    let (a, b, c) = (
                        corners[indices[0]],
                        corners[indices[1]],
                        corners[indices[2]],
                    );
                    model_triangle(&mut out.mesh, [a.vertex, b.vertex, c.vertex], &o)?;
                    out.triangle_normals.push([a.normal, b.normal, c.normal]);
                    out.triangle_textures
                        .push([a.texture, b.texture, c.texture]);
                    out.triangle_materials.push(material);
                }
            }
            "usemtl" => {
                let name = model_name(line.strip_prefix("usemtl").unwrap_or(&line), &o)?;
                material = match names.get(&name) {
                    Some(index) => *index,
                    None => {
                        if names.len() as i64 >= o.max_materials {
                            return Err(limit(""));
                        }
                        let index = out.material_names.len() as i32;
                        names.insert(name.clone(), index);
                        out.material_names.push(name);
                        index
                    }
                };
            }
            "mtllib" => {
                if f.len() < 2 {
                    return Err(bad());
                }
                for raw in &f[1..] {
                    let name = model_name(raw, &o)?;
                    if out.material_libraries.len() as i64 >= o.max_materials {
                        return Err(limit(""));
                    }
                    out.material_libraries.push(name);
                }
            }
            "o" | "g" => {
                if line.len() as i64 > o.max_name_bytes + 2 {
                    return Err(limit(""));
                }
            }
            "s" => {
                if f.len() != 2 {
                    return Err(bad());
                }
                if f[1] != "off" && f[1] != "on" && !is_u32(f[1]) {
                    return Err(bad());
                }
            }
            other => return Err(unsupported(format!("OBJ directive {other}"))),
        }
        let memory = out.mesh.bytes()
            + (out.normals.len() + out.texture_coordinates.len()) as i64 * 12
            + out.mesh.triangles.len() as i64 * 28
            + (out.material_names.len() + out.material_libraries.len()) as i64
                * (o.max_name_bytes + 16);
        if memory > o.max_output_bytes {
            return Err(limit(""));
        }
    }
    if lines.failed {
        return Err(limit("OBJ line exceeds capacity"));
    }
    if !continuation.is_empty() || out.mesh.triangles.is_empty() {
        return Err(invalid("incomplete/empty OBJ"));
    }
    Ok(out)
}
fn is_u32(s: &str) -> bool {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut n: u64 = 0;
    for b in s.bytes() {
        n = n * 10 + u64::from(b - b'0');
        if n > u64::from(u32::MAX) {
            return false;
        }
    }
    true
}
fn triangulate_obj(m: &Mesh, corners: &[ObjCorner]) -> Result<Vec<[usize; 3]>, MeshError> {
    let n = corners.len();
    if n == 3 {
        return Ok(vec![[0, 1, 2]]);
    }
    let mut points: Vec<[f64; 3]> = Vec::with_capacity(n);
    for c in corners {
        let p = m.point(c.vertex);
        if points.contains(&p) {
            return Err(invalid("polygon repeats a point"));
        }
        points.push(p);
    }
    let mut normal = [0.0f64; 3];
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % n];
        normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
        normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
        normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let length = dot3(normal, normal).sqrt();
    if length == 0.0 {
        return Err(invalid("zero-area polygon"));
    }
    let mut axis = 0;
    for i in 1..3 {
        if normal[i].abs() > normal[axis].abs() {
            axis = i;
        }
    }
    let mut extent = 0.0;
    for p in &points {
        let d = sub3(*p, points[0]);
        extent = fmath::max(extent, dot3(d, d).sqrt());
    }
    let tolerance = fmath::max(extent * 1e-6, 1e-12);
    let mut projected: Vec<[f64; 2]> = Vec::with_capacity(n);
    for p in &points {
        if (dot3(normal, sub3(*p, points[0])) / length).abs() > tolerance {
            return Err(invalid("non-planar OBJ polygon"));
        }
        projected.push(match axis {
            0 => [p[1], p[2]],
            1 => [p[0], p[2]],
            _ => [p[0], p[1]],
        });
    }
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let mut area = 0.0;
    for (i, a) in projected.iter().enumerate() {
        let b = projected[(i + 1) % n];
        area += a[0] * b[1] - b[0] * a[1];
    }
    if area == 0.0 {
        return Err(invalid("zero projected polygon area"));
    }
    let sign = if area < 0.0 { -1.0 } else { 1.0 };
    let on_segment = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
        cross(a, b, p) == 0.0
            && p[0] >= fmath::min(a[0], b[0])
            && p[0] <= fmath::max(a[0], b[0])
            && p[1] >= fmath::min(a[1], b[1])
            && p[1] <= fmath::max(a[1], b[1])
    };
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let (a, b, c, d) = (
                projected[i],
                projected[(i + 1) % n],
                projected[j],
                projected[(j + 1) % n],
            );
            if (cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0)
                || on_segment(a, b, c)
                || on_segment(a, b, d)
                || on_segment(c, d, a)
                || on_segment(c, d, b)
            {
                return Err(invalid("self-intersecting polygon"));
            }
        }
    }
    let mut remaining: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    while remaining.len() > 3 {
        let mut found = None;
        let len = remaining.len();
        for (i, b) in remaining.iter().enumerate() {
            let (a, b, c) = (remaining[(i + len - 1) % len], *b, remaining[(i + 1) % len]);
            if cross(projected[a], projected[b], projected[c]) * sign <= 0.0 {
                continue;
            }
            let blocked = remaining.iter().any(|p| {
                let p = *p;
                p != a
                    && p != b
                    && p != c
                    && cross(projected[a], projected[b], projected[p]) * sign >= 0.0
                    && cross(projected[b], projected[c], projected[p]) * sign >= 0.0
                    && cross(projected[c], projected[a], projected[p]) * sign >= 0.0
            });
            if blocked {
                continue;
            }
            out.push([a, b, c]);
            found = Some(i);
            break;
        }
        match found {
            Some(i) => {
                remaining.remove(i);
            }
            None => return Err(invalid("degenerate/non-simple OBJ polygon")),
        }
    }
    out.push([remaining[0], remaining[1], remaining[2]]);
    Ok(out)
}
