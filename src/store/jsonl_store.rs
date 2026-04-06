use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::events::{
    validation::{validate_envelope, Validate},
    DecisionFormed, EventEnvelope, EventType, EventTyped, FillReceived, HypothesisGenerated,
    Linkage, OrderRegistered, Provenance, SignalConfirmed, SignalGenerated, VetoRaised, VetoScope,
};

use super::error::StoreError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredEvent {
    pub event_id: String,
    pub event_type: EventType,
    pub schema_version: String,
    pub occurred_at: DateTime<Utc>,
    pub produced_by: String,
    pub idempotency_key: String,
    pub aggregate_key: Option<String>,
    pub linkage: Linkage,
    pub provenance: Provenance,
    pub payload: Value,
}

impl StoredEvent {
    pub fn validate(&self) -> Result<(), StoreError> {
        validate_required_string(&self.event_id, "event_id")?;
        Uuid::parse_str(&self.event_id)
            .map_err(|_| StoreError::invalid_data("event_id must be a valid UUID"))?;
        validate_required_string(&self.schema_version, "schema_version")?;
        if self.schema_version != "v1" {
            return Err(StoreError::invalid_data("schema_version must be v1"));
        }
        validate_required_string(&self.produced_by, "produced_by")?;
        validate_required_string(&self.idempotency_key, "idempotency_key")?;
        validate_optional_string(self.aggregate_key.as_deref(), "aggregate_key")?;
        self.linkage
            .validate()
            .map_err(|error| StoreError::invalid_data(error.to_string()))?;
        self.provenance
            .validate()
            .map_err(|error| StoreError::invalid_data(error.to_string()))?;
        if !self.payload.is_object() {
            return Err(StoreError::invalid_data("payload must be a JSON object"));
        }

        match self.event_type {
            EventType::HypothesisGenerated => {
                let payload = self.validate_typed_payload::<HypothesisGenerated>()?;
                validate_matching_ref(
                    self.linkage.hypothesis_id.as_deref(),
                    Some(payload.hypothesis_id.as_str()),
                    "linkage.hypothesis_id",
                    "payload.hypothesis_id",
                )?;
            }
            EventType::SignalGenerated => {
                let payload = self.validate_typed_payload::<SignalGenerated>()?;
                validate_matching_ref(
                    self.linkage.signal_id.as_deref(),
                    Some(payload.signal_id.as_str()),
                    "linkage.signal_id",
                    "payload.signal_id",
                )?;
                validate_matching_ref(
                    self.linkage.hypothesis_id.as_deref(),
                    payload.hypothesis_id.as_deref(),
                    "linkage.hypothesis_id",
                    "payload.hypothesis_id",
                )?;
            }
            EventType::SignalConfirmed => {
                let payload = self.validate_typed_payload::<SignalConfirmed>()?;
                validate_matching_ref(
                    self.linkage.signal_id.as_deref(),
                    Some(payload.signal_id.as_str()),
                    "linkage.signal_id",
                    "payload.signal_id",
                )?;
            }
            EventType::VetoRaised => {
                let payload = self.validate_typed_payload::<VetoRaised>()?;
                match payload.scope {
                    VetoScope::Signal => validate_matching_ref(
                        self.linkage.signal_id.as_deref(),
                        Some(payload.target_id.as_str()),
                        "linkage.signal_id",
                        "payload.target_id",
                    )?,
                    VetoScope::Decision => validate_matching_ref(
                        self.linkage.decision_id.as_deref(),
                        Some(payload.target_id.as_str()),
                        "linkage.decision_id",
                        "payload.target_id",
                    )?,
                    VetoScope::Order => validate_matching_ref(
                        self.linkage.order_id.as_deref(),
                        Some(payload.target_id.as_str()),
                        "linkage.order_id",
                        "payload.target_id",
                    )?,
                    VetoScope::Global => {}
                }
            }
            EventType::DecisionFormed => {
                let payload = self.validate_typed_payload::<DecisionFormed>()?;
                validate_matching_ref(
                    self.linkage.decision_id.as_deref(),
                    Some(payload.decision_id.as_str()),
                    "linkage.decision_id",
                    "payload.decision_id",
                )?;
            }
            EventType::OrderRegistered => {
                let payload = self.validate_typed_payload::<OrderRegistered>()?;
                validate_matching_ref(
                    self.linkage.order_id.as_deref(),
                    Some(payload.order_id.as_str()),
                    "linkage.order_id",
                    "payload.order_id",
                )?;
                validate_matching_ref(
                    self.linkage.decision_id.as_deref(),
                    payload.decision_id.as_deref(),
                    "linkage.decision_id",
                    "payload.decision_id",
                )?;
            }
            EventType::FillReceived => {
                let payload = self.validate_typed_payload::<FillReceived>()?;
                validate_matching_ref(
                    self.linkage.order_id.as_deref(),
                    Some(payload.order_id.as_str()),
                    "linkage.order_id",
                    "payload.order_id",
                )?;
                validate_matching_ref(
                    self.linkage.decision_id.as_deref(),
                    payload.decision_id.as_deref(),
                    "linkage.decision_id",
                    "payload.decision_id",
                )?;
            }
        }

        Ok(())
    }

    fn validate_typed_payload<TPayload>(&self) -> Result<TPayload, StoreError>
    where
        TPayload: DeserializeOwned + Serialize + Clone + Validate + EventTyped,
    {
        let payload: TPayload = serde_json::from_value(self.payload.clone()).map_err(|error| {
            StoreError::invalid_data(format!(
                "payload does not match {} schema: {error}",
                self.event_type.as_str()
            ))
        })?;
        let envelope = EventEnvelope {
            event_id: self.event_id.clone(),
            event_type: self.event_type,
            schema_version: self.schema_version.clone(),
            occurred_at: self.occurred_at,
            produced_by: self.produced_by.clone(),
            idempotency_key: self.idempotency_key.clone(),
            aggregate_key: self.aggregate_key.clone(),
            linkage: self.linkage.clone(),
            provenance: self.provenance.clone(),
            payload: payload.clone(),
        };

        validate_envelope(&envelope)
            .map_err(|error| StoreError::invalid_data(error.to_string()))?;
        Ok(payload)
    }
}

impl<TPayload> TryFrom<&EventEnvelope<TPayload>> for StoredEvent
where
    TPayload: Serialize,
{
    type Error = StoreError;

    fn try_from(value: &EventEnvelope<TPayload>) -> Result<Self, Self::Error> {
        let stored = Self {
            event_id: value.event_id.clone(),
            event_type: value.event_type,
            schema_version: value.schema_version.clone(),
            occurred_at: value.occurred_at,
            produced_by: value.produced_by.clone(),
            idempotency_key: value.idempotency_key.clone(),
            aggregate_key: value.aggregate_key.clone(),
            linkage: value.linkage.clone(),
            provenance: value.provenance.clone(),
            payload: serde_json::to_value(&value.payload)?,
        };

        stored.validate()?;
        Ok(stored)
    }
}

impl<TPayload> TryFrom<EventEnvelope<TPayload>> for StoredEvent
where
    TPayload: Serialize,
{
    type Error = StoreError;

    fn try_from(value: EventEnvelope<TPayload>) -> Result<Self, Self::Error> {
        let stored = Self {
            event_id: value.event_id,
            event_type: value.event_type,
            schema_version: value.schema_version,
            occurred_at: value.occurred_at,
            produced_by: value.produced_by,
            idempotency_key: value.idempotency_key,
            aggregate_key: value.aggregate_key,
            linkage: value.linkage,
            provenance: value.provenance,
            payload: serde_json::to_value(value.payload)?,
        };

        stored.validate()?;
        Ok(stored)
    }
}

#[derive(Debug, Clone)]
pub struct JsonlEventStore {
    path: PathBuf,
}

impl JsonlEventStore {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = path.into();
        ensure_store_file(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append_event(&self, event: &StoredEvent) -> Result<bool, StoreError> {
        event.validate()?;

        if let Some(existing) = self.find_existing_by_idempotency_key(&event.idempotency_key)? {
            if existing.is_contractually_equivalent(event) {
                return Ok(false);
            }

            return Err(StoreError::invalid_data(format!(
                "conflicting event already exists for idempotency_key {}",
                event.idempotency_key
            )));
        }

        self.append_serialized(event)?;
        Ok(true)
    }

    pub fn append_events(&self, events: &[StoredEvent]) -> Result<usize, StoreError> {
        if events.is_empty() {
            return Ok(0);
        }

        let mut existing_events = self
            .read_all()?
            .into_iter()
            .map(|event| (event.idempotency_key.clone(), event))
            .collect::<std::collections::HashMap<_, _>>();
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;
        let mut appended = 0usize;

        for event in events {
            event.validate()?;

            if let Some(existing) = existing_events.get(&event.idempotency_key) {
                if existing.is_contractually_equivalent(event) {
                    continue;
                }

                return Err(StoreError::invalid_data(format!(
                    "conflicting event already exists for idempotency_key {}",
                    event.idempotency_key
                )));
            }

            serde_json::to_writer(&mut file, event)?;
            file.write_all(b"\n")?;
            existing_events.insert(event.idempotency_key.clone(), event.clone());
            appended += 1;
        }

        file.flush()?;
        Ok(appended)
    }

    pub fn read_all(&self) -> Result<Vec<StoredEvent>, StoreError> {
        ensure_store_file(&self.path)?;

        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let event: StoredEvent = serde_json::from_str(&line).map_err(|error| {
                StoreError::invalid_data(format!(
                    "failed to parse JSONL line {}: {error}",
                    index + 1
                ))
            })?;
            event.validate().map_err(|error| {
                StoreError::invalid_data(format!("invalid event at line {}: {error}", index + 1))
            })?;
            events.push(event);
        }

        Ok(events)
    }

    pub fn replay(&self) -> Result<std::vec::IntoIter<StoredEvent>, StoreError> {
        Ok(self.read_all()?.into_iter())
    }

    pub fn exists_by_idempotency_key(&self, idempotency_key: &str) -> Result<bool, StoreError> {
        Ok(self
            .read_all()?
            .into_iter()
            .any(|event| event.idempotency_key == idempotency_key))
    }

    pub fn find_by_event_type(
        &self,
        event_type: EventType,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        Ok(self
            .read_all()?
            .into_iter()
            .filter(|event| event.event_type == event_type)
            .collect())
    }

    pub fn find_by_correlation_id(
        &self,
        correlation_id: &str,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        Ok(self
            .read_all()?
            .into_iter()
            .filter(|event| event.linkage.correlation_id.as_deref() == Some(correlation_id))
            .collect())
    }

    pub fn find_by_signal_id(&self, signal_id: &str) -> Result<Vec<StoredEvent>, StoreError> {
        Ok(self
            .read_all()?
            .into_iter()
            .filter(|event| event.linkage.signal_id.as_deref() == Some(signal_id))
            .collect())
    }

    pub fn find_by_decision_id(&self, decision_id: &str) -> Result<Vec<StoredEvent>, StoreError> {
        Ok(self
            .read_all()?
            .into_iter()
            .filter(|event| event.linkage.decision_id.as_deref() == Some(decision_id))
            .collect())
    }

    fn append_serialized(&self, event: &StoredEvent) -> Result<(), StoreError> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;
        serde_json::to_writer(&mut file, event)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    fn find_existing_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<StoredEvent>, StoreError> {
        Ok(self
            .read_all()?
            .into_iter()
            .find(|event| event.idempotency_key == idempotency_key))
    }
}

impl StoredEvent {
    fn is_contractually_equivalent(&self, other: &Self) -> bool {
        self.event_type == other.event_type
            && self.schema_version == other.schema_version
            && self.produced_by == other.produced_by
            && self.aggregate_key == other.aggregate_key
            && self.linkage == other.linkage
            && self.payload == other.payload
    }
}

fn ensure_store_file(path: &Path) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    OpenOptions::new().create(true).append(true).open(path)?;
    Ok(())
}

fn validate_required_string(value: &str, field: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        return Err(StoreError::invalid_data(format!("{field} cannot be empty")));
    }

    Ok(())
}

fn validate_optional_string(value: Option<&str>, field: &str) -> Result<(), StoreError> {
    if let Some(value) = value {
        if value.trim().is_empty() {
            return Err(StoreError::invalid_data(format!(
                "{field} cannot be blank when provided"
            )));
        }
    }

    Ok(())
}

fn validate_matching_ref(
    linkage_value: Option<&str>,
    payload_value: Option<&str>,
    linkage_field: &str,
    payload_field: &str,
) -> Result<(), StoreError> {
    if let (Some(linkage_value), Some(payload_value)) = (linkage_value, payload_value) {
        if linkage_value != payload_value {
            return Err(StoreError::invalid_data(format!(
                "{linkage_field} must match {payload_field}"
            )));
        }
    }

    Ok(())
}
