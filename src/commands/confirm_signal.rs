use crate::{
    commands::CommandError,
    events::{EventEnvelope, Linkage, Provenance, SignalConfirmed, SignalConfirmedPayload},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmSignalCommand {
    pub produced_by: String,
    pub provenance: Provenance,
    pub aggregate_key: Option<String>,
    pub signal_id: String,
    pub hypothesis_id: Option<String>,
    pub confirmed_by: String,
    pub confirmation_reason: Option<String>,
    pub confirmation_score: Option<f64>,
    pub parent_event_id: Option<String>,
    pub correlation_id: Option<String>,
}

impl ConfirmSignalCommand {
    pub fn execute(&self) -> Result<EventEnvelope<SignalConfirmed>, CommandError> {
        let payload: SignalConfirmedPayload = SignalConfirmed {
            signal_id: self.signal_id.clone(),
            confirmed_by: self.confirmed_by.clone(),
            confirmation_reason: self.confirmation_reason.clone(),
            confirmation_score: self.confirmation_score,
        };

        let linkage = Linkage {
            hypothesis_id: self.hypothesis_id.clone(),
            signal_id: Some(self.signal_id.clone()),
            decision_id: None,
            order_id: None,
            position_id: None,
            parent_event_id: self.parent_event_id.clone(),
            correlation_id: self.correlation_id.clone(),
        };

        EventEnvelope::new_signal_confirmed(
            self.produced_by.clone(),
            self.aggregate_key.clone(),
            linkage,
            self.provenance.clone(),
            payload,
        )
        .map_err(CommandError::from)
    }
}
