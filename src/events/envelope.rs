use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::events::{
    decision_formed::DecisionFormed,
    error::EventError,
    fill_received::FillReceived,
    hypothesis_generated::HypothesisGenerated,
    linkage::Linkage,
    order_registered::OrderRegistered,
    order_submitted::OrderSubmitted,
    provenance::Provenance,
    signal_confirmed::SignalConfirmed,
    signal_generated::SignalGenerated,
    validation::{validate_envelope, Validate},
    veto_raised::VetoRaised,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    #[serde(rename = "hypothesis.generated")]
    HypothesisGenerated,
    #[serde(rename = "signal.generated")]
    SignalGenerated,
    #[serde(rename = "signal.confirmed")]
    SignalConfirmed,
    #[serde(rename = "veto.raised")]
    VetoRaised,
    #[serde(rename = "decision.formed")]
    DecisionFormed,
    #[serde(rename = "order.registered")]
    OrderRegistered,
    #[serde(rename = "order.submitted")]
    OrderSubmitted,
    #[serde(rename = "fill.received")]
    FillReceived,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HypothesisGenerated => "hypothesis.generated",
            Self::SignalGenerated => "signal.generated",
            Self::SignalConfirmed => "signal.confirmed",
            Self::VetoRaised => "veto.raised",
            Self::DecisionFormed => "decision.formed",
            Self::OrderRegistered => "order.registered",
            Self::OrderSubmitted => "order.submitted",
            Self::FillReceived => "fill.received",
        }
    }
}

pub trait EventTyped {
    fn event_type() -> EventType;
    fn idempotency_key(&self) -> String;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope<TPayload> {
    pub event_id: String,
    pub event_type: EventType,
    pub schema_version: String,
    pub occurred_at: DateTime<Utc>,
    pub produced_by: String,
    pub idempotency_key: String,
    pub aggregate_key: Option<String>,
    pub linkage: Linkage,
    pub provenance: Provenance,
    pub payload: TPayload,
}

impl<TPayload> EventEnvelope<TPayload>
where
    TPayload: Validate + EventTyped,
{
    fn build(
        produced_by: impl Into<String>,
        aggregate_key: Option<String>,
        linkage: Linkage,
        provenance: Provenance,
        payload: TPayload,
    ) -> Result<Self, EventError> {
        let event = Self {
            event_id: Uuid::new_v4().to_string(),
            event_type: TPayload::event_type(),
            schema_version: "v1".to_string(),
            occurred_at: Utc::now(),
            produced_by: produced_by.into(),
            idempotency_key: payload.idempotency_key(),
            aggregate_key,
            linkage,
            provenance,
            payload,
        };

        validate_envelope(&event)?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), EventError> {
        validate_envelope(self)
    }
}

impl EventEnvelope<HypothesisGenerated> {
    pub fn new_hypothesis_generated(
        produced_by: impl Into<String>,
        aggregate_key: Option<String>,
        linkage: Linkage,
        provenance: Provenance,
        payload: HypothesisGenerated,
    ) -> Result<Self, EventError> {
        Self::build(produced_by, aggregate_key, linkage, provenance, payload)
    }
}

impl EventEnvelope<SignalGenerated> {
    pub fn new_signal_generated(
        produced_by: impl Into<String>,
        aggregate_key: Option<String>,
        linkage: Linkage,
        provenance: Provenance,
        payload: SignalGenerated,
    ) -> Result<Self, EventError> {
        Self::build(produced_by, aggregate_key, linkage, provenance, payload)
    }
}

impl EventEnvelope<SignalConfirmed> {
    pub fn new_signal_confirmed(
        produced_by: impl Into<String>,
        aggregate_key: Option<String>,
        linkage: Linkage,
        provenance: Provenance,
        payload: SignalConfirmed,
    ) -> Result<Self, EventError> {
        Self::build(produced_by, aggregate_key, linkage, provenance, payload)
    }
}

impl EventEnvelope<VetoRaised> {
    pub fn new_veto_raised(
        produced_by: impl Into<String>,
        aggregate_key: Option<String>,
        linkage: Linkage,
        provenance: Provenance,
        payload: VetoRaised,
    ) -> Result<Self, EventError> {
        Self::build(produced_by, aggregate_key, linkage, provenance, payload)
    }
}

impl EventEnvelope<DecisionFormed> {
    pub fn new_decision_formed(
        produced_by: impl Into<String>,
        aggregate_key: Option<String>,
        linkage: Linkage,
        provenance: Provenance,
        payload: DecisionFormed,
    ) -> Result<Self, EventError> {
        Self::build(produced_by, aggregate_key, linkage, provenance, payload)
    }
}

impl EventEnvelope<OrderRegistered> {
    pub fn new_order_registered(
        produced_by: impl Into<String>,
        aggregate_key: Option<String>,
        linkage: Linkage,
        provenance: Provenance,
        payload: OrderRegistered,
    ) -> Result<Self, EventError> {
        Self::build(produced_by, aggregate_key, linkage, provenance, payload)
    }
}

impl EventEnvelope<OrderSubmitted> {
    pub fn new_order_submitted(
        produced_by: impl Into<String>,
        aggregate_key: Option<String>,
        linkage: Linkage,
        provenance: Provenance,
        payload: OrderSubmitted,
    ) -> Result<Self, EventError> {
        Self::build(produced_by, aggregate_key, linkage, provenance, payload)
    }
}

impl EventEnvelope<FillReceived> {
    pub fn new_fill_received(
        produced_by: impl Into<String>,
        aggregate_key: Option<String>,
        linkage: Linkage,
        provenance: Provenance,
        payload: FillReceived,
    ) -> Result<Self, EventError> {
        Self::build(produced_by, aggregate_key, linkage, provenance, payload)
    }
}
