use chrono::{DateTime, Utc};

use crate::{
    commands::CommandError,
    events::{EventEnvelope, FillReceived, FillReceivedPayload, FillSide, Linkage, Provenance},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ObserveFillCommand {
    pub produced_by: String,
    pub provenance: Provenance,
    pub fill_id: String,
    pub decision_id: Option<String>,
    pub hypothesis_id: Option<String>,
    pub signal_id: Option<String>,
    pub order_id: String,
    pub instrument: String,
    pub side: FillSide,
    pub quantity: f64,
    pub price: f64,
    pub venue: String,
    pub executed_at: DateTime<Utc>,
    pub parent_event_id: Option<String>,
    pub correlation_id: Option<String>,
}

impl ObserveFillCommand {
    pub fn execute(&self) -> Result<EventEnvelope<FillReceived>, CommandError> {
        let payload: FillReceivedPayload = FillReceived {
            fill_id: self.fill_id.clone(),
            decision_id: self.decision_id.clone(),
            order_id: self.order_id.clone(),
            instrument: self.instrument.clone(),
            side: self.side,
            quantity: self.quantity,
            price: self.price,
            venue: self.venue.clone(),
            executed_at: self.executed_at,
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

        EventEnvelope::new_fill_received(
            self.produced_by.clone(),
            Some(self.instrument.clone()),
            linkage,
            self.provenance.clone(),
            payload,
        )
        .map_err(CommandError::from)
    }
}
