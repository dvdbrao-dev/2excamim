use chrono::Utc;

use crate::{
    events::{
        DecisionAction, DecisionFormed, EventEnvelope, FillReceived, FillSide, Linkage, Provenance,
        SignalConfirmed, SignalGenerated, SignalSide, SourceKind, VetoRaised, VetoScope,
    },
    store::StoredEvent,
};

use super::ScenarioError;

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioFixture {
    pub name: String,
    pub description: Option<String>,
    pub events: Vec<StoredEvent>,
}

pub fn available_fixtures() -> Vec<&'static str> {
    vec![
        "confirmed_then_filled_signal",
        "vetoed_signal_without_fill",
        "decision_with_multiple_fills",
    ]
}

pub fn load_fixture_named(name: &str) -> Result<ScenarioFixture, ScenarioError> {
    match name {
        "confirmed_then_filled_signal" => Ok(confirmed_then_filled_signal()),
        "vetoed_signal_without_fill" => Ok(vetoed_signal_without_fill()),
        "decision_with_multiple_fills" => Ok(decision_with_multiple_fills()),
        _ => Err(ScenarioError::unknown_fixture(name)),
    }
}

pub fn confirmed_then_filled_signal() -> ScenarioFixture {
    ScenarioFixture {
        name: "confirmed_then_filled_signal".into(),
        description: Some("Signal confirmado que forma decisión y recibe un fill".into()),
        events: vec![
            signal_generated(
                "sig-confirmed-1",
                "hyp-confirmed-1",
                "corr-confirmed-1",
                "BTCUSDT",
                "1h",
                SignalSide::Long,
            ),
            signal_confirmed(
                "sig-confirmed-1",
                "hyp-confirmed-1",
                "corr-confirmed-1",
                "BTCUSDT",
            ),
            decision_formed(
                "sig-confirmed-1",
                "dec-confirmed-1",
                "hyp-confirmed-1",
                "corr-confirmed-1",
                "BTCUSDT",
                SignalSide::Long,
            ),
            fill_received(
                "sig-confirmed-1",
                "dec-confirmed-1",
                "ord-confirmed-1",
                "fill-confirmed-1",
                "hyp-confirmed-1",
                "corr-confirmed-1",
                "BTCUSDT",
                1.5,
                101.0,
            ),
        ],
    }
}

pub fn vetoed_signal_without_fill() -> ScenarioFixture {
    ScenarioFixture {
        name: "vetoed_signal_without_fill".into(),
        description: Some("Signal vetada con decisión sin fills".into()),
        events: vec![
            signal_generated(
                "sig-vetoed-1",
                "hyp-vetoed-1",
                "corr-vetoed-1",
                "ETHUSDT",
                "4h",
                SignalSide::Short,
            ),
            veto_for_signal("sig-vetoed-1", "hyp-vetoed-1", "corr-vetoed-1", "ETHUSDT"),
            decision_formed(
                "sig-vetoed-1",
                "dec-vetoed-1",
                "hyp-vetoed-1",
                "corr-vetoed-1",
                "ETHUSDT",
                SignalSide::Short,
            ),
        ],
    }
}

pub fn decision_with_multiple_fills() -> ScenarioFixture {
    ScenarioFixture {
        name: "decision_with_multiple_fills".into(),
        description: Some("Decisión con dos fills sobre la misma orden".into()),
        events: vec![
            signal_generated(
                "sig-multi-fill-1",
                "hyp-multi-fill-1",
                "corr-multi-fill-1",
                "SOLUSDT",
                "15m",
                SignalSide::Long,
            ),
            decision_formed(
                "sig-multi-fill-1",
                "dec-multi-fill-1",
                "hyp-multi-fill-1",
                "corr-multi-fill-1",
                "SOLUSDT",
                SignalSide::Long,
            ),
            fill_received(
                "sig-multi-fill-1",
                "dec-multi-fill-1",
                "ord-multi-fill-1",
                "fill-multi-fill-1",
                "hyp-multi-fill-1",
                "corr-multi-fill-1",
                "SOLUSDT",
                2.0,
                100.0,
            ),
            fill_received(
                "sig-multi-fill-1",
                "dec-multi-fill-1",
                "ord-multi-fill-1",
                "fill-multi-fill-2",
                "hyp-multi-fill-1",
                "corr-multi-fill-1",
                "SOLUSDT",
                1.0,
                106.0,
            ),
        ],
    }
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("scenario://fixtures".into()),
        producer_run_id: Some("run-scenario-1".into()),
        actor: Some("scenario-fixtures".into()),
        trace_id: Some("trace-scenario-1".into()),
        notes: None,
    }
}

fn signal_linkage(signal_id: &str, hypothesis_id: &str, correlation_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some(hypothesis_id.into()),
        signal_id: Some(signal_id.into()),
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn decision_linkage(
    signal_id: &str,
    decision_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
) -> Linkage {
    Linkage {
        hypothesis_id: Some(hypothesis_id.into()),
        signal_id: Some(signal_id.into()),
        decision_id: Some(decision_id.into()),
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn fill_linkage(
    signal_id: &str,
    decision_id: &str,
    order_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
) -> Linkage {
    Linkage {
        hypothesis_id: Some(hypothesis_id.into()),
        signal_id: Some(signal_id.into()),
        decision_id: Some(decision_id.into()),
        order_id: Some(order_id.into()),
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn signal_generated(
    signal_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
    timeframe: &str,
    side: SignalSide,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some(instrument.into()),
            signal_linkage(signal_id, hypothesis_id, correlation_id),
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: Some(hypothesis_id.into()),
                instrument: instrument.into(),
                timeframe: timeframe.into(),
                side,
                strength: 0.8,
                rationale: Some("fixture".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn signal_confirmed(
    signal_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "risk-engine",
            Some(instrument.into()),
            signal_linkage(signal_id, hypothesis_id, correlation_id),
            provenance(),
            SignalConfirmed {
                signal_id: signal_id.into(),
                confirmed_by: "risk-check".into(),
                confirmation_reason: Some("validated".into()),
                confirmation_score: Some(0.9),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn veto_for_signal(
    signal_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "risk-engine",
            Some(instrument.into()),
            signal_linkage(signal_id, hypothesis_id, correlation_id),
            provenance(),
            VetoRaised {
                veto_id: format!("veto-{signal_id}"),
                scope: VetoScope::Signal,
                target_id: signal_id.into(),
                reason_code: "risk_limit".into(),
                reason_text: Some("fixture veto".into()),
                raised_by: "risk-engine".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn decision_formed(
    signal_id: &str,
    decision_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
    side: SignalSide,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some(instrument.into()),
            decision_linkage(signal_id, decision_id, hypothesis_id, correlation_id),
            provenance(),
            DecisionFormed {
                decision_id: decision_id.into(),
                instrument: instrument.into(),
                action: DecisionAction::Enter,
                side: Some(side),
                size_hint: Some(1.0),
                rationale: Some("fixture decision".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn fill_received(
    signal_id: &str,
    decision_id: &str,
    order_id: &str,
    fill_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
    quantity: f64,
    price: f64,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution-gateway",
            Some(instrument.into()),
            fill_linkage(
                signal_id,
                decision_id,
                order_id,
                hypothesis_id,
                correlation_id,
            ),
            provenance(),
            FillReceived {
                fill_id: fill_id.into(),
                decision_id: Some(decision_id.into()),
                order_id: order_id.into(),
                instrument: instrument.into(),
                side: FillSide::Buy,
                quantity,
                price,
                venue: "binance".into(),
                executed_at: Utc::now(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}
