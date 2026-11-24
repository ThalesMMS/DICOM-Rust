use super::checksum::metadata_checksum;
use super::{Decoder, File, MAX_BTREE_NODES, MAX_LINKS, format_err, le_uint, unsupported};
use crate::registration::error::RegError;
use std::collections::{HashMap, HashSet};
const MAX_LINK_HEAP_BYTES: u64 = 16 << 20;
pub(crate) struct LinkHeap {
    pub id_size: usize,
    pub managed_count: u64,
    offset_size: usize,
    length_size: usize,
    max_object: u64,
    blocks: Vec<Block>,
    objects: HashMap<u64, u64>,
}
struct Block {
    offset: u64,
    header: usize,
    data: Vec<u8>,
}
struct Reader<'f, 'a, 'h> {
    file: &'f File<'a>,
    heap: &'h mut LinkHeap,
    address: u64,
    start_size: u64,
    max_direct: u64,
    width: usize,
    checksummed: bool,
    remaining: u64,
    allocated: u64,
    seen: HashSet<u64>,
}
fn power_of_two(n: u64) -> bool {
    n != 0 && n & (n - 1) == 0
}
fn bits_len(n: u64) -> usize {
    64 - n.leading_zeros() as usize
}
impl File<'_> {
    pub(crate) fn open_link_heap(&self, addr: u64) -> Result<LinkHeap, RegError> {
        const HEADER_FIXED_SIZE: usize = 26;
        let data = self.read(
            addr,
            (HEADER_FIXED_SIZE + 3 * self.offset_size + 12 * self.length_size) as i64,
        )?;
        let mut d = Decoder::new(data.clone());
        if d.take(4) != b"FRHP" {
            return Err(format_err("missing fractal heap header"));
        }
        if d.u8() != 0 {
            return Err(unsupported("fractal heap header version"));
        }
        let id_size = usize::from(d.u16());
        let filters = d.u16();
        let flags = d.u8();
        if filters != 0 || flags & !3u8 != 0 {
            return Err(unsupported("filtered fractal heap or unknown heap flags"));
        }
        let max_object = u64::from(d.u32());
        d.skip(self.length_size + self.offset_size);
        let free_space = d.uint(self.length_size);
        d.skip(self.offset_size);
        let space = d.uint(self.length_size);
        let allocated = d.uint(self.length_size);
        let iterator = d.uint(self.length_size);
        let managed = d.uint(self.length_size);
        let huge_bytes = d.uint(self.length_size);
        let huge = d.uint(self.length_size);
        let tiny_bytes = d.uint(self.length_size);
        let tiny = d.uint(self.length_size);
        let width = d.u16();
        let start = d.uint(self.length_size);
        let max_direct = d.uint(self.length_size);
        let max_index = usize::from(d.u16());
        let initial_rows = usize::from(d.u16());
        let root = d.uint(self.offset_size);
        let root_rows = usize::from(d.u16());
        if d.u32() != metadata_checksum(&data[..data.len() - 4]) {
            return Err(format_err("fractal heap header checksum"));
        }
        if huge_bytes != 0 || huge != 0 || tiny_bytes != 0 || tiny != 0 {
            return Err(unsupported("huge or tiny fractal heap link object"));
        }
        if managed > MAX_LINKS || allocated > MAX_LINK_HEAP_BYTES {
            return Err(format_err("fractal heap resource limit"));
        }
        if free_space > space
            || allocated > space
            || iterator > space
            || !power_of_two(u64::from(width))
            || !power_of_two(start)
            || !power_of_two(max_direct)
            || start > max_direct
            || max_index == 0
            || max_index > 8 * self.length_size
        {
            return Err(format_err("fractal heap dimensions"));
        }
        let heap_offset_size = max_index.div_ceil(8);
        let heap_length_size = bits_len(max_object).div_ceil(8);
        let mut direct_header = 5 + self.offset_size + heap_offset_size;
        if flags & 2 != 0 {
            direct_header += 4;
        }
        if start <= direct_header as u64
            || max_object == 0
            || max_object > max_direct - direct_header as u64
        {
            return Err(format_err("fractal heap managed object size"));
        }
        if id_size != 1 + heap_offset_size + heap_length_size {
            return Err(unsupported("fractal heap managed ID size"));
        }
        let first_row_bits =
            start.trailing_zeros() as i64 + u64::from(width).trailing_zeros() as i64;
        let max_rows = max_index as i64 - first_row_bits + 1;
        if max_rows < 1 || root_rows as i64 > max_rows || initial_rows as i64 > max_rows {
            return Err(format_err("fractal heap doubling table rows"));
        }
        let mut heap = LinkHeap {
            id_size,
            managed_count: managed,
            offset_size: heap_offset_size,
            length_size: heap_length_size,
            max_object,
            blocks: Vec::new(),
            objects: HashMap::new(),
        };
        if self.undefined(root) {
            if space != 0 || allocated != 0 || managed != 0 || root_rows != 0 {
                return Err(format_err("missing fractal heap root"));
            }
            return Ok(heap);
        }
        let mut root_span = start;
        if root_rows != 0 {
            let shift = first_row_bits + root_rows as i64 - 1;
            if shift >= 64 {
                return Err(format_err("fractal heap address space overflows"));
            }
            root_span = 1u64 << shift;
        }
        if space != root_span {
            return Err(format_err("inconsistent fractal heap address space"));
        }
        let mut seen = HashSet::new();
        seen.insert(addr);
        let mut reader = Reader {
            file: self,
            heap: &mut heap,
            address: addr,
            start_size: start,
            max_direct,
            width: usize::from(width),
            checksummed: flags & 2 != 0,
            remaining: MAX_LINK_HEAP_BYTES - data.len() as u64,
            allocated,
            seen,
        };
        if root_rows == 0 {
            reader.direct(root, 0, start)?;
        } else {
            reader.indirect(root, 0, root_rows)?;
        }
        if reader.allocated != 0 {
            return Err(format_err("fractal heap allocated byte count"));
        }
        Ok(heap)
    }
}
impl Reader<'_, '_, '_> {
    fn block(
        &mut self,
        addr: u64,
        size: u64,
        signature: &str,
        offset: u64,
    ) -> Result<(Vec<u8>, usize), RegError> {
        if self.seen.contains(&addr) || self.seen.len() >= MAX_BTREE_NODES || size > self.remaining
        {
            return Err(format_err(
                "fractal heap cycle, repeated block or resource limit",
            ));
        }
        self.seen.insert(addr);
        self.remaining -= size;
        let data = self.file.read(addr, size as i64)?;
        let mut d = Decoder::new(data.clone());
        let ok = d.take(4) == signature.as_bytes()
            && d.u8() == 0
            && d.uint(self.file.offset_size) == self.address
            && d.uint(self.heap.offset_size) == offset
            && d.err.is_none();
        if !ok {
            return Err(format_err(format!(
                "invalid fractal heap {signature} block at address {addr}"
            )));
        }
        Ok((data, d.p))
    }
    fn direct(&mut self, addr: u64, offset: u64, size: u64) -> Result<(), RegError> {
        if size > self.allocated {
            return Err(format_err(
                "direct block exceeds fractal heap allocated space",
            ));
        }
        let (mut data, mut p) = self.block(addr, size, "FHDB", offset)?;
        if self.checksummed {
            let mut d = Decoder::at(data.clone(), p);
            let want = d.u32();
            if d.err.is_some() {
                return Err(format_err("fractal heap direct block checksum"));
            }
            data[p..p + 4].fill(0);
            let got = metadata_checksum(&data);
            data[p..p + 4].copy_from_slice(&want.to_le_bytes());
            p += 4;
            if got != want {
                return Err(format_err("fractal heap direct block checksum"));
            }
        }
        self.allocated -= size;
        self.heap.blocks.push(Block {
            offset,
            header: p,
            data,
        });
        Ok(())
    }
    fn indirect(&mut self, addr: u64, offset: u64, rows: usize) -> Result<(), RegError> {
        let header = 5 + self.file.offset_size + self.heap.offset_size;
        let size = header + rows * self.width * self.file.offset_size + 4;
        let (data, p) = self.block(addr, size as u64, "FHIB", offset)?;
        let stored = u32::from_le_bytes([
            data[size - 4],
            data[size - 3],
            data[size - 2],
            data[size - 1],
        ]);
        if stored != metadata_checksum(&data[..size - 4]) {
            return Err(format_err("fractal heap indirect block checksum"));
        }
        let mut d = Decoder::at(data, p);
        let first_row_bits =
            self.start_size.trailing_zeros() as i64 + (self.width as u64).trailing_zeros() as i64;
        let (mut child_offset, mut child_size) = (offset, self.start_size);
        for row in 0..rows {
            if row > 1 {
                child_size = child_size.wrapping_mul(2);
            }
            for _ in 0..self.width {
                let child = d.uint(self.file.offset_size);
                if !self.file.undefined(child) {
                    if child_size <= self.max_direct {
                        self.direct(child, child_offset, child_size)?;
                    } else {
                        let child_rows = bits_len(child_size) as i64 - first_row_bits;
                        if child_rows < 1 || child_rows >= rows as i64 {
                            return Err(format_err("invalid child fractal heap span"));
                        }
                        self.indirect(child, child_offset, child_rows as usize)?;
                    }
                }
                child_offset = child_offset.wrapping_add(child_size);
            }
        }
        Ok(())
    }
}
impl LinkHeap {
    pub(crate) fn link_object(&mut self, id: &[u8]) -> Result<Vec<u8>, RegError> {
        if id.len() != self.id_size {
            return Err(format_err("invalid fractal heap ID length"));
        }
        if id[0] & 0xf0 != 0 {
            return Err(unsupported("fractal heap ID type or version"));
        }
        if id[0] != 0 {
            return Err(format_err("fractal heap ID flags"));
        }
        let offset = le_uint(&id[1..1 + self.offset_size]);
        let size = le_uint(&id[1 + self.offset_size..1 + self.offset_size + self.length_size]);
        if size == 0 || size > self.max_object {
            return Err(format_err("fractal heap object size"));
        }
        let i = self.blocks.partition_point(|b| b.offset <= offset);
        if i == 0 {
            return Err(format_err("fractal heap link before first block"));
        }
        let block = &self.blocks[i - 1];
        let at = offset - block.offset;
        if at < block.header as u64
            || at >= block.data.len() as u64
            || size > block.data.len() as u64 - at
        {
            return Err(format_err("fractal heap link outside block data"));
        }
        if let Some(&known) = self.objects.get(&offset)
            && known != size
        {
            return Err(format_err("conflicting fractal heap object sizes"));
        }
        let out = block.data[at as usize..(at + size) as usize].to_vec();
        self.objects.insert(offset, size);
        if self.objects.len() as u64 > self.managed_count {
            return Err(format_err("too many fractal heap objects"));
        }
        Ok(out)
    }
    pub(crate) fn validate_objects(&self) -> Result<(), RegError> {
        if self.objects.len() as u64 != self.managed_count {
            return Err(format_err("fractal heap object count"));
        }
        let mut offsets: Vec<u64> = self.objects.keys().copied().collect();
        offsets.sort_unstable();
        let mut end = 0u64;
        for offset in offsets {
            if offset < end {
                return Err(format_err("overlapping fractal heap objects"));
            }
            end = offset.wrapping_add(self.objects[&offset]);
        }
        Ok(())
    }
}
