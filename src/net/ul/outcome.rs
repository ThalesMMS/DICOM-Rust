#![forbid(unsafe_code)]
use super::pdu::*;
use std::collections::HashMap;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresentationContextOutcome {
    pub id: u8,
    pub abstract_syntax_uid: String,
    pub proposed_transfer_syntax_uids: Vec<String>,
    pub result: u8,
    pub transfer_syntax_uid: String,
}
impl PresentationContextOutcome {
    pub fn accepted(&self) -> bool {
        self.result == PRESENTATION_CONTEXT_ACCEPTANCE
    }
    pub fn result_name(&self) -> String {
        presentation_context_result_name(self.result)
    }
    pub fn explain(&self) -> String {
        let mut proposed = self.proposed_transfer_syntax_uids.join(",");
        if proposed.is_empty() {
            proposed = "<none>".to_string();
        }
        if self.accepted() {
            return format!(
                "presentation context {} abstract-syntax={} transfer-syntax={} result={}",
                self.id,
                self.abstract_syntax_uid,
                self.transfer_syntax_uid,
                self.result_name()
            );
        }
        format!(
            "presentation context {} abstract-syntax={} proposed-transfer-syntaxes=[{proposed}] result={}",
            self.id,
            self.abstract_syntax_uid,
            self.result_name()
        )
    }
}
pub(crate) fn presentation_context_result_name(result: u8) -> String {
    match result {
        PRESENTATION_CONTEXT_ACCEPTANCE => "acceptance".into(),
        PRESENTATION_CONTEXT_USER_REJECTION => "user-rejection".into(),
        PRESENTATION_CONTEXT_NO_REASON => "no-reason".into(),
        PRESENTATION_CONTEXT_ABSTRACT_SYNTAX_NOT_SUPPORTED => {
            "abstract-syntax-not-supported".into()
        }
        PRESENTATION_CONTEXT_TRANSFER_SYNTAXES_NOT_SUPPORTED => {
            "transfer-syntaxes-not-supported".into()
        }
        other => format!("unrecognized-result-{other}"),
    }
}
pub(crate) fn presentation_context_outcomes_from_results(
    proposed: &[PresentationContextProposed],
    results: &[PresentationContextResult],
) -> Vec<PresentationContextOutcome> {
    let by_id: HashMap<u8, &PresentationContextProposed> =
        proposed.iter().map(|pc| (pc.id, pc)).collect();
    results
        .iter()
        .map(|result| {
            let pc = by_id.get(&result.id);
            PresentationContextOutcome {
                id: result.id,
                abstract_syntax_uid: pc
                    .map(|p| p.abstract_syntax_uid.clone())
                    .unwrap_or_default(),
                proposed_transfer_syntax_uids: pc
                    .map(|p| p.transfer_syntax_uids.clone())
                    .unwrap_or_default(),
                result: result.result,
                transfer_syntax_uid: if result.result == PRESENTATION_CONTEXT_ACCEPTANCE {
                    result.transfer_syntax_uid.clone()
                } else {
                    String::new()
                },
            }
        })
        .collect()
}
