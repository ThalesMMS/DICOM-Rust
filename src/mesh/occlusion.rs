use super::bvh::{TriangleBvh, scale3};
use super::{Mesh, MeshError, add3, dot3, face_normals, sub3};
use crate::fmath;
use crate::textfmt::float_v;
use std::f64::consts::TAU;
pub const DEFAULT_OCCLUSION_RAYS: i64 = 32;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OcclusionOptions {
    pub rays: i64,
    pub radius: f64,
    pub workers: i64,
}
pub fn ambient_occlusion(m: &Mesh, opts: &OcclusionOptions) -> Result<Vec<f32>, MeshError> {
    m.validate()?;
    let rays = if opts.rays == 0 {
        DEFAULT_OCCLUSION_RAYS
    } else {
        opts.rays
    };
    if !(1..=1024).contains(&rays)
        || opts.radius.is_nan()
        || opts.radius < 0.0
        || opts.radius == f64::INFINITY
    {
        return Err(MeshError::invalid(format!(
            "occlusion rays {}, radius {}",
            opts.rays,
            float_v(opts.radius)
        )));
    }
    let n = m.vertices.len();
    let mut out = vec![1f32; n];
    if n == 0 || m.triangles.is_empty() {
        return Ok(out);
    }
    let computed;
    let normals: &[[f32; 3]] = match &m.normals {
        Some(normals) => normals,
        None => {
            let mut copy = Mesh {
                vertices: m.vertices.clone(),
                normals: None,
                triangles: m.triangles.clone(),
            };
            face_normals(&mut copy, None);
            computed = copy.normals.unwrap_or_default();
            &computed
        }
    };
    let tree = TriangleBvh::new(m);
    let mut radius = opts.radius;
    if radius == 0.0 {
        let diagonal = sub3(tree.nodes[0].hi, tree.nodes[0].lo);
        radius = 0.1 * dot3(diagonal, diagonal).sqrt();
    }
    if radius.is_nan() || radius <= 0.0 {
        return Ok(out);
    }
    let directions = hemisphere_spiral(rays as usize);
    for v in 0..n {
        let normal = [
            f64::from(normals[v][0]),
            f64::from(normals[v][1]),
            f64::from(normals[v][2]),
        ];
        if dot3(normal, normal) < 0.5 {
            continue;
        }
        let p = m.point(v as u32);
        let (tangent, bitangent) = orthonormal_frame(normal);
        let angle = TAU * f64::from(hash_index(v as u32)) / 4_294_967_296.0;
        let (c, s) = (fmath::cos(angle), fmath::sin(angle));
        let (tangent, bitangent) = (
            add3(scale3(tangent, c), scale3(bitangent, s)),
            sub3(scale3(bitangent, c), scale3(tangent, s)),
        );
        let mut obscured = 0.0;
        for d in &directions {
            let dir = add3(
                add3(scale3(tangent, d[0]), scale3(bitangent, d[1])),
                scale3(normal, d[2]),
            );
            if let Some((t, _)) = tree.closest_hit(m, p, dir, radius, v as u32, 0.0) {
                obscured += 1.0 - t / radius;
            }
        }
        out[v] = (1.0 - obscured / directions.len() as f64) as f32;
    }
    Ok(out)
}
fn hash_index(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}
fn hemisphere_spiral(n: usize) -> Vec<[f64; 3]> {
    let golden = (5f64.sqrt() - 1.0) / 2.0;
    (0..n)
        .map(|i| {
            let u = (i as f64 + 0.5) / n as f64;
            let phi = TAU * ((i as f64 * golden) % 1.0);
            let r = u.sqrt();
            [r * fmath::cos(phi), r * fmath::sin(phi), (1.0 - u).sqrt()]
        })
        .collect()
}
fn orthonormal_frame(n: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let sign = 1f64.copysign(n[2]);
    let a = -1.0 / (sign + n[2]);
    let c = n[0] * n[1] * a;
    (
        [1.0 + sign * n[0] * n[0] * a, sign * c, -sign * n[0]],
        [c, sign + n[1] * n[1] * a, -n[1]],
    )
}
