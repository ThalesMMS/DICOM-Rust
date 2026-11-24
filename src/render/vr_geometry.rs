#![forbid(unsafe_code)]
use super::error::RenderError;
use super::parallel::{Cancellation, ctx_err};
use super::vec3::Vec3;
use super::vr_camera::VrCamera;
use super::vr_raycast::RayGenerator;
use super::vr_scalpel::Projector;
use super::vr_transfer::{Rgba, VrLightingMaterial};
use super::vr_volume::headlight_shading;
use crate::fmath;
type Ctx<'a> = Option<&'a dyn Cancellation>;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VrGeometryMesh {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub normals: Vec<Vec3>,
    pub occlusion: Vec<f32>,
    pub color: Rgba,
    pub colors: Vec<[f32; 3]>,
    pub opacity: f64,
}
impl VrGeometryMesh {
    pub fn validate_vertex_colors(&self, ctx: Ctx<'_>) -> Result<(), RenderError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        if self.colors.is_empty() {
            return Ok(());
        }
        if self.colors.len() != self.vertices.len() {
            return Err(RenderError::other(
                "render: vertex color count differs from geometry",
            ));
        }
        for (i, rgb) in self.colors.iter().enumerate() {
            if i & 255 == 0
                && let Some(e) = ctx_err(ctx)
            {
                return Err(e);
            }
            for v in rgb {
                if !v.is_finite() || *v < 0.0 || *v > 1.0 {
                    return Err(RenderError::other(
                        "render: vertex color must be finite linear RGB in [0,1]",
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn translucent(&self) -> bool {
        self.opacity > 0.0 && self.opacity < 1.0
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VrGeometry {
    pub meshes: Vec<VrGeometryMesh>,
}
impl VrGeometry {
    pub fn empty(&self) -> bool {
        !self
            .meshes
            .iter()
            .any(|m| !m.triangles.is_empty() && !m.vertices.is_empty())
    }
    pub fn translucent(&self) -> bool {
        self.meshes
            .iter()
            .any(|m| m.translucent() && !m.triangles.is_empty() && !m.vertices.is_empty())
    }
    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(|m| m.triangles.len()).sum()
    }
}
pub fn geometry_empty(g: Option<&VrGeometry>) -> bool {
    g.is_none_or(VrGeometry::empty)
}
pub fn vr_tube_mesh(points: &[Vec3], radius: f64, sides: i64, color: Rgba) -> VrGeometryMesh {
    let mut mesh = VrGeometryMesh {
        color,
        ..Default::default()
    };
    let sides = sides.max(3);
    let mut path: Vec<Vec3> = Vec::new();
    for p in points {
        if path.last().is_none_or(|last| p.sub(*last).length() > 1e-9) {
            path.push(*p);
        }
    }
    if path.len() < 2 || !(radius > 0.0) {
        return mesh;
    }
    let n = path.len();
    let tangent = |i: usize| -> Vec3 {
        if i == 0 {
            path[1].sub(path[0]).normalize()
        } else if i == n - 1 {
            path[i].sub(path[i - 1]).normalize()
        } else {
            path[i + 1].sub(path[i - 1]).normalize()
        }
    };
    let t = tangent(0);
    let mut normal = t.cross(Vec3::new(1.0, 0.0, 0.0));
    if normal.length() < 0.1 {
        normal = t.cross(Vec3::new(0.0, 1.0, 0.0));
    }
    normal = normal.normalize();
    for (i, point) in path.iter().enumerate() {
        let next = tangent(i);
        normal = normal.sub(next.scale(normal.dot(next)));
        if normal.length() < 1e-9 {
            normal = next.cross(Vec3::new(0.0, 0.0, 1.0));
            if normal.length() < 0.1 {
                normal = next.cross(Vec3::new(0.0, 1.0, 0.0));
            }
        }
        normal = normal.normalize();
        let binormal = next.cross(normal);
        for s in 0..sides {
            let angle = 2.0 * std::f64::consts::PI * s as f64 / sides as f64;
            let radial = normal
                .scale(fmath::cos(angle))
                .add(binormal.scale(fmath::sin(angle)));
            mesh.vertices.push(point.add(radial.scale(radius)));
            mesh.normals.push(radial);
        }
    }
    let sides = sides as usize;
    for i in 0..n - 1 {
        for s in 0..sides {
            let a = (i * sides + s) as u32;
            let b = (i * sides + (s + 1) % sides) as u32;
            let c = a.wrapping_add(sides as u32);
            let d = b.wrapping_add(sides as u32);
            mesh.triangles.push([a, b, d]);
            mesh.triangles.push([a, d, c]);
        }
    }
    mesh
}
pub fn vr_sphere_mesh(center: Vec3, radius: f64, color: Rgba) -> VrGeometryMesh {
    const RINGS: u32 = 8;
    const SEGMENTS: u32 = 16;
    let mut mesh = VrGeometryMesh {
        color,
        ..Default::default()
    };
    if !(radius > 0.0) {
        return mesh;
    }
    for r in 0..=RINGS {
        let theta = std::f64::consts::PI * f64::from(r) / f64::from(RINGS);
        for s in 0..SEGMENTS {
            let phi = 2.0 * std::f64::consts::PI * f64::from(s) / f64::from(SEGMENTS);
            let (st, ct) = (fmath::sin(theta), fmath::cos(theta));
            let n = Vec3::new(st * fmath::cos(phi), st * fmath::sin(phi), ct);
            mesh.vertices.push(center.add(n.scale(radius)));
            mesh.normals.push(n);
        }
    }
    for r in 0..RINGS {
        for s in 0..SEGMENTS {
            let a = r * SEGMENTS + s;
            let b = r * SEGMENTS + (s + 1) % SEGMENTS;
            let c = a + SEGMENTS;
            let d = b + SEGMENTS;
            mesh.triangles.push([a, c, d]);
            mesh.triangles.push([a, d, b]);
        }
    }
    mesh
}
pub const VR_MAX_TRANSLUCENT_FRAGMENTS: usize = 4;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrFragment {
    pub t: f64,
    pub color: Rgba,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VrFragmentBuffer {
    pub count: Vec<u8>,
    pub t: Vec<f32>,
    pub rgba: Vec<[f32; 4]>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VrDepthImage {
    pub width: usize,
    pub height: usize,
    pub t: Vec<f32>,
    pub r: Vec<f32>,
    pub g: Vec<f32>,
    pub b: Vec<f32>,
    pub fragments: Option<VrFragmentBuffer>,
}
impl VrDepthImage {
    fn inside(&self, x: i64, y: i64) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        Some(y as usize * self.width + x as usize)
    }
    #[doc(hidden)]
    pub fn fragments_into(&self, x: i64, y: i64, dst: &mut Vec<VrFragment>) {
        let (Some(i), Some(buffer)) = (self.inside(x, y), self.fragments.as_ref()) else {
            return;
        };
        let base = i * VR_MAX_TRANSLUCENT_FRAGMENTS;
        for k in 0..usize::from(buffer.count[i]) {
            let c = buffer.rgba[base + k];
            dst.push(VrFragment {
                t: f64::from(buffer.t[base + k]),
                color: Rgba {
                    r: f64::from(c[0]),
                    g: f64::from(c[1]),
                    b: f64::from(c[2]),
                    a: f64::from(c[3]),
                },
            });
        }
    }
    pub fn has_fragments(&self) -> bool {
        self.fragments
            .as_ref()
            .is_some_and(|b| b.count.iter().any(|n| *n > 0))
    }
    pub fn at(&self, x: i64, y: i64) -> Option<(f64, Rgba)> {
        let i = self.inside(x, y)?;
        if self.t[i] == f32::INFINITY {
            return None;
        }
        Some((
            f64::from(self.t[i]),
            Rgba {
                r: f64::from(self.r[i]),
                g: f64::from(self.g[i]),
                b: f64::from(self.b[i]),
                a: 1.0,
            },
        ))
    }
    fn insert_fragment(&mut self, i: usize, t: f32, c: [f32; 4]) {
        const K: usize = VR_MAX_TRANSLUCENT_FRAGMENTS;
        let Some(buffer) = self.fragments.as_mut() else {
            return;
        };
        let base = i * K;
        let n = usize::from(buffer.count[i]);
        let ts = &mut buffer.t[base..base + K];
        let cs = &mut buffer.rgba[base..base + K];
        let mut at = n;
        while at > 0 && ts[at - 1] > t {
            at -= 1;
        }
        if n < K {
            ts.copy_within(at..n, at + 1);
            cs.copy_within(at..n, at + 1);
            ts[at] = t;
            cs[at] = c;
            buffer.count[i] += 1;
            return;
        }
        if at == K {
            cs[K - 1] = blend_under(cs[K - 1], c);
            return;
        }
        let evicted = cs[K - 1];
        ts.copy_within(at..K - 1, at + 1);
        cs.copy_within(at..K - 1, at + 1);
        ts[at] = t;
        cs[at] = c;
        cs[K - 1] = blend_under(cs[K - 1], evicted);
    }
}
pub(crate) fn depth_fragments(depth: Option<&VrDepthImage>, x: i64, y: i64) -> Vec<VrFragment> {
    let mut out = Vec::new();
    if let Some(d) = depth {
        d.fragments_into(x, y, &mut out);
    }
    out
}
pub(crate) fn depth_at(depth: Option<&VrDepthImage>, x: i64, y: i64) -> Option<(f64, Rgba)> {
    depth.and_then(|d| d.at(x, y))
}
fn blend_under(front: [f32; 4], back: [f32; 4]) -> [f32; 4] {
    let inv = 1.0 - front[3];
    [
        front[0] + inv * back[0],
        front[1] + inv * back[1],
        front[2] + inv * back[2],
        front[3] + inv * back[3],
    ]
}
pub(crate) fn composite_vr_fragments(fragments: &[VrFragment], base: Rgba) -> Rgba {
    let mut out = Rgba::default();
    for f in fragments {
        let inv = 1.0 - out.a;
        out.r += inv * f.color.r;
        out.g += inv * f.color.g;
        out.b += inv * f.color.b;
        out.a += inv * f.color.a;
    }
    let inv = 1.0 - out.a;
    out.r += inv * base.r;
    out.g += inv * base.g;
    out.b += inv * base.b;
    out.a += inv * base.a;
    out
}
pub(crate) fn composite_fragments_before(
    fragments: &mut &[VrFragment],
    t: f64,
    acc: (f64, f64, f64, f64),
) -> (f64, f64, f64, f64) {
    let (mut r, mut g, mut b, mut a) = acc;
    while let Some(f) = fragments.first()
        && f.t < t
    {
        let inv = 1.0 - a;
        r += inv * f.color.r;
        g += inv * f.color.g;
        b += inv * f.color.b;
        a += inv * f.color.a;
        *fragments = &fragments[1..];
    }
    (r, g, b, a)
}
const VR_GEOMETRY_MATERIAL: VrLightingMaterial = VrLightingMaterial {
    ambient: 0.25,
    diffuse: 0.75,
    specular: 0.25,
    specular_power: 24.0,
};
const VR_RASTER_BAND: i64 = 16;
struct ScreenTriangle {
    mesh: usize,
    translucent: bool,
    index: [u32; 3],
    x: [f64; 3],
    y: [f64; 3],
    min_x: i64,
    max_x: i64,
    min_y: i64,
    max_y: i64,
    normal: Vec3,
}
fn min3(a: f64, b: f64, c: f64) -> f64 {
    fmath::min(fmath::min(a, b), c)
}
fn max3(a: f64, b: f64, c: f64) -> f64 {
    fmath::max(fmath::max(a, b), c)
}
pub fn rasterize_vr_geometry(
    ctx: Ctx<'_>,
    cam: &VrCamera,
    w: i64,
    h: i64,
    geometry: Option<&VrGeometry>,
) -> (Option<VrDepthImage>, Option<RenderError>) {
    if w <= 0 || h <= 0 {
        return (None, None);
    }
    if let Some(g) = geometry {
        for mesh in &g.meshes {
            if let Err(e) = mesh.validate_vertex_colors(ctx) {
                return (None, Some(e));
            }
        }
    }
    let n = (w * h) as usize;
    let mut depth = VrDepthImage {
        width: w as usize,
        height: h as usize,
        t: vec![f32::INFINITY; n],
        r: vec![0.0; n],
        g: vec![0.0; n],
        b: vec![0.0; n],
        fragments: None,
    };
    let Some(geometry) = geometry.filter(|g| !g.empty()) else {
        return (Some(depth), None);
    };
    let projector = Projector::new(cam, w, h);
    let mut triangles = Vec::new();
    let mut translucent = false;
    for (m, mesh) in geometry.meshes.iter().enumerate() {
        let nv = mesh.vertices.len();
        for tri in &mesh.triangles {
            if tri.iter().any(|k| *k as usize >= nv) {
                continue;
            }
            let mut screen = ScreenTriangle {
                mesh: m,
                translucent: mesh.translucent(),
                index: *tri,
                x: [0.0; 3],
                y: [0.0; 3],
                min_x: 0,
                max_x: 0,
                min_y: 0,
                max_y: 0,
                normal: Vec3::ZERO,
            };
            let mut visible = true;
            for k in 0..3 {
                match projector.project(mesh.vertices[tri[k] as usize]) {
                    Some((x, y)) if !x.is_nan() && !y.is_nan() => {
                        screen.x[k] = x;
                        screen.y[k] = y;
                    }
                    _ => {
                        visible = false;
                        break;
                    }
                }
            }
            if !visible {
                continue;
            }
            let v0 = mesh.vertices[tri[0] as usize];
            let v1 = mesh.vertices[tri[1] as usize];
            let v2 = mesh.vertices[tri[2] as usize];
            screen.normal = v1.sub(v0).cross(v2.sub(v0));
            if screen.normal.length() < 1e-18 {
                continue;
            }
            let (x, y) = (screen.x, screen.y);
            screen.min_x = fmath::f64_to_i64((min3(x[0], x[1], x[2]) - 0.5).ceil()).max(0);
            screen.max_x = fmath::f64_to_i64((max3(x[0], x[1], x[2]) - 0.5).floor()).min(w - 1);
            screen.min_y = fmath::f64_to_i64((min3(y[0], y[1], y[2]) - 0.5).ceil()).max(0);
            screen.max_y = fmath::f64_to_i64((max3(y[0], y[1], y[2]) - 0.5).floor()).min(h - 1);
            if screen.min_x > screen.max_x || screen.min_y > screen.max_y {
                continue;
            }
            translucent = translucent || screen.translucent;
            triangles.push(screen);
        }
    }
    if translucent {
        depth.fragments = Some(VrFragmentBuffer {
            count: vec![0; n],
            t: vec![0.0; n * VR_MAX_TRANSLUCENT_FRAGMENTS],
            rgba: vec![[0.0; 4]; n * VR_MAX_TRANSLUCENT_FRAGMENTS],
        });
    }
    let ray_gen = RayGenerator::new(cam, w, h);
    let bands = (h + VR_RASTER_BAND - 1) / VR_RASTER_BAND;
    for pass in [false, true] {
        if pass && !translucent {
            break;
        }
        for band in 0..bands {
            if let Some(e) = ctx_err(ctx) {
                return (Some(depth), Some(e));
            }
            let top = band * VR_RASTER_BAND;
            let bottom = (h - 1).min(band * VR_RASTER_BAND + VR_RASTER_BAND - 1);
            for tri in &triangles {
                if tri.translucent == pass && tri.max_y >= top && tri.min_y <= bottom {
                    rasterize_vr_triangle(
                        tri,
                        &geometry.meshes[tri.mesh],
                        &mut depth,
                        &ray_gen,
                        top.max(tri.min_y),
                        bottom.min(tri.max_y),
                    );
                }
            }
        }
        if let Some(e) = ctx_err(ctx) {
            return (Some(depth), Some(e));
        }
    }
    (Some(depth), None)
}
fn rasterize_vr_triangle(
    tri: &ScreenTriangle,
    mesh: &VrGeometryMesh,
    depth: &mut VrDepthImage,
    ray_gen: &RayGenerator,
    top: i64,
    bottom: i64,
) {
    let w = depth.width;
    let (tx, ty) = (tri.x, tri.y);
    let area = (tx[1] - tx[0]) * (ty[2] - ty[0]) - (tx[2] - tx[0]) * (ty[1] - ty[0]);
    if area.abs() < 1e-12 {
        return;
    }
    let sign = if area < 0.0 { -1.0 } else { 1.0 };
    let owns = |k: usize| -> bool {
        let dx = sign * (tx[(k + 2) % 3] - tx[(k + 1) % 3]);
        let dy = sign * (ty[(k + 2) % 3] - ty[(k + 1) % 3]);
        dy > 0.0 || dy == 0.0 && dx > 0.0
    };
    let v0 = mesh.vertices[tri.index[0] as usize];
    let per_vertex_normals = mesh.normals.len() == mesh.vertices.len();
    let per_vertex_occlusion = mesh.occlusion.len() == mesh.vertices.len();
    let per_vertex_colors = mesh.colors.len() == mesh.vertices.len();
    for py in top..=bottom {
        let row = ray_gen.row_base(py);
        let cy = py as f64 + 0.5;
        for px in tri.min_x..=tri.max_x {
            let cx = px as f64 + 0.5;
            let e = [
                (tx[1] - cx) * (ty[2] - cy) - (tx[2] - cx) * (ty[1] - cy),
                (tx[2] - cx) * (ty[0] - cy) - (tx[0] - cx) * (ty[2] - cy),
                (tx[0] - cx) * (ty[1] - cy) - (tx[1] - cx) * (ty[0] - cy),
            ];
            let b0 = e[0] / area;
            let b1 = e[1] / area;
            let b2 = 1.0 - b0 - b1;
            if tri.translucent {
                let mut inside = true;
                for (k, ek) in e.iter().enumerate() {
                    let v = sign * ek;
                    inside = v > 0.0 || v == 0.0 && owns(k);
                    if !inside {
                        break;
                    }
                }
                if !inside {
                    continue;
                }
            } else {
                const EDGE: f64 = -1e-9;
                if b0 < EDGE || b1 < EDGE || b2 < EDGE {
                    continue;
                }
            }
            let (o, d) = ray_gen.ray(px, &row);
            let denominator = d.dot(tri.normal);
            if denominator.abs() < 1e-18 {
                continue;
            }
            let t = v0.sub(o).dot(tri.normal) / denominator;
            let index = py as usize * w + px as usize;
            if !(t > 0.0) || (t as f32) >= depth.t[index] {
                continue;
            }
            let mut normal = tri.normal;
            if per_vertex_normals {
                normal = mesh.normals[tri.index[0] as usize]
                    .scale(b0)
                    .add(mesh.normals[tri.index[1] as usize].scale(b1))
                    .add(mesh.normals[tri.index[2] as usize].scale(b2));
            }
            let (mut lit, mut specular) = (1.0, 0.0);
            if normal.length() > 1e-12 {
                (lit, specular) =
                    headlight_shading(normal.normalize(), d.scale(-1.0), &VR_GEOMETRY_MATERIAL);
            }
            if per_vertex_occlusion {
                let occ = &mesh.occlusion;
                let mut o = b0 * f64::from(occ[tri.index[0] as usize])
                    + b1 * f64::from(occ[tri.index[1] as usize])
                    + b2 * f64::from(occ[tri.index[2] as usize]);
                o = fmath::max(0.0, fmath::min(1.0, o));
                lit *= o;
                specular *= o;
            }
            let mut base = mesh.color;
            if per_vertex_colors {
                let weights = [b0, b1, b2];
                base = Rgba {
                    a: 1.0,
                    ..Default::default()
                };
                for (k, vi) in tri.index.iter().enumerate() {
                    let c = mesh.colors[*vi as usize];
                    base.r += weights[k] * f64::from(c[0]);
                    base.g += weights[k] * f64::from(c[1]);
                    base.b += weights[k] * f64::from(c[2]);
                }
            }
            let r = base.r * lit + specular;
            let g = base.g * lit + specular;
            let b = base.b * lit + specular;
            if tri.translucent {
                let a = mesh.opacity;
                depth.insert_fragment(
                    index,
                    t as f32,
                    [(r * a) as f32, (g * a) as f32, (b * a) as f32, a as f32],
                );
                continue;
            }
            depth.t[index] = t as f32;
            depth.r[index] = r as f32;
            depth.g[index] = g as f32;
            depth.b[index] = b as f32;
        }
    }
}
pub(crate) fn clip_geometry_depth(
    ctx: Ctx<'_>,
    clip: Option<&super::vr_clip::VrClip>,
    cam: &VrCamera,
    w: i64,
    h: i64,
) -> Result<Option<VrDepthImage>, RenderError> {
    let Some(geometry) = clip.and_then(|c| c.geometry()) else {
        return Ok(None);
    };
    if geometry.empty() {
        return Ok(None);
    }
    match rasterize_vr_geometry(ctx, cam, w, h, Some(geometry)) {
        (_, Some(e)) => Err(e),
        (depth, None) => Ok(depth),
    }
}
