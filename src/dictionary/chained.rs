use super::dic::{DicDictionary, VrSpec};
use super::private_catalog::PrivateCatalog;
use crate::object::{Tag, Vr};
use std::fmt;
pub trait ChainedDictionary: Send + Sync + fmt::Debug {
    fn absolute_vr(&self, tag: Tag) -> Option<Option<Vr>>;
    fn vr_spec(&self, tag: Tag) -> Option<VrSpec>;
    fn private_vr(&self, creator: &str, tag: Tag) -> Option<Option<Vr>>;
    fn has_private_definitions(&self) -> bool;
}
impl ChainedDictionary for PrivateCatalog {
    fn absolute_vr(&self, _tag: Tag) -> Option<Option<Vr>> {
        None
    }
    fn vr_spec(&self, _tag: Tag) -> Option<VrSpec> {
        None
    }
    fn private_vr(&self, creator: &str, tag: Tag) -> Option<Option<Vr>> {
        self.by_private(creator, tag.group, tag.element as u8)
            .map(Some)
    }
    fn has_private_definitions(&self) -> bool {
        true
    }
}
impl ChainedDictionary for DicDictionary {
    fn absolute_vr(&self, tag: Tag) -> Option<Option<Vr>> {
        self.by_tag(tag).map(|e| e.vr)
    }
    fn vr_spec(&self, tag: Tag) -> Option<VrSpec> {
        self.vr_spec_by_tag(tag)
    }
    fn private_vr(&self, creator: &str, tag: Tag) -> Option<Option<Vr>> {
        self.by_private_tag(creator, tag).map(|e| e.vr)
    }
    fn has_private_definitions(&self) -> bool {
        DicDictionary::has_private_definitions(self)
    }
}
