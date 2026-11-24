use super::attributes::{VertexScalarAttribute, validate_vertex_scalars};
use super::gltf::Part;
use super::{Mesh, MeshError, cross3, dot3, format_f32, sub3, validate_opt};
use std::collections::HashMap;
use std::io::Write;
impl Mesh {
    pub fn area(&self) -> f64 {
        let mut area = 0.0;
        for t in &self.triangles {
            let (a, b, c) = (self.point(t[0]), self.point(t[1]), self.point(t[2]));
            let n = cross3(sub3(b, a), sub3(c, a));
            area += dot3(n, n).sqrt() / 2.0;
        }
        area
    }
    pub fn volume(&self) -> f64 {
        let mut volume = 0.0;
        for t in &self.triangles {
            let (a, b, c) = (self.point(t[0]), self.point(t[1]), self.point(t[2]));
            volume += dot3(a, cross3(b, c)) / 6.0;
        }
        volume
    }
    pub fn check_closure(&self) -> Closure {
        let mut c = Closure::default();
        let mut edges: HashMap<[u32; 2], (i64, i64)> =
            HashMap::with_capacity(3 * self.triangles.len() / 2);
        for t in &self.triangles {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                let uses = edges.entry([a.min(b), a.max(b)]).or_insert((0, 0));
                if a < b {
                    uses.0 += 1;
                } else {
                    uses.1 += 1;
                }
            }
        }
        for (forward, backward) in edges.values() {
            let total = forward + backward;
            if total == 1 {
                c.boundary_edges += 1;
            } else if total > 2 {
                c.non_manifold_edges += 1;
            } else if *forward != 1 {
                c.inconsistent_edges += 1;
            }
        }
        c
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Closure {
    pub boundary_edges: i64,
    pub non_manifold_edges: i64,
    pub inconsistent_edges: i64,
}
impl Closure {
    pub fn closed(&self) -> bool {
        self.boundary_edges == 0 && self.non_manifold_edges == 0 && self.inconsistent_edges == 0
    }
}
pub fn write_stl(w: &mut dyn Write, m: &Mesh, header: &str) -> Result<(), MeshError> {
    m.validate()?;
    if m.triangles.len() as u64 > u64::from(u32::MAX) {
        return Err(MeshError::invalid(format!(
            "{} triangles exceed STL's count",
            m.triangles.len()
        )));
    }
    let mut out = Vec::with_capacity(84 + 50 * m.triangles.len());
    let mut head = [0u8; 84];
    let bytes = header.as_bytes();
    let n = bytes.len().min(80);
    head[..n].copy_from_slice(&bytes[..n]);
    head[80..].copy_from_slice(&(m.triangles.len() as u32).to_le_bytes());
    out.extend_from_slice(&head);
    for t in &m.triangles {
        let n = m.triangle_normal(*t);
        for value in n {
            out.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        for k in t {
            for value in m.vertices[*k as usize] {
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    w.write_all(&out)?;
    Ok(w.flush()?)
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlyOptions {
    pub face_colors: Option<Vec<[u8; 3]>>,
    pub comments: Vec<String>,
    pub vertex_scalars: Vec<VertexScalarAttribute>,
}
pub fn write_ply(w: &mut dyn Write, m: &Mesh, opts: &PlyOptions) -> Result<(), MeshError> {
    m.validate()?;
    if let Some(colors) = &opts.face_colors
        && colors.len() != m.triangles.len()
    {
        return Err(MeshError::invalid(format!(
            "{} face colours for {} triangles",
            colors.len(),
            m.triangles.len()
        )));
    }
    validate_vertex_scalars(&opts.vertex_scalars, m.vertices.len(), false)?;
    let mut header = String::from("ply\nformat binary_little_endian 1.0\ncomment units mm\n");
    for comment in &opts.comments {
        header += &format!("comment {}\n", sanitize_header_line(comment));
    }
    for attribute in &opts.vertex_scalars {
        if !attribute.units.is_empty() {
            header += &format!(
                "comment vertex_scalar {} units {}\n",
                attribute.name, attribute.units
            );
        }
    }
    header += &format!(
        "element vertex {}\nproperty float x\nproperty float y\nproperty float z\n",
        m.vertices.len()
    );
    if m.normals.is_some() {
        header += "property float nx\nproperty float ny\nproperty float nz\n";
    }
    for attribute in &opts.vertex_scalars {
        header += &format!("property double {}\n", attribute.name);
    }
    header += &format!(
        "element face {}\nproperty list uchar int vertex_indices\n",
        m.triangles.len()
    );
    if opts.face_colors.is_some() {
        header += "property uchar red\nproperty uchar green\nproperty uchar blue\n";
    }
    header += "end_header\n";
    let mut out = header.into_bytes();
    for (i, v) in m.vertices.iter().enumerate() {
        for value in v {
            out.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        if let Some(normals) = &m.normals {
            for value in normals[i] {
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        for attribute in &opts.vertex_scalars {
            out.extend_from_slice(&attribute.values[i].to_bits().to_le_bytes());
        }
    }
    for (i, t) in m.triangles.iter().enumerate() {
        out.push(3);
        for k in t {
            out.extend_from_slice(&k.to_le_bytes());
        }
        if let Some(colors) = &opts.face_colors {
            out.extend_from_slice(&colors[i]);
        }
    }
    w.write_all(&out)?;
    Ok(w.flush()?)
}
fn obj_line(out: &mut String, prefix: &str, v: [f32; 3]) {
    out.push_str(prefix);
    for c in v {
        out.push(' ');
        out.push_str(&format_f32(c));
    }
    out.push('\n');
}
pub fn write_obj(w: &mut dyn Write, m: &Mesh, comment: &str) -> Result<(), MeshError> {
    m.validate()?;
    let mut out = String::from("# units mm\n");
    if !comment.is_empty() {
        out += &format!("# {}\n", sanitize_header_line(comment));
    }
    for v in &m.vertices {
        obj_line(&mut out, "v", *v);
    }
    if let Some(normals) = &m.normals {
        for n in normals {
            obj_line(&mut out, "vn", *n);
        }
    }
    for t in &m.triangles {
        let (a, b, c) = (
            t[0].wrapping_add(1),
            t[1].wrapping_add(1),
            t[2].wrapping_add(1),
        );
        if m.normals.is_some() {
            out += &format!("f {a}//{a} {b}//{b} {c}//{c}\n");
        } else {
            out += &format!("f {a} {b} {c}\n");
        }
    }
    w.write_all(out.as_bytes())?;
    Ok(w.flush()?)
}
pub fn write_obj_parts(
    w: &mut dyn Write,
    parts: &[Part<'_>],
    mtl_name: &str,
    comment: &str,
) -> Result<(), MeshError> {
    if parts.is_empty() {
        return Err(MeshError::invalid("OBJ export without parts"));
    }
    for (i, part) in parts.iter().enumerate() {
        validate_opt(part.mesh).map_err(|e| e.wrap(&format!("OBJ part {i}: ")))?;
    }
    let mut out = String::from("# units mm\n");
    if !comment.is_empty() {
        out += &format!("# {}\n", sanitize_header_line(comment));
    }
    if !mtl_name.is_empty() {
        out += &format!("mtllib {}\n", sanitize_header_line(mtl_name));
    }
    let (mut vertex_base, mut normal_base) = (0u32, 0u32);
    for (i, part) in parts.iter().enumerate() {
        let Some(m) = part.mesh else { continue };
        let name = obj_material_name(i as i64, part.name);
        out += &format!("g {name}\nusemtl {name}\n");
        for v in &m.vertices {
            obj_line(&mut out, "v", *v);
        }
        let normal_count = m.normals.as_ref().map_or(0, Vec::len);
        if let Some(normals) = &m.normals {
            for n in normals {
                obj_line(&mut out, "vn", *n);
            }
        }
        for t in &m.triangles {
            let corner = |base: u32, k: usize| base.wrapping_add(t[k]).wrapping_add(1);
            let (a, b, c) = (
                corner(vertex_base, 0),
                corner(vertex_base, 1),
                corner(vertex_base, 2),
            );
            if m.normals.is_some() {
                let (na, nb, nc) = (
                    corner(normal_base, 0),
                    corner(normal_base, 1),
                    corner(normal_base, 2),
                );
                out += &format!("f {a}//{na} {b}//{nb} {c}//{nc}\n");
            } else {
                out += &format!("f {a} {b} {c}\n");
            }
        }
        vertex_base = vertex_base.wrapping_add(m.vertices.len() as u32);
        normal_base = normal_base.wrapping_add(normal_count as u32);
    }
    w.write_all(out.as_bytes())?;
    Ok(w.flush()?)
}
pub fn write_mtl(w: &mut dyn Write, parts: &[Part<'_>]) -> Result<(), MeshError> {
    let mut out = String::from("# dcm-rust mesh materials\n");
    let channel = |c: u8| format!("{:.4}", f64::from(c) / 255.0);
    for (i, part) in parts.iter().enumerate() {
        let c = part.color;
        out += &format!(
            "\nnewmtl {}\nKa 0 0 0\nKd {} {} {}\nKs 0 0 0\nd 1\nillum 1\n",
            obj_material_name(i as i64, part.name),
            channel(c[0]),
            channel(c[1]),
            channel(c[2])
        );
    }
    w.write_all(out.as_bytes())?;
    Ok(w.flush()?)
}
pub fn obj_material_name(i: i64, name: &str) -> String {
    let mut out = format!("part{i}");
    if !name.is_empty() {
        out.push('_');
    }
    for r in name.chars() {
        out.push(if r == '-' || r == '_' || r.is_ascii_alphanumeric() {
            r
        } else {
            '_'
        });
    }
    out
}
pub(crate) fn sanitize_header_line(s: &str) -> String {
    s.chars()
        .map(|r| if r == '\n' || r == '\r' { ' ' } else { r })
        .collect()
}
pub fn merge(meshes: &[Option<&Mesh>], colors: Option<&[[u8; 3]]>) -> (Mesh, Option<Vec<[u8; 3]>>) {
    let mut out = Mesh::default();
    let mut faces: Vec<[u8; 3]> = Vec::new();
    let mut normals = !meshes.is_empty();
    for m in meshes.iter().flatten() {
        if m.normals.is_none() && !m.vertices.is_empty() {
            normals = false;
        }
    }
    let mut merged_normals: Vec<[f32; 3]> = Vec::new();
    for (i, m) in meshes.iter().enumerate() {
        let Some(m) = m else { continue };
        let base = out.vertices.len() as u32;
        out.vertices.extend_from_slice(&m.vertices);
        if normals && let Some(n) = &m.normals {
            merged_normals.extend_from_slice(n);
        }
        for t in &m.triangles {
            out.triangles.push([
                t[0].wrapping_add(base),
                t[1].wrapping_add(base),
                t[2].wrapping_add(base),
            ]);
            if let Some(colors) = colors {
                faces.push(colors.get(i).copied().unwrap_or_default());
            }
        }
    }
    if !merged_normals.is_empty() {
        out.normals = Some(merged_normals);
    }
    let faces = (!faces.is_empty()).then_some(faces);
    (out, faces)
}
