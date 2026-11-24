mod btree2;
mod checksum;
mod dense_links;
mod fractal_heap;
mod write;
use super::error::{RegError, Sentinel};
use crate::nifti::gzip::{Terminal, zlib_stream};
use crate::textfmt::quote;
use std::collections::HashMap;
pub use write::{WriteDataset, write};
pub(crate) fn format_err(detail: impl AsRef<str>) -> RegError {
    RegError::new(Sentinel::Hdf5Format, detail)
}
pub(crate) fn unsupported(detail: impl AsRef<str>) -> RegError {
    RegError::new(Sentinel::Hdf5Unsupported, detail)
}
fn not_found(detail: impl AsRef<str>) -> RegError {
    RegError::new(Sentinel::Hdf5NotFound, detail)
}
fn at_path(path: &str, e: RegError) -> RegError {
    e.prefixed(path)
}
pub const SIGNATURE: &[u8] = b"\x89HDF\r\n\x1a\n";
const MAX_HEADER_BLOCKS: usize = 256;
const MAX_MESSAGES: usize = 4096;
pub(crate) const MAX_BTREE_DEPTH: usize = 32;
pub(crate) const MAX_BTREE_NODES: usize = 1 << 16;
pub(crate) const MAX_LINKS: u64 = 1 << 16;
const MAX_CHUNK_BYTES: u64 = 1 << 30;
const MAX_PATH_COMPONENTS: usize = 64;
#[doc(hidden)]
pub fn is_hdf5(data: &[u8]) -> bool {
    data.starts_with(SIGNATURE)
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Link {
    pub name: Vec<u8>,
    pub address: u64,
}
pub struct File<'a> {
    data: &'a [u8],
    size: u64,
    pub(crate) base: u64,
    pub(crate) offset_size: usize,
    pub(crate) length_size: usize,
    root: u64,
    links: HashMap<u64, Vec<Link>>,
}
pub(crate) struct Decoder {
    pub b: Vec<u8>,
    pub p: usize,
    pub err: Option<RegError>,
}
impl Decoder {
    pub fn new(b: Vec<u8>) -> Decoder {
        Decoder { b, p: 0, err: None }
    }
    pub fn at(b: Vec<u8>, p: usize) -> Decoder {
        Decoder { b, p, err: None }
    }
    pub fn take(&mut self, n: i64) -> Vec<u8> {
        if self.err.is_some() || n < 0 || n > self.b.len() as i64 - self.p as i64 {
            if self.err.is_none() {
                self.err = Some(format_err("structure overruns its block"));
            }
            return vec![0; n.max(0) as usize];
        }
        let n = n as usize;
        let out = self.b[self.p..self.p + n].to_vec();
        self.p += n;
        out
    }
    pub fn skip(&mut self, n: usize) {
        self.take(n as i64);
    }
    pub fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    pub fn u16(&mut self) -> u16 {
        let b = self.take(2);
        u16::from_le_bytes([b[0], b[1]])
    }
    pub fn u32(&mut self) -> u32 {
        let b = self.take(4);
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }
    pub fn left(&self) -> i64 {
        self.b.len() as i64 - self.p as i64
    }
    pub fn uint(&mut self, n: usize) -> u64 {
        le_uint(&self.take(n as i64))
    }
    pub fn check(&self) -> Result<(), RegError> {
        match &self.err {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}
pub(crate) fn le_uint(b: &[u8]) -> u64 {
    let mut v = 0u64;
    for &x in b.iter().rev() {
        v = v << 8 | u64::from(x);
    }
    v
}
struct Message {
    kind: u16,
    flags: u8,
    data: Vec<u8>,
}
const MSG_NIL: u16 = 0x00;
const MSG_DATASPACE: u16 = 0x01;
const MSG_LINK_INFO: u16 = 0x02;
const MSG_DATATYPE: u16 = 0x03;
const MSG_LINK: u16 = 0x06;
const MSG_LAYOUT: u16 = 0x08;
const MSG_FILTERS: u16 = 0x0B;
const MSG_CONTINUATION: u16 = 0x10;
const MSG_SYMBOL_TABLE: u16 = 0x11;
impl<'a> File<'a> {
    pub fn open(data: &'a [u8]) -> Result<File<'a>, RegError> {
        let size = data.len() as u64;
        let mut f = File {
            data,
            size,
            base: 0,
            offset_size: 0,
            length_size: 0,
            root: 0,
            links: HashMap::new(),
        };
        let mut at: Option<u64> = None;
        let mut offset = 0u64;
        while offset + SIGNATURE.len() as u64 <= size {
            let o = offset as usize;
            if &data[o..o + SIGNATURE.len()] == SIGNATURE {
                at = Some(offset);
                break;
            }
            offset = 512u64.max(offset * 2);
        }
        let Some(at) = at else {
            return Err(format_err("no HDF5 signature"));
        };
        let n = 256u64.min(size - at) as usize;
        let header = data[at as usize..at as usize + n].to_vec();
        let mut d = Decoder::at(header, 8);
        let superblock = d.u8();
        match superblock {
            0 | 1 => {
                d.skip(4);
                f.offset_size = d.u8() as usize;
                f.length_size = d.u8() as usize;
                d.skip(1 + 2 + 2 + 4);
                if superblock == 1 {
                    d.skip(4);
                }
                f.check_sizes()?;
                f.base = d.uint(f.offset_size);
                d.skip(3 * f.offset_size);
                d.skip(f.offset_size);
                f.root = d.uint(f.offset_size);
            }
            2 | 3 => {
                f.offset_size = d.u8() as usize;
                f.length_size = d.u8() as usize;
                d.skip(1);
                f.check_sizes()?;
                f.base = d.uint(f.offset_size);
                d.skip(2 * f.offset_size);
                f.root = d.uint(f.offset_size);
            }
            v => {
                return Err(unsupported(format!("superblock version {v}")));
            }
        }
        if d.err.is_some() {
            return Err(format_err("truncated superblock"));
        }
        if f.base == 0 && at > 0 {
            f.base = at;
        }
        Ok(f)
    }
    fn check_sizes(&self) -> Result<(), RegError> {
        for n in [self.offset_size, self.length_size] {
            if n != 2 && n != 4 && n != 8 {
                return Err(format_err(format!("offset or length size {n}")));
            }
        }
        Ok(())
    }
    pub fn undefined(&self, addr: u64) -> bool {
        if self.offset_size == 8 {
            return addr == u64::MAX;
        }
        addr == (1u64 << (8 * self.offset_size)) - 1
    }
    pub fn read(&self, addr: u64, n: i64) -> Result<Vec<u8>, RegError> {
        if self.undefined(addr) {
            return Err(format_err("undefined address"));
        }
        let abs = self.base.wrapping_add(addr);
        if n < 0 || abs < addr || abs > self.size || n as u64 > self.size - abs {
            return Err(format_err(format!(
                "{n} bytes at {abs} beyond the end of the file"
            )));
        }
        Ok(self.data[abs as usize..abs as usize + n as usize].to_vec())
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    fn messages(&self, addr: u64) -> Result<Vec<Message>, RegError> {
        let size = self.size;
        let avail = size - size.min(self.base.wrapping_add(addr));
        let prefix = self.read(addr, 16.min(avail as i64))?;
        struct Block {
            addr: u64,
            length: u64,
        }
        let mut blocks = Vec::new();
        let mut v2 = false;
        let mut creation_order = false;
        let mut expected: i64 = -1;
        if prefix.len() >= 4 && &prefix[..4] == b"OHDR" {
            v2 = true;
            let avail = size.wrapping_sub(self.base.wrapping_add(addr)) as i64;
            let head = self.read(addr, (4 + 1 + 1 + 16 + 4 + 8i64).min(avail))?;
            let mut d = Decoder::at(head, 4);
            let version = d.u8();
            if version != 2 {
                return Err(unsupported(format!("object header version {version}")));
            }
            let flags = d.u8();
            creation_order = flags & 0x04 != 0;
            if flags & 0x20 != 0 {
                d.skip(16);
            }
            if flags & 0x10 != 0 {
                d.skip(4);
            }
            let size = d.uint(1 << (flags & 3));
            d.check()?;
            blocks.push(Block {
                addr: addr.wrapping_add(d.p as u64),
                length: size,
            });
        } else if prefix.len() >= 16 && prefix[0] == 1 {
            let mut d = Decoder::at(prefix, 2);
            expected = i64::from(d.u16());
            d.skip(4);
            let size = d.u32();
            blocks.push(Block {
                addr: addr.wrapping_add(16),
                length: u64::from(size),
            });
        } else {
            return Err(format_err(format!("no object header at {addr}")));
        }
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut i = 0;
        while i < blocks.len() {
            let (baddr, blength) = (blocks[i].addr, blocks[i].length);
            if blocks.len() > MAX_HEADER_BLOCKS || seen.contains(&baddr) {
                return Err(format_err("looping object header continuations"));
            }
            seen.insert(baddr);
            if blength > self.size {
                return Err(format_err(format!(
                    "object header block of {blength} bytes"
                )));
            }
            let data = self.read(baddr, blength as i64)?;
            let mut d = Decoder::new(data);
            if v2 && i > 0 {
                if d.take(4) != b"OCHK" {
                    return Err(format_err("missing continuation signature"));
                }
                let keep = (d.b.len() as i64 - 4).max(d.p as i64) as usize;
                d.b.truncate(keep);
            }
            let header: i64 = if v2 {
                if creation_order { 6 } else { 4 }
            } else {
                8
            };
            while d.left() >= header && d.err.is_none() {
                let (kind, size, flags);
                if v2 {
                    kind = u16::from(d.u8());
                    size = d.u16();
                    flags = d.u8();
                    if creation_order {
                        d.skip(2);
                    }
                } else {
                    kind = d.u16();
                    size = d.u16();
                    flags = d.u8();
                    d.skip(3);
                }
                let data = d.take(i64::from(size));
                d.check()?;
                if out.len() >= MAX_MESSAGES {
                    return Err(format_err("too many header messages"));
                }
                if kind == MSG_CONTINUATION {
                    let mut c = Decoder::new(data);
                    let next = Block {
                        addr: c.uint(self.offset_size),
                        length: c.uint(self.length_size),
                    };
                    c.check()?;
                    blocks.push(next);
                    continue;
                }
                if kind != MSG_NIL {
                    out.push(Message { kind, flags, data });
                }
                if expected >= 0 && out.len() as i64 >= expected {
                    break;
                }
            }
            i += 1;
        }
        Ok(out)
    }
    pub fn links(&mut self, path: &str) -> Result<Vec<Link>, RegError> {
        let addr = self.lookup(path)?;
        self.group_links(addr)
    }
    pub fn exists(&mut self, path: &str) -> bool {
        self.lookup(path).is_ok()
    }
    fn lookup(&mut self, path: &str) -> Result<u64, RegError> {
        let mut addr = self.root;
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        if parts.len() > MAX_PATH_COMPONENTS {
            return Err(not_found(format!("path {} is too deep", quote(path))));
        }
        let mut ancestors: Vec<u64> = Vec::new();
        for part in parts {
            if part.is_empty() {
                continue;
            }
            ancestors.push(addr);
            let links = self.group_links(addr)?;
            match links.iter().find(|l| l.name == part.as_bytes()) {
                Some(l) => addr = l.address,
                None => return Err(not_found(path)),
            }
            if ancestors.contains(&addr) {
                return Err(format_err(format!("cyclic group link in {}", quote(path))));
            }
        }
        Ok(addr)
    }
    fn group_links(&mut self, addr: u64) -> Result<Vec<Link>, RegError> {
        if let Some(links) = self.links.get(&addr) {
            return Ok(links.clone());
        }
        let messages = self.messages(addr)?;
        let mut links = Vec::new();
        let mut info = dense_links::LinkStorage::default();
        let (mut has_info, mut symbol_table, mut compact) = (false, false, false);
        for m in &messages {
            match m.kind {
                MSG_SYMBOL_TABLE => {
                    if symbol_table || has_info || compact {
                        return Err(format_err("conflicting group link storage"));
                    }
                    symbol_table = true;
                    let mut d = Decoder::new(m.data.clone());
                    let tree = d.uint(self.offset_size);
                    let heap = d.uint(self.offset_size);
                    d.check()?;
                    links = self.symbol_table(tree, heap)?;
                }
                MSG_LINK => {
                    if symbol_table {
                        return Err(format_err("conflicting group link storage"));
                    }
                    compact = true;
                    let l = self.link_message(&m.data)?;
                    if l.hard {
                        links.push(l.link);
                    }
                }
                MSG_LINK_INFO => {
                    if has_info || symbol_table {
                        return Err(format_err("conflicting group link storage"));
                    }
                    has_info = true;
                    info = self.link_info(&m.data)?;
                }
                _ => {}
            }
        }
        if !has_info && !symbol_table && !compact {
            return Err(not_found(format!("object at {addr} is not a group")));
        }
        if has_info && !self.undefined(info.heap) {
            if compact {
                return Err(format_err("both compact and dense links"));
            }
            links = self.dense_links(addr, &info)?;
        }
        self.links.insert(addr, links.clone());
        Ok(links)
    }
    pub(crate) fn link_message(&self, data: &[u8]) -> Result<DecodedLink, RegError> {
        let mut l = DecodedLink::default();
        let mut d = Decoder::new(data.to_vec());
        let version = d.u8();
        let flags = d.u8();
        d.check()?;
        if version != 1 || flags & !0x1f != 0 {
            return Err(unsupported("link message version or flags"));
        }
        let mut kind = 0u8;
        if flags & 0x08 != 0 {
            kind = d.u8();
        }
        l.ordered = flags & 0x04 != 0;
        if l.ordered {
            l.order = d.uint(8);
        }
        if flags & 0x10 != 0 && d.u8() > 1 {
            return Err(unsupported("link name character set"));
        }
        let length = d.uint(1 << (flags & 3));
        d.check()?;
        if length > d.left() as u64 {
            return Err(format_err("link name overruns its message"));
        }
        l.link.name = d.take(length as i64);
        if kind != 0 {
            return Ok(l);
        }
        l.hard = true;
        l.link.address = d.uint(self.offset_size);
        d.check()?;
        l.trailing = d.left();
        Ok(l)
    }
    fn symbol_table(&self, tree: u64, heap_addr: u64) -> Result<Vec<Link>, RegError> {
        let head = self.read(
            heap_addr,
            (8 + 2 * self.length_size + self.offset_size) as i64,
        )?;
        let mut d = Decoder::new(head);
        if d.take(4) != b"HEAP" {
            return Err(format_err("missing local heap"));
        }
        d.skip(4);
        let heap_size = d.uint(self.length_size);
        d.skip(self.length_size);
        let heap_data = d.uint(self.offset_size);
        if heap_size > self.size {
            return Err(format_err(format!("local heap of {heap_size} bytes")));
        }
        let heap = self.read(heap_data, heap_size as i64)?;
        let mut links: Vec<Link> = Vec::new();
        let mut nodes = 0usize;
        let entry_size = 2 * self.offset_size + 24;
        self.walk_btree(tree, 0, 0, &mut nodes, &mut |child, _| {
            let node = self.read(child, 8)?;
            if &node[..4] != b"SNOD" {
                return Err(format_err("missing symbol table node"));
            }
            let count = usize::from(u16::from_le_bytes([node[6], node[7]]));
            let entries = self.read(child.wrapping_add(8), (count * entry_size) as i64)?;
            let mut e = Decoder::new(entries);
            for _ in 0..count {
                let name_offset = e.uint(self.offset_size);
                let address = e.uint(self.offset_size);
                e.skip(24);
                e.check()?;
                if name_offset >= heap.len() as u64 {
                    return Err(format_err("link name outside the local heap"));
                }
                let mut name = &heap[name_offset as usize..];
                if let Some(end) = name.iter().position(|&b| b == 0) {
                    name = &name[..end];
                }
                if links.len() as u64 >= MAX_LINKS {
                    return Err(format_err("too many links"));
                }
                links.push(Link {
                    name: name.to_vec(),
                    address,
                });
            }
            Ok(())
        })?;
        Ok(links)
    }
    #[allow(clippy::type_complexity)]
    fn walk_btree(
        &self,
        addr: u64,
        kind: u8,
        dims: usize,
        nodes: &mut usize,
        visit: &mut dyn FnMut(u64, &[u8]) -> Result<(), RegError>,
    ) -> Result<(), RegError> {
        self.walk_node(addr, kind, dims, nodes, -1, 0, visit)
    }
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::type_complexity)]
    fn walk_node(
        &self,
        addr: u64,
        kind: u8,
        dims: usize,
        nodes: &mut usize,
        want_level: i64,
        depth: usize,
        visit: &mut dyn FnMut(u64, &[u8]) -> Result<(), RegError>,
    ) -> Result<(), RegError> {
        *nodes += 1;
        if *nodes > MAX_BTREE_NODES || depth > MAX_BTREE_DEPTH {
            return Err(format_err("B-tree too large or looping"));
        }
        let head = self.read(addr, (8 + 2 * self.offset_size) as i64)?;
        let head_len = head.len() as u64;
        let mut d = Decoder::new(head);
        if d.take(4) != b"TREE" {
            return Err(format_err("missing B-tree node"));
        }
        if d.u8() != kind {
            return Err(format_err("B-tree node type"));
        }
        let level = i64::from(d.u8());
        if want_level >= 0 && level != want_level {
            return Err(format_err("B-tree levels"));
        }
        let entries = usize::from(d.u16());
        let key_size = if kind == 1 {
            8 + 8 * dims
        } else {
            self.length_size
        };
        let body = self.read(
            addr.wrapping_add(head_len),
            (entries * (key_size + self.offset_size) + key_size) as i64,
        )?;
        let mut b = Decoder::new(body);
        for _ in 0..entries {
            let key = b.take(key_size as i64);
            let child = b.uint(self.offset_size);
            b.check()?;
            if level > 0 {
                self.walk_node(child, kind, dims, nodes, level - 1, depth + 1, visit)?;
            } else {
                visit(child, &key)?;
            }
        }
        Ok(())
    }
    fn dataset(&mut self, path: &str) -> Result<Dataset, RegError> {
        let addr = self.lookup(path)?;
        let messages = self.messages(addr)?;
        let mut ds = Dataset::default();
        let (mut have_space, mut have_type, mut have_layout) = (false, false, false);
        for m in &messages {
            if m.flags & 0x02 != 0
                && matches!(
                    m.kind,
                    MSG_DATATYPE | MSG_DATASPACE | MSG_LAYOUT | MSG_FILTERS
                )
            {
                return Err(unsupported(format!("shared message in {path}")));
            }
            match m.kind {
                MSG_DATASPACE => {
                    (ds.elements, ds.rank) = self.parse_dataspace(&m.data)?;
                    have_space = true;
                }
                MSG_DATATYPE => {
                    ds.dtype = parse_datatype(&m.data).map_err(|e| at_path(path, e))?;
                    have_type = true;
                }
                MSG_LAYOUT => {
                    ds.layout = self.parse_layout(&m.data)?;
                    have_layout = true;
                }
                MSG_FILTERS => {
                    ds.filters = parse_filters(&m.data)?;
                }
                _ => {}
            }
        }
        if !have_space || !have_type || !have_layout {
            return Err(not_found(format!("{path} is not a dataset")));
        }
        Ok(ds)
    }
    fn parse_dataspace(&self, data: &[u8]) -> Result<(u64, i64), RegError> {
        let mut d = Decoder::new(data.to_vec());
        let version = d.u8();
        let rank = i64::from(d.u8());
        d.skip(1);
        match version {
            1 => d.skip(5),
            2 => {
                if d.u8() == 2 {
                    return Ok((0, 0));
                }
            }
            v => return Err(unsupported(format!("dataspace version {v}"))),
        }
        let mut elements = 1u64;
        for _ in 0..rank {
            let n = d.uint(self.length_size);
            if n != 0 && elements > u64::MAX / n {
                return Err(format_err("dataspace too large"));
            }
            elements = elements.wrapping_mul(n);
        }
        d.check()?;
        Ok((elements, rank))
    }
    fn parse_layout(&self, data: &[u8]) -> Result<Layout, RegError> {
        let mut d = Decoder::new(data.to_vec());
        let version = d.u8();
        let mut l = Layout::default();
        match version {
            1 | 2 => {
                let rank = d.u8();
                l.class = d.u8();
                d.skip(5);
                if l.class != 0 {
                    l.address = d.uint(self.offset_size);
                }
                for _ in 0..rank {
                    l.chunk.push(d.u32());
                }
                if l.class == 0 {
                    let n = d.u32();
                    l.compact = d.take(i64::from(n));
                }
                if l.class != 2 {
                    l.chunk.clear();
                }
            }
            3 | 4 => {
                l.class = d.u8();
                match l.class {
                    0 => {
                        let n = d.u16();
                        l.compact = d.take(i64::from(n));
                    }
                    1 => {
                        l.address = d.uint(self.offset_size);
                        l.size = d.uint(self.length_size);
                    }
                    2 => {
                        if version == 4 {
                            return Err(unsupported("version 4 chunk indexes"));
                        }
                        let rank = d.u8();
                        l.address = d.uint(self.offset_size);
                        for _ in 0..rank {
                            l.chunk.push(d.u32());
                        }
                    }
                    c => return Err(unsupported(format!("layout class {c}"))),
                }
            }
            v => return Err(unsupported(format!("layout message version {v}"))),
        }
        d.check()?;
        if l.class == 2 && l.chunk.len() < 2 {
            return Err(format_err("chunk dimensionality"));
        }
        Ok(l)
    }
    fn raw(&self, ds: &Dataset, element_size: usize) -> Result<Vec<u8>, RegError> {
        let total = ds.elements.wrapping_mul(element_size as u64);
        if total > self.size.wrapping_mul(1024) || total > MAX_CHUNK_BYTES {
            return Err(format_err(format!("dataset of {total} bytes")));
        }
        let l = &ds.layout;
        match l.class {
            0 => {
                if (l.compact.len() as u64) < total {
                    return Err(format_err("short compact dataset"));
                }
                return Ok(l.compact[..total as usize].to_vec());
            }
            1 => {
                if self.undefined(l.address) {
                    return Ok(vec![0; total as usize]);
                }
                if !ds.filters.is_empty() {
                    return Err(format_err("filtered contiguous dataset"));
                }
                return self.read(l.address, total as i64);
            }
            _ => {}
        }
        if l.chunk.len() != 2 || ds.rank != 1 {
            return Err(unsupported(format!("chunked dataset of rank {}", ds.rank)));
        }
        if l.chunk[1] as usize != element_size {
            return Err(format_err(format!(
                "chunk element size {}, datatype {element_size}",
                l.chunk[1]
            )));
        }
        let mut out = vec![0u8; total as usize];
        if self.undefined(l.address) {
            return Ok(out);
        }
        let chunk_bytes = u64::from(l.chunk[0]) * element_size as u64;
        if l.chunk[0] == 0 || chunk_bytes > MAX_CHUNK_BYTES {
            return Err(format_err(format!("chunk of {} elements", l.chunk[0])));
        }
        let mut nodes = 0usize;
        let chunk0 = u64::from(l.chunk[0]);
        self.walk_btree(
            l.address,
            1,
            l.chunk.len(),
            &mut nodes,
            &mut |child, key| {
                let mut k = Decoder::new(key.to_vec());
                let size = k.u32();
                let mask = k.u32();
                let offset = le_uint(&k.take(8));
                k.check()?;
                if offset >= ds.elements || !offset.is_multiple_of(chunk0) {
                    return Err(format_err(format!("chunk offset {offset}")));
                }
                if u64::from(size) > self.size {
                    return Err(format_err(format!("chunk of {size} bytes")));
                }
                let stored = self.read(child, i64::from(size))?;
                let data = unfilter(stored, &ds.filters, mask, chunk_bytes as usize)?;
                let start = offset * element_size as u64;
                let n = (data.len() as u64).min(total - start).min(chunk_bytes) as usize;
                let start = start as usize;
                out[start..start + n].copy_from_slice(&data[..n]);
                Ok(())
            },
        )?;
        Ok(out)
    }
    #[doc(hidden)]
    pub fn read_float64s(&mut self, path: &str, limit: i64) -> Result<Vec<f64>, RegError> {
        let ds = self.dataset(path)?;
        if ds.rank > 1 {
            return Err(unsupported(format!("{path} has rank {}", ds.rank)));
        }
        if ds.dtype.class != 1 {
            return Err(unsupported(format!("{path} is not floating point")));
        }
        if ds.elements > limit as u64 {
            return Err(format_err(format!(
                "{path} has {} elements, at most {limit} are read",
                ds.elements
            )));
        }
        let raw = self
            .raw(&ds, ds.dtype.size as usize)
            .map_err(|e| at_path(path, e))?;
        let big = ds.dtype.big_endian;
        let out = (0..ds.elements as usize)
            .map(|i| {
                if ds.dtype.size == 8 {
                    let b: [u8; 8] = raw[8 * i..8 * i + 8].try_into().unwrap_or_default();
                    f64::from_bits(if big {
                        u64::from_be_bytes(b)
                    } else {
                        u64::from_le_bytes(b)
                    })
                } else {
                    let b: [u8; 4] = raw[4 * i..4 * i + 4].try_into().unwrap_or_default();
                    f64::from(f32::from_bits(if big {
                        u32::from_be_bytes(b)
                    } else {
                        u32::from_le_bytes(b)
                    }))
                }
            })
            .collect();
        Ok(out)
    }
    #[doc(hidden)]
    pub fn read_string(&mut self, path: &str) -> Result<String, RegError> {
        let ds = self.dataset(path)?;
        if ds.elements < 1 || ds.rank > 1 {
            return Err(format_err(format!("{path} holds no single string")));
        }
        if ds.dtype.class == 3 {
            let mut one = ds.clone();
            one.elements = 1;
            let mut raw = self
                .raw(&one, ds.dtype.size as usize)
                .map_err(|e| at_path(path, e))?;
            if let Some(end) = raw.iter().position(|&b| b == 0) {
                raw.truncate(end);
            }
            let s = lossy_text(&raw);
            return Ok(s.trim_end_matches(' ').to_string());
        }
        if ds.dtype.vlen_string {
            let size = 4 + self.offset_size + 4;
            let mut one = ds.clone();
            one.elements = 1;
            let raw = self.raw(&one, size).map_err(|e| at_path(path, e))?;
            let mut d = Decoder::new(raw);
            let length = d.u32();
            let collection = d.uint(self.offset_size);
            let index = d.u32();
            let mut value = self
                .global_heap_object(collection, index)
                .map_err(|e| at_path(path, e))?;
            if u64::from(length) > value.len() as u64 {
                return Err(format_err(format!(
                    "{path}: string longer than its heap object"
                )));
            }
            value.truncate(length as usize);
            if let Some(end) = value.iter().position(|&b| b == 0) {
                value.truncate(end);
            }
            return Ok(lossy_text(&value));
        }
        Err(unsupported(format!("{path} is not a string")))
    }
    fn global_heap_object(&self, addr: u64, index: u32) -> Result<Vec<u8>, RegError> {
        let head = self.read(addr, (8 + self.length_size) as i64)?;
        let head_len = head.len();
        let mut d = Decoder::new(head);
        if d.take(4) != b"GCOL" {
            return Err(format_err("missing global heap collection"));
        }
        d.skip(4);
        let size = d.uint(self.length_size);
        if size > self.size || size < head_len as u64 {
            return Err(format_err(format!(
                "global heap collection of {size} bytes"
            )));
        }
        let collection = self.read(addr, size as i64)?;
        let mut c = Decoder::at(collection, head_len);
        while c.left() >= (8 + self.length_size) as i64 {
            let id = c.u16();
            c.skip(6);
            let object_size = c.uint(self.length_size);
            if id == 0 || c.err.is_some() {
                break;
            }
            if object_size > c.left() as u64 {
                return Err(format_err("global heap object overruns its collection"));
            }
            let data = c.take(object_size as i64);
            let pad = (object_size % 8) as i64;
            if pad != 0 {
                let n = (8 - pad).min(c.left());
                c.skip(n as usize);
            }
            if u32::from(id) == index {
                return Ok(data);
            }
        }
        Err(format_err(format!("global heap object {index} not found")))
    }
}
fn lossy_text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}
#[derive(Debug, Clone, Default)]
pub(crate) struct DecodedLink {
    pub link: Link,
    pub order: u64,
    pub ordered: bool,
    pub hard: bool,
    pub trailing: i64,
}
#[derive(Debug, Clone, Default)]
struct Datatype {
    class: u8,
    size: u32,
    big_endian: bool,
    vlen_string: bool,
}
fn parse_datatype(data: &[u8]) -> Result<Datatype, RegError> {
    let mut d = Decoder::new(data.to_vec());
    let head = d.u8();
    let bits = d.take(3);
    let mut t = Datatype {
        class: head & 0x0F,
        size: d.u32(),
        ..Datatype::default()
    };
    d.check()?;
    match t.class {
        1 => {
            t.big_endian = bits[0] & 1 != 0;
            if bits[0] & 0x40 != 0 {
                return Err(unsupported("VAX floating point"));
            }
            if t.size != 4 && t.size != 8 {
                return Err(unsupported(format!("{}-byte floating point", t.size)));
            }
        }
        3 => {}
        9 => {
            if bits[0] & 0x0F != 1 {
                return Err(unsupported("variable-length sequence"));
            }
            t.vlen_string = true;
        }
        c => return Err(unsupported(format!("datatype class {c}"))),
    }
    Ok(t)
}
#[derive(Debug, Clone, Default)]
struct Layout {
    class: u8,
    address: u64,
    size: u64,
    compact: Vec<u8>,
    chunk: Vec<u32>,
}
#[derive(Debug, Clone, Default)]
struct Filter {
    id: u16,
    values: Vec<u32>,
}
fn parse_filters(data: &[u8]) -> Result<Vec<Filter>, RegError> {
    let mut d = Decoder::new(data.to_vec());
    let version = d.u8();
    let count = d.u8();
    if version == 1 {
        d.skip(6);
    } else if version != 2 {
        return Err(unsupported(format!("filter pipeline version {version}")));
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let mut fl = Filter {
            id: d.u16(),
            ..Filter::default()
        };
        let mut name_length = 0usize;
        if version == 1 || fl.id >= 256 {
            name_length = usize::from(d.u16());
        }
        d.skip(2);
        let values = d.u16();
        if version == 1 && !name_length.is_multiple_of(8) {
            name_length += 8 - name_length % 8;
        }
        d.skip(name_length);
        for _ in 0..values {
            fl.values.push(d.u32());
        }
        if version == 1 && values % 2 == 1 {
            d.skip(4);
        }
        d.check()?;
        match fl.id {
            1..=3 => {}
            id => return Err(unsupported(format!("filter {id}"))),
        }
        out.push(fl);
    }
    Ok(out)
}
fn unfilter(
    mut data: Vec<u8>,
    filters: &[Filter],
    mask: u32,
    limit: usize,
) -> Result<Vec<u8>, RegError> {
    for i in (0..filters.len()).rev() {
        if i < 32 && mask & (1 << i) != 0 {
            continue;
        }
        match filters[i].id {
            1 => {
                let mut stream = match zlib_stream(&data) {
                    Ok(s) => s,
                    Err(t) => return Err(format_err(format!("deflate: {}", t.text()))),
                };
                let out = stream.rest().to_vec();
                if out.len() > limit + 1 {
                    return Err(format_err("chunk inflates beyond its size"));
                }
                let _ = stream.read_full(out.len());
                match stream.read_full(1) {
                    Err(Terminal::Eof) | Ok(_) => {}
                    Err(t) => return Err(format_err(format!("deflate: {}", t.text()))),
                }
                if out.len() > limit {
                    return Err(format_err("chunk inflates beyond its size"));
                }
                data = out;
            }
            2 => {
                let size = filters[i].values.first().map_or(0, |&v| v as usize);
                if size > 1 {
                    let n = data.len() / size;
                    let mut out = vec![0u8; data.len()];
                    for b in 0..size {
                        for e in 0..n {
                            out[e * size + b] = data[b * n + e];
                        }
                    }
                    out[n * size..].copy_from_slice(&data[n * size..]);
                    data = out;
                }
            }
            3 => {
                if data.len() < 4 {
                    return Err(format_err("Fletcher-32 chunk"));
                }
                data.truncate(data.len() - 4);
            }
            _ => {}
        }
    }
    Ok(data)
}
#[derive(Debug, Clone, Default)]
struct Dataset {
    elements: u64,
    rank: i64,
    dtype: Datatype,
    layout: Layout,
    filters: Vec<Filter>,
}
