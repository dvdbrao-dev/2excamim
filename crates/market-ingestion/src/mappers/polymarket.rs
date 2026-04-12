use chrono::Utc;
use market_domain::{
    MarketActivity, MarketActivityKind, MarketSnapshot, MarketSource, MarketStatus, Validate,
};

use crate::{
    dto::polymarket::{PolymarketActivityDto, PolymarketMarketDto},
    mappers::MappingError,
    storage::{CanonicalActivityBatch, CanonicalSnapshotBatch},
};

/// Maps Polymarket DTOs into canonical market snapshots.
pub struct PolymarketSnapshotMapper;
/// Maps Polymarket DTOs into canonical market activity.
pub struct PolymarketActivityMapper;

impl PolymarketSnapshotMapper {
    /// Maps a full Polymarket discovery payload.
    pub fn map_markets(
        &self,
        markets: &[PolymarketMarketDto],
    ) -> Result<CanonicalSnapshotBatch, MappingError> {
        let snapshots = markets
            .iter()
            .map(|market| self.map_market(market))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CanonicalSnapshotBatch { snapshots })
    }

    /// Maps a single Polymarket market DTO.
    pub fn map_market(&self, market: &PolymarketMarketDto) -> Result<MarketSnapshot, MappingError> {
        let status = map_status(market);
        let mut snapshot = MarketSnapshot::new(
            market.condition_id.clone(),
            MarketSource::Polymarket,
            market.question.clone(),
            status,
            Utc::now(),
        )
        .map_err(|err| MappingError::new(err.to_string()))?;
        snapshot.best_bid = market.best_bid;
        snapshot.best_ask = market.best_ask;
        snapshot.last_price = market.last_trade_price;
        snapshot.volume = market.volume;
        snapshot
            .validate()
            .map_err(|err| MappingError::new(err.to_string()))?;

        Ok(snapshot)
    }

    /// Maps all markets, keeping per-item failures explicit.
    pub fn map_markets_lossy(
        &self,
        markets: &[PolymarketMarketDto],
    ) -> (Vec<MarketSnapshot>, Vec<MappingError>) {
        let mut snapshots = Vec::new();
        let mut errors = Vec::new();

        for market in markets {
            match self.map_market(market) {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(error) => errors.push(error),
            }
        }

        (snapshots, errors)
    }
}

impl PolymarketActivityMapper {
    /// Maps a full Polymarket activity page.
    pub fn map_activity(
        &self,
        activity: &[PolymarketActivityDto],
    ) -> Result<CanonicalActivityBatch, MappingError> {
        let activity = activity
            .iter()
            .map(|item| self.map_item(item))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CanonicalActivityBatch { activity })
    }

    /// Maps one Polymarket activity item.
    pub fn map_item(&self, item: &PolymarketActivityDto) -> Result<MarketActivity, MappingError> {
        let kind = map_activity_kind(&item.activity_type)?;
        let observed_at = parse_timestamp(&item.timestamp)?;
        let activity = MarketActivity {
            market_id: item.condition_id.clone(),
            source: MarketSource::Polymarket,
            kind,
            price: item.price,
            quantity: item.quantity,
            observed_at,
        };
        activity
            .validate()
            .map_err(|err| MappingError::new(err.to_string()))?;

        Ok(activity)
    }

    /// Maps all items, keeping per-item failures explicit.
    pub fn map_activity_lossy(
        &self,
        activity: &[PolymarketActivityDto],
    ) -> (Vec<MarketActivity>, Vec<MappingError>) {
        let mut mapped = Vec::new();
        let mut errors = Vec::new();

        for item in activity {
            match self.map_item(item) {
                Ok(activity) => mapped.push(activity),
                Err(error) => errors.push(error),
            }
        }

        (mapped, errors)
    }
}

fn map_status(market: &PolymarketMarketDto) -> MarketStatus {
    if market.archived {
        MarketStatus::Cancelled
    } else if market.closed {
        MarketStatus::Closed
    } else if market.active || market.accepting_orders {
        MarketStatus::Open
    } else {
        MarketStatus::Suspended
    }
}

fn map_activity_kind(value: &str) -> Result<MarketActivityKind, MappingError> {
    let normalized = value.trim().to_ascii_lowercase();

    match normalized.as_str() {
        "trade" => Ok(MarketActivityKind::Trade),
        "volume" => Ok(MarketActivityKind::Volume),
        "open_interest" | "openinterest" => Ok(MarketActivityKind::OpenInterest),
        "resolution" | "resolved" => Ok(MarketActivityKind::Resolution),
        _ => Err(MappingError::new(format!(
            "unsupported polymarket activity type: {value}"
        ))),
    }
}

fn parse_timestamp(value: &str) -> Result<chrono::DateTime<chrono::Utc>, MappingError> {
    if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(timestamp.with_timezone(&chrono::Utc));
    }

    if let Ok(seconds) = value.parse::<i64>() {
        if let Some(timestamp) = chrono::DateTime::from_timestamp(seconds, 0) {
            return Ok(timestamp);
        }
    }

    Err(MappingError::new(format!(
        "invalid activity timestamp: {value}"
    )))
}

#[cfg(test)]
mod tests {
    use crate::dto::polymarket::{PolymarketActivityDto, PolymarketMarketDto};

    use super::{PolymarketActivityMapper, PolymarketSnapshotMapper};
    use market_domain::{MarketActivityKind, MarketStatus};

    #[test]
    fn maps_valid_market_to_canonical_snapshot() {
        let mapper = PolymarketSnapshotMapper;
        let market = PolymarketMarketDto {
            condition_id: "0xabc".into(),
            question: "Will BTC close above 100k?".into(),
            active: true,
            closed: false,
            archived: false,
            accepting_orders: true,
            best_bid: Some(0.44),
            best_ask: Some(0.46),
            last_trade_price: Some(0.45),
            volume: Some(1200.0),
        };

        let snapshot = mapper.map_market(&market).unwrap();

        assert_eq!(snapshot.market_id, "0xabc");
        assert_eq!(snapshot.status, MarketStatus::Open);
        assert_eq!(snapshot.best_bid, Some(0.44));
    }

    #[test]
    fn rejects_invalid_provider_values() {
        let mapper = PolymarketSnapshotMapper;
        let market = PolymarketMarketDto {
            condition_id: "0xabc".into(),
            question: "Broken market".into(),
            active: true,
            closed: false,
            archived: false,
            accepting_orders: true,
            best_bid: Some(0.9),
            best_ask: Some(0.4),
            last_trade_price: None,
            volume: Some(100.0),
        };

        let err = mapper.map_market(&market).unwrap_err();
        assert!(err.message().contains("best_bid"));
    }

    #[test]
    fn lossy_mapping_collects_valid_snapshots_and_errors() {
        let mapper = PolymarketSnapshotMapper;
        let valid = PolymarketMarketDto {
            condition_id: "0xgood".into(),
            question: "Valid market".into(),
            active: true,
            closed: false,
            archived: false,
            accepting_orders: true,
            best_bid: Some(0.41),
            best_ask: Some(0.43),
            last_trade_price: Some(0.42),
            volume: Some(100.0),
        };
        let invalid = PolymarketMarketDto {
            condition_id: "0xbad".into(),
            question: "Invalid market".into(),
            active: true,
            closed: false,
            archived: false,
            accepting_orders: true,
            best_bid: Some(0.91),
            best_ask: Some(0.40),
            last_trade_price: None,
            volume: Some(1.0),
        };

        let (snapshots, errors) = mapper.map_markets_lossy(&[valid, invalid]);

        assert_eq!(snapshots.len(), 1);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn maps_trade_activity_to_canonical_activity() {
        let mapper = PolymarketActivityMapper;
        let dto = PolymarketActivityDto {
            activity_id: Some("trade-1".into()),
            condition_id: "0xabc".into(),
            activity_type: "trade".into(),
            price: Some(0.52),
            quantity: Some(15.0),
            timestamp: "2026-04-12T00:00:00Z".into(),
        };

        let activity = mapper.map_item(&dto).unwrap();
        assert_eq!(activity.kind, MarketActivityKind::Trade);
        assert_eq!(activity.quantity, Some(15.0));
    }

    #[test]
    fn rejects_unsupported_activity_type() {
        let mapper = PolymarketActivityMapper;
        let dto = PolymarketActivityDto {
            activity_id: Some("evt-1".into()),
            condition_id: "0xabc".into(),
            activity_type: "mystery".into(),
            price: None,
            quantity: None,
            timestamp: "2026-04-12T00:00:00Z".into(),
        };

        let err = mapper.map_item(&dto).unwrap_err();
        assert!(err.message().contains("unsupported"));
    }
}
