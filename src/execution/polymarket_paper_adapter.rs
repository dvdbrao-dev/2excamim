use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    events::{FillReceived, FillSide},
    materialization::FillObservationRequest,
};

pub const DEFAULT_POLYMARKET_PAPER_DATA_DIR: &str = "./var/polymarket-paper";
pub const POLYMARKET_PAPER_VENUE: &str = "polymarket-paper";

/// Prototype boundary for polymarket-paper-trader.
///
/// 2EXCAMIM remains the system of record. This adapter is deliberately narrow:
/// it shells out to `pm-trader` or reads a fixture export, maps one backend trade
/// into canonical fill data, and leaves persistence to the existing
/// `fill.received` materialization path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperExecutionRequest {
    pub order_id: String,
    pub decision_id: String,
    pub market_slug: String,
    pub outcome: String,
    pub side: FillSide,
    pub amount_usd: f64,
    pub backend_account: String,
    pub backend_data_dir: PathBuf,
}

impl PaperExecutionRequest {
    pub fn validate(&self) -> Result<(), PolymarketPaperAdapterError> {
        if self.order_id.trim().is_empty() {
            return Err(PolymarketPaperAdapterError::InvalidRequest(
                "order_id cannot be empty".into(),
            ));
        }
        if self.decision_id.trim().is_empty() {
            return Err(PolymarketPaperAdapterError::InvalidRequest(
                "decision_id cannot be empty".into(),
            ));
        }
        if self.market_slug.trim().is_empty() {
            return Err(PolymarketPaperAdapterError::InvalidRequest(
                "market_slug cannot be empty".into(),
            ));
        }
        if self.outcome.trim().is_empty() {
            return Err(PolymarketPaperAdapterError::InvalidRequest(
                "outcome cannot be empty".into(),
            ));
        }
        if self.backend_account.trim().is_empty() {
            return Err(PolymarketPaperAdapterError::InvalidRequest(
                "backend_account cannot be empty".into(),
            ));
        }
        if !self.amount_usd.is_finite() || self.amount_usd <= 0.0 {
            return Err(PolymarketPaperAdapterError::InvalidRequest(
                "amount_usd must be > 0".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperExecutionResult {
    pub fill_id: String,
    pub order_id: String,
    pub decision_id: String,
    pub instrument: String,
    pub side: FillSide,
    pub quantity: f64,
    pub avg_price: f64,
    pub fee: Option<f64>,
    pub slippage_bps: Option<f64>,
    pub executed_at: DateTime<Utc>,
    pub backend_trade_id: String,
}

impl PaperExecutionResult {
    pub fn to_fill_received_payload(&self) -> FillReceived {
        FillReceived {
            fill_id: self.fill_id.clone(),
            decision_id: Some(self.decision_id.clone()),
            order_id: self.order_id.clone(),
            instrument: self.instrument.clone(),
            side: self.side,
            quantity: self.quantity,
            price: self.avg_price,
            venue: POLYMARKET_PAPER_VENUE.to_string(),
            executed_at: self.executed_at,
        }
    }

    pub fn to_fill_observation_request(&self) -> FillObservationRequest {
        FillObservationRequest {
            fill_id: self.fill_id.clone(),
            order_id: self.order_id.clone(),
            decision_id: Some(self.decision_id.clone()),
            instrument: Some(self.instrument.clone()),
            side: self.side,
            quantity: self.quantity,
            price: self.avg_price,
            venue: Some(POLYMARKET_PAPER_VENUE.to_string()),
            executed_at: self.executed_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolymarketPaperTrade {
    #[serde(alias = "id", alias = "trade_id")]
    pub backend_trade_id: String,
    #[serde(alias = "market", alias = "instrument")]
    pub market_slug: String,
    pub outcome: String,
    #[serde(deserialize_with = "deserialize_fill_side")]
    pub side: FillSide,
    #[serde(alias = "quantity")]
    pub shares: f64,
    #[serde(alias = "price")]
    pub avg_price: f64,
    #[serde(default)]
    pub fee: Option<f64>,
    #[serde(default)]
    pub slippage_bps: Option<f64>,
    #[serde(alias = "created_at")]
    pub executed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaperExecutionImportDisposition {
    Imported,
    SkippedDuplicate,
    NoMatchingTrade,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperExecutionImportReport {
    pub disposition: PaperExecutionImportDisposition,
    pub backend_account: String,
    pub backend_data_dir: PathBuf,
    pub backend_trade_id: Option<String>,
    pub fill_result: Option<PaperExecutionResult>,
    pub notes: Vec<String>,
}

pub trait PolymarketPaperBackend {
    fn execute_and_export_trades(
        &self,
        request: &PaperExecutionRequest,
    ) -> Result<Vec<PolymarketPaperTrade>, PolymarketPaperAdapterError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliPolymarketPaperBackend {
    pub binary: String,
}

impl Default for CliPolymarketPaperBackend {
    fn default() -> Self {
        Self {
            binary: "pm-trader".into(),
        }
    }
}

impl PolymarketPaperBackend for CliPolymarketPaperBackend {
    fn execute_and_export_trades(
        &self,
        request: &PaperExecutionRequest,
    ) -> Result<Vec<PolymarketPaperTrade>, PolymarketPaperAdapterError> {
        request.validate()?;
        let amount = request.amount_usd.to_string();
        let action = match request.side {
            FillSide::Buy => "buy",
            FillSide::Sell => "sell",
        };
        let status = Command::new(&self.binary)
            .arg("--data-dir")
            .arg(&request.backend_data_dir)
            .arg("--account")
            .arg(&request.backend_account)
            .arg(action)
            .arg(&request.market_slug)
            .arg(&request.outcome)
            .arg(amount)
            .arg("--type")
            .arg("fok")
            .status()?;
        if !status.success() {
            return Err(PolymarketPaperAdapterError::BackendFailed(format!(
                "{action} command exited with status {status}"
            )));
        }

        let output = Command::new(&self.binary)
            .arg("--data-dir")
            .arg(&request.backend_data_dir)
            .arg("--account")
            .arg(&request.backend_account)
            .arg("export")
            .arg("trades")
            .arg("--format")
            .arg("json")
            .output()?;
        if !output.status.success() {
            return Err(PolymarketPaperAdapterError::BackendFailed(format!(
                "export trades command exited with status {}",
                output.status
            )));
        }

        parse_trades_json(&output.stdout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixturePolymarketPaperBackend {
    pub trades_path: PathBuf,
}

impl PolymarketPaperBackend for FixturePolymarketPaperBackend {
    fn execute_and_export_trades(
        &self,
        request: &PaperExecutionRequest,
    ) -> Result<Vec<PolymarketPaperTrade>, PolymarketPaperAdapterError> {
        request.validate()?;
        let bytes = std::fs::read(&self.trades_path)?;
        parse_trades_json(&bytes)
    }
}

pub fn submit_paper_order_and_map_fill(
    backend: &dyn PolymarketPaperBackend,
    request: &PaperExecutionRequest,
    imported_backend_trade_ids: &BTreeSet<String>,
) -> Result<PaperExecutionImportReport, PolymarketPaperAdapterError> {
    request.validate()?;
    let trades = backend.execute_and_export_trades(request)?;
    let matching_trades = matching_trades(&trades, request);
    let imported_match = matching_trades
        .iter()
        .find(|trade| imported_backend_trade_ids.contains(&trade.backend_trade_id));
    let imported_match_id = imported_match.map(|trade| trade.backend_trade_id.clone());
    let selected = matching_trades
        .into_iter()
        .filter(|trade| !imported_backend_trade_ids.contains(&trade.backend_trade_id))
        .max_by(|left, right| {
            left.executed_at
                .cmp(&right.executed_at)
                .then_with(|| left.backend_trade_id.cmp(&right.backend_trade_id))
        });

    if let Some(trade) = selected {
        let fill_result = map_trade_to_fill_result(&trade, request);
        return Ok(PaperExecutionImportReport {
            disposition: PaperExecutionImportDisposition::Imported,
            backend_account: request.backend_account.clone(),
            backend_data_dir: request.backend_data_dir.clone(),
            backend_trade_id: Some(trade.backend_trade_id.clone()),
            fill_result: Some(fill_result),
            notes: vec![
                "submitted paper order through polymarket-paper-trader boundary".into(),
                "mapped backend trade into canonical fill.received-compatible data".into(),
            ],
        });
    }

    if let Some(backend_trade_id) = imported_match_id {
        return Ok(PaperExecutionImportReport {
            disposition: PaperExecutionImportDisposition::SkippedDuplicate,
            backend_account: request.backend_account.clone(),
            backend_data_dir: request.backend_data_dir.clone(),
            backend_trade_id: Some(backend_trade_id),
            fill_result: None,
            notes: vec!["backend trade was already imported".into()],
        });
    }

    Ok(PaperExecutionImportReport {
        disposition: PaperExecutionImportDisposition::NoMatchingTrade,
        backend_account: request.backend_account.clone(),
        backend_data_dir: request.backend_data_dir.clone(),
        backend_trade_id: None,
        fill_result: None,
        notes: vec!["backend export contained no matching trade".into()],
    })
}

fn matching_trades<'a>(
    trades: &'a [PolymarketPaperTrade],
    request: &PaperExecutionRequest,
) -> Vec<&'a PolymarketPaperTrade> {
    trades
        .iter()
        .filter(|trade| {
            trade.market_slug == request.market_slug
                && trade.outcome.eq_ignore_ascii_case(&request.outcome)
                && trade.side == request.side
        })
        .collect()
}

fn map_trade_to_fill_result(
    trade: &PolymarketPaperTrade,
    request: &PaperExecutionRequest,
) -> PaperExecutionResult {
    PaperExecutionResult {
        fill_id: format!(
            "pm-paper-{}-{}",
            clean_id_component(&request.backend_account),
            clean_id_component(&trade.backend_trade_id)
        ),
        order_id: request.order_id.clone(),
        decision_id: request.decision_id.clone(),
        instrument: trade.market_slug.clone(),
        side: trade.side,
        quantity: trade.shares,
        avg_price: trade.avg_price,
        fee: trade.fee,
        slippage_bps: trade.slippage_bps,
        executed_at: trade.executed_at,
        backend_trade_id: trade.backend_trade_id.clone(),
    }
}

fn clean_id_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn parse_trades_json(
    bytes: &[u8],
) -> Result<Vec<PolymarketPaperTrade>, PolymarketPaperAdapterError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    if value.is_array() {
        return Ok(serde_json::from_value(value)?);
    }
    if let Some(trades) = value.get("trades") {
        return Ok(serde_json::from_value(trades.clone())?);
    }
    Err(PolymarketPaperAdapterError::InvalidBackendOutput(
        "expected JSON array or object with trades array".into(),
    ))
}

fn deserialize_fill_side<'de, D>(deserializer: D) -> Result<FillSide, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    match value.to_ascii_lowercase().as_str() {
        "buy" => Ok(FillSide::Buy),
        "sell" => Ok(FillSide::Sell),
        _ => Err(serde::de::Error::custom(format!(
            "invalid fill side: {value}"
        ))),
    }
}

#[derive(Debug)]
pub enum PolymarketPaperAdapterError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidRequest(String),
    InvalidBackendOutput(String),
    BackendFailed(String),
}

impl std::fmt::Display for PolymarketPaperAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::InvalidRequest(message) => {
                write!(f, "invalid paper execution request: {message}")
            }
            Self::InvalidBackendOutput(message) => {
                write!(f, "invalid polymarket-paper-trader output: {message}")
            }
            Self::BackendFailed(message) => write!(f, "polymarket-paper-trader failed: {message}"),
        }
    }
}

impl std::error::Error for PolymarketPaperAdapterError {}

impl From<std::io::Error> for PolymarketPaperAdapterError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for PolymarketPaperAdapterError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use chrono::{TimeZone, Utc};

    use super::{
        submit_paper_order_and_map_fill, PaperExecutionRequest, PolymarketPaperAdapterError,
        PolymarketPaperBackend, PolymarketPaperTrade, POLYMARKET_PAPER_VENUE,
    };
    use crate::events::FillSide;

    struct MockBackend {
        calls: RefCell<usize>,
        trades: Vec<PolymarketPaperTrade>,
    }

    impl PolymarketPaperBackend for MockBackend {
        fn execute_and_export_trades(
            &self,
            _request: &PaperExecutionRequest,
        ) -> Result<Vec<PolymarketPaperTrade>, PolymarketPaperAdapterError> {
            *self.calls.borrow_mut() += 1;
            Ok(self.trades.clone())
        }
    }

    fn request() -> PaperExecutionRequest {
        PaperExecutionRequest {
            order_id: "ord-1".into(),
            decision_id: "dec-1".into(),
            market_slug: "will-bitcoin-hit-100k".into(),
            outcome: "yes".into(),
            side: FillSide::Buy,
            amount_usd: 100.0,
            backend_account: "paper-main".into(),
            backend_data_dir: "var/polymarket-paper".into(),
        }
    }

    fn trade(backend_trade_id: &str) -> PolymarketPaperTrade {
        PolymarketPaperTrade {
            backend_trade_id: backend_trade_id.into(),
            market_slug: "will-bitcoin-hit-100k".into(),
            outcome: "YES".into(),
            side: FillSide::Buy,
            shares: 200.0,
            avg_price: 0.5,
            fee: Some(0.02),
            slippage_bps: Some(12.0),
            executed_at: Utc.with_ymd_and_hms(2026, 4, 14, 12, 0, 0).unwrap(),
        }
    }

    #[test]
    fn request_to_adapter_call_to_mapped_fill_result() {
        let backend = MockBackend {
            calls: RefCell::new(0),
            trades: vec![trade("trade-1")],
        };

        let report =
            submit_paper_order_and_map_fill(&backend, &request(), &Default::default()).unwrap();
        let fill = report.fill_result.unwrap();
        let payload = fill.to_fill_received_payload();

        assert_eq!(*backend.calls.borrow(), 1);
        assert_eq!(fill.backend_trade_id, "trade-1");
        assert_eq!(fill.fill_id, "pm-paper-paper-main-trade-1");
        assert_eq!(payload.fill_id, fill.fill_id);
        assert_eq!(payload.venue, POLYMARKET_PAPER_VENUE);
        assert_eq!(payload.order_id, "ord-1");
        assert_eq!(payload.decision_id.as_deref(), Some("dec-1"));
        assert_eq!(payload.quantity, 200.0);
        assert_eq!(payload.price, 0.5);
    }

    #[test]
    fn repeated_import_does_not_duplicate_same_backend_trade() {
        let backend = MockBackend {
            calls: RefCell::new(0),
            trades: vec![trade("trade-1")],
        };
        let mut imported = std::collections::BTreeSet::new();
        imported.insert("trade-1".to_string());

        let report = submit_paper_order_and_map_fill(&backend, &request(), &imported).unwrap();

        assert_eq!(
            report.disposition,
            super::PaperExecutionImportDisposition::SkippedDuplicate
        );
        assert_eq!(report.backend_trade_id.as_deref(), Some("trade-1"));
        assert!(report.fill_result.is_none());
    }

    #[test]
    fn mapping_into_canonical_fill_data_is_stable() {
        let backend = MockBackend {
            calls: RefCell::new(0),
            trades: vec![trade("trade-1")],
        };

        let first =
            submit_paper_order_and_map_fill(&backend, &request(), &Default::default()).unwrap();
        let second =
            submit_paper_order_and_map_fill(&backend, &request(), &Default::default()).unwrap();

        assert_eq!(
            first.fill_result.unwrap().to_fill_received_payload(),
            second.fill_result.unwrap().to_fill_received_payload()
        );
    }

    #[test]
    fn fixture_export_shape_parses_trades_array() {
        let raw = br#"{
  "trades": [
    {
      "id": "trade-1",
      "market": "will-bitcoin-hit-100k",
      "outcome": "yes",
      "side": "Buy",
      "quantity": 200.0,
      "price": 0.5,
      "fee": 0.02,
      "slippage_bps": 12.0,
      "created_at": "2026-04-14T12:00:00Z"
    }
  ]
}"#;

        let trades = super::parse_trades_json(raw).unwrap();

        assert_eq!(trades[0].backend_trade_id, "trade-1");
        assert_eq!(trades[0].market_slug, "will-bitcoin-hit-100k");
        assert_eq!(trades[0].outcome, "yes");
        assert_eq!(trades[0].side, FillSide::Buy);
        assert_eq!(trades[0].shares, 200.0);
    }
}
