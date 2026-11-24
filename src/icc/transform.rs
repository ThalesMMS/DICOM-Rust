use super::{
    Curve, IccError, Profile, be16, invalid, parse_curve, printable, s15_fixed16, unsupported,
};
use crate::fmath;
use crate::textfmt::quote;
use std::collections::HashMap;
pub const D50: [f64; 3] = [0.9642, 1.0, 0.8249];
const PERCEPTUAL_BLACK: [f64; 3] = [0.00336, 0.0034731, 0.00287];
const XYZ_D50_TO_LINEAR_SRGB: [[f64; 3]; 3] = [
    [3.1338561, -1.6168667, -0.4906146],
    [-0.9787684, 1.9161415, 0.0334540],
    [0.0719453, -0.2289914, 1.4052427],
];
const SRGB_COLORANTS: [[f64; 3]; 3] = [
    [0.4360747, 0.3850649, 0.1430804],
    [0.2225045, 0.7168786, 0.0606169],
    [0.0139322, 0.0971045, 0.7141733],
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcsEncoding {
    Legacy8,
    Legacy16,
    V4,
}
#[derive(Debug, Clone)]
enum Stage {
    Curves(Vec<Curve>),
    Clut(Clut),
    Matrix([f64; 12]),
}
impl Stage {
    fn eval(&self, v: &[f64]) -> Vec<f64> {
        match self {
            Stage::Curves(curves) => (0..v.len()).map(|i| curves[i].eval(v[i])).collect(),
            Stage::Clut(c) => c.eval(v),
            Stage::Matrix(e) => vec![
                e[0] * v[0] + e[1] * v[1] + e[2] * v[2] + e[9],
                e[3] * v[0] + e[4] * v[1] + e[5] * v[2] + e[10],
                e[6] * v[0] + e[7] * v[1] + e[8] * v[2] + e[11],
            ],
        }
    }
}
#[derive(Debug, Clone)]
enum ToPcs {
    Matrix,
    Lut {
        stages: Vec<Stage>,
        pcs_xyz: bool,
        encoding: PcsEncoding,
        black_point: bool,
    },
}
#[derive(Debug, Clone)]
pub struct Transform {
    to_pcs: ToPcs,
    identity: bool,
    pub source: &'static str,
    trc: [Curve; 3],
    matrix: [[f64; 3]; 3],
}
impl Transform {
    pub fn new_srgb(p: &Profile) -> Result<Transform, IccError> {
        if &p.color_space != b"RGB " {
            return Err(unsupported(format!(
                "data colour space {}",
                quote(&printable(&p.color_space))
            )));
        }
        match &p.class {
            b"mntr" | b"scnr" | b"spac" => {}
            other => {
                return Err(unsupported(format!(
                    "profile class {}",
                    quote(&printable(other))
                )));
            }
        }
        let identity_curves = [Curve::Identity, Curve::Identity, Curve::Identity];
        if p.has(b"A2B0") {
            let (stages, encoding) = lut_stages(p, "A2B0")?;
            return Ok(Transform {
                to_pcs: ToPcs::Lut {
                    stages,
                    pcs_xyz: &p.pcs == b"XYZ ",
                    encoding,
                    black_point: p.version >> 24 >= 4,
                },
                identity: false,
                source: "AToB0",
                trc: identity_curves,
                matrix: [[0.0; 3]; 3],
            });
        }
        let mut t = Transform {
            to_pcs: ToPcs::Matrix,
            identity: false,
            source: "matrix/TRC",
            trc: identity_curves,
            matrix: [[0.0; 3]; 3],
        };
        for (i, name) in ["rTRC", "gTRC", "bTRC"].into_iter().enumerate() {
            let data = p.tag(name, &[b"curv", b"para"])?;
            t.trc[i] = parse_curve(data)?.0;
        }
        for (column, name) in ["rXYZ", "gXYZ", "bXYZ"].into_iter().enumerate() {
            let data = p.tag(name, &[b"XYZ "])?;
            if data.len() < 20 {
                return Err(invalid(format!("truncated {name}")));
            }
            for row in 0..3 {
                t.matrix[row][column] = s15_fixed16(&data[8 + 4 * row..]);
            }
        }
        if &p.pcs != b"XYZ " {
            return Err(invalid(format!(
                "matrix/TRC profile with a {} connection space",
                quote(&printable(&p.pcs))
            )));
        }
        t.identity = t.is_srgb_like();
        if t.identity {
            t.source = "sRGB";
        }
        Ok(t)
    }
    pub fn identity(&self) -> bool {
        self.identity
    }
    fn to_pcs(&self, input: [f64; 3]) -> [f64; 3] {
        match &self.to_pcs {
            ToPcs::Matrix => {
                let lin = [
                    self.trc[0].eval(input[0]),
                    self.trc[1].eval(input[1]),
                    self.trc[2].eval(input[2]),
                ];
                mul3(&self.matrix, lin)
            }
            ToPcs::Lut {
                stages,
                pcs_xyz,
                encoding,
                black_point,
            } => {
                let mut v = input.to_vec();
                for stage in stages {
                    v = stage.eval(&v);
                }
                let mut xyz = decode_pcs(*pcs_xyz, *encoding, [v[0], v[1], v[2]]);
                if *black_point {
                    for i in 0..3 {
                        xyz[i] = D50[i] * (xyz[i] - PERCEPTUAL_BLACK[i])
                            / (D50[i] - PERCEPTUAL_BLACK[i]);
                    }
                }
                xyz
            }
        }
    }
    pub fn convert(&self, r: f64, g: f64, b: f64) -> (f64, f64, f64) {
        if self.identity {
            return (r, g, b);
        }
        let xyz = self.to_pcs([r, g, b]);
        let lin = mul3(&XYZ_D50_TO_LINEAR_SRGB, xyz);
        (
            encode_srgb(lin[0]),
            encode_srgb(lin[1]),
            encode_srgb(lin[2]),
        )
    }
    #[doc(hidden)]
    pub fn apply_rgba8(&self, pix: &mut [u8], stride: usize, width: usize, height: usize) {
        if self.identity {
            return;
        }
        let table = matches!(self.to_pcs, ToPcs::Matrix).then(|| {
            let mut table = [[0.0f64; 256]; 3];
            for (c, row) in table.iter_mut().enumerate() {
                for (v, slot) in row.iter_mut().enumerate() {
                    *slot = self.trc[c].eval(v as f64 / 255.0);
                }
            }
            table
        });
        let mut cache: HashMap<u32, [u8; 3]> = HashMap::new();
        for y in 0..height {
            let row = &mut pix[y * stride..y * stride + width * 4];
            let mut i = 0;
            while i + 3 < row.len() {
                let key =
                    u32::from(row[i]) << 16 | u32::from(row[i + 1]) << 8 | u32::from(row[i + 2]);
                let out = match cache.get(&key) {
                    Some(out) => *out,
                    None => {
                        let xyz = match &table {
                            Some(t) => mul3(
                                &self.matrix,
                                [
                                    t[0][row[i] as usize],
                                    t[1][row[i + 1] as usize],
                                    t[2][row[i + 2] as usize],
                                ],
                            ),
                            None => self.to_pcs([
                                f64::from(row[i]) / 255.0,
                                f64::from(row[i + 1]) / 255.0,
                                f64::from(row[i + 2]) / 255.0,
                            ]),
                        };
                        let lin = mul3(&XYZ_D50_TO_LINEAR_SRGB, xyz);
                        let mut out = [0u8; 3];
                        for c in 0..3 {
                            out[c] = (encode_srgb(lin[c]) * 255.0).round() as u8;
                        }
                        if cache.len() < 1 << 20 {
                            cache.insert(key, out);
                        }
                        out
                    }
                };
                row[i..i + 3].copy_from_slice(&out);
                i += 4;
            }
        }
    }
    #[doc(hidden)]
    pub fn apply_rgba64(&self, pix: &mut [u8], stride: usize, width: usize, height: usize) {
        if self.identity {
            return;
        }
        for y in 0..height {
            let row = &mut pix[y * stride..y * stride + width * 8];
            let mut i = 0;
            while i + 7 < row.len() {
                let input: [f64; 3] =
                    std::array::from_fn(|c| f64::from(be16(&row[i + 2 * c..])) / 65535.0);
                let (r, g, b) = self.convert(input[0], input[1], input[2]);
                for (c, v) in [r, g, b].into_iter().enumerate() {
                    let q = (v * 65535.0).round() as u16;
                    row[i + 2 * c..i + 2 * c + 2].copy_from_slice(&q.to_be_bytes());
                }
                i += 8;
            }
        }
    }
    fn is_srgb_like(&self) -> bool {
        for r in 0..3 {
            for c in 0..3 {
                if (self.matrix[r][c] - SRGB_COLORANTS[r][c]).abs() > 1e-3 {
                    return false;
                }
            }
        }
        for trc in &self.trc {
            for i in 0..=64 {
                let x = f64::from(i) / 64.0;
                if (trc.eval(x) - decode_srgb(x)).abs() > 1e-3 {
                    return false;
                }
            }
        }
        true
    }
}
fn mul3(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}
const INV_2_4: f64 = 0.4166666666666667;
fn encode_srgb(v: f64) -> f64 {
    let v = super::clamp01(v);
    if v <= 0.0031308 {
        return 12.92 * v;
    }
    1.055 * fmath::pow(v, INV_2_4) - 0.055
}
fn decode_srgb(v: f64) -> f64 {
    if v <= 0.04045 {
        return v / 12.92;
    }
    fmath::pow((v + 0.055) / 1.055, 2.4)
}
const LAB_LINEAR: f64 = 0.12841854934601665;
fn lab_to_xyz(l: f64, a: f64, b: f64) -> [f64; 3] {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let inv = |t: f64| {
        if t > 6.0 / 29.0 {
            t * t * t
        } else {
            LAB_LINEAR * (t - 4.0 / 29.0)
        }
    };
    [D50[0] * inv(fx), D50[1] * inv(fy), D50[2] * inv(fz)]
}
fn decode_pcs(pcs_xyz: bool, encoding: PcsEncoding, n: [f64; 3]) -> [f64; 3] {
    if pcs_xyz {
        let scale = 65535.0 / 32768.0;
        return [n[0] * scale, n[1] * scale, n[2] * scale];
    }
    let mut scale = 1.0;
    if encoding == PcsEncoding::Legacy16 {
        scale = 65535.0 / 65280.0;
    }
    lab_to_xyz(
        n[0] * scale * 100.0,
        n[1] * scale * 255.0 - 128.0,
        n[2] * scale * 255.0 - 128.0,
    )
}
fn lut_stages(p: &Profile, signature: &str) -> Result<(Vec<Stage>, PcsEncoding), IccError> {
    let data = p.tag(signature, &[b"mft1", b"mft2", b"mAB "])?;
    match &data[0..4] {
        b"mft1" => Ok((parse_legacy_lut(data, false)?, PcsEncoding::Legacy8)),
        b"mft2" => Ok((parse_legacy_lut(data, true)?, PcsEncoding::Legacy16)),
        _ => Ok((parse_lut_a_to_b(data)?, PcsEncoding::V4)),
    }
}
fn parse_legacy_lut(data: &[u8], wide: bool) -> Result<Vec<Stage>, IccError> {
    if data.len() < 52 {
        return Err(invalid("truncated lut"));
    }
    let (inputs, outputs, grid) = (data[8] as usize, data[9] as usize, data[10] as usize);
    if inputs != 3 || outputs != 3 {
        return Err(unsupported(format!(
            "lut with {inputs} inputs and {outputs} outputs"
        )));
    }
    if grid < 2 {
        return Err(invalid(format!("lut grid of {grid} points")));
    }
    let (mut in_entries, mut out_entries, mut at, mut sample) =
        (256usize, 256usize, 48usize, 1usize);
    if wide {
        in_entries = be16(&data[48..50]) as usize;
        out_entries = be16(&data[50..52]) as usize;
        at = 52;
        sample = 2;
        if !(2..=4096).contains(&in_entries) || !(2..=4096).contains(&out_entries) {
            return Err(invalid(format!(
                "lut16 table sizes {in_entries} and {out_entries}"
            )));
        }
    }
    let clut_points = grid * grid * grid;
    let need = at + sample * (inputs * in_entries + clut_points * outputs + outputs * out_entries);
    if need > data.len() {
        return Err(invalid(format!("lut of {} bytes needs {need}", data.len())));
    }
    let mut read = |count: usize| -> Vec<f64> {
        let values = (0..count)
            .map(|i| {
                if wide {
                    f64::from(be16(&data[at + 2 * i..])) / 65535.0
                } else {
                    f64::from(data[at + i]) / 255.0
                }
            })
            .collect();
        at += count * sample;
        values
    };
    let in_curves: Vec<Curve> = (0..inputs)
        .map(|_| Curve::Table(read(in_entries)))
        .collect();
    let clut = Clut {
        dims: vec![grid; 3],
        out: outputs,
        values: read(clut_points * outputs),
    };
    let out_curves: Vec<Curve> = (0..outputs)
        .map(|_| Curve::Table(read(out_entries)))
        .collect();
    Ok(vec![
        Stage::Curves(in_curves),
        Stage::Clut(clut),
        Stage::Curves(out_curves),
    ])
}
fn parse_lut_a_to_b(data: &[u8]) -> Result<Vec<Stage>, IccError> {
    if data.len() < 32 {
        return Err(invalid("truncated lutAToB"));
    }
    let (inputs, outputs) = (data[8] as usize, data[9] as usize);
    if inputs != 3 || outputs != 3 {
        return Err(unsupported(format!(
            "lutAToB with {inputs} inputs and {outputs} outputs"
        )));
    }
    let mut offsets = [0usize; 5];
    for (i, slot) in offsets.iter_mut().enumerate() {
        let o = super::be32(&data[12 + 4 * i..]) as usize;
        if o != 0 && (o < 32 || o >= data.len()) {
            return Err(invalid(format!("lutAToB element offset {o}")));
        }
        *slot = o;
    }
    let curves = |mut at: usize, count: usize| -> Result<Vec<Curve>, IccError> {
        let mut list = Vec::with_capacity(count);
        for _ in 0..count {
            let (c, size) = parse_curve(&data[at..])?;
            list.push(c);
            at += size;
            if at > data.len() {
                return Err(invalid("truncated lutAToB curves"));
            }
        }
        Ok(list)
    };
    if offsets[0] == 0 {
        return Err(invalid("lutAToB without B curves"));
    }
    let mut stages = Vec::new();
    if offsets[4] != 0 {
        stages.push(Stage::Curves(curves(offsets[4], inputs)?));
    }
    let o = offsets[3];
    if o != 0 {
        if o + 20 > data.len() {
            return Err(invalid("truncated lutAToB CLUT"));
        }
        let mut dims = vec![0usize; inputs];
        let mut points = 1usize;
        for i in 0..inputs {
            dims[i] = data[o + i] as usize;
            if dims[i] < 2 {
                return Err(invalid(format!("CLUT dimension of {} points", dims[i])));
            }
            points *= dims[i];
        }
        let precision = data[o + 16] as usize;
        if precision != 1 && precision != 2 {
            return Err(invalid(format!("CLUT precision {precision}")));
        }
        let start = o + 20;
        if start + points * outputs * precision > data.len() {
            return Err(invalid("truncated lutAToB CLUT"));
        }
        let values = (0..points * outputs)
            .map(|i| {
                if precision == 2 {
                    f64::from(be16(&data[start + 2 * i..])) / 65535.0
                } else {
                    f64::from(data[start + i]) / 255.0
                }
            })
            .collect();
        stages.push(Stage::Clut(Clut {
            dims,
            out: outputs,
            values,
        }));
    }
    if offsets[2] != 0 {
        stages.push(Stage::Curves(curves(offsets[2], outputs)?));
    }
    let o = offsets[1];
    if o != 0 {
        if o + 48 > data.len() {
            return Err(invalid("truncated lutAToB matrix"));
        }
        let mut e = [0.0f64; 12];
        for (i, slot) in e.iter_mut().enumerate() {
            *slot = s15_fixed16(&data[o + 4 * i..]);
        }
        stages.push(Stage::Matrix(e));
    }
    stages.push(Stage::Curves(curves(offsets[0], outputs)?));
    Ok(stages)
}
#[derive(Debug, Clone)]
struct Clut {
    dims: Vec<usize>,
    out: usize,
    values: Vec<f64>,
}
impl Clut {
    fn eval(&self, v: &[f64]) -> Vec<f64> {
        let mut base = [0usize; 3];
        let mut frac = [0.0f64; 3];
        for i in 0..3 {
            let pos = super::clamp01(v[i]) * (self.dims[i] - 1) as f64;
            base[i] = pos as usize;
            if base[i] >= self.dims[i] - 1 {
                base[i] = self.dims[i] - 2;
            }
            frac[i] = pos - base[i] as f64;
        }
        let index = |i0: usize, i1: usize, i2: usize| {
            ((base[0] + i0) * self.dims[1] * self.dims[2]
                + (base[1] + i1) * self.dims[2]
                + (base[2] + i2))
                * self.out
        };
        let (rx, ry, rz) = (frac[0], frac[1], frac[2]);
        let c000 = index(0, 0, 0);
        let c111 = index(1, 1, 1);
        let (c1, c2, w1, w2, w3) = if rx >= ry && ry >= rz {
            (index(1, 0, 0), index(1, 1, 0), rx, ry, rz)
        } else if rx >= rz && rz >= ry {
            (index(1, 0, 0), index(1, 0, 1), rx, rz, ry)
        } else if rz >= rx && rx >= ry {
            (index(0, 0, 1), index(1, 0, 1), rz, rx, ry)
        } else if ry >= rx && rx >= rz {
            (index(0, 1, 0), index(1, 1, 0), ry, rx, rz)
        } else if ry >= rz && rz >= rx {
            (index(0, 1, 0), index(0, 1, 1), ry, rz, rx)
        } else {
            (index(0, 0, 1), index(0, 1, 1), rz, ry, rx)
        };
        (0..self.out)
            .map(|k| {
                let (p0, p1, p2, p3) = (
                    self.values[c000 + k],
                    self.values[c1 + k],
                    self.values[c2 + k],
                    self.values[c111 + k],
                );
                p0 + (p1 - p0) * w1 + (p2 - p1) * w2 + (p3 - p2) * w3
            })
            .collect()
    }
}
