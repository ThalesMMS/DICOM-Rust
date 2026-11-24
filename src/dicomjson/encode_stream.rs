#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::{Budget, ERR_MAX_JSON_BYTES, Options, jsonwire, marshal_elements};
use crate::object::Object;
struct StreamOut {
    out: String,
    maximum: i64,
    pretty: bool,
}
impl StreamOut {
    fn write(&mut self, chunk: &str) -> Result<(), String> {
        if self.maximum > 0
            && chunk.len() as i64 > self.maximum.saturating_sub(self.out.len() as i64)
        {
            return Err(ERR_MAX_JSON_BYTES.to_string());
        }
        self.out.push_str(chunk);
        Ok(())
    }
}
fn encode_object(
    w: &mut StreamOut,
    obj: Option<&Object>,
    opts: &mut Options,
    budget: &mut Budget,
    depth: usize,
    lead: &str,
) -> Result<(), String> {
    let big = opts
        .byte_order
        .unwrap_or_else(|| obj.is_some_and(Object::value_big_endian));
    let m = marshal_elements(obj, opts, big, budget, 0)?;
    let mut text = lead.to_string();
    if w.pretty {
        jsonwire::write_pretty(&m, &mut text, depth);
    } else {
        jsonwire::write_compact(&m, &mut text);
    }
    w.write(&text)
}
pub fn encode_context(obj: Option<&Object>, opts: &mut Options) -> Result<Vec<u8>, super::Error> {
    encode_context_text(obj, opts).map_err(super::Error::new)
}
pub(crate) fn encode_context_text(
    obj: Option<&Object>,
    opts: &mut Options,
) -> Result<Vec<u8>, String> {
    let mut budget = Budget::new(opts.limits)?;
    let mut w = StreamOut {
        out: String::new(),
        maximum: opts.limits.max_json_bytes,
        pretty: opts.pretty,
    };
    encode_object(&mut w, obj, opts, &mut budget, 0, "")?;
    Ok(w.out.into_bytes())
}
pub enum Produced {
    Object(Option<Box<Object>>),
    Eof,
    Err(crate::BoxError),
}
pub fn encode_array_context(
    next: Option<&mut dyn FnMut() -> Produced>,
    opts: &mut Options,
) -> Result<Vec<u8>, super::Error> {
    encode_array_context_text(next, opts).map_err(super::Error::new)
}
pub(crate) fn encode_array_context_text(
    next: Option<&mut dyn FnMut() -> Produced>,
    opts: &mut Options,
) -> Result<Vec<u8>, String> {
    let mut budget = Budget::new(opts.limits)?;
    let mut w = StreamOut {
        out: String::new(),
        maximum: opts.limits.max_json_bytes,
        pretty: opts.pretty,
    };
    let Some(next) = next else {
        return Err("dicomjson: nil dataset producer".to_string());
    };
    w.write("[")?;
    let mut count = 0usize;
    loop {
        let obj = match next() {
            Produced::Eof => {
                if w.pretty && count > 0 {
                    w.write("\n]")?;
                } else {
                    w.write("]")?;
                }
                return Ok(w.out.into_bytes());
            }
            Produced::Err(err) => return Err(err.to_string()),
            Produced::Object(obj) => obj,
        };
        let lead = match (count > 0, w.pretty) {
            (true, true) => {
                w.write(",\n  ")?;
                ""
            }
            (true, false) => {
                w.write(",")?;
                ""
            }
            (false, true) => "\n  ",
            (false, false) => "",
        };
        encode_object(&mut w, obj.as_deref(), opts, &mut budget, 1, lead)?;
        count = count.saturating_add(1);
    }
}
#[derive(Default)]
struct IndentState {
    depth: usize,
    quoted: bool,
    escaped: bool,
    pending: bool,
}
impl IndentState {
    fn newline(&self, b: &mut Vec<u8>) {
        b.push(b'\n');
        for _ in 0..self.depth {
            b.extend_from_slice(b"  ");
        }
    }
    fn transform(&mut self, p: &[u8]) -> Vec<u8> {
        let mut b = Vec::with_capacity(p.len().saturating_add(16));
        for &c in p {
            if self.quoted {
                b.push(c);
                if self.escaped {
                    self.escaped = false;
                } else if c == b'\\' {
                    self.escaped = true;
                } else if c == b'"' {
                    self.quoted = false;
                }
                continue;
            }
            if self.pending && c != b'}' && c != b']' {
                self.newline(&mut b);
                self.pending = false;
            }
            match c {
                b'{' | b'[' => {
                    b.push(c);
                    self.depth = self.depth.saturating_add(1);
                    self.pending = true;
                }
                b'}' | b']' => {
                    self.depth = self.depth.saturating_sub(1);
                    if self.pending {
                        self.pending = false;
                    } else {
                        self.newline(&mut b);
                    }
                    b.push(c);
                }
                b',' => {
                    b.push(c);
                    self.newline(&mut b);
                }
                b':' => b.extend_from_slice(b": "),
                _ => {
                    b.push(c);
                    if c == b'"' {
                        self.quoted = true;
                    }
                }
            }
        }
        b
    }
}
struct WriteThrough<'a> {
    out: Option<&'a mut dyn std::io::Write>,
    maximum: i64,
    written: i64,
    indent: Option<IndentState>,
}
impl WriteThrough<'_> {
    fn new<'a>(out: Option<&'a mut dyn std::io::Write>, opts: &Options) -> WriteThrough<'a> {
        WriteThrough {
            out,
            maximum: opts.limits.max_json_bytes,
            written: 0,
            indent: opts.pretty.then(IndentState::default),
        }
    }
    fn chunk(&mut self, s: &str) -> Result<(), String> {
        let transformed;
        let bytes = match &mut self.indent {
            Some(indent) => {
                transformed = indent.transform(s.as_bytes());
                &transformed[..]
            }
            None => s.as_bytes(),
        };
        let Some(out) = self.out.as_deref_mut() else {
            return Err("dicomjson: nil writer".to_string());
        };
        if self.maximum > 0 && bytes.len() as i64 > self.maximum.saturating_sub(self.written) {
            return Err(ERR_MAX_JSON_BYTES.to_string());
        }
        match out.write(bytes) {
            Ok(n) => {
                self.written = self.written.saturating_add(n as i64);
                if n != bytes.len() {
                    return Err("short write".to_string());
                }
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }
    fn value(&mut self, v: &jsonwire::JVal) -> Result<(), String> {
        let mut text = String::new();
        jsonwire::write_compact(v, &mut text);
        self.chunk(&text)
    }
    fn data_set(&mut self, v: &jsonwire::JVal) -> Result<(), String> {
        let jsonwire::JVal::Obj(entries) = v else {
            return self.value(v);
        };
        self.chunk("{")?;
        for (index, (key, element)) in entries.iter().enumerate() {
            if index > 0 {
                self.chunk(",")?;
            }
            self.value(&jsonwire::JVal::Str(key.clone()))?;
            self.chunk(":")?;
            self.element(element)?;
        }
        self.chunk("}")
    }
    fn element(&mut self, v: &jsonwire::JVal) -> Result<(), String> {
        let jsonwire::JVal::Obj(fields) = v else {
            return self.value(v);
        };
        let sequence = fields
            .iter()
            .any(|(k, v)| k == "vr" && matches!(v, jsonwire::JVal::Str(s) if s == "SQ"));
        self.chunk("{")?;
        for (index, (field, value)) in fields.iter().enumerate() {
            if index > 0 {
                self.chunk(",")?;
            }
            self.value(&jsonwire::JVal::Str(field.clone()))?;
            self.chunk(":")?;
            match value {
                jsonwire::JVal::Arr(items) if sequence && field == "Value" => {
                    self.chunk("[")?;
                    for (j, item) in items.iter().enumerate() {
                        if j > 0 {
                            self.chunk(",")?;
                        }
                        self.data_set(item)?;
                    }
                    self.chunk("]")?;
                }
                other => self.value(other)?,
            }
        }
        self.chunk("}")
    }
}
fn encode_object_to(
    w: &mut WriteThrough<'_>,
    obj: Option<&Object>,
    opts: &mut Options,
    budget: &mut Budget,
) -> Result<(), String> {
    let big = opts
        .byte_order
        .unwrap_or_else(|| obj.is_some_and(Object::value_big_endian));
    let m = marshal_elements(obj, opts, big, budget, 0)?;
    w.data_set(&m)
}
#[doc(hidden)]
pub fn encode_context_to(
    out: Option<&mut dyn std::io::Write>,
    obj: Option<&Object>,
    opts: &mut Options,
) -> Result<(), String> {
    let mut budget = Budget::new(opts.limits)?;
    let mut w = WriteThrough::new(out, opts);
    encode_object_to(&mut w, obj, opts, &mut budget)
}
#[doc(hidden)]
pub fn encode_array_context_to(
    out: Option<&mut dyn std::io::Write>,
    next: Option<&mut dyn FnMut() -> Produced>,
    opts: &mut Options,
) -> Result<(), String> {
    let mut budget = Budget::new(opts.limits)?;
    let mut w = WriteThrough::new(out, opts);
    let Some(next) = next else {
        return Err("dicomjson: nil dataset producer".to_string());
    };
    w.chunk("[")?;
    let mut count = 0usize;
    loop {
        let obj = match next() {
            Produced::Eof => return w.chunk("]"),
            Produced::Err(err) => return Err(err.to_string()),
            Produced::Object(obj) => obj,
        };
        if count > 0 {
            w.chunk(",")?;
        }
        encode_object_to(&mut w, obj.as_deref(), opts, &mut budget)?;
        count = count.saturating_add(1);
    }
}
