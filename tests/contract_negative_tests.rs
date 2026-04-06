use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use serde_json::json;
use twoexcamim::events::{
    EventEnvelope, FillReceived, FillSide, Linkage, Provenance, SignalConfirmed, SignalGenerated,
    SignalSide, SourceKind, VetoRaised, VetoScope,
};
use twoexcamim::store::{JsonlEventStore, StoreError, StoredEvent};
use uuid::Uuid;

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-contract-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("contract://tests".into()),
        producer_run_id: Some("run-contract-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-contract-1".into()),
        notes: None,
    }
}

fn signal_linkage() -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: Some(Uuid::new_v4().to_string()),
        correlation_id: Some("corr-1".into()),
    }
}

fn make_signal_generated() -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some("BTCUSDT".into()),
            signal_linkage(),
            provenance(),
            SignalGenerated {
                signal_id: "sig-1".into(),
                hypothesis_id: Some("hyp-1".into()),
                instrument: "BTCUSDT".into(),
                timeframe: "1h".into(),
                side: SignalSide::Long,
                strength: 0.8,
                rationale: Some("contract baseline".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn stored_event_rejects_invalid_uuid() {
    let mut stored = make_signal_generated();
    stored.event_id = "not-a-uuid".into();

    let err = stored.validate().unwrap_err();

    assert!(matches!(err, StoreError::InvalidData(message) if message.contains("valid UUID")));
}

#[test]
fn stored_event_rejects_invalid_schema_version() {
    let mut stored = make_signal_generated();
    stored.schema_version = "v2".into();

    let err = stored.validate().unwrap_err();

    assert!(
        matches!(err, StoreError::InvalidData(message) if message.contains("schema_version must be v1"))
    );
}

#[test]
fn read_all_rejects_parseable_but_contract_corrupt_event() {
    let path = temp_store_path("parseable-corrupt");
    let store = JsonlEventStore::new(&path).unwrap();
    let mut stored = make_signal_generated();
    stored.payload = json!({
        "signal_id": "sig-1",
        "hypothesis_id": "hyp-1",
        "instrument": "BTCUSDT",
        "timeframe": "1h",
        "side": "Long",
        "strength": 1.5,
        "rationale": "tampered on disk"
    });

    std::fs::write(
        &path,
        format!("{}\n", serde_json::to_string(&stored).unwrap()),
    )
    .unwrap();

    let err = store.read_all().unwrap_err();

    assert!(matches!(err, StoreError::InvalidData(message) if message.contains("line 1")));
    cleanup(&path);
}

#[test]
fn append_event_rejects_conflicting_duplicate_idempotency_key() {
    let path = temp_store_path("conflicting-idempotency");
    let store = JsonlEventStore::new(&path).unwrap();
    let first = StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "runtime",
            None,
            Linkage {
                signal_id: Some("sig-42".into()),
                correlation_id: Some("corr-42".into()),
                ..Linkage::default()
            },
            provenance(),
            SignalConfirmed {
                signal_id: "sig-42".into(),
                confirmed_by: "risk-engine".into(),
                confirmation_reason: Some("threshold met".into()),
                confirmation_score: Some(0.91),
            },
        )
        .unwrap(),
    )
    .unwrap();
    let second = StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "runtime",
            None,
            Linkage {
                signal_id: Some("sig-42".into()),
                correlation_id: Some("corr-42".into()),
                ..Linkage::default()
            },
            provenance(),
            SignalConfirmed {
                signal_id: "sig-42".into(),
                confirmed_by: "risk-engine".into(),
                confirmation_reason: Some("manual override".into()),
                confirmation_score: Some(0.12),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert!(store.append_event(&first).unwrap());

    let err = store.append_event(&second).unwrap_err();

    assert!(
        matches!(err, StoreError::InvalidData(message) if message.contains("conflicting event already exists"))
    );
    cleanup(&path);
}

#[test]
fn stored_event_rejects_fill_with_contradictory_linkage() {
    let mut stored = StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution",
            Some("BTCUSDT".into()),
            Linkage {
                signal_id: Some("sig-1".into()),
                decision_id: Some("dec-1".into()),
                order_id: Some("ord-1".into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            Provenance {
                source_kind: SourceKind::ExecutionVenue,
                source_ref: Some("binance".into()),
                producer_run_id: Some("run-fill-1".into()),
                actor: Some("venue".into()),
                trace_id: Some("trace-fill-1".into()),
                notes: None,
            },
            FillReceived {
                fill_id: "fill-1".into(),
                decision_id: Some("dec-1".into()),
                order_id: "ord-1".into(),
                instrument: "BTCUSDT".into(),
                side: FillSide::Buy,
                quantity: 1.0,
                price: 62000.0,
                venue: "binance".into(),
                executed_at: Utc::now(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    stored.linkage.decision_id = Some("dec-other".into());

    let err = stored.validate().unwrap_err();

    assert!(
        matches!(err, StoreError::InvalidData(message) if message.contains("linkage.decision_id must match payload.decision_id"))
    );
}

#[test]
fn stored_event_rejects_scoped_veto_with_mismatched_target_linkage() {
    let mut stored = StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "risk",
            None,
            Linkage {
                signal_id: Some("sig-1".into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            provenance(),
            VetoRaised {
                veto_id: "veto-1".into(),
                scope: VetoScope::Signal,
                target_id: "sig-1".into(),
                reason_code: "score_below_threshold".into(),
                reason_text: Some("below floor".into()),
                raised_by: "risk-engine".into(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    stored.linkage.signal_id = Some("sig-other".into());

    let err = stored.validate().unwrap_err();

    assert!(
        matches!(err, StoreError::InvalidData(message) if message.contains("linkage.signal_id must match payload.target_id"))
    );
}

#[test]
fn stored_event_rejects_signal_confirmation_with_mismatched_linkage() {
    let mut stored = StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "runtime",
            None,
            Linkage {
                signal_id: Some("sig-1".into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            provenance(),
            SignalConfirmed {
                signal_id: "sig-1".into(),
                confirmed_by: "risk-engine".into(),
                confirmation_reason: Some("threshold met".into()),
                confirmation_score: Some(0.9),
            },
        )
        .unwrap(),
    )
    .unwrap();
    stored.linkage.signal_id = Some("sig-other".into());

    let err = stored.validate().unwrap_err();

    assert!(
        matches!(err, StoreError::InvalidData(message) if message.contains("linkage.signal_id must match payload.signal_id"))
    );
}
