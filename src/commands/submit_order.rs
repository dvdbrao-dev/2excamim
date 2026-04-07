use crate::{
    commands::CommandError,
    events::{EventEnvelope, Linkage, OrderSubmitted, OrderSubmittedPayload, Provenance},
};

#[derive(Debug, Clone, PartialEq)]
pub struct SubmitOrderCommand {
    pub produced_by: String,
    pub provenance: Provenance,
    pub order_id: String,
    pub decision_id: Option<String>,
    pub hypothesis_id: Option<String>,
    pub signal_id: Option<String>,
    pub instrument: String,
    pub venue: String,
    pub parent_event_id: Option<String>,
    pub correlation_id: Option<String>,
}

impl SubmitOrderCommand {
    pub fn execute(&self) -> Result<EventEnvelope<OrderSubmitted>, CommandError> {
        let payload: OrderSubmittedPayload = OrderSubmitted {
            order_id: self.order_id.clone(),
            decision_id: self.decision_id.clone(),
            instrument: self.instrument.clone(),
            venue: self.venue.clone(),
        };

        let linkage = Linkage {
            hypothesis_id: self.hypothesis_id.clone(),
            signal_id: self.signal_id.clone(),
            decision_id: self.decision_id.clone(),
            order_id: Some(self.order_id.clone()),
            position_id: None,
            parent_event_id: self.parent_event_id.clone(),
            correlation_id: self.correlation_id.clone(),
        };

        EventEnvelope::new_order_submitted(
            self.produced_by.clone(),
            Some(self.instrument.clone()),
            linkage,
            self.provenance.clone(),
            payload,
        )
        .map_err(CommandError::from)
    }
}
