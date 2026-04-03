use crate::{
    commands::CommandError,
    events::{
        DecisionAction, DecisionFormed, DecisionFormedPayload, EventEnvelope, Linkage, Provenance,
        SignalSide,
    },
};

#[derive(Debug, Clone, PartialEq)]
pub struct FormDecisionCommand {
    pub produced_by: String,
    pub provenance: Provenance,
    pub decision_id: String,
    pub hypothesis_id: Option<String>,
    pub signal_id: Option<String>,
    pub instrument: String,
    pub action: DecisionAction,
    pub side: Option<SignalSide>,
    pub size_hint: Option<f64>,
    pub rationale: Option<String>,
    pub parent_event_id: Option<String>,
    pub correlation_id: Option<String>,
}

impl FormDecisionCommand {
    pub fn execute(&self) -> Result<EventEnvelope<DecisionFormed>, CommandError> {
        let payload: DecisionFormedPayload = DecisionFormed {
            decision_id: self.decision_id.clone(),
            instrument: self.instrument.clone(),
            action: self.action,
            side: self.side,
            size_hint: self.size_hint,
            rationale: self.rationale.clone(),
        };

        let linkage = Linkage {
            hypothesis_id: self.hypothesis_id.clone(),
            signal_id: self.signal_id.clone(),
            decision_id: Some(self.decision_id.clone()),
            order_id: None,
            position_id: None,
            parent_event_id: self.parent_event_id.clone(),
            correlation_id: self.correlation_id.clone(),
        };

        EventEnvelope::new_decision_formed(
            self.produced_by.clone(),
            Some(self.instrument.clone()),
            linkage,
            self.provenance.clone(),
            payload,
        )
        .map_err(CommandError::from)
    }
}
