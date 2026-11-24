use super::Reader;
use crate::dictionary::chained::ChainedDictionary;
use crate::dictionary::private_catalog::PrivateCatalog;
use crate::dictionary::reservations::{
    PrivateError, PrivateReservations, is_private_creator_tag, is_private_data_tag,
};
use crate::object::dict;
use crate::object::error::{Error, Result};
use crate::object::types::{Element, ElementHeader, Tag, Value, Vr};
use std::fmt;
use std::sync::Arc;
pub const DEFAULT_MAX_PRIVATE_ISSUES: usize = 128;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateIssue {
    Reservation(PrivateError),
    DefinitionMissing,
}
impl fmt::Display for PrivateIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrivateIssue::Reservation(err) => err.fmt(f),
            PrivateIssue::DefinitionMissing => {
                f.write_str("dicom: private attribute definition missing")
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateDiagnostic {
    pub tag: Tag,
    pub offset: i64,
    pub item_offset: Option<i64>,
    pub err: PrivateIssue,
}
#[derive(Clone, Debug)]
pub struct PrivateDictionaryOptions {
    pub catalog: Arc<dyn ChainedDictionary>,
    pub max_private_creators: i64,
    pub max_private_diagnostics: i64,
    pub reject_invalid_private_creators: bool,
}
impl Default for PrivateDictionaryOptions {
    fn default() -> Self {
        PrivateDictionaryOptions {
            catalog: Arc::new(PrivateCatalog::default()),
            max_private_creators: 0,
            max_private_diagnostics: 0,
            reject_invalid_private_creators: false,
        }
    }
}
struct ItemScope {
    index: usize,
    base_offset: u64,
    reservations: PrivateReservations,
}
#[derive(Default)]
pub(crate) struct PrivateState {
    catalog: Option<Arc<dyn ChainedDictionary>>,
    active: bool,
    root: PrivateReservations,
    items: Vec<ItemScope>,
    max_creators: usize,
    max_diagnostics: usize,
    reject_invalid: bool,
    diagnostics: Vec<PrivateDiagnostic>,
    truncated: bool,
}
pub(super) fn creator_vr(tag: Tag, chained: Option<&dyn ChainedDictionary>) -> Vr {
    if let Some(e) = dict::by_tag(tag) {
        return e.vr;
    }
    match chained.and_then(|c| c.absolute_vr(tag)) {
        Some(Some(vr)) => vr,
        _ => Vr::LO,
    }
}
pub(super) fn chained_vr(tag: Tag, chained: &dyn ChainedDictionary) -> Vr {
    if dict::by_tag(tag).is_some() {
        return dict::lookup_vr(tag);
    }
    chained.absolute_vr(tag).flatten().unwrap_or(Vr::UN)
}
fn resource_limit() -> Error {
    Error::msg(PrivateError::ResourceLimit.to_string())
}
impl Reader {
    pub fn set_private_dictionary(&mut self, options: &PrivateDictionaryOptions) {
        if !options.catalog.has_private_definitions() {
            self.private = PrivateState {
                catalog: Some(options.catalog.clone()),
                ..PrivateState::default()
            };
            return;
        }
        let mut failure = None;
        if options.max_private_creators < 0 || options.max_private_diagnostics < 0 {
            failure = Some(resource_limit());
        }
        let max_creators = options.max_private_creators.max(0) as usize;
        let max_diagnostics = match options.max_private_diagnostics {
            n if n > 0 => n as usize,
            0 => DEFAULT_MAX_PRIVATE_ISSUES,
            _ => 0,
        };
        self.private = PrivateState {
            catalog: Some(options.catalog.clone()),
            active: true,
            root: PrivateReservations::new(max_creators),
            items: Vec::new(),
            max_creators,
            max_diagnostics,
            reject_invalid: options.reject_invalid_private_creators,
            diagnostics: Vec::new(),
            truncated: false,
        };
        if let Some(failure) = failure {
            self.init_error = Error::join(Some(failure), self.init_error.take());
        }
    }
    pub fn private_diagnostics(&self) -> (Vec<PrivateDiagnostic>, bool) {
        (self.private.diagnostics.clone(), self.private.truncated)
    }
    pub(super) fn private_active(&self) -> bool {
        self.private.active
    }
    pub(super) fn chained_dictionary(&self) -> Option<&dyn ChainedDictionary> {
        self.private.catalog.as_deref()
    }
    pub(super) fn single_dictionary_vr(&self, tag: Tag) -> Option<Vr> {
        if dict::by_tag(tag).is_some() {
            return dict::single_vr(tag);
        }
        let chained = self.chained_dictionary()?;
        let values = match chained.vr_spec(tag) {
            Some(spec) => spec.values,
            None => vec![chained.absolute_vr(tag)??],
        };
        match values.as_slice() {
            [vr] if *vr != Vr::UN => Some(*vr),
            _ => None,
        }
    }
    pub(super) fn dictionary_knows(&self, tag: Tag) -> bool {
        dict::by_tag(tag).is_some()
            || self
                .chained_dictionary()
                .is_some_and(|c| c.absolute_vr(tag).is_some())
    }
    fn private_scope(&mut self) -> &mut PrivateReservations {
        let innermost = self
            .seq_delimiters
            .last()
            .map(|t| t.item_index)
            .filter(|i| *i >= 0)
            .and_then(|i| Some((i as usize, self.seq_delimiters.get(i as usize)?.base_offset)));
        let stack = &self.seq_delimiters;
        self.private.items.retain(|scope| {
            stack
                .get(scope.index)
                .is_some_and(|t| t.base_offset == scope.base_offset)
        });
        let Some((index, base_offset)) = innermost else {
            return &mut self.private.root;
        };
        let at = match self.private.items.iter().position(|s| s.index == index) {
            Some(at) => at,
            None => {
                self.private.items.push(ItemScope {
                    index,
                    base_offset,
                    reservations: PrivateReservations::new(self.private.max_creators),
                });
                self.private.items.len().saturating_sub(1)
            }
        };
        match self.private.items.get_mut(at) {
            Some(scope) => &mut scope.reservations,
            None => &mut self.private.root,
        }
    }
    fn private_issue(&mut self, tag: Tag, err: PrivateIssue) {
        if self.private.diagnostics.len() >= self.private.max_diagnostics {
            self.private.truncated = true;
            return;
        }
        let diagnostic = PrivateDiagnostic {
            tag,
            offset: self.position(),
            item_offset: self.item_offset(),
            err,
        };
        self.private.diagnostics.push(diagnostic);
    }
    pub(super) fn capture_private_reservation(
        &mut self,
        header: &ElementHeader,
        value: Option<&[u8]>,
    ) -> Result<()> {
        if !self.private_active() || !is_private_creator_tag(header.tag) {
            return Ok(());
        }
        let element = Element {
            header: *header,
            value: match value {
                Some(raw) => Value::Raw(raw.to_vec()),
                None => Value::Nil,
            },
        };
        if let Err(err) = self.private_scope().observe(&element) {
            self.private_issue(header.tag, PrivateIssue::Reservation(err));
            if err == PrivateError::ResourceLimit || self.private.reject_invalid {
                return Err(Error::msg(err.to_string()));
            }
        }
        Ok(())
    }
    pub(super) fn lookup_private_vr(&mut self, tag: Tag) -> Vr {
        if is_private_creator_tag(tag) {
            return creator_vr(tag, self.chained_dictionary());
        }
        let scoped = self.private_scope().creator(tag);
        if let Some(entry) = dict::by_tag(tag) {
            return entry.vr;
        }
        let chained = self.private.catalog.clone();
        if let Some(c) = &chained
            && let Some(vr) = c.absolute_vr(tag)
        {
            return vr.unwrap_or(Vr::UN);
        }
        if let Ok(creator) = &scoped
            && is_private_data_tag(tag)
            && let Some(vr) = chained.as_ref().and_then(|c| c.private_vr(creator, tag))
        {
            return vr.unwrap_or(Vr::UN);
        }
        match scoped {
            Err(err) => self.private_issue(tag, PrivateIssue::Reservation(err)),
            Ok(_) => self.private_issue(tag, PrivateIssue::DefinitionMissing),
        }
        Vr::UN
    }
}
