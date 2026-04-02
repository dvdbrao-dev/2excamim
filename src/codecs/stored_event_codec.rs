use serde::{de::DeserializeOwned, Serialize};

use crate::{
    codecs::CodecError,
    events::{
        DecisionFormed, EventEnvelope, EventType, EventTyped, FillReceived, HypothesisGenerated,
        SignalConfirmed, SignalGenerated, Validate, VetoRaised,
    },
    store::StoredEvent,
};

#[derive(Debug, Clone, PartialEq)]
pub enum RehydratedEvent {
    HypothesisGenerated(EventEnvelope<HypothesisGenerated>),
    SignalGenerated(EventEnvelope<SignalGenerated>),
    SignalConfirmed(EventEnvelope<SignalConfirmed>),
    VetoRaised(EventEnvelope<VetoRaised>),
    DecisionFormed(EventEnvelope<DecisionFormed>),
    FillReceived(EventEnvelope<FillReceived>),
}

impl RehydratedEvent {
    pub fn event_type(&self) -> EventType {
        match self {
            Self::HypothesisGenerated(_) => EventType::HypothesisGenerated,
            Self::SignalGenerated(_) => EventType::SignalGenerated,
            Self::SignalConfirmed(_) => EventType::SignalConfirmed,
            Self::VetoRaised(_) => EventType::VetoRaised,
            Self::DecisionFormed(_) => EventType::DecisionFormed,
            Self::FillReceived(_) => EventType::FillReceived,
        }
    }
}

impl TryFrom<StoredEvent> for RehydratedEvent {
    type Error = CodecError;

    fn try_from(value: StoredEvent) -> Result<Self, Self::Error> {
        match value.event_type {
            EventType::HypothesisGenerated => {
                Ok(Self::HypothesisGenerated(rehydrate_envelope(value)?))
            }
            EventType::SignalGenerated => Ok(Self::SignalGenerated(rehydrate_envelope(value)?)),
            EventType::SignalConfirmed => Ok(Self::SignalConfirmed(rehydrate_envelope(value)?)),
            EventType::VetoRaised => Ok(Self::VetoRaised(rehydrate_envelope(value)?)),
            EventType::DecisionFormed => Ok(Self::DecisionFormed(rehydrate_envelope(value)?)),
            EventType::FillReceived => Ok(Self::FillReceived(rehydrate_envelope(value)?)),
        }
    }
}

impl TryFrom<&StoredEvent> for RehydratedEvent {
    type Error = CodecError;

    fn try_from(value: &StoredEvent) -> Result<Self, Self::Error> {
        value.clone().try_into()
    }
}

impl TryFrom<&RehydratedEvent> for StoredEvent {
    type Error = crate::store::StoreError;

    fn try_from(value: &RehydratedEvent) -> Result<Self, Self::Error> {
        match value {
            RehydratedEvent::HypothesisGenerated(event) => Self::try_from(event),
            RehydratedEvent::SignalGenerated(event) => Self::try_from(event),
            RehydratedEvent::SignalConfirmed(event) => Self::try_from(event),
            RehydratedEvent::VetoRaised(event) => Self::try_from(event),
            RehydratedEvent::DecisionFormed(event) => Self::try_from(event),
            RehydratedEvent::FillReceived(event) => Self::try_from(event),
        }
    }
}

impl TryFrom<RehydratedEvent> for StoredEvent {
    type Error = crate::store::StoreError;

    fn try_from(value: RehydratedEvent) -> Result<Self, Self::Error> {
        Self::try_from(&value)
    }
}

fn rehydrate_envelope<TPayload>(stored: StoredEvent) -> Result<EventEnvelope<TPayload>, CodecError>
where
    TPayload: DeserializeOwned + Serialize + Validate + EventTyped,
{
    let event_type = stored.event_type;
    let payload = serde_json::from_value(stored.payload)
        .map_err(|source| CodecError::payload_decode(event_type, source))?;
    let envelope = EventEnvelope {
        event_id: stored.event_id,
        event_type,
        schema_version: stored.schema_version,
        occurred_at: stored.occurred_at,
        produced_by: stored.produced_by,
        idempotency_key: stored.idempotency_key,
        aggregate_key: stored.aggregate_key,
        linkage: stored.linkage,
        provenance: stored.provenance,
        payload,
    };

    envelope.validate().map_err(CodecError::validation)?;
    Ok(envelope)
}
