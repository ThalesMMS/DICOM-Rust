use super::attributes::{
    MAX_GLB_SCALAR_ATTRIBUTES, MAX_VERTEX_SCALAR_ATTRIBUTES, VertexScalarAttribute,
    validate_vertex_scalars,
};
use super::io::sanitize_header_line;
use super::{Mesh, MeshError, unit32, validate_opt};
use crate::fmath;
use crate::numtext::format_float;
use std::collections::BTreeMap;
use std::io::Write;
#[derive(Debug, Clone, Copy, Default)]
pub struct Part<'a> {
    pub name: &'a str,
    pub mesh: Option<&'a Mesh>,
    pub color: [u8; 3],
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlbOptions {
    pub generator: String,
    pub comment: String,
    pub patient_axes: bool,
    pub vertex_scalars: Option<Vec<Vec<VertexScalarAttribute>>>,
}
const GLB_MAGIC: u32 = 0x4654_6C67;
const GLB_VERSION: u32 = 2;
const GLB_CHUNK_JSON: u32 = 0x4E4F_534A;
const GLB_CHUNK_BIN: u32 = 0x004E_4942;
const GLTF_FLOAT: i64 = 5126;
const GLTF_UNSIGNED_SHORT: i64 = 5123;
const GLTF_UNSIGNED_INT: i64 = 5125;
const GLTF_ARRAY_BUFFER: i64 = 34962;
const GLTF_ELEMENT_BUFFER: i64 = 34963;
const GLTF_TRIANGLES: i64 = 4;
const GLTF_SHORT_INDEX_LIMIT: usize = 65535;
fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
fn json_float(out: &mut String, f: f64) {
    let abs = f.abs();
    let exponent = abs != 0.0 && !(1e-6..1e21).contains(&abs);
    let mut text = format_float(f, if exponent { 'e' } else { 'f' }, -1);
    if exponent {
        let b = text.as_bytes();
        let n = b.len();
        if n >= 4 && b[n - 4] == b'e' && (b[n - 3] == b'-' || b[n - 3] == b'+') && b[n - 2] == b'0'
        {
            text.remove(n - 2);
        }
    }
    out.push_str(&text);
}
fn json_floats(out: &mut String, values: &[f64]) {
    out.push('[');
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_float(out, *v);
    }
    out.push(']');
}
fn json_ints(out: &mut String, values: &[usize]) {
    out.push('[');
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&v.to_string());
    }
    out.push(']');
}
struct Accessor {
    buffer_view: usize,
    component_type: i64,
    count: usize,
    kind: &'static str,
    bounds: Option<(Vec<f64>, Vec<f64>)>,
    units: Option<String>,
}
struct BufferView {
    offset: i64,
    length: i64,
    target: i64,
}
struct Prepared<'a> {
    mesh: &'a Mesh,
    normals: Option<Vec<[f32; 3]>>,
    short: bool,
    scalars: &'a [VertexScalarAttribute],
    name: &'a str,
    attributes: BTreeMap<String, usize>,
    indices: usize,
    color: [f64; 4],
}
fn min32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}
fn max32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}
fn vertex_bounds(vertices: &[[f32; 3]]) -> (Vec<f64>, Vec<f64>) {
    let (mut lo, mut hi) = (vertices[0], vertices[0]);
    for v in &vertices[1..] {
        for k in 0..3 {
            lo[k] = min32(lo[k], v[k]);
            hi[k] = max32(hi[k], v[k]);
        }
    }
    (
        lo.iter().map(|c| f64::from(*c)).collect(),
        hi.iter().map(|c| f64::from(*c)).collect(),
    )
}
fn unit_normals(normals: Option<&Vec<[f32; 3]>>) -> Option<Vec<[f32; 3]>> {
    let normals = normals?;
    let mut out = Vec::with_capacity(normals.len());
    for n in normals {
        let u = unit32([f64::from(n[0]), f64::from(n[1]), f64::from(n[2])]);
        if u == [0.0; 3] {
            return None;
        }
        out.push(u);
    }
    Some(out)
}
fn srgb_to_linear(c: u8) -> f64 {
    let v = f64::from(c) / 255.0;
    if v <= 0.04045 {
        return v / 12.92;
    }
    fmath::pow((v + 0.055) / 1.055, 2.4)
}
pub fn write_glb(
    w: &mut dyn Write,
    parts: &[Part<'_>],
    opts: &GlbOptions,
) -> Result<(), MeshError> {
    if parts.is_empty() {
        return Err(MeshError::invalid("glTF export without parts"));
    }
    if let Some(lists) = &opts.vertex_scalars {
        if lists.len() != parts.len() {
            return Err(MeshError::invalid(format!(
                "{} scalar attribute lists for {} glTF parts",
                lists.len(),
                parts.len()
            )));
        }
        let mut total = 0usize;
        for attributes in lists {
            if attributes.len() > MAX_VERTEX_SCALAR_ATTRIBUTES
                || total + attributes.len() > MAX_GLB_SCALAR_ATTRIBUTES
            {
                return Err(MeshError::invalid("glTF scalar attribute count"));
            }
            total += attributes.len();
        }
        for (i, attributes) in lists.iter().enumerate() {
            let Some(mesh) = parts[i].mesh else {
                return Err(MeshError::invalid(format!("nil glTF part {i}")));
            };
            validate_vertex_scalars(attributes, mesh.vertices.len(), true)
                .map_err(|e| e.wrap(&format!("glTF part {i}: ")))?;
        }
    }
    let units = if opts.patient_axes {
        "Vertex positions are DICOM patient LPS millimetres; the root node scales them to metres."
    } else {
        "Vertex positions are DICOM patient LPS millimetres; the root node scales them to metres and turns superior to +Y."
    };
    let generator = if opts.generator.is_empty() {
        "dcm-rust mesh"
    } else {
        &opts.generator
    };
    let mut accessors: Vec<Accessor> = Vec::new();
    let mut views: Vec<BufferView> = Vec::new();
    let mut offset: i64 = 0;
    let mut view = |views: &mut Vec<BufferView>, length: i64, target: i64| -> usize {
        views.push(BufferView {
            offset,
            length,
            target,
        });
        offset += (length + 3) & !3;
        views.len() - 1
    };
    let mut prepared: Vec<Prepared<'_>> = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        validate_opt(part.mesh).map_err(|e| e.wrap(&format!("glTF part {i}: ")))?;
        let Some(m) = part.mesh else { continue };
        if m.triangles.is_empty() {
            return Err(MeshError::invalid(format!(
                "glTF part {i} has no triangles"
            )));
        }
        let normals = unit_normals(m.normals.as_ref());
        let short = m.vertices.len() <= GLTF_SHORT_INDEX_LIMIT;
        let scalars: &[VertexScalarAttribute] = match &opts.vertex_scalars {
            Some(lists) => &lists[i],
            None => &[],
        };
        let count = m.vertices.len();
        let mut attributes = BTreeMap::new();
        attributes.insert("POSITION".to_string(), accessors.len());
        accessors.push(Accessor {
            buffer_view: view(&mut views, 12 * count as i64, GLTF_ARRAY_BUFFER),
            component_type: GLTF_FLOAT,
            count,
            kind: "VEC3",
            bounds: Some(vertex_bounds(&m.vertices)),
            units: None,
        });
        if normals.is_some() {
            attributes.insert("NORMAL".to_string(), accessors.len());
            accessors.push(Accessor {
                buffer_view: view(&mut views, 12 * count as i64, GLTF_ARRAY_BUFFER),
                component_type: GLTF_FLOAT,
                count,
                kind: "VEC3",
                bounds: None,
                units: None,
            });
        }
        for scalar in scalars {
            attributes.insert(format!("_{}", scalar.name), accessors.len());
            accessors.push(Accessor {
                buffer_view: view(&mut views, 4 * count as i64, GLTF_ARRAY_BUFFER),
                component_type: GLTF_FLOAT,
                count,
                kind: "SCALAR",
                bounds: None,
                units: (!scalar.units.is_empty()).then(|| scalar.units.clone()),
            });
        }
        let (index_size, component_type) = if short {
            (2, GLTF_UNSIGNED_SHORT)
        } else {
            (4, GLTF_UNSIGNED_INT)
        };
        let indices = accessors.len();
        accessors.push(Accessor {
            buffer_view: view(
                &mut views,
                3 * index_size * m.triangles.len() as i64,
                GLTF_ELEMENT_BUFFER,
            ),
            component_type,
            count: 3 * m.triangles.len(),
            kind: "SCALAR",
            bounds: None,
            units: None,
        });
        prepared.push(Prepared {
            mesh: m,
            normals,
            short,
            scalars,
            name: part.name,
            attributes,
            indices,
            color: [
                srgb_to_linear(part.color[0]),
                srgb_to_linear(part.color[1]),
                srgb_to_linear(part.color[2]),
                1.0,
            ],
        });
    }
    let mut text = String::from("{\"asset\":{\"version\":\"2.0\",\"generator\":");
    json_string(&mut text, generator);
    text.push_str(",\"extras\":{\"units\":");
    json_string(&mut text, units);
    let comment = sanitize_header_line(&opts.comment);
    if !comment.is_empty() {
        text.push_str(",\"comment\":");
        json_string(&mut text, &comment);
    }
    text.push_str("}},\"scene\":0,\"scenes\":[{\"nodes\":[0]}],\"nodes\":[{\"name\":\"patient\"");
    text.push_str(",\"children\":");
    json_ints(&mut text, &(1..=prepared.len()).collect::<Vec<_>>());
    if !opts.patient_axes {
        text.push_str(",\"rotation\":");
        let half = std::f64::consts::FRAC_1_SQRT_2;
        json_floats(&mut text, &[-half, 0.0, 0.0, half]);
    }
    text.push_str(",\"scale\":[0.001,0.001,0.001]}");
    for (i, p) in prepared.iter().enumerate() {
        text.push_str(",{");
        if !p.name.is_empty() {
            text.push_str("\"name\":");
            json_string(&mut text, p.name);
            text.push(',');
        }
        text.push_str(&format!("\"mesh\":{i}}}"));
    }
    text.push_str("],\"meshes\":[");
    for (i, p) in prepared.iter().enumerate() {
        if i > 0 {
            text.push(',');
        }
        text.push('{');
        if !p.name.is_empty() {
            text.push_str("\"name\":");
            json_string(&mut text, p.name);
            text.push(',');
        }
        text.push_str("\"primitives\":[{\"attributes\":{");
        for (k, (name, accessor)) in p.attributes.iter().enumerate() {
            if k > 0 {
                text.push(',');
            }
            json_string(&mut text, name);
            text.push_str(&format!(":{accessor}"));
        }
        text.push_str(&format!(
            "}},\"indices\":{},\"material\":{i},\"mode\":{GLTF_TRIANGLES}}}]}}",
            p.indices
        ));
    }
    text.push_str("],\"materials\":[");
    for (i, p) in prepared.iter().enumerate() {
        if i > 0 {
            text.push(',');
        }
        text.push('{');
        if !p.name.is_empty() {
            text.push_str("\"name\":");
            json_string(&mut text, p.name);
            text.push(',');
        }
        text.push_str("\"pbrMetallicRoughness\":{\"baseColorFactor\":");
        json_floats(&mut text, &p.color);
        text.push_str(",\"metallicFactor\":0,\"roughnessFactor\":0.6}}");
    }
    text.push_str("],\"accessors\":[");
    for (i, a) in accessors.iter().enumerate() {
        if i > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            "{{\"bufferView\":{},\"componentType\":{},\"count\":{},\"type\":\"{}\"",
            a.buffer_view, a.component_type, a.count, a.kind
        ));
        if let Some((lower, upper)) = &a.bounds {
            text.push_str(",\"min\":");
            json_floats(&mut text, lower);
            text.push_str(",\"max\":");
            json_floats(&mut text, upper);
        }
        if let Some(units) = &a.units {
            text.push_str(",\"extras\":{\"units\":");
            json_string(&mut text, units);
            text.push('}');
        }
        text.push('}');
    }
    text.push_str("],\"bufferViews\":[");
    for (i, v) in views.iter().enumerate() {
        if i > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            "{{\"buffer\":0,\"byteOffset\":{},\"byteLength\":{},\"target\":{}}}",
            v.offset, v.length, v.target
        ));
    }
    text.push_str(&format!("],\"buffers\":[{{\"byteLength\":{offset}}}]}}"));
    let mut text = text.into_bytes();
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    let total = (12 + 8 + text.len() + 8) as i64 + offset;
    if total > i64::from(u32::MAX) {
        return Err(MeshError::invalid(format!(
            "glTF binary of {total} bytes exceeds 4 GiB"
        )));
    }
    let mut out: Vec<u8> = Vec::with_capacity(total as usize);
    let put = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    put(&mut out, GLB_MAGIC);
    put(&mut out, GLB_VERSION);
    put(&mut out, total as u32);
    put(&mut out, text.len() as u32);
    put(&mut out, GLB_CHUNK_JSON);
    out.extend_from_slice(&text);
    put(&mut out, offset as u32);
    put(&mut out, GLB_CHUNK_BIN);
    for p in &prepared {
        for v in &p.mesh.vertices {
            for c in v {
                put(&mut out, c.to_bits());
            }
        }
        if let Some(normals) = &p.normals {
            for n in normals {
                for c in n {
                    put(&mut out, c.to_bits());
                }
            }
        }
        for scalar in p.scalars {
            for value in &scalar.values {
                put(&mut out, (*value as f32).to_bits());
            }
        }
        if p.short {
            for t in &p.mesh.triangles {
                for index in t {
                    out.extend_from_slice(&(*index as u16).to_le_bytes());
                }
            }
            if p.mesh.triangles.len() % 2 != 0 {
                out.extend_from_slice(&[0, 0]);
            }
        } else {
            for t in &p.mesh.triangles {
                for index in t {
                    put(&mut out, *index);
                }
            }
        }
    }
    w.write_all(&out)?;
    Ok(w.flush()?)
}
