#![forbid(unsafe_code)]
use super::error::{Error, Kind};
use super::obj::Object;
use super::types::{Element, Tag, Value};
pub const WALK_PATH_NO_ITEM: i64 = -1;
const DEFAULT_WALK_MAX_DEPTH: i64 = 64;
const DEFAULT_WALK_MAX_ELEMENTS: i64 = 100_000;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WalkOptions {
    pub max_depth: i64,
    pub max_elements: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkPathStep {
    pub tag: Tag,
    pub item_index: i64,
}
#[doc(hidden)]
pub fn walk_path_string(path: &[WalkPathStep]) -> String {
    path.iter()
        .map(|s| {
            if s.item_index >= 0 {
                format!("{}[{}]", s.tag, s.item_index)
            } else {
                s.tag.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}
fn normalize(opts: WalkOptions) -> Result<WalkOptions, Error> {
    if opts.max_depth < 0 || opts.max_elements < 0 {
        return Err(Error::kind_suffix(
            Kind::InvalidWalkOptions,
            ": limits must not be negative",
        ));
    }
    Ok(WalkOptions {
        max_depth: if opts.max_depth == 0 {
            DEFAULT_WALK_MAX_DEPTH
        } else {
            opts.max_depth
        },
        max_elements: if opts.max_elements == 0 {
            DEFAULT_WALK_MAX_ELEMENTS
        } else {
            opts.max_elements
        },
    })
}
fn limit_error(limit: &str, maximum: i64, visited: i64) -> Error {
    Error::kind_suffix(
        Kind::WalkResourceLimit,
        &format!(": {limit} maximum {maximum} after {visited} visits"),
    )
}
struct State<'a, F> {
    opts: WalkOptions,
    fn_: &'a mut F,
    path: Vec<WalkPathStep>,
    visited: i64,
}
impl<F: FnMut(&[WalkPathStep], &Element) -> Result<(), Error>> State<'_, F> {
    fn walk(&mut self, elements: &[Element], depth: i64) -> Result<(), Error> {
        for elem in elements {
            if self.visited >= self.opts.max_elements {
                return Err(limit_error(
                    "elements",
                    self.opts.max_elements,
                    self.visited,
                ));
            }
            let tag = elem.tag();
            self.path.push(WalkPathStep {
                tag,
                item_index: WALK_PATH_NO_ITEM,
            });
            let res = (self.fn_)(&self.path, elem);
            self.path.pop();
            res?;
            self.visited += 1;
            let Value::Sequence(items) = &elem.value else {
                continue;
            };
            if !items.is_empty() && depth >= self.opts.max_depth {
                return Err(limit_error("depth", self.opts.max_depth, self.visited));
            }
            for (i, item) in items.iter().enumerate() {
                self.path.push(WalkPathStep {
                    tag,
                    item_index: i as i64,
                });
                let res = self.walk(&item.elements, depth + 1);
                self.path.pop();
                res?;
            }
        }
        Ok(())
    }
}
impl Object {
    #[doc(hidden)]
    pub fn walk_path_with<F>(&self, opts: WalkOptions, mut f: F) -> Result<(), Error>
    where
        F: FnMut(&[WalkPathStep], &Element) -> Result<(), Error>,
    {
        let opts = normalize(opts)?;
        let mut state = State {
            opts,
            fn_: &mut f,
            path: Vec::new(),
            visited: 0,
        };
        state.walk(self.elements(), 0)
    }
    pub fn walk_path<F>(&self, f: F) -> Result<(), Error>
    where
        F: FnMut(&[WalkPathStep], &Element) -> Result<(), Error>,
    {
        self.walk_path_with(WalkOptions::default(), f)
    }
    pub fn walk_with<F>(&self, opts: WalkOptions, mut f: F) -> Result<(), Error>
    where
        F: FnMut(&[Tag], &Element) -> Result<(), Error>,
    {
        self.walk_path_with(opts, |path, elem| {
            let tags: Vec<Tag> = path.iter().map(|s| s.tag).collect();
            f(&tags, elem)
        })
    }
    pub fn walk<F>(&self, f: F) -> Result<(), Error>
    where
        F: FnMut(&[Tag], &Element) -> Result<(), Error>,
    {
        self.walk_with(WalkOptions::default(), f)
    }
}
