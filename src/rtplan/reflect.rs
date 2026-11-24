use super::{Error, ErrorKind, Limits, path_error};
use crate::numtext::{format_float, parse_float_value};
use crate::object::Tag;
use crate::object::element_setters::format_ds;
use std::fmt::Write as _;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElemKind {
    Float32,
    Other,
}
pub enum Node<'a> {
    Nil,
    Ptr(&'a dyn Reflect),
    Struct(&'static str, Vec<(&'static str, &'a dyn Reflect)>),
    Slice(&'static str, ElemKind, Vec<&'a dyn Reflect>),
    Bytes(&'a [u8]),
    Str(&'a str),
    F64(f64),
    F32(f32),
    Int(i64),
    Bool(bool),
}
pub trait Reflect {
    fn node(&self) -> Node<'_>;
    fn type_name() -> &'static str
    where
        Self: Sized,
    {
        ""
    }
    fn elem_kind() -> ElemKind
    where
        Self: Sized,
    {
        ElemKind::Other
    }
}
impl Reflect for i64 {
    fn node(&self) -> Node<'_> {
        Node::Int(*self)
    }
    fn type_name() -> &'static str {
        "int"
    }
}
impl Reflect for u16 {
    fn node(&self) -> Node<'_> {
        Node::Int(i64::from(*self))
    }
    fn type_name() -> &'static str {
        "uint16"
    }
}
impl Reflect for u32 {
    fn node(&self) -> Node<'_> {
        Node::Int(i64::from(*self))
    }
    fn type_name() -> &'static str {
        "uint32"
    }
}
impl Reflect for bool {
    fn node(&self) -> Node<'_> {
        Node::Bool(*self)
    }
    fn type_name() -> &'static str {
        "bool"
    }
}
impl Reflect for f64 {
    fn node(&self) -> Node<'_> {
        Node::F64(*self)
    }
    fn type_name() -> &'static str {
        "float64"
    }
}
impl Reflect for f32 {
    fn node(&self) -> Node<'_> {
        Node::F32(*self)
    }
    fn type_name() -> &'static str {
        "float32"
    }
    fn elem_kind() -> ElemKind {
        ElemKind::Float32
    }
}
impl Reflect for String {
    fn node(&self) -> Node<'_> {
        Node::Str(self)
    }
    fn type_name() -> &'static str {
        "string"
    }
}
impl<T: Reflect> Reflect for Option<T> {
    fn node(&self) -> Node<'_> {
        match self {
            None => Node::Nil,
            Some(v) => Node::Ptr(v),
        }
    }
}
impl<T: Reflect> Reflect for Vec<T> {
    fn node(&self) -> Node<'_> {
        Node::Slice(
            T::type_name(),
            T::elem_kind(),
            self.iter().map(|v| v as &dyn Reflect).collect(),
        )
    }
}
impl Reflect for [f64; 2] {
    fn node(&self) -> Node<'_> {
        Node::Slice(
            "float64",
            ElemKind::Other,
            self.iter().map(|v| v as &dyn Reflect).collect(),
        )
    }
}
impl Reflect for Tag {
    fn node(&self) -> Node<'_> {
        Node::Struct(
            "Tag",
            vec![("Group", &self.group), ("Element", &self.element)],
        )
    }
    fn type_name() -> &'static str {
        "Tag"
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);
impl Reflect for Bytes {
    fn node(&self) -> Node<'_> {
        Node::Bytes(&self.0)
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NilFloats(pub Option<Vec<f64>>);
impl NilFloats {
    pub fn is_nil(&self) -> bool {
        self.0.is_none()
    }
}
impl std::ops::Deref for NilFloats {
    type Target = [f64];
    fn deref(&self) -> &[f64] {
        self.0.as_deref().unwrap_or(&[])
    }
}
impl Reflect for NilFloats {
    fn node(&self) -> Node<'_> {
        Node::Slice(
            "float64",
            ElemKind::Other,
            self.iter().map(|v| v as &dyn Reflect).collect(),
        )
    }
}
pub struct Pair<'a>(
    pub &'static str,
    pub &'a dyn Reflect,
    pub &'static str,
    pub &'a dyn Reflect,
);
struct Pointer<'a>(&'a dyn Reflect);
impl Reflect for Pointer<'_> {
    fn node(&self) -> Node<'_> {
        Node::Ptr(self.0)
    }
}
pub(crate) fn model_admission(v: &dyn Reflect, l: &Limits) -> Result<i64, Error> {
    admit(&Pointer(v), l)
}
pub(crate) fn model_admission_pair(pair: &Pair<'_>, l: &Limits) -> Result<i64, Error> {
    let (a, b) = (Pointer(pair.1), Pointer(pair.3));
    struct Both<'a>(&'static str, &'a Pointer<'a>, &'static str, &'a Pointer<'a>);
    impl Reflect for Both<'_> {
        fn node(&self) -> Node<'_> {
            Node::Struct("", vec![(self.0, self.1), (self.2, self.3)])
        }
    }
    admit(&Both(pair.0, &a, pair.2, &b), l)
}
fn admit(v: &dyn Reflect, l: &Limits) -> Result<i64, Error> {
    let mut a = Admission {
        l,
        bytes: 0,
        beams: 0,
        entities: 0,
        points: 0,
        spots: 0,
    };
    match a.visit(v, 0) {
        Ok(()) => Ok(a.bytes),
        Err(e) => Err(Error {
            text: format!("{}: model admission", e.text),
            ..e
        }),
    }
}
struct Admission<'a> {
    l: &'a Limits,
    bytes: i64,
    beams: i64,
    entities: i64,
    points: i64,
    spots: i64,
}
impl Admission<'_> {
    fn reserve(&mut self, n: i64) -> Result<(), Error> {
        if n < 0 || n > self.l.max_model_bytes - self.bytes {
            return Err(Error::bare(ErrorKind::ResourceLimit));
        }
        self.bytes += n;
        Ok(())
    }
    fn visit(&mut self, v: &dyn Reflect, depth: i64) -> Result<(), Error> {
        if depth > 128 {
            return Err(Error::bare(ErrorKind::ResourceLimit));
        }
        match v.node() {
            Node::Nil => Ok(()),
            Node::Ptr(inner) => self.visit(inner, depth + 1),
            Node::Struct(name, fields) => {
                if name.starts_with("Attribute[")
                    && let [(_, present), (_, value)] = fields.as_slice()
                    && matches!(present.node(), Node::Bool(false))
                    && !matches!(value.node(), Node::Nil)
                {
                    return Err(path_error(
                        ErrorKind::InvalidObject,
                        "",
                        "absent attribute has a value",
                    ));
                }
                for (field, value) in fields {
                    if field == "SpotPositions" {
                        let mut n = match value.node() {
                            Node::Slice(_, _, items) => items.len() as i64,
                            _ => 0,
                        };
                        if n % 2 != 0 {
                            return Err(path_error(
                                ErrorKind::InvalidObject,
                                "",
                                "spot position map must contain pairs",
                            ));
                        }
                        n /= 2;
                        if n > self.l.max_spots - self.spots {
                            return Err(Error::bare(ErrorKind::ResourceLimit));
                        }
                        self.spots += n;
                    }
                    self.visit(value, depth + 1)?;
                }
                Ok(())
            }
            Node::Str(s) => self.reserve(s.len() as i64 * 4 + 32),
            Node::Bytes(b) => {
                if b.len() as i64 > self.l.max_pixel_bytes {
                    return Err(Error::bare(ErrorKind::ResourceLimit));
                }
                self.reserve(b.len() as i64)
            }
            Node::Slice(elem, kind, items) => {
                let n = items.len() as i64;
                match elem {
                    "Beam" | "IonBeam" | "BeamSession" | "RecordBeamSession" => self.beams += n,
                    "BrachySource"
                    | "BrachySetup"
                    | "BrachyChannel"
                    | "RangeShifter"
                    | "BrachySession"
                    | "RecordedBrachySource"
                    | "RecordBrachySession"
                    | "RecordBrachyChannel"
                    | "RecordBrachyPulse"
                    | "RecordedRangeShifter"
                    | "RecordedSourceApplicator"
                    | "RecordFractionGroupSummary"
                    | "RecordFractionStatus" => self.entities += n,
                    "ControlPoint"
                    | "IonControlPoint"
                    | "BrachyControlPoint"
                    | "DeliveryControlPoint"
                    | "RecordControlPoint"
                    | "RecordBrachyControlPoint" => self.points += n,
                    _ => {}
                }
                if self.beams > self.l.max_beams
                    || self.entities > self.l.max_entities
                    || self.points > self.l.max_control_points
                {
                    return Err(Error::bare(ErrorKind::ResourceLimit));
                }
                if n > self.l.max_elements && kind != ElemKind::Float32 {
                    return Err(Error::bare(ErrorKind::ResourceLimit));
                }
                for item in items {
                    self.visit(item, depth + 1)?;
                }
                Ok(())
            }
            Node::F64(x) => self.float(x),
            Node::F32(x) => self.float(f64::from(x)),
            Node::Int(_) | Node::Bool(_) => self.reserve(16),
        }
    }
    fn float(&mut self, x: f64) -> Result<(), Error> {
        if !x.is_finite() {
            return Err(path_error(
                ErrorKind::InvalidObject,
                "",
                "non-finite model value",
            ));
        }
        self.reserve(8)
    }
}
pub(crate) fn semantic_floats(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let (Ok(sa), Ok(sb)) = (format_ds(a), format_ds(b)) else {
        return false;
    };
    parse_float_value(&sa).0 == parse_float_value(&sb).0
}
const SKIPPED: [&str; 8] = [
    "ModelBytes",
    "Warnings",
    "SOPInstanceUID",
    "Encoded",
    "EncodedDeviceTypes",
    "EncodedSnoutPosition",
    "EncodedGantryPitchAngle",
    "EncodedRangeShifterSettings",
];
pub(crate) fn semantic_equal(a: &dyn Reflect, b: &dyn Reflect) -> bool {
    match (a.node(), b.node()) {
        (Node::Nil, Node::Nil) => true,
        (Node::Ptr(x), Node::Ptr(y)) => semantic_equal(x, y),
        (Node::Struct(_, x), Node::Struct(_, y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(&y)
                    .all(|((name, p), (_, q))| SKIPPED.contains(name) || semantic_equal(*p, *q))
        }
        (Node::Slice(_, _, x), Node::Slice(_, _, y)) => {
            x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| semantic_equal(*p, *q))
        }
        (Node::Bytes(x), Node::Bytes(y)) => x == y,
        (Node::Str(x), Node::Str(y)) => x == y,
        (Node::F64(x), Node::F64(y)) => semantic_floats(x, y),
        (Node::F32(x), Node::F32(y)) => x == y,
        (Node::Int(x), Node::Int(y)) => x == y,
        (Node::Bool(x), Node::Bool(y)) => x == y,
        _ => false,
    }
}
pub(crate) fn format_float32(x: f32) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    let neg = x.is_sign_negative();
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if x == 0.0 {
        out.push('0');
        return out;
    }
    let nd = digits.len() as i32;
    if !(-4..6).contains(&exp) {
        out.push_str(&digits[..1]);
        if nd > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(
            out,
            "e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.unsigned_abs()
        );
        return out;
    }
    let dp = exp + 1;
    if dp <= 0 {
        out.push_str("0.");
        for _ in 0..-dp {
            out.push('0');
        }
        out.push_str(&digits);
    } else if nd <= dp {
        out.push_str(&digits);
        for _ in 0..dp - nd {
            out.push('0');
        }
    } else {
        out.push_str(&digits[..dp as usize]);
        out.push('.');
        out.push_str(&digits[dp as usize..]);
    }
    out
}
pub fn dump(v: &dyn Reflect) -> String {
    let mut out = String::new();
    match v.node() {
        Node::Struct(_, fields) => {
            for (name, value) in fields {
                out.push_str(name);
                out.push_str(": ");
                dump_value(&mut out, value);
                out.push('\n');
            }
        }
        _ => {
            dump_value(&mut out, v);
            out.push('\n');
        }
    }
    out
}
fn dump_value(out: &mut String, v: &dyn Reflect) {
    match v.node() {
        Node::Nil => out.push_str("nil"),
        Node::Ptr(inner) => {
            out.push('&');
            dump_value(out, inner);
        }
        Node::Struct(_, fields) => {
            out.push('{');
            for (i, (name, value)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(name);
                out.push(':');
                dump_value(out, *value);
            }
            out.push('}');
        }
        Node::Slice(_, _, items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                dump_value(out, *item);
            }
            out.push(']');
        }
        Node::Bytes(b) => {
            let _ = write!(out, "bytes({}:", b.len());
            for byte in b.iter().take(64) {
                let _ = write!(out, "{byte:02x}");
            }
            out.push(')');
        }
        Node::Str(s) => out.push_str(&crate::textfmt::quote(s)),
        Node::F64(x) => out.push_str(&format_float(x, 'g', -1)),
        Node::F32(x) => out.push_str(&format_float32(x)),
        Node::Int(n) => {
            let _ = write!(out, "{n}");
        }
        Node::Bool(b) => out.push_str(if b { "true" } else { "false" }),
    }
}
