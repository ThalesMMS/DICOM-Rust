use super::checksum::metadata_checksum;
use super::{
    Decoder, File, MAX_BTREE_DEPTH, MAX_BTREE_NODES, MAX_LINKS, format_err, le_uint, unsupported,
};
use crate::registration::error::RegError;
use std::collections::HashSet;
pub(crate) const LINK_NAME_INDEX: u8 = 5;
pub(crate) const LINK_CREATION_ORDER_INDEX: u8 = 6;
pub(crate) const DENSE_LINK_ID_SIZE: usize = 7;
const DENSE_NODE_PREFIX: usize = 6;
const DENSE_NODE_OVERHEAD: usize = DENSE_NODE_PREFIX + 4;
const MAX_DENSE_BTREE_NODE_BYTES: u32 = 1 << 20;
#[derive(Clone, Copy, Default)]
struct Level {
    capacity: usize,
    child_total_size: usize,
}
#[allow(clippy::type_complexity)]
struct LinkBTree<'f, 'a, 'v> {
    file: &'f File<'a>,
    kind: u8,
    key_size: usize,
    record_size: usize,
    node_size: usize,
    child_count_size: usize,
    levels: Vec<Level>,
    seen: HashSet<u64>,
    remaining: i64,
    last_key: u64,
    visited: bool,
    visit: &'v mut dyn FnMut(u64, &[u8]) -> Result<(), RegError>,
}
fn uint_bytes(n: u64) -> usize {
    (64 - n.leading_zeros() as usize).div_ceil(8)
}
impl File<'_> {
    #[allow(clippy::type_complexity)]
    pub(crate) fn walk_link_btree(
        &self,
        addr: u64,
        kind: u8,
        id_size: usize,
        visit: &mut dyn FnMut(u64, &[u8]) -> Result<(), RegError>,
    ) -> Result<i64, RegError> {
        let key_size = match kind {
            LINK_NAME_INDEX => 4,
            LINK_CREATION_ORDER_INDEX => 8,
            _ => return Err(unsupported(format!("v2 link B-tree type {kind}"))),
        };
        if id_size != DENSE_LINK_ID_SIZE {
            return Err(unsupported(format!("dense link B-tree ID size {id_size}")));
        }
        let header = self.read(addr, (22 + self.offset_size + self.length_size) as i64)?;
        let mut d = Decoder::new(header.clone());
        if d.take(4) != b"BTHD" {
            return Err(format_err("missing dense link B-tree header"));
        }
        let version = d.u8();
        if version != 0 {
            return Err(unsupported(format!("link B-tree version {version}")));
        }
        if d.u8() != kind {
            return Err(format_err("incorrect dense link B-tree type"));
        }
        let node_size = d.u32();
        let record_size = usize::from(d.u16());
        let depth = usize::from(d.u16());
        d.skip(2);
        let root = d.uint(self.offset_size);
        let root_records = usize::from(d.u16());
        let total = d.uint(self.length_size);
        let checksum = d.u32();
        if metadata_checksum(&header[..header.len() - 4]) != checksum {
            return Err(format_err("dense link B-tree header checksum"));
        }
        if record_size != key_size + id_size
            || node_size < (DENSE_NODE_OVERHEAD + record_size) as u32
            || node_size > MAX_DENSE_BTREE_NODE_BYTES
            || depth > MAX_BTREE_DEPTH
            || total > MAX_LINKS
        {
            return Err(format_err("dense link B-tree dimensions or record limit"));
        }
        if total == 0 && depth == 0 && root_records == 0 && self.undefined(root) {
            return Ok(0);
        }
        if self.undefined(root) || root_records == 0 || root_records as u64 > total {
            return Err(format_err("invalid dense link B-tree root"));
        }
        let leaf_capacity = (node_size as usize - DENSE_NODE_OVERHEAD) / record_size;
        if leaf_capacity > usize::from(u16::MAX) {
            return Err(format_err("dense link B-tree leaf capacity"));
        }
        let mut index = LinkBTree {
            file: self,
            kind,
            key_size,
            record_size,
            node_size: node_size as usize,
            child_count_size: uint_bytes(leaf_capacity as u64),
            levels: vec![Level::default(); depth + 1],
            seen: HashSet::new(),
            remaining: total as i64,
            last_key: 0,
            visited: false,
            visit,
        };
        index.levels[0].capacity = leaf_capacity;
        let mut cumulative = leaf_capacity as u64;
        for level in 1..=depth {
            let total_size = if level > 1 { uint_bytes(cumulative) } else { 0 };
            let pointer_size = self.offset_size + index.child_count_size + total_size;
            let space = index.node_size as i64 - DENSE_NODE_OVERHEAD as i64 - pointer_size as i64;
            if space < (record_size + pointer_size) as i64 {
                return Err(format_err("dense link B-tree internal node capacity"));
            }
            let capacity = space as usize / (record_size + pointer_size);
            index.levels[level] = Level {
                capacity,
                child_total_size: total_size,
            };
            let cap = capacity as u64;
            if cumulative > (u64::MAX - cap) / (cap + 1) {
                cumulative = u64::MAX;
            } else {
                cumulative = (cap + 1) * cumulative + cap;
            }
        }
        index.walk(root, depth, root_records as i64, total as i64)?;
        Ok(total as i64)
    }
}
impl LinkBTree<'_, '_, '_> {
    fn walk(
        &mut self,
        addr: u64,
        depth: usize,
        records: i64,
        expected: i64,
    ) -> Result<(), RegError> {
        if self.seen.contains(&addr) || self.seen.len() >= MAX_BTREE_NODES {
            return Err(format_err("repeated dense link B-tree node or node limit"));
        }
        self.seen.insert(addr);
        let level = self.levels[depth];
        if records <= 0
            || records > level.capacity as i64
            || records > expected
            || expected > self.remaining
        {
            return Err(format_err("invalid dense link B-tree node count"));
        }
        let f = self.file;
        let absolute = f.base.wrapping_add(addr);
        if absolute < addr || absolute > f.size() || self.node_size as u64 > f.size() - absolute {
            return Err(format_err("dense link B-tree node outside file"));
        }
        let records = records as usize;
        let mut signature: &[u8] = b"BTLF";
        let mut used = DENSE_NODE_OVERHEAD + records * self.record_size;
        if depth > 0 {
            signature = b"BTIN";
            used +=
                (records + 1) * (f.offset_size + self.child_count_size + level.child_total_size);
        }
        let node = f.read(addr, used as i64)?;
        if &node[..4] != signature || node[4] != 0 || node[5] != self.kind {
            return Err(format_err(
                "invalid dense link B-tree node signature, version or type",
            ));
        }
        let n = node.len();
        let stored = u32::from_le_bytes([node[n - 4], node[n - 3], node[n - 2], node[n - 1]]);
        if metadata_checksum(&node[..n - 4]) != stored {
            return Err(format_err("dense link B-tree node checksum"));
        }
        let body = &node[DENSE_NODE_PREFIX..n - 4];
        let keys = &body[..records * self.record_size];
        let mut children = Decoder::new(body[keys.len()..].to_vec());
        let start = self.remaining;
        for i in 0..=records {
            if depth > 0 {
                let child = children.uint(f.offset_size);
                let child_records = children.uint(self.child_count_size) as i64;
                let mut child_total = child_records as u64;
                if level.child_total_size > 0 {
                    child_total = children.uint(level.child_total_size);
                }
                if child_total > self.remaining as u64 {
                    return Err(format_err("invalid dense link B-tree subtree count"));
                }
                self.walk(child, depth - 1, child_records, child_total as i64)?;
            }
            if i == records {
                break;
            }
            let record = &keys[i * self.record_size..(i + 1) * self.record_size];
            let key = le_uint(&record[..self.key_size]);
            if self.visited
                && (key < self.last_key
                    || key == self.last_key && self.kind == LINK_CREATION_ORDER_INDEX)
            {
                return Err(format_err("unordered dense link B-tree records"));
            }
            if self.remaining == 0 {
                return Err(format_err("too many dense link B-tree records"));
            }
            self.last_key = key;
            self.visited = true;
            self.remaining -= 1;
            (self.visit)(key, &record[self.key_size..])?;
        }
        if start - self.remaining != expected {
            return Err(format_err("dense link B-tree subtree count mismatch"));
        }
        Ok(())
    }
}
