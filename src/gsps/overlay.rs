use super::{Error, State, graphic_layer_name, invalid, signed_shorts};
use crate::deid::trim_space;
use crate::derivedio as dio;
use crate::display::{Overlay, Shutter, overlays_from_object};
use crate::object::{Element, Object, Tag, Vr};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverlayActivation {
    pub group: u16,
    pub layer: String,
}
pub(crate) fn read_overlay_activations(obj: &Object) -> Vec<OverlayActivation> {
    let mut out = Vec::new();
    let mut group: u16 = 0x6000;
    while group <= 0x601E {
        if let Some(layer) = obj.get_string(Tag::new(group, 0x1001)) {
            out.push(OverlayActivation {
                group,
                layer: trim_space(&layer).to_string(),
            });
        }
        group += 2;
    }
    out
}
pub(crate) fn read_state_overlays(obj: &Object, shutter: Option<&Shutter>) -> Vec<Overlay> {
    let (overlays, _) = overlays_from_object(obj);
    match shutter.and_then(|s| s.bitmap.as_ref()) {
        None => overlays,
        Some(bitmap) => overlays
            .into_iter()
            .filter(|o| o.group != bitmap.group)
            .collect(),
    }
}
pub fn active_overlays(state: Option<&State>, image_overlays: &[Overlay]) -> Vec<Overlay> {
    let Some(state) = state else {
        return Vec::new();
    };
    let find = |list: &[Overlay], group: u16| list.iter().rev().find(|o| o.group == group).cloned();
    let mut out = Vec::new();
    for activation in &state.overlay_activations {
        if let Some(o) = find(&state.overlays, activation.group) {
            out.push(o);
        } else if let Some(o) = find(image_overlays, activation.group) {
            out.push(o);
        }
    }
    out.sort_by_key(|o| o.group);
    out
}
fn hex04(v: u16) -> String {
    format!("0x{v:04x}")
}
pub(crate) fn overlay_activation_elements(state: &State) -> Result<Vec<Element>, Error> {
    let mut out = Vec::new();
    let shutter_group = state
        .shutter
        .as_ref()
        .and_then(|s| s.bitmap.as_ref())
        .map_or(0, |b| b.group);
    for overlay in &state.overlays {
        let group = overlay.group;
        if !(0x6000..=0x601E).contains(&group) || group % 2 != 0 || group == shutter_group {
            return Err(invalid(format!("overlay group {}", hex04(group))));
        }
        if let Err(e) = overlay.validate() {
            return Err(invalid(format!(
                "overlay {} dimensions or data: {e}",
                hex04(group)
            )));
        }
        let overlay_type = if overlay.overlay_type == "R" {
            "R"
        } else {
            "G"
        };
        let mut data = overlay.data.clone();
        if data.len() % 2 != 0 {
            data.push(0);
        }
        out.extend([
            dio::us(Tag::new(group, 0x0010), &[overlay.rows as u16]),
            dio::us(Tag::new(group, 0x0011), &[overlay.columns as u16]),
            dio::cs(Tag::new(group, 0x0040), overlay_type),
            signed_shorts(
                Tag::new(group, 0x0050),
                &[overlay.origin_row as i16, overlay.origin_col as i16],
            ),
            dio::us(Tag::new(group, 0x0100), &[1]),
            dio::us(Tag::new(group, 0x0102), &[0]),
            dio::raw(Tag::new(group, 0x3000), Vr::OW, data),
        ]);
        if !overlay.label.is_empty() {
            out.push(dio::lo(Tag::new(group, 0x1500), &overlay.label));
        }
    }
    for activation in &state.overlay_activations {
        let group = activation.group;
        if !(0x6000..=0x601E).contains(&group) || group % 2 != 0 {
            return Err(invalid(format!(
                "overlay activation group {}",
                hex04(group)
            )));
        }
        out.push(dio::cs(
            Tag::new(group, 0x1001),
            &graphic_layer_name(&activation.layer),
        ));
    }
    Ok(out)
}
