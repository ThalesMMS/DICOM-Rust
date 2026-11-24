use super::affine::{AffineObject, TransformType};
use super::chain::{
    Chain, DEFAULT_MAX_CHAIN_HOPS, Hop, append_affine_hops, append_deformable_hops, find_chains,
};
use super::{Error, IDENTITY, Kind, Object, Registration, finite_vec};
use crate::dicomjson::trim_unicode_space;
use crate::render::affine::GeometryAffine;
use crate::render::vec3::Vec3;
use std::collections::{HashMap, HashSet, VecDeque};
#[derive(Clone, Debug, Default)]
pub struct RegistrationCandidate {
    pub sop_instance_uid: String,
    pub affine: Option<AffineObject>,
    pub deformable: Option<Object>,
}
pub const RESOLUTION_MISSING_FRAME: &str = "missing-frame";
pub const RESOLUTION_CYCLE: &str = "cycle";
pub const RESOLUTION_AMBIGUOUS_DIRECT: &str = "ambiguous-direct";
pub const RESOLUTION_AMBIGUOUS_CHAINS: &str = "ambiguous-chains";
pub const RESOLUTION_MISSING_SOURCE_FRAME: &str = "missing-source-frame";
pub const RESOLUTION_NO_LINK: &str = "no-link";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionError {
    pub reason: &'static str,
    pub count: usize,
}
impl std::fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.count > 0 {
            write!(
                f,
                "dicom/spatialreg: {} ({} alternatives)",
                self.reason, self.count
            )
        } else {
            write!(f, "dicom/spatialreg: {}", self.reason)
        }
    }
}
impl std::error::Error for ResolutionError {}
fn resolution_error(reason: &'static str, count: usize) -> Error {
    let text = if count > 0 {
        format!("dicom/spatialreg: {reason} ({count} alternatives)")
    } else {
        format!("dicom/spatialreg: {reason}")
    };
    Error::plain(text).with_resolution(ResolutionError { reason, count })
}
#[derive(Clone, Debug, Default)]
pub struct FrameResolution {
    same_frame: bool,
    chain: Chain,
}
impl FrameResolution {
    pub fn same_frame(&self) -> bool {
        self.same_frame
    }
    pub fn hops(&self) -> &[Hop] {
        &self.chain.hops
    }
    pub fn transform_type(&self) -> TransformType {
        match self.chain.hops.len() {
            0 => String::new(),
            1 => self.chain.hops[0].r#type.clone(),
            _ => self.chain.transform_type(),
        }
    }
    pub fn matrix(&self) -> (GeometryAffine, bool) {
        if self.same_frame {
            return (IDENTITY, true);
        }
        if self.chain.hops.len() == 1 {
            return match self.chain.hops[0].matrix() {
                Some(m) => (m, true),
                None => (GeometryAffine::default(), false),
            };
        }
        self.chain.matrix()
    }
    pub fn direct_deformation(&self) -> Option<(&Registration, bool)> {
        if self.chain.hops.len() == 1 {
            let hop = &self.chain.hops[0];
            if let Some(deformation) = hop.deformation() {
                return Some((deformation, hop.source_to_registered));
            }
        }
        None
    }
    pub fn map(&self, point: Vec3) -> Result<Vec3, Error> {
        if self.same_frame {
            if !finite_vec(point) {
                return Err(Error::new(Kind::InvalidObject, "non-finite patient point"));
            }
            return Ok(point);
        }
        if self.chain.hops.len() == 1 {
            return self.chain.hops[0].map(point);
        }
        self.chain.map(point)
    }
}
pub fn resolve_frames(
    from: &str,
    to: &str,
    candidates: &[RegistrationCandidate],
) -> Result<FrameResolution, Error> {
    let (from, to) = (trim_unicode_space(from), trim_unicode_space(to));
    if from.is_empty() || to.is_empty() {
        return Err(resolution_error(RESOLUTION_MISSING_FRAME, 0));
    }
    if from == to {
        return Ok(FrameResolution {
            same_frame: true,
            chain: Chain::default(),
        });
    }
    let mut hops = Vec::new();
    for candidate in candidates {
        if let Some(affine) = &candidate.affine {
            let uid = if candidate.sop_instance_uid.is_empty() {
                &affine.sop_instance_uid
            } else {
                &candidate.sop_instance_uid
            };
            append_affine_hops(&mut hops, affine, uid);
        }
        if let Some(deformable) = &candidate.deformable {
            let uid = if candidate.sop_instance_uid.is_empty() {
                &deformable.sop_instance_uid
            } else {
                &candidate.sop_instance_uid
            };
            append_deformable_hops(&mut hops, deformable, uid);
        }
    }
    if registration_cycle(&hops, from, to) {
        return Err(resolution_error(RESOLUTION_CYCLE, 0));
    }
    let mut direct = None;
    let mut direct_count = 0;
    for hop in &hops {
        let mut hop = hop.clone();
        if hop.deformable.is_some() && hop.from == to && hop.to == from {
            std::mem::swap(&mut hop.from, &mut hop.to);
            hop.source_to_registered = true;
        }
        if hop.from == from && hop.to == to {
            direct = Some(hop);
            direct_count += 1;
        }
    }
    if direct_count > 1 {
        return Err(resolution_error(RESOLUTION_AMBIGUOUS_DIRECT, direct_count));
    }
    if let Some(direct) = direct {
        return Ok(FrameResolution {
            same_frame: false,
            chain: Chain { hops: vec![direct] },
        });
    }
    let mut chains = find_chains(&hops, from, to, DEFAULT_MAX_CHAIN_HOPS);
    if chains.len() > 1 {
        return Err(resolution_error(RESOLUTION_AMBIGUOUS_CHAINS, chains.len()));
    }
    if let Some(chain) = chains.pop() {
        return Ok(FrameResolution {
            same_frame: false,
            chain,
        });
    }
    if missing_source_frame(candidates) {
        return Err(resolution_error(RESOLUTION_MISSING_SOURCE_FRAME, 0));
    }
    Err(resolution_error(RESOLUTION_NO_LINK, 0))
}
fn missing_source_frame(candidates: &[RegistrationCandidate]) -> bool {
    candidates.iter().any(|candidate| {
        candidate.affine.as_ref().is_some_and(|a| {
            a.registrations
                .iter()
                .any(|item| trim_unicode_space(&item.source_frame_of_reference_uid).is_empty())
        }) || candidate.deformable.as_ref().is_some_and(|d| {
            d.registrations
                .iter()
                .any(|item| trim_unicode_space(&item.source_frame_of_reference_uid).is_empty())
        })
    })
}
fn registration_cycle(hops: &[Hop], from: &str, to: &str) -> bool {
    let mut directed: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut neighbors: HashMap<&str, Vec<&str>> = HashMap::new();
    for hop in hops {
        if hop.source_to_registered {
            continue;
        }
        let (source, registered) = (hop.to.as_str(), hop.from.as_str());
        directed.entry(source).or_default().push(registered);
        neighbors.entry(source).or_default().push(registered);
        neighbors.entry(registered).or_default().push(source);
    }
    let mut reached: Vec<&str> = vec![from];
    let mut seen: HashSet<&str> = HashSet::from([from, to]);
    if to != from {
        reached.push(to);
    }
    let mut queue: VecDeque<&str> = reached.iter().copied().collect();
    while let Some(node) = queue.pop_front() {
        for &neighbor in neighbors.get(node).map_or(&[][..], |v| v.as_slice()) {
            if seen.insert(neighbor) {
                reached.push(neighbor);
                queue.push_back(neighbor);
            }
        }
    }
    fn visit<'a>(
        node: &'a str,
        directed: &HashMap<&'a str, Vec<&'a str>>,
        state: &mut HashMap<&'a str, u8>,
    ) -> bool {
        state.insert(node, 1);
        for &next in directed.get(node).map_or(&[][..], |v| v.as_slice()) {
            let s = state.get(next).copied().unwrap_or(0);
            if s == 1 || (s == 0 && visit(next, directed, state)) {
                return true;
            }
        }
        state.insert(node, 2);
        false
    }
    let mut state: HashMap<&str, u8> = HashMap::new();
    for node in reached {
        if state.get(node).copied().unwrap_or(0) == 0 && visit(node, &directed, &mut state) {
            return true;
        }
    }
    false
}
