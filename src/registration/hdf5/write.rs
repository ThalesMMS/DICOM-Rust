use super::{SIGNATURE, format_err};
use crate::optimize::Context;
use crate::registration::error::{RegError, Sentinel};
use std::collections::BTreeMap;
pub struct WriteDataset<'a> {
    pub path: String,
    pub text: Option<String>,
    pub count: i64,
    pub float64_at: Option<Box<dyn Fn(usize) -> f64 + 'a>>,
}
const WRITE_OBJECTS: usize = 8192;
const WRITE_DATASETS: usize = 4096;
const WRITE_VECTOR_ELEMENTS: i64 = 1 << 24;
const WRITE_CHUNK: usize = 64 * 1024;
const UNDEFINED: u64 = u64::MAX;
fn budget() -> RegError {
    RegError::bare(Sentinel::Hdf5WriteBudget)
}
fn bare_format() -> RegError {
    RegError::bare(Sentinel::Hdf5Format)
}
#[derive(Default)]
struct Object {
    name: String,
    address: u64,
    raw: u64,
    string_heap: u64,
    data: Option<usize>,
    children: BTreeMap<String, usize>,
    ordered: Vec<usize>,
    heap: Vec<u8>,
    names: BTreeMap<String, u64>,
    heap_address: u64,
    heap_data: u64,
    symbols: Vec<Symbol>,
    trees: Vec<Tree>,
    tree: usize,
}
struct Symbol {
    address: u64,
    hi: u64,
    objects: Vec<usize>,
}
#[derive(Clone)]
struct Tree {
    address: u64,
    lo: u64,
    hi: u64,
    left: u64,
    right: u64,
    level: u8,
    entries: Vec<usize>,
}
#[derive(Clone, Copy)]
enum Bytes {
    Header(usize),
    HeapHeader(usize, usize),
    HeapData(usize),
    Symbol(usize, usize),
    Tree(usize, usize),
    Collection(usize),
    Reference(usize),
    Vector(usize),
}
struct Part {
    at: u64,
    size: u64,
    bytes: Bytes,
}
struct Plan {
    objects: Vec<Object>,
    order: Vec<usize>,
    parts: Vec<Part>,
    end: u64,
    max: u64,
}
fn ascii(s: &str) -> bool {
    s.bytes().all(|v| v != 0 && v <= 127)
}
fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}
fn check(ctx: Context<'_>) -> Result<(), RegError> {
    ctx.err().map_err(|_| RegError::canceled())
}
impl Plan {
    fn alloc(&mut self, size: u64) -> Result<u64, RegError> {
        let max_int = i64::MAX as u64;
        if size > max_int || self.end > max_int - 7 {
            return Err(budget());
        }
        let at = (self.end + 7) & !7u64;
        if size > max_int - at {
            return Err(budget());
        }
        let end = at + size;
        if end > self.max {
            return Err(budget());
        }
        self.end = end;
        Ok(at)
    }
    fn metadata(&mut self, size: u64, bytes: Bytes) -> Result<u64, RegError> {
        let at = self.alloc(size)?;
        self.parts.push(Part { at, size, bytes });
        Ok(at)
    }
}
fn plan_write(ctx: Context<'_>, ds: &[WriteDataset<'_>], max_bytes: i64) -> Result<Plan, RegError> {
    if max_bytes <= 0 || ds.is_empty() || ds.len() > WRITE_DATASETS {
        return Err(format_err("writer dataset/budget admission"));
    }
    let mut p = Plan {
        objects: vec![Object::default()],
        order: Vec::new(),
        parts: Vec::new(),
        end: 96,
        max: max_bytes as u64,
    };
    if p.end > p.max {
        return Err(budget());
    }
    let mut count = 1usize;
    for (i, d) in ds.iter().enumerate() {
        check(ctx)?;
        if d.path.len() > 512 || !d.path.starts_with('/') || !ascii(&d.path) {
            return Err(format_err("dataset path"));
        }
        let names: Vec<&str> = d.path[1..].split('/').collect();
        if names.is_empty() || names.len() > 8 {
            return Err(bare_format());
        }
        if let Some(text) = &d.text {
            if d.count != 0 || d.float64_at.is_some() || text.len() > 1024 || !ascii(text) {
                return Err(format_err("metadata string"));
            }
        } else if d.count < 0
            || d.count > WRITE_VECTOR_ELEMENTS
            || (d.count > 0 && d.float64_at.is_none())
        {
            return Err(format_err("vector admission"));
        }
        let mut obj = 0usize;
        for (j, name) in names.iter().enumerate() {
            if name.is_empty()
                || *name == "."
                || *name == ".."
                || name.len() > 256
                || p.objects[obj].data.is_some()
            {
                return Err(format_err("dataset/group path collision"));
            }
            let child = match p.objects[obj].children.get(*name) {
                Some(&c) => c,
                None => {
                    count += 1;
                    if count > WRITE_OBJECTS {
                        return Err(format_err("metadata object limit"));
                    }
                    p.objects.push(Object {
                        name: name.to_string(),
                        ..Object::default()
                    });
                    let c = p.objects.len() - 1;
                    p.objects[obj].children.insert(name.to_string(), c);
                    c
                }
            };
            obj = child;
            if j == names.len() - 1 {
                if p.objects[obj].data.is_some() || !p.objects[obj].children.is_empty() {
                    return Err(format_err("duplicate dataset/group"));
                }
                p.objects[obj].data = Some(i);
            }
        }
    }
    fn visit(p: &mut Plan, o: usize) {
        p.order.push(o);
        let children: Vec<usize> = p.objects[o].children.values().copied().collect();
        for child in children {
            p.objects[o].ordered.push(child);
            visit(p, child);
        }
    }
    visit(&mut p, 0);
    let order = p.order.clone();
    for &o in &order {
        let size = if p.objects[o].data.is_some() { 104 } else { 40 };
        p.objects[o].address = p.metadata(size, Bytes::Header(o))?;
    }
    for &o in &order {
        if p.objects[o].data.is_some() {
            continue;
        }
        check(ctx)?;
        let mut heap = vec![0u8; 8];
        let mut names = BTreeMap::new();
        for &child in &p.objects[o].ordered {
            let name = &p.objects[child].name;
            names.insert(name.clone(), heap.len() as u64);
            heap.extend_from_slice(name.as_bytes());
            heap.push(0);
            while !heap.len().is_multiple_of(8) {
                heap.push(0);
            }
        }
        let free_offset = heap.len();
        heap.extend_from_slice(&[0u8; 16]);
        put64(&mut heap, free_offset, 1);
        put64(&mut heap, free_offset + 8, 16);
        let heap_len = heap.len() as u64;
        p.objects[o].heap = heap;
        p.objects[o].names = names;
        p.objects[o].heap_address = p.metadata(32, Bytes::HeapHeader(o, free_offset))?;
        p.objects[o].heap_data = p.metadata(heap_len, Bytes::HeapData(o))?;
        let ordered = p.objects[o].ordered.clone();
        let mut previous = 0u64;
        let mut start = 0;
        while start < ordered.len() {
            let end = (start + 8).min(ordered.len());
            let hi = p.objects[o].names[&p.objects[ordered[end - 1]].name];
            let _lo = previous;
            previous = hi;
            let index = p.objects[o].symbols.len();
            p.objects[o].symbols.push(Symbol {
                address: 0,
                hi,
                objects: ordered[start..end].to_vec(),
            });
            let at = p.metadata(328, Bytes::Symbol(o, index))?;
            p.objects[o].symbols[index].address = at;
            start += 8;
        }
        let lows: Vec<u64> = (0..p.objects[o].symbols.len())
            .map(|i| {
                if i == 0 {
                    0
                } else {
                    p.objects[o].symbols[i - 1].hi
                }
            })
            .collect();
        let mut level: Vec<Tree> = Vec::new();
        let nsym = p.objects[o].symbols.len();
        let mut start = 0;
        while start < nsym {
            let end = (start + 32).min(nsym);
            level.push(Tree {
                address: 0,
                lo: lows[start],
                hi: p.objects[o].symbols[end - 1].hi,
                left: UNDEFINED,
                right: UNDEFINED,
                level: 0,
                entries: (start..end).collect(),
            });
            start += 32;
        }
        if level.is_empty() {
            level.push(Tree {
                address: 0,
                lo: 0,
                hi: 0,
                left: UNDEFINED,
                right: UNDEFINED,
                level: 0,
                entries: Vec::new(),
            });
        }
        loop {
            let mut indexes = Vec::new();
            for t in &level {
                let index = p.objects[o].trees.len();
                p.objects[o].trees.push(t.clone());
                let at = p.metadata(544, Bytes::Tree(o, index))?;
                p.objects[o].trees[index].address = at;
                indexes.push(index);
            }
            for (i, &index) in indexes.iter().enumerate() {
                if i > 0 {
                    p.objects[o].trees[index].left = p.objects[o].trees[indexes[i - 1]].address;
                }
                if i + 1 < indexes.len() {
                    p.objects[o].trees[index].right = p.objects[o].trees[indexes[i + 1]].address;
                }
            }
            if indexes.len() == 1 {
                p.objects[o].tree = indexes[0];
                break;
            }
            let mut next = Vec::new();
            let mut start = 0;
            while start < indexes.len() {
                let end = (start + 32).min(indexes.len());
                let children = &indexes[start..end];
                let trees = &p.objects[o].trees;
                next.push(Tree {
                    address: 0,
                    lo: trees[children[0]].lo,
                    hi: trees[children[children.len() - 1]].hi,
                    level: trees[children[0]].level + 1,
                    left: UNDEFINED,
                    right: UNDEFINED,
                    entries: children.to_vec(),
                });
                start += 32;
            }
            level = next;
        }
    }
    for &o in &order {
        let Some(d) = p.objects[o].data else {
            continue;
        };
        if ds[d].text.is_some() {
            p.objects[o].string_heap = p.metadata(4096, Bytes::Collection(o))?;
            p.objects[o].raw = p.metadata(16, Bytes::Reference(o))?;
            continue;
        }
        let size = (ds[d].count as u64)
            .checked_mul(8)
            .ok_or_else(bare_format)?;
        if size == 0 {
            p.objects[o].raw = UNDEFINED;
            continue;
        }
        let at = p.alloc(size)?;
        p.objects[o].raw = at;
        p.parts.push(Part {
            at,
            size,
            bytes: Bytes::Vector(o),
        });
    }
    for d in ds {
        if d.text.is_some() {
            continue;
        }
        if let Some(at) = &d.float64_at {
            for i in 0..d.count.max(0) as usize {
                if i & 8191 == 0 {
                    check(ctx)?;
                }
                let v = at(i);
                if v.is_nan() || v.is_infinite() {
                    return Err(format_err("non-finite vector"));
                }
            }
        }
    }
    check(ctx)?;
    Ok(p)
}
fn object_header(messages: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut size = 16;
    for (_, data) in messages {
        size += 8 + ((data.len() + 7) & !7);
    }
    let mut b = vec![0u8; size];
    b[0] = 1;
    put16(&mut b, 2, messages.len() as u16);
    put32(&mut b, 4, 1);
    put32(&mut b, 8, (size - 16) as u32);
    let mut at = 16;
    for (kind, data) in messages {
        let n = (data.len() + 7) & !7;
        put16(&mut b, at, *kind);
        put16(&mut b, at + 2, n as u16);
        b[at + 8..at + 8 + data.len()].copy_from_slice(data);
        at += 8 + n;
    }
    b
}
impl Plan {
    fn bytes(&self, ds: &[WriteDataset<'_>], kind: Bytes) -> Vec<u8> {
        match kind {
            Bytes::Header(o) => self.header(ds, o),
            Bytes::HeapHeader(o, free_offset) => {
                let obj = &self.objects[o];
                let mut b = vec![0u8; 32];
                b[..4].copy_from_slice(b"HEAP");
                put64(&mut b, 8, obj.heap.len() as u64);
                put64(&mut b, 16, free_offset as u64);
                put64(&mut b, 24, obj.heap_data);
                b
            }
            Bytes::HeapData(o) => self.objects[o].heap.clone(),
            Bytes::Symbol(o, s) => {
                let obj = &self.objects[o];
                let sym = &obj.symbols[s];
                let mut b = vec![0u8; 328];
                b[..4].copy_from_slice(b"SNOD");
                b[4] = 1;
                put16(&mut b, 6, sym.objects.len() as u16);
                for (i, &child) in sym.objects.iter().enumerate() {
                    let c = &self.objects[child];
                    let at = 8 + 40 * i;
                    put64(&mut b, at, obj.names[&c.name]);
                    put64(&mut b, at + 8, c.address);
                    if c.data.is_none() {
                        put32(&mut b, at + 16, 1);
                        put64(&mut b, at + 24, c.trees[c.tree].address);
                        put64(&mut b, at + 32, c.heap_address);
                    }
                }
                b
            }
            Bytes::Tree(o, t) => {
                let obj = &self.objects[o];
                let tree = &obj.trees[t];
                let mut b = vec![0u8; 544];
                b[..4].copy_from_slice(b"TREE");
                b[5] = tree.level;
                put64(&mut b, 8, tree.left);
                put64(&mut b, 16, tree.right);
                put16(&mut b, 6, tree.entries.len() as u16);
                put64(&mut b, 24, tree.lo);
                for (i, &e) in tree.entries.iter().enumerate() {
                    let (address, hi) = if tree.level == 0 {
                        (obj.symbols[e].address, obj.symbols[e].hi)
                    } else {
                        (obj.trees[e].address, obj.trees[e].hi)
                    };
                    put64(&mut b, 32 + 16 * i, address);
                    put64(&mut b, 40 + 16 * i, hi);
                }
                b
            }
            Bytes::Collection(o) => {
                let text = self.text(ds, o);
                let mut b = vec![0u8; 4096];
                b[..4].copy_from_slice(b"GCOL");
                b[4] = 1;
                put64(&mut b, 8, 4096);
                put16(&mut b, 16, 1);
                put16(&mut b, 18, 1);
                put64(&mut b, 24, text.len() as u64);
                b[32..32 + text.len()].copy_from_slice(text.as_bytes());
                let free = (32 + text.len() + 7) & !7;
                put64(&mut b, free + 8, (4096 - free) as u64);
                b
            }
            Bytes::Reference(o) => {
                let mut b = vec![0u8; 16];
                put32(&mut b, 0, self.text(ds, o).len() as u32);
                put64(&mut b, 4, self.objects[o].string_heap);
                put32(&mut b, 12, 1);
                b
            }
            Bytes::Vector(_) => Vec::new(),
        }
    }
    fn text<'d>(&self, ds: &'d [WriteDataset<'_>], o: usize) -> &'d str {
        self.objects[o]
            .data
            .and_then(|d| ds[d].text.as_deref())
            .unwrap_or("")
    }
    fn header(&self, ds: &[WriteDataset<'_>], o: usize) -> Vec<u8> {
        let obj = &self.objects[o];
        let Some(d) = obj.data else {
            let mut b = vec![0u8; 16];
            put64(&mut b, 0, obj.trees[obj.tree].address);
            put64(&mut b, 8, obj.heap_address);
            return object_header(&[(17, b)]);
        };
        let d = &ds[d];
        let mut count = d.count as u64;
        let mut dtype = vec![
            0x11, 0x20, 0x3f, 0, 8, 0, 0, 0, 0, 0, 64, 0, 52, 11, 0, 52, 0xff, 3, 0, 0,
        ];
        let mut size = (d.count as u64).wrapping_mul(8);
        if d.text.is_some() {
            count = 1;
            size = 16;
            dtype = vec![
                0x19, 1, 0, 0, 16, 0, 0, 0, 0x10, 0, 0, 0, 1, 0, 0, 0, 0, 0, 8, 0,
            ];
        }
        let mut space = vec![0u8; 16];
        space[0] = 1;
        space[1] = 1;
        put64(&mut space, 8, count);
        let mut layout = vec![0u8; 18];
        layout[0] = 3;
        layout[1] = 1;
        put64(&mut layout, 2, obj.raw);
        put64(&mut layout, 10, size);
        object_header(&[(1, space), (3, dtype), (8, layout)])
    }
    fn superblock(&self) -> Vec<u8> {
        let root = &self.objects[0];
        let mut b = vec![0u8; 96];
        b[..8].copy_from_slice(SIGNATURE);
        b[13] = 8;
        b[14] = 8;
        put16(&mut b, 16, 4);
        put16(&mut b, 18, 16);
        put64(&mut b, 32, UNDEFINED);
        put64(&mut b, 40, self.end);
        put64(&mut b, 48, UNDEFINED);
        put64(&mut b, 64, root.address);
        put32(&mut b, 72, 1);
        put64(&mut b, 80, root.trees[root.tree].address);
        put64(&mut b, 88, root.heap_address);
        b
    }
}
pub fn write(
    ctx: Context<'_>,
    w: Option<&mut Vec<u8>>,
    ds: &[WriteDataset<'_>],
    max_bytes: i64,
) -> Result<(), RegError> {
    check(ctx)?;
    let Some(w) = w else {
        return Err(bare_format());
    };
    let p = plan_write(ctx, ds, max_bytes)?;
    let emit = |w: &mut Vec<u8>, mut b: &[u8]| -> Result<(), RegError> {
        while !b.is_empty() {
            check(ctx)?;
            let n = b.len().min(WRITE_CHUNK);
            w.extend_from_slice(&b[..n]);
            check(ctx)?;
            b = &b[n..];
        }
        check(ctx)
    };
    emit(w, &p.superblock())?;
    let mut at = 96u64;
    let mut buf = vec![0u8; WRITE_CHUNK];
    for part in &p.parts {
        let gap = part.at - at;
        if gap > 0 {
            if gap > buf.len() as u64 {
                return Err(bare_format());
            }
            buf[..gap as usize].fill(0);
            emit(w, &buf[..gap as usize])?;
        }
        if let Bytes::Vector(o) = part.bytes {
            let d = p.objects[o].data.map(|d| &ds[d]);
            if let Some(d) = d {
                let count = d.count as usize;
                let step = buf.len() / 8;
                let mut start = 0;
                while start < count {
                    let end = (start + step).min(count);
                    for i in start..end {
                        let v = d.float64_at.as_ref().map_or(0.0, |f| f(i));
                        buf[8 * (i - start)..8 * (i - start) + 8]
                            .copy_from_slice(&v.to_bits().to_le_bytes());
                    }
                    emit(w, &buf[..8 * (end - start)])?;
                    start += step;
                }
            }
        } else {
            let data = p.bytes(ds, part.bytes);
            if data.len() as u64 != part.size {
                return Err(bare_format());
            }
            emit(w, &data)?;
        }
        at = part.at + part.size;
    }
    if at != p.end {
        return Err(bare_format());
    }
    check(ctx)
}
