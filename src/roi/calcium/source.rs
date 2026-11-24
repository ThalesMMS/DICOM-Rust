use crate::dicomdir::pathrules::equal_fold;
use crate::object::{Object, Tag};
use crate::roi::Error;
use crate::textfmt::quote;
pub const ERR_NOT_CT: &str = "calcium: calcium scoring needs a CT image";
pub const ERR_CONTRAST_ENHANCED: &str = "calcium: calcium scoring needs a non-contrast CT";
const TAG_CONTRAST_BOLUS_AGENT: Tag = Tag::new(0x0018, 0x0010);
const TAG_CONTRAST_BOLUS_AGENT_SEQUENCE: Tag = Tag::new(0x0018, 0x0012);
const TAG_CONTRAST_BOLUS_USAGE_SEQUENCE: Tag = Tag::new(0x0018, 0x9341);
const TAG_CONTRAST_BOLUS_AGENT_ADMINISTERED: Tag = Tag::new(0x0018, 0x9342);
const TAG_CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
const TAG_MODALITY: Tag = Tag::new(0x0008, 0x0060);
pub fn contrast_agent(obj: Option<&Object>, frame_index: i64) -> Option<String> {
    let obj = obj?;
    if let Some(agent) = obj.get_string(TAG_CONTRAST_BOLUS_AGENT)
        && !agent.trim().is_empty()
    {
        return Some(agent.trim().to_string());
    }
    let (usage, _, _) = obj.functional_group_items(frame_index, TAG_CONTRAST_BOLUS_USAGE_SEQUENCE);
    let agents = obj
        .get_sequence(TAG_CONTRAST_BOLUS_AGENT_SEQUENCE)
        .unwrap_or_default();
    if !usage.is_empty() {
        for item in &usage {
            if let Some(administered) = item.get_string(TAG_CONTRAST_BOLUS_AGENT_ADMINISTERED)
                && equal_fold(administered.trim(), "YES")
            {
                return Some(agent_sequence_name(&agents));
            }
        }
        return None;
    }
    if !agents.is_empty() {
        return Some(agent_sequence_name(&agents));
    }
    None
}
fn agent_sequence_name(agents: &[Object]) -> String {
    for agent in agents {
        if let Some(meaning) = agent.get_string(TAG_CODE_MEANING)
            && !meaning.trim().is_empty()
        {
            return meaning.trim().to_string();
        }
    }
    "contrast agent".to_string()
}
pub fn check_source(obj: Option<&Object>, frame_index: i64) -> Result<(), Error> {
    let Some(obj) = obj else {
        return Err(Error::wrap(ERR_NOT_CT, "no image"));
    };
    let modality = obj.get_string(TAG_MODALITY).unwrap_or_default();
    if !equal_fold(modality.trim(), "CT") {
        return Err(Error::wrap(
            ERR_NOT_CT,
            format!("modality {}", quote(modality.trim())),
        ));
    }
    if let Some(agent) = contrast_agent(Some(obj), frame_index) {
        return Err(Error::wrap(
            ERR_CONTRAST_ENHANCED,
            format!("{agent} recorded"),
        ));
    }
    Ok(())
}
