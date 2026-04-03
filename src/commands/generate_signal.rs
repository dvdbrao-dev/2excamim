use crate::{
    commands::CommandError,
    events::{
        EventEnvelope, Linkage, Provenance, SignalGenerated, SignalGeneratedPayload, SignalSide,
    },
};

#[derive(Debug, Clone, PartialEq)]
pub struct GenerateSignalCommand {
    pub produced_by: String,
    pub provenance: Provenance,
    pub signal_id: String,
    pub hypothesis_id: Option<String>,
    pub instrument: String,
    pub timeframe: String,
    pub side: SignalSide,
    pub strength: f64,
    pub rationale: Option<String>,
    pub parent_event_id: Option<String>,
    pub correlation_id: Option<String>,
}

impl GenerateSignalCommand {
    pub fn execute(&self) -> Result<EventEnvelope<SignalGenerated>, CommandError> {
        let payload: SignalGeneratedPayload = SignalGenerated {
            signal_id: self.signal_id.clone(),
            hypothesis_id: self.hypothesis_id.clone(),
            instrument: self.instrument.clone(),
            timeframe: self.timeframe.clone(),
            side: self.side,
            strength: self.strength,
            rationale: self.rationale.clone(),
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

        EventEnvelope::new_signal_generated(
            self.produced_by.clone(),
            Some(self.instrument.clone()),
            linkage,
            self.provenance.clone(),
            payload,
        )
        .map_err(CommandError::from)
    }
}
