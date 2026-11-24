use super::error::{RegError, Sentinel};
use super::itkfile::{MAX_HDF5_TRANSFORMS, file_wrap};
use super::matrix::{Matrix3, finite};
use super::transform::Transform;
use super::{Affine, Composite};
use crate::numtext::parse_float;
use crate::render::Vec3;
use crate::textfmt::quote;
const MINC_TRANSFORM_HEADER: &str = "MNI Transform File";
pub(crate) const MAX_MINC_FILE_BYTES: usize = 16 << 20;
const MAX_MINC_TOKEN_BYTES: usize = 256;
fn trim_left_ws(mut b: &[u8]) -> &[u8] {
    while let Some((&c, rest)) = b.split_first() {
        if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
            b = rest;
        } else {
            break;
        }
    }
    b
}
pub(crate) fn minc_transform_body(data: &[u8]) -> Option<&[u8]> {
    let data = trim_left_ws(data);
    let data = trim_left_ws(data.strip_prefix("\u{FEFF}".as_bytes()).unwrap_or(data));
    if !data.starts_with(MINC_TRANSFORM_HEADER.as_bytes()) {
        return None;
    }
    let (header, body) = match data.iter().position(|&b| b == b'\n') {
        Some(n) => (&data[..n], &data[n + 1..]),
        None => (data, &data[data.len()..]),
    };
    let mut end = header.len();
    while end > 0 && matches!(header[end - 1], b' ' | b'\t' | b'\r') {
        end -= 1;
    }
    (&header[..end] == MINC_TRANSFORM_HEADER.as_bytes()).then_some(body)
}
enum TokenError {
    Eof,
    UnexpectedEof,
    Other(RegError),
}
impl TokenError {
    fn into_reg(self) -> RegError {
        match self {
            TokenError::Eof => RegError::with_text(Vec::new(), "EOF"),
            TokenError::UnexpectedEof => RegError::with_text(Vec::new(), "unexpected EOF"),
            TokenError::Other(e) => e,
        }
    }
}
fn plain(text: impl Into<String>) -> TokenError {
    TokenError::Other(RegError::with_text(Vec::new(), text))
}
struct Tokens<'a> {
    data: &'a [u8],
    pos: usize,
}
impl Tokens<'_> {
    fn next(&mut self) -> Result<String, TokenError> {
        let mut word: Vec<u8> = Vec::new();
        loop {
            let Some(&ch) = self.data.get(self.pos) else {
                if !word.is_empty() {
                    return Ok(String::from_utf8_lossy(&word).into_owned());
                }
                return Err(TokenError::Eof);
            };
            self.pos += 1;
            match ch {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    if !word.is_empty() {
                        return Ok(String::from_utf8_lossy(&word).into_owned());
                    }
                }
                b'=' | b';' | b'%' | b'#' => {
                    if !word.is_empty() {
                        self.pos -= 1;
                        return Ok(String::from_utf8_lossy(&word).into_owned());
                    }
                    if ch == b'=' || ch == b';' {
                        return Ok((ch as char).to_string());
                    }
                    match self.data[self.pos..].iter().position(|&b| b == b'\n') {
                        Some(n) => self.pos += n + 1,
                        None => {
                            self.pos = self.data.len();
                            return Err(TokenError::Eof);
                        }
                    }
                }
                _ => {
                    if word.len() == MAX_MINC_TOKEN_BYTES {
                        return Err(plain(format!("token exceeds {MAX_MINC_TOKEN_BYTES} bytes")));
                    }
                    word.push(ch);
                }
            }
        }
    }
    fn required(&mut self) -> Result<String, TokenError> {
        match self.next() {
            Err(TokenError::Eof) => Err(TokenError::UnexpectedEof),
            r => r,
        }
    }
    fn expect(&mut self, want: &str) -> Result<(), TokenError> {
        let got = self.required()?;
        if got != want {
            return Err(plain(format!(
                "expected {}, got {}",
                quote(want),
                quote(&got)
            )));
        }
        Ok(())
    }
}
pub(crate) fn read_minc_transform(body: &[u8], limit: i64) -> Result<Box<dyn Transform>, RegError> {
    let mut tokens = Tokens { data: body, pos: 0 };
    parse_minc_transform(&mut tokens, limit)
        .map_err(|e| file_wrap(&e.into_reg()).prefixed_after_sentinel("MINC XFM".to_string()))
}
fn parse_minc_transform(
    tokens: &mut Tokens<'_>,
    mut limit: i64,
) -> Result<Box<dyn Transform>, TokenError> {
    const PARAMETERS: i64 = 12;
    let mut transforms: Vec<Box<dyn Transform>> = Vec::new();
    loop {
        let tag = match tokens.next() {
            Err(TokenError::Eof) => break,
            Err(e) => return Err(e),
            Ok(t) => t,
        };
        if tag != "Transform_Type" {
            return Err(plain(format!(
                "expected Transform_Type, got {}",
                quote(&tag)
            )));
        }
        if transforms.len() == MAX_HDF5_TRANSFORMS {
            return Err(plain(format!(
                "transform count exceeds {MAX_HDF5_TRANSFORMS}"
            )));
        }
        if limit < PARAMETERS {
            return Err(plain("total parameter budget"));
        }
        let t = parse_minc_linear(tokens).map_err(|e| {
            let n = transforms.len() + 1;
            let e = e.into_reg();
            TokenError::Other(RegError::with_text(
                e.sentinels().to_vec(),
                format!("component {n}: {e}"),
            ))
        })?;
        transforms.push(Box::new(t));
        limit -= PARAMETERS;
    }
    match transforms.len() {
        0 => return Err(plain("no transform")),
        1 => return Ok(transforms.remove(0)),
        _ => {}
    }
    let chain = Composite::new(transforms);
    let (matrix, _) = chain
        .linear()
        .map_err(|e| TokenError::Other(RegError::from(e)))?;
    if let Err(e) = matrix.inverse() {
        let e = RegError::from(e);
        return Err(TokenError::Other(RegError::with_text(
            e.sentinels().to_vec(),
            format!("composed matrix: {e}"),
        )));
    }
    Ok(Box::new(chain))
}
fn parse_minc_linear(tokens: &mut Tokens<'_>) -> Result<Affine, TokenError> {
    tokens.expect("=")?;
    let kind = tokens.required()?;
    if kind != "Linear" {
        return Err(TokenError::Other(RegError::new(
            Sentinel::UnsupportedTransform,
            format!("MINC Transform_Type {}", quote(&kind)),
        )));
    }
    tokens.expect(";")?;
    let mut next = tokens.required()?;
    let mut invert = false;
    if next == "Invert_Flag" {
        tokens.expect("=")?;
        let flag = tokens.required()?;
        if flag != "True" && flag != "False" {
            return Err(plain(format!("invalid Invert_Flag {}", quote(&flag))));
        }
        invert = flag == "True";
        tokens.expect(";")?;
        next = tokens.required()?;
    }
    if next != "Linear_Transform" {
        return Err(plain(format!(
            "expected Linear_Transform, got {}",
            quote(&next)
        )));
    }
    tokens.expect("=")?;
    let mut values = [[0f64; 4]; 3];
    for row in values.iter_mut() {
        for v in row.iter_mut() {
            let token = tokens.required()?;
            match parse_float(&token) {
                Ok(value) if finite(value) && !token.contains('_') => *v = value,
                _ => {
                    return Err(plain(format!(
                        "matrix value {} is not finite numeric input",
                        quote(&token)
                    )));
                }
            }
        }
    }
    tokens.expect(";")?;
    let flip = [-1.0, -1.0, 1.0];
    let mut matrix = Matrix3([[0.0; 3]; 3]);
    for row in 0..3 {
        for column in 0..3 {
            matrix.0[row][column] = flip[row] * values[row][column] * flip[column];
        }
    }
    let mut offset = Vec3::new(-values[0][3], -values[1][3], values[2][3]);
    let inverse = matrix
        .inverse()
        .map_err(|e| TokenError::Other(RegError::from(e)))?;
    if invert {
        offset = inverse.mul_vec(offset).scale(-1.0);
        matrix = inverse;
    }
    let mut affine = Affine::new();
    affine.base.matrix = matrix;
    affine
        .set_offset(offset)
        .map_err(|e| TokenError::Other(RegError::from(e)))?;
    Ok(affine)
}
