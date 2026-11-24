use super::btree2::{LINK_CREATION_ORDER_INDEX, LINK_NAME_INDEX};
use super::checksum::metadata_checksum;
use super::{Decoder, File, Link, format_err, unsupported};
use crate::registration::error::RegError;
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LinkStorage {
    pub heap: u64,
    names: u64,
    creation_order: u64,
    tracks_creation_order: bool,
    indexes_creation_order: bool,
    max_creation_order: u64,
}
impl File<'_> {
    pub(crate) fn link_info(&self, data: &[u8]) -> Result<LinkStorage, RegError> {
        let mut info = LinkStorage::default();
        let mut d = Decoder::new(data.to_vec());
        let version = d.u8();
        let flags = d.u8();
        d.check()?;
        if version != 0 || flags & !3 != 0 {
            return Err(unsupported("link info version or flags"));
        }
        info.tracks_creation_order = flags & 1 != 0;
        info.indexes_creation_order = flags & 2 != 0;
        if info.indexes_creation_order && !info.tracks_creation_order {
            return Err(format_err("link order is indexed without tracking"));
        }
        if info.tracks_creation_order {
            info.max_creation_order = d.uint(8);
            if info.max_creation_order > i64::MAX as u64 {
                return Err(format_err("invalid maximum link creation order"));
            }
        }
        info.heap = d.uint(self.offset_size);
        info.names = d.uint(self.offset_size);
        if info.indexes_creation_order {
            info.creation_order = d.uint(self.offset_size);
        }
        d.check()?;
        let is_compact = self.undefined(info.heap);
        if self.undefined(info.names) != is_compact
            || (info.indexes_creation_order && self.undefined(info.creation_order) != is_compact)
        {
            return Err(format_err("inconsistent dense link index addresses"));
        }
        Ok(info)
    }
    pub(crate) fn dense_links(
        &self,
        group: u64,
        info: &LinkStorage,
    ) -> Result<Vec<Link>, RegError> {
        let mut h = self.open_link_heap(info.heap)?;
        let mut links: Vec<Link> = Vec::new();
        let mut by_id: HashMap<Vec<u8>, usize> = HashMap::new();
        let mut names: HashSet<Vec<u8>> = HashSet::new();
        let mut orders: HashSet<u64> = HashSet::new();
        let mut creation_order: Vec<u64> = Vec::new();
        let id_size = h.id_size;
        let size = self.size();
        let n = self.walk_link_btree(info.names, LINK_NAME_INDEX, id_size, &mut |hash, id| {
            if by_id.contains_key(id) {
                return Err(format_err("repeated link heap ID"));
            }
            let p = h.link_object(id)?;
            let l = self.link_message(&p)?;
            if !l.hard {
                return Err(unsupported("non-hard link in dense group"));
            }
            let name = &l.link.name;
            if l.trailing != 0
                || name.is_empty()
                || name.iter().any(|&b| b == b'/' || b == 0)
                || l.link.address == group
                || self.undefined(l.link.address)
                || l.link.address >= size - self.base.min(size)
            {
                return Err(format_err("invalid dense link name or target"));
            }
            if names.contains(name) || metadata_checksum(name) != hash as u32 {
                return Err(format_err("inconsistent dense name index"));
            }
            if l.ordered != info.tracks_creation_order
                || (l.ordered && (l.order >= info.max_creation_order || orders.contains(&l.order)))
            {
                return Err(format_err("inconsistent dense link creation order"));
            }
            names.insert(name.clone());
            if l.ordered {
                orders.insert(l.order);
            }
            by_id.insert(id.to_vec(), links.len());
            links.push(l.link.clone());
            creation_order.push(l.order);
            Ok(())
        })?;
        if n as u64 != h.managed_count {
            return Err(format_err("dense heap and name index counts differ"));
        }
        h.validate_objects()?;
        if !info.indexes_creation_order {
            return Ok(links);
        }
        let mut ordered = Vec::new();
        let o = self.walk_link_btree(
            info.creation_order,
            LINK_CREATION_ORDER_INDEX,
            id_size,
            &mut |order, id| match by_id.get(id) {
                Some(&index) if creation_order[index] == order => {
                    ordered.push(links[index].clone());
                    Ok(())
                }
                _ => Err(format_err("creation order index does not match link")),
            },
        )?;
        if o != n {
            return Err(format_err("dense link index counts differ"));
        }
        Ok(ordered)
    }
}
