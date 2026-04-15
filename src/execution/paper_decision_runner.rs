use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    codecs::{CodecError, RehydratedEvent},
    commands::{CommandError, RegisterOrderCommand, SubmitOrderCommand},
    events::{FillSide, Provenance, SignalSide, SourceKind},
    materialization::{
        observe_fill, FillObservationDisposition, FillObservationOptions, MaterializationError,
    },
    queries::{PromotionPolicyStatus, QueryError, QueryService},
    store::StoreError,
    PaperExecutionRequest, PaperLedgerProjection, PaperRiskGuardConfig, PaperRiskGuardDecision,
    PolymarketPaperAdapterError, PolymarketPaperBackend, POLYMARKET_PAPER_VENUE,
};

const PAPER_DECISION_RUNNER_PRODUCED_BY: &str = "runtime.paper_decision_runner";
const PAPER_DECISION_RUNNER_ACTOR: &str = "paper_decision_runner_v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperDecisionRunConfig {
    pub usd_size: f64,
    pub backend_account: String,
    pub backend_data_dir: PathBuf,
    pub risk: PaperRiskGuardConfig,
    pub dry_run: bool,
}

impl Default for PaperDecisionRunConfig {
    fn default() -> Self {
        Self {
            usd_size: 100.0,
            backend_account: "default".into(),
            backend_data_dir: PathBuf::from(crate::DEFAULT_POLYMARKET_PAPER_DATA_DIR),
            risk: PaperRiskGuardConfig::default(),
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaperDecisionRunDisposition {
    Executed,
    Eligible,
    SkippedAlreadyExecuted,
    SkippedUnsupported,
    BlockedByRisk,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperDecisionRunItem {
    pub signal_id: String,
    pub decision_id: Option<String>,
    pub order_id: Option<String>,
    pub market_slug: Option<String>,
    pub outcome: Option<String>,
    pub side: Option<FillSide>,
    pub disposition: PaperDecisionRunDisposition,
    pub execution_request_sent: bool,
    pub fill_id: Option<String>,
    pub backend_trade_id: Option<String>,
    pub persisted: bool,
    pub duplicate: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperDecisionRunReport {
    pub dry_run: bool,
    pub confirmed_signals_seen: usize,
    pub execution_requests_sent: usize,
    pub fills_persisted: usize,
    pub skipped_already_executed: usize,
    pub skipped_unsupported: usize,
    pub duplicates: usize,
    pub blocked_by_risk: usize,
    pub blocked_max_open_positions: usize,
    pub blocked_max_total_exposure: usize,
    pub blocked_market_exposure: usize,
    pub blocked_duplicate_market_outcome: usize,
    pub blocked_market_order_limit: usize,
    pub items: Vec<PaperDecisionRunItem>,
}

pub fn run_paper_decisions(
    query_service: &QueryService<'_>,
    backend: &dyn PolymarketPaperBackend,
    config: &PaperDecisionRunConfig,
) -> Result<PaperDecisionRunReport, PaperDecisionRunnerError> {
    validate_config(config)?;
    let candidates = confirmed_signal_candidates(query_service)?;
    let already_executed = executed_signal_ids(query_service)?;
    let mut items = Vec::new();
    let mut execution_requests_sent = 0usize;
    let mut fills_persisted = 0usize;
    let mut skipped_already_executed = 0usize;
    let mut skipped_unsupported = 0usize;
    let mut duplicates = 0usize;
    let mut blocked_by_risk = 0usize;
    let mut blocked_max_open_positions = 0usize;
    let mut blocked_max_total_exposure = 0usize;
    let mut blocked_market_exposure = 0usize;
    let mut blocked_duplicate_market_outcome = 0usize;
    let mut blocked_market_order_limit = 0usize;

    for candidate in candidates {
        if already_executed.contains(&candidate.signal_id) {
            skipped_already_executed += 1;
            items.push(PaperDecisionRunItem {
                signal_id: candidate.signal_id,
                decision_id: None,
                order_id: None,
                market_slug: Some(candidate.market_slug),
                outcome: None,
                side: None,
                disposition: PaperDecisionRunDisposition::SkippedAlreadyExecuted,
                execution_request_sent: false,
                fill_id: None,
                backend_trade_id: None,
                persisted: false,
                duplicate: false,
                reasons: vec!["SignalAlreadyHasPaperFill".into()],
            });
            continue;
        }

        let Some((outcome, side)) = paper_order_side(candidate.signal_side) else {
            skipped_unsupported += 1;
            items.push(PaperDecisionRunItem {
                signal_id: candidate.signal_id,
                decision_id: None,
                order_id: None,
                market_slug: Some(candidate.market_slug),
                outcome: None,
                side: None,
                disposition: PaperDecisionRunDisposition::SkippedUnsupported,
                execution_request_sent: false,
                fill_id: None,
                backend_trade_id: None,
                persisted: false,
                duplicate: false,
                reasons: vec![format!(
                    "UnsupportedSignalSide({:?})",
                    candidate.signal_side
                )],
            });
            continue;
        };

        let policy = query_service.signal_promotion_policy(&candidate.signal_id)?;
        if policy.as_ref().map(|policy| policy.status) != Some(PromotionPolicyStatus::Eligible) {
            skipped_unsupported += 1;
            items.push(PaperDecisionRunItem {
                signal_id: candidate.signal_id,
                decision_id: None,
                order_id: None,
                market_slug: Some(candidate.market_slug),
                outcome: Some(outcome),
                side: Some(side),
                disposition: PaperDecisionRunDisposition::SkippedUnsupported,
                execution_request_sent: false,
                fill_id: None,
                backend_trade_id: None,
                persisted: false,
                duplicate: false,
                reasons: vec!["SignalNotEligibleForPaperDecision".into()],
            });
            continue;
        }

        let decision_id = build_paper_decision_id(&candidate.signal_id);
        let order_id = build_paper_order_id(&candidate.signal_id, &outcome);
        let request = PaperExecutionRequest {
            order_id: order_id.clone(),
            decision_id: decision_id.clone(),
            market_slug: candidate.market_slug.clone(),
            outcome: outcome.clone(),
            side,
            amount_usd: config.usd_size,
            backend_account: config.backend_account.clone(),
            backend_data_dir: config.backend_data_dir.clone(),
        };
        let ledger = current_ledger(query_service)?;
        let risk = crate::evaluate_paper_risk(&ledger, &request, &config.risk);
        if risk.decision != PaperRiskGuardDecision::Allowed {
            blocked_by_risk += 1;
            match risk.decision {
                PaperRiskGuardDecision::BlockedMaxOpenPositions => blocked_max_open_positions += 1,
                PaperRiskGuardDecision::BlockedMaxTotalExposure => blocked_max_total_exposure += 1,
                PaperRiskGuardDecision::BlockedMarketExposure => blocked_market_exposure += 1,
                PaperRiskGuardDecision::BlockedDuplicateMarketOutcome => {
                    blocked_duplicate_market_outcome += 1
                }
                PaperRiskGuardDecision::BlockedMarketOrderLimit => blocked_market_order_limit += 1,
                PaperRiskGuardDecision::Allowed => {}
            }
            items.push(PaperDecisionRunItem {
                signal_id: candidate.signal_id,
                decision_id: Some(decision_id),
                order_id: Some(order_id),
                market_slug: Some(candidate.market_slug),
                outcome: Some(outcome),
                side: Some(side),
                disposition: PaperDecisionRunDisposition::BlockedByRisk,
                execution_request_sent: false,
                fill_id: None,
                backend_trade_id: None,
                persisted: false,
                duplicate: false,
                reasons: vec![risk
                    .reason
                    .unwrap_or_else(|| format!("{:?}", risk.decision))],
            });
            continue;
        }

        if config.dry_run {
            items.push(PaperDecisionRunItem {
                signal_id: candidate.signal_id,
                decision_id: Some(decision_id),
                order_id: Some(order_id),
                market_slug: Some(candidate.market_slug),
                outcome: Some(outcome),
                side: Some(side),
                disposition: PaperDecisionRunDisposition::Eligible,
                execution_request_sent: false,
                fill_id: None,
                backend_trade_id: None,
                persisted: false,
                duplicate: false,
                reasons: Vec::new(),
            });
            continue;
        }

        persist_paper_order_evidence(query_service, &candidate, &decision_id, &order_id)?;

        let import_report =
            crate::submit_paper_order_and_map_fill(backend, &request, &BTreeSet::new())?;
        execution_requests_sent += 1;

        let Some(fill_result) = import_report.fill_result else {
            skipped_unsupported += 1;
            items.push(PaperDecisionRunItem {
                signal_id: candidate.signal_id,
                decision_id: Some(decision_id),
                order_id: Some(order_id),
                market_slug: Some(candidate.market_slug),
                outcome: Some(outcome),
                side: Some(side),
                disposition: PaperDecisionRunDisposition::SkippedUnsupported,
                execution_request_sent: true,
                fill_id: None,
                backend_trade_id: import_report.backend_trade_id,
                persisted: false,
                duplicate: false,
                reasons: import_report.notes,
            });
            continue;
        };

        let observation = observe_fill(
            query_service,
            &fill_result.to_fill_observation_request(),
            FillObservationOptions { dry_run: false },
        )?;
        if observation.persisted {
            fills_persisted += 1;
        }
        if observation.duplicate {
            duplicates += 1;
        }

        items.push(PaperDecisionRunItem {
            signal_id: candidate.signal_id,
            decision_id: Some(decision_id),
            order_id: Some(order_id),
            market_slug: Some(candidate.market_slug),
            outcome: Some(outcome),
            side: Some(side),
            disposition: if observation.duplicate {
                PaperDecisionRunDisposition::Duplicate
            } else if observation.disposition == FillObservationDisposition::Observed {
                PaperDecisionRunDisposition::Executed
            } else {
                PaperDecisionRunDisposition::SkippedUnsupported
            },
            execution_request_sent: true,
            fill_id: Some(fill_result.fill_id),
            backend_trade_id: Some(fill_result.backend_trade_id),
            persisted: observation.persisted,
            duplicate: observation.duplicate,
            reasons: observation.reasons,
        });
    }

    Ok(PaperDecisionRunReport {
        dry_run: config.dry_run,
        confirmed_signals_seen: items.len(),
        execution_requests_sent,
        fills_persisted,
        skipped_already_executed,
        skipped_unsupported,
        duplicates,
        blocked_by_risk,
        blocked_max_open_positions,
        blocked_max_total_exposure,
        blocked_market_exposure,
        blocked_duplicate_market_outcome,
        blocked_market_order_limit,
        items,
    })
}

fn validate_config(config: &PaperDecisionRunConfig) -> Result<(), PaperDecisionRunnerError> {
    if !config.usd_size.is_finite() || config.usd_size <= 0.0 {
        return Err(PaperDecisionRunnerError::InvalidConfig(
            "usd_size must be > 0".into(),
        ));
    }
    if config.backend_account.trim().is_empty() {
        return Err(PaperDecisionRunnerError::InvalidConfig(
            "backend_account cannot be empty".into(),
        ));
    }
    Ok(())
}

fn current_ledger(
    query_service: &QueryService<'_>,
) -> Result<PaperLedgerProjection, PaperDecisionRunnerError> {
    let events = query_service.all_events()?;
    Ok(crate::project_paper_ledger(&events)?)
}

fn paper_order_side(signal_side: SignalSide) -> Option<(String, FillSide)> {
    match signal_side {
        SignalSide::Long => Some(("yes".into(), FillSide::Buy)),
        SignalSide::Short => Some(("no".into(), FillSide::Buy)),
        SignalSide::Flat => None,
    }
}

fn persist_paper_order_evidence(
    query_service: &QueryService<'_>,
    candidate: &ConfirmedSignalCandidate,
    decision_id: &str,
    order_id: &str,
) -> Result<(), PaperDecisionRunnerError> {
    let store = query_service.store_ref();
    let registered = RegisterOrderCommand {
        produced_by: PAPER_DECISION_RUNNER_PRODUCED_BY.into(),
        provenance: runner_provenance(candidate, order_id),
        order_id: order_id.into(),
        decision_id: Some(decision_id.into()),
        hypothesis_id: candidate.hypothesis_id.clone(),
        signal_id: Some(candidate.signal_id.clone()),
        instrument: candidate.market_slug.clone(),
        venue: POLYMARKET_PAPER_VENUE.into(),
        parent_event_id: Some(candidate.confirmed_event_id.clone()),
        correlation_id: candidate.correlation_id.clone(),
    }
    .execute()?;
    let submitted = SubmitOrderCommand {
        produced_by: PAPER_DECISION_RUNNER_PRODUCED_BY.into(),
        provenance: runner_provenance(candidate, order_id),
        order_id: order_id.into(),
        decision_id: Some(decision_id.into()),
        hypothesis_id: candidate.hypothesis_id.clone(),
        signal_id: Some(candidate.signal_id.clone()),
        instrument: candidate.market_slug.clone(),
        venue: POLYMARKET_PAPER_VENUE.into(),
        parent_event_id: Some(registered.event_id.clone()),
        correlation_id: candidate.correlation_id.clone(),
    }
    .execute()?;

    let registered = crate::store::StoredEvent::try_from(registered)?;
    let submitted = crate::store::StoredEvent::try_from(submitted)?;
    let _ = store.append_event(&registered)?;
    let _ = store.append_event(&submitted)?;
    Ok(())
}

fn runner_provenance(candidate: &ConfirmedSignalCandidate, order_id: &str) -> Provenance {
    Provenance {
        source_kind: SourceKind::Runtime,
        source_ref: Some(format!(
            "run-paper-decisions://signal/{}",
            candidate.signal_id
        )),
        producer_run_id: Some(order_id.into()),
        actor: Some(PAPER_DECISION_RUNNER_ACTOR.into()),
        trace_id: Some(candidate.signal_id.clone()),
        notes: Some(
            serde_json::json!({
                "runner": PAPER_DECISION_RUNNER_ACTOR,
                "signal_id": candidate.signal_id,
                "venue": POLYMARKET_PAPER_VENUE,
            })
            .to_string(),
        ),
    }
}

fn confirmed_signal_candidates(
    query_service: &QueryService<'_>,
) -> Result<Vec<ConfirmedSignalCandidate>, PaperDecisionRunnerError> {
    let mut generated = BTreeMap::<String, GeneratedSignalEvidence>::new();
    let mut confirmed = BTreeSet::<String>::new();
    let mut confirmed_event_ids = BTreeMap::<String, String>::new();
    let events = query_service.all_events()?;

    for stored in events {
        match RehydratedEvent::try_from(&stored)? {
            RehydratedEvent::SignalGenerated(event) => {
                generated.insert(
                    event.payload.signal_id.clone(),
                    GeneratedSignalEvidence {
                        signal_id: event.payload.signal_id,
                        hypothesis_id: event.payload.hypothesis_id.or(event.linkage.hypothesis_id),
                        market_slug: event.aggregate_key.unwrap_or(event.payload.instrument),
                        signal_side: event.payload.side,
                        generated_at: event.occurred_at,
                        correlation_id: event.linkage.correlation_id,
                    },
                );
            }
            RehydratedEvent::SignalConfirmed(event) => {
                confirmed.insert(event.payload.signal_id.clone());
                confirmed_event_ids.insert(event.payload.signal_id, event.event_id);
            }
            _ => {}
        }
    }

    let mut candidates = generated
        .into_iter()
        .filter(|(signal_id, _)| confirmed.contains(signal_id))
        .map(|(signal_id, generated)| ConfirmedSignalCandidate {
            signal_id,
            hypothesis_id: generated.hypothesis_id,
            market_slug: generated.market_slug,
            signal_side: generated.signal_side,
            generated_at: generated.generated_at,
            confirmed_event_id: confirmed_event_ids
                .get(&generated.signal_id)
                .cloned()
                .unwrap_or_else(|| generated.signal_id.clone()),
            correlation_id: generated.correlation_id,
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.generated_at
            .cmp(&right.generated_at)
            .then_with(|| left.signal_id.cmp(&right.signal_id))
    });
    Ok(candidates)
}

fn executed_signal_ids(
    query_service: &QueryService<'_>,
) -> Result<BTreeSet<String>, PaperDecisionRunnerError> {
    let mut ids = BTreeSet::new();
    for stored in query_service.all_events()? {
        if let RehydratedEvent::FillReceived(event) = RehydratedEvent::try_from(&stored)? {
            if event.payload.venue == POLYMARKET_PAPER_VENUE {
                if let Some(signal_id) = event.linkage.signal_id {
                    ids.insert(signal_id);
                } else if let Some(signal_id) =
                    signal_id_from_paper_order_id(&event.payload.order_id)
                {
                    ids.insert(signal_id);
                }
            }
        }
    }
    Ok(ids)
}

fn signal_id_from_paper_order_id(order_id: &str) -> Option<String> {
    let value = order_id.strip_prefix("pm-paper-order-")?;
    if let Some(signal_id) = value.strip_prefix("yes-") {
        return Some(signal_id.to_string());
    }
    if let Some(signal_id) = value.strip_prefix("no-") {
        return Some(signal_id.to_string());
    }
    Some(value.to_string())
}

fn build_paper_decision_id(signal_id: &str) -> String {
    format!("paper-decision-{signal_id}")
}

fn build_paper_order_id(signal_id: &str, outcome: &str) -> String {
    format!("pm-paper-order-{outcome}-{signal_id}")
}

#[derive(Debug, Clone)]
struct GeneratedSignalEvidence {
    signal_id: String,
    hypothesis_id: Option<String>,
    market_slug: String,
    signal_side: SignalSide,
    generated_at: chrono::DateTime<chrono::Utc>,
    correlation_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ConfirmedSignalCandidate {
    signal_id: String,
    hypothesis_id: Option<String>,
    market_slug: String,
    signal_side: SignalSide,
    generated_at: chrono::DateTime<chrono::Utc>,
    confirmed_event_id: String,
    correlation_id: Option<String>,
}

#[derive(Debug)]
pub enum PaperDecisionRunnerError {
    Query(QueryError),
    Codec(CodecError),
    Command(CommandError),
    Store(StoreError),
    Materialization(MaterializationError),
    Adapter(PolymarketPaperAdapterError),
    Serde(serde_json::Error),
    InvalidConfig(String),
}

impl std::fmt::Display for PaperDecisionRunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Query(error) => write!(f, "paper decision query error: {error}"),
            Self::Codec(error) => write!(f, "paper decision codec error: {error}"),
            Self::Command(error) => write!(f, "paper decision command error: {error}"),
            Self::Store(error) => write!(f, "paper decision store error: {error}"),
            Self::Materialization(error) => {
                write!(f, "paper decision materialization error: {error}")
            }
            Self::Adapter(error) => write!(f, "paper decision adapter error: {error}"),
            Self::Serde(error) => write!(f, "paper decision serialization error: {error}"),
            Self::InvalidConfig(message) => write!(f, "paper decision invalid config: {message}"),
        }
    }
}

impl std::error::Error for PaperDecisionRunnerError {}

impl From<QueryError> for PaperDecisionRunnerError {
    fn from(value: QueryError) -> Self {
        Self::Query(value)
    }
}

impl From<CodecError> for PaperDecisionRunnerError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<CommandError> for PaperDecisionRunnerError {
    fn from(value: CommandError) -> Self {
        Self::Command(value)
    }
}

impl From<StoreError> for PaperDecisionRunnerError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<MaterializationError> for PaperDecisionRunnerError {
    fn from(value: MaterializationError) -> Self {
        Self::Materialization(value)
    }
}

impl From<PolymarketPaperAdapterError> for PaperDecisionRunnerError {
    fn from(value: PolymarketPaperAdapterError) -> Self {
        Self::Adapter(value)
    }
}

impl From<serde_json::Error> for PaperDecisionRunnerError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use chrono::{TimeZone, Utc};

    use super::{run_paper_decisions, PaperDecisionRunConfig, PaperDecisionRunDisposition};
    use crate::{
        events::{
            EventEnvelope, Linkage, Provenance, SignalConfirmed, SignalGenerated, SignalSide,
            SourceKind,
        },
        queries::QueryService,
        store::{JsonlEventStore, StoredEvent},
        FillSide, PaperRiskGuardConfig, PolymarketPaperAdapterError, PolymarketPaperBackend,
        PolymarketPaperTrade,
    };

    struct MockBackend {
        calls: RefCell<usize>,
        trades: Vec<PolymarketPaperTrade>,
    }

    impl PolymarketPaperBackend for MockBackend {
        fn execute_and_export_trades(
            &self,
            _request: &crate::PaperExecutionRequest,
        ) -> Result<Vec<PolymarketPaperTrade>, PolymarketPaperAdapterError> {
            *self.calls.borrow_mut() += 1;
            Ok(self.trades.clone())
        }
    }

    fn provenance() -> Provenance {
        Provenance {
            source_kind: SourceKind::Derived,
            source_ref: None,
            producer_run_id: Some("test-run".into()),
            actor: Some("test".into()),
            trace_id: Some("trace".into()),
            notes: None,
        }
    }

    fn stored<T: serde::Serialize>(event: EventEnvelope<T>) -> StoredEvent {
        StoredEvent::try_from(event).unwrap()
    }

    fn store_with_signal(
        signal_id: &str,
        side: SignalSide,
    ) -> (tempfile_path::TempPath, JsonlEventStore) {
        let path = tempfile_path::TempPath::new("paper-runner", "jsonl");
        let store = JsonlEventStore::new(path.as_path()).unwrap();
        let linkage = Linkage {
            signal_id: Some(signal_id.into()),
            correlation_id: Some(format!("corr-{signal_id}")),
            ..Linkage::default()
        };
        store
            .append_events(&[
                stored(
                    EventEnvelope::new_signal_generated(
                        "signal-engine",
                        Some("will-bitcoin-hit-100k".into()),
                        linkage.clone(),
                        provenance(),
                        SignalGenerated {
                            signal_id: signal_id.into(),
                            hypothesis_id: Some("hyp-1".into()),
                            instrument: "will-bitcoin-hit-100k".into(),
                            timeframe: "odds_jump".into(),
                            side,
                            strength: 0.9,
                            rationale: None,
                        },
                    )
                    .unwrap(),
                ),
                stored(
                    EventEnvelope::new_signal_confirmed(
                        "confirmation-agent-v1",
                        Some("will-bitcoin-hit-100k".into()),
                        linkage,
                        provenance(),
                        SignalConfirmed {
                            signal_id: signal_id.into(),
                            confirmed_by: "confirmation-agent-v1".into(),
                            confirmation_reason: None,
                            confirmation_score: Some(0.9),
                        },
                    )
                    .unwrap(),
                ),
            ])
            .unwrap();
        (path, store)
    }

    fn trade(outcome: &str) -> PolymarketPaperTrade {
        PolymarketPaperTrade {
            backend_trade_id: "trade-1".into(),
            market_slug: "will-bitcoin-hit-100k".into(),
            outcome: outcome.into(),
            side: FillSide::Buy,
            shares: 200.0,
            avg_price: 0.5,
            fee: Some(0.02),
            slippage_bps: Some(12.0),
            executed_at: Utc.with_ymd_and_hms(2026, 4, 14, 12, 0, 0).unwrap(),
        }
    }

    #[test]
    fn confirmed_signal_maps_to_execution_request_and_fill() {
        let (_path, store) = store_with_signal("sig-1", SignalSide::Long);
        let query = QueryService::new(&store);
        let backend = MockBackend {
            calls: RefCell::new(0),
            trades: vec![trade("yes")],
        };

        let report = run_paper_decisions(
            &query,
            &backend,
            &PaperDecisionRunConfig {
                usd_size: 100.0,
                backend_account: "paper-main".into(),
                backend_data_dir: "var/polymarket-paper".into(),
                risk: Default::default(),
                dry_run: false,
            },
        )
        .unwrap();

        assert_eq!(*backend.calls.borrow(), 1);
        assert_eq!(report.confirmed_signals_seen, 1);
        assert_eq!(report.execution_requests_sent, 1);
        assert_eq!(report.fills_persisted, 1);
        assert_eq!(report.items[0].outcome.as_deref(), Some("yes"));
        assert_eq!(report.items[0].side, Some(FillSide::Buy));
        assert_eq!(
            report.items[0].disposition,
            PaperDecisionRunDisposition::Executed
        );
    }

    #[test]
    fn risk_block_prevents_backend_execution() {
        let (_path, store) = store_with_signal("sig-risk", SignalSide::Long);
        let query = QueryService::new(&store);
        let backend = MockBackend {
            calls: RefCell::new(0),
            trades: vec![trade("yes")],
        };

        let report = run_paper_decisions(
            &query,
            &backend,
            &PaperDecisionRunConfig {
                risk: PaperRiskGuardConfig {
                    max_open_positions: Some(0),
                    ..PaperRiskGuardConfig::default()
                },
                ..PaperDecisionRunConfig::default()
            },
        )
        .unwrap();

        assert_eq!(*backend.calls.borrow(), 0);
        assert_eq!(report.execution_requests_sent, 0);
        assert_eq!(report.blocked_by_risk, 1);
        assert_eq!(report.blocked_max_open_positions, 1);
        assert_eq!(
            report.items[0].disposition,
            PaperDecisionRunDisposition::BlockedByRisk
        );
    }

    #[test]
    fn unsupported_direction_is_skipped() {
        let (_path, store) = store_with_signal("sig-flat", SignalSide::Flat);
        let query = QueryService::new(&store);
        let backend = MockBackend {
            calls: RefCell::new(0),
            trades: vec![trade("yes")],
        };

        let report =
            run_paper_decisions(&query, &backend, &PaperDecisionRunConfig::default()).unwrap();

        assert_eq!(*backend.calls.borrow(), 0);
        assert_eq!(report.skipped_unsupported, 1);
        assert_eq!(
            report.items[0].disposition,
            PaperDecisionRunDisposition::SkippedUnsupported
        );
    }

    #[test]
    fn repeated_runs_do_not_duplicate_fills() {
        let (_path, store) = store_with_signal("sig-1", SignalSide::Long);
        let query = QueryService::new(&store);
        let backend = MockBackend {
            calls: RefCell::new(0),
            trades: vec![trade("yes")],
        };

        let first =
            run_paper_decisions(&query, &backend, &PaperDecisionRunConfig::default()).unwrap();
        let second =
            run_paper_decisions(&query, &backend, &PaperDecisionRunConfig::default()).unwrap();

        assert_eq!(first.fills_persisted, 1);
        assert_eq!(second.execution_requests_sent, 0);
        assert_eq!(second.skipped_already_executed, 1);
        assert_eq!(
            store
                .read_all()
                .unwrap()
                .iter()
                .filter(|event| event.event_type.as_str() == "fill.received")
                .count(),
            1
        );
    }

    mod tempfile_path {
        use std::path::{Path, PathBuf};
        use std::time::{SystemTime, UNIX_EPOCH};

        pub struct TempPath(PathBuf);

        impl TempPath {
            pub fn new(name: &str, suffix: &str) -> Self {
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                Self(std::env::temp_dir().join(format!("twoexcamim-{name}-{nanos}.{suffix}")))
            }

            pub fn as_path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempPath {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
    }
}
