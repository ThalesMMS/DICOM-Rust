use super::affine::{
    AffineObject, AffineRegistration, TRANSFORM_AFFINE, TRANSFORM_RIGID, TRANSFORM_RIGID_SCALE,
    TransformType, multiply_affine, validate_matrix_type,
};
use super::{Error, IDENTITY, Kind, Object, Registration};
use crate::dicomjson::trim_unicode_space;
use crate::render::affine::GeometryAffine;
use crate::render::vec3::Vec3;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
pub const TRANSFORM_DEFORMABLE: &str = "DEFORMABLE";
pub const DEFAULT_MAX_CHAIN_HOPS: i64 = 2;
#[derive(Clone, Debug, Default)]
pub struct Hop {
    pub sop_instance_uid: String,
    pub from: String,
    pub to: String,
    pub r#type: TransformType,
    pub source_to_registered: bool,
    pub(crate) affine: Option<Arc<AffineRegistration>>,
    pub(crate) deformable: Option<Arc<Registration>>,
}
impl Hop {
    pub fn map(&self, point: Vec3) -> Result<Vec3, Error> {
        if let Some(affine) = &self.affine {
            return if self.source_to_registered {
                affine.map_source_to_registered(point)
            } else {
                affine.map_registered_to_source(point)
            };
        }
        if let Some(deformable) = &self.deformable {
            if self.source_to_registered {
                let (mapped, err) = deformable.map_source_to_registered(point);
                return match err {
                    Some(err) => Err(err),
                    None => Ok(mapped),
                };
            }
            return deformable.map_registered_to_source(point);
        }
        Err(Error::new(Kind::InvalidObject, "empty hop"))
    }
    pub fn matrix(&self) -> Option<GeometryAffine> {
        let affine = self.affine.as_ref()?;
        Some(if self.source_to_registered {
            affine.source_to_registered
        } else {
            affine.registered_to_source
        })
    }
    pub fn deformation(&self) -> Option<&Registration> {
        self.deformable.as_deref()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.affine.is_none() && self.deformable.is_none()
    }
}
impl std::fmt::Display for Hop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let direction = if self.source_to_registered {
            " inverse"
        } else {
            ""
        };
        write!(
            f,
            "{}{} (SOP {})",
            self.r#type, direction, self.sop_instance_uid
        )
    }
}
pub fn hops(affine: &[&AffineObject], deformable: &[&Object]) -> Vec<Hop> {
    let mut out = Vec::new();
    for object in affine {
        append_affine_hops(&mut out, object, &object.sop_instance_uid);
    }
    for object in deformable {
        append_deformable_hops(&mut out, object, &object.sop_instance_uid);
    }
    out
}
pub(crate) fn append_affine_hops(hops: &mut Vec<Hop>, object: &AffineObject, sop_uid: &str) {
    let registered = trim_unicode_space(&object.registered_frame_of_reference_uid);
    for item in &object.registrations {
        let source = trim_unicode_space(&item.source_frame_of_reference_uid);
        if source.is_empty() || registered.is_empty() || source == registered {
            continue;
        }
        let shared = Arc::new(item.clone());
        hops.push(Hop {
            sop_instance_uid: sop_uid.to_string(),
            from: registered.to_string(),
            to: source.to_string(),
            r#type: item.r#type.clone(),
            source_to_registered: false,
            affine: Some(shared.clone()),
            deformable: None,
        });
        hops.push(Hop {
            sop_instance_uid: sop_uid.to_string(),
            from: source.to_string(),
            to: registered.to_string(),
            r#type: item.r#type.clone(),
            source_to_registered: true,
            affine: Some(shared),
            deformable: None,
        });
    }
}
pub(crate) fn append_deformable_hops(hops: &mut Vec<Hop>, object: &Object, sop_uid: &str) {
    let registered = trim_unicode_space(&object.registered_frame_of_reference_uid);
    for item in &object.registrations {
        let source = trim_unicode_space(&item.source_frame_of_reference_uid);
        if source.is_empty() || registered.is_empty() || source == registered {
            continue;
        }
        hops.push(Hop {
            sop_instance_uid: sop_uid.to_string(),
            from: registered.to_string(),
            to: source.to_string(),
            r#type: TRANSFORM_DEFORMABLE.to_string(),
            source_to_registered: false,
            affine: None,
            deformable: Some(Arc::new(item.clone())),
        });
    }
}
#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub hops: Vec<Hop>,
}
impl Chain {
    pub fn new(hops: &[Hop]) -> Result<Chain, Error> {
        if hops.is_empty() {
            return Err(Error::new(Kind::Chain, "no hops"));
        }
        let mut visited: HashSet<&str> = HashSet::new();
        visited.insert(&hops[0].from);
        for (index, hop) in hops.iter().enumerate() {
            if hop.is_empty() {
                return Err(Error::new(
                    Kind::Chain,
                    format!("hop {} is empty", index + 1),
                ));
            }
            if index > 0 && hops[index - 1].to != hop.from {
                return Err(Error::new(
                    Kind::Chain,
                    format!(
                        "hop {} starts in {}, not in {}",
                        index + 1,
                        hop.from,
                        hops[index - 1].to
                    ),
                ));
            }
            if !visited.insert(&hop.to) {
                return Err(Error::new(
                    Kind::Chain,
                    format!("Frame of Reference {} is visited twice", hop.to),
                ));
            }
        }
        Ok(Chain {
            hops: hops.to_vec(),
        })
    }
    pub fn from(&self) -> &str {
        self.hops.first().map_or("", |h| h.from.as_str())
    }
    pub fn to(&self) -> &str {
        self.hops.last().map_or("", |h| h.to.as_str())
    }
    pub fn map(&self, mut point: Vec3) -> Result<Vec3, Error> {
        if self.hops.is_empty() {
            return Err(Error::new(Kind::Chain, "no hops"));
        }
        for (index, hop) in self.hops.iter().enumerate() {
            point = hop.map(point).map_err(|e| {
                e.wrap(
                    Kind::Chain,
                    format!(
                        "hop {} ({}, SOP {})",
                        index + 1,
                        hop.r#type,
                        hop.sop_instance_uid
                    ),
                )
            })?;
        }
        Ok(point)
    }
    pub fn matrix(&self) -> (GeometryAffine, bool) {
        let mut matrix = IDENTITY;
        for hop in &self.hops {
            let Some(next) = hop.matrix() else {
                return (GeometryAffine::default(), false);
            };
            matrix = multiply_affine(&next, &matrix);
        }
        (matrix, !self.hops.is_empty())
    }
    pub fn transform_type(&self) -> TransformType {
        let (matrix, ok) = self.matrix();
        if !ok {
            return TRANSFORM_DEFORMABLE.to_string();
        }
        for kind in [TRANSFORM_RIGID, TRANSFORM_RIGID_SCALE] {
            if validate_matrix_type(&matrix, kind).is_ok() {
                return kind.to_string();
            }
        }
        TRANSFORM_AFFINE.to_string()
    }
    pub fn sop_instance_uids(&self) -> Vec<String> {
        self.hops
            .iter()
            .map(|h| h.sop_instance_uid.clone())
            .collect()
    }
}
impl std::fmt::Display for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<String> = self.hops.iter().map(Hop::to_string).collect();
        f.write_str(&parts.join(" then "))
    }
}
pub fn find_chains(hops: &[Hop], from: &str, to: &str, max_hops: i64) -> Vec<Chain> {
    let (from, to) = (trim_unicode_space(from), trim_unicode_space(to));
    let max_hops = if max_hops <= 0 {
        DEFAULT_MAX_CHAIN_HOPS
    } else {
        max_hops
    } as usize;
    if from.is_empty() || to.is_empty() || from == to {
        return Vec::new();
    }
    let mut outgoing: HashMap<&str, Vec<&Hop>> = HashMap::new();
    for hop in hops {
        outgoing.entry(hop.from.as_str()).or_default().push(hop);
    }
    struct Walk<'a> {
        from: &'a str,
        to: &'a str,
        max_hops: usize,
        outgoing: HashMap<&'a str, Vec<&'a Hop>>,
        found: Vec<Vec<Hop>>,
    }
    impl<'a> Walk<'a> {
        fn walk(&mut self, path: &mut Vec<&'a Hop>, visited: &mut HashSet<&'a str>) {
            let at = path.last().map_or(self.from, |h| h.to.as_str());
            if at == self.to {
                self.found.push(path.iter().map(|h| (*h).clone()).collect());
                return;
            }
            if path.len() == self.max_hops {
                return;
            }
            let next: Vec<&'a Hop> = self.outgoing.get(at).cloned().unwrap_or_default();
            for hop in next {
                if visited.contains(hop.to.as_str()) {
                    continue;
                }
                visited.insert(hop.to.as_str());
                path.push(hop);
                self.walk(path, visited);
                path.pop();
                visited.remove(hop.to.as_str());
            }
        }
    }
    let mut walk = Walk {
        from,
        to,
        max_hops,
        outgoing,
        found: Vec::new(),
    };
    let mut visited = HashSet::new();
    visited.insert(from);
    walk.walk(&mut Vec::new(), &mut visited);
    let mut chains = Vec::with_capacity(walk.found.len());
    for length in 1..=max_hops {
        for path in &walk.found {
            if path.len() == length {
                chains.push(Chain { hops: path.clone() });
            }
        }
    }
    chains
}
