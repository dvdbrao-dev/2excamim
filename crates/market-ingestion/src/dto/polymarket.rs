use serde::Deserialize;

/// Polymarket market discovery response item.
#[derive(Debug, Clone, PartialEq)]
pub struct PolymarketMarketDto {
    /// Provider market identifier.
    pub condition_id: String,
    /// Provider title or question.
    pub question: String,
    /// Provider active flag.
    pub active: bool,
    /// Provider closed flag.
    pub closed: bool,
    /// Provider archived flag.
    pub archived: bool,
    /// Provider order acceptance flag.
    pub accepting_orders: bool,
    /// Provider best bid probability.
    pub best_bid: Option<f64>,
    /// Provider best ask probability.
    pub best_ask: Option<f64>,
    /// Provider last traded price or probability.
    pub last_trade_price: Option<f64>,
    /// Provider market volume.
    pub volume: Option<f64>,
}

impl<'de> Deserialize<'de> for PolymarketMarketDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("expected polymarket market object"))?;

        let condition_id = pick_string(object, &["conditionId", "condition_id", "id"])
            .ok_or_else(|| serde::de::Error::custom("missing polymarket condition id"))?;
        let question = pick_string(object, &["question", "title"])
            .ok_or_else(|| serde::de::Error::custom("missing polymarket question"))?;

        Ok(Self {
            condition_id,
            question,
            active: pick_bool(object, &["active"]),
            closed: pick_bool(object, &["closed"]),
            archived: pick_bool(object, &["archived"]),
            accepting_orders: pick_bool(object, &["acceptingOrders", "accepting_orders"]),
            best_bid: pick_optional_f64(object, &["bestBid", "best_bid"])
                .map_err(serde::de::Error::custom)?,
            best_ask: pick_optional_f64(object, &["bestAsk", "best_ask"])
                .map_err(serde::de::Error::custom)?,
            last_trade_price: pick_optional_f64(object, &["lastTradePrice", "last_trade_price"])
                .map_err(serde::de::Error::custom)?,
            volume: pick_optional_f64(object, &["volume", "volumeNum", "volume_num"])
                .map_err(serde::de::Error::custom)?,
        })
    }
}

/// Polymarket paginated discovery response.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PolymarketDiscoveryPageDto {
    /// Markets in the current page.
    #[serde(alias = "data")]
    pub markets: Vec<PolymarketMarketDto>,
    /// Cursor for the next page when present.
    #[serde(default, alias = "next_cursor", alias = "nextCursor")]
    pub next_cursor: Option<String>,
}

/// Polymarket market activity response item.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PolymarketActivityDto {
    /// Provider activity identifier when available.
    #[serde(default, alias = "id", alias = "tradeID", alias = "trade_id")]
    pub activity_id: Option<String>,
    /// Provider market identifier.
    #[serde(alias = "conditionId", alias = "condition_id", alias = "market")]
    pub condition_id: String,
    /// Provider activity type.
    #[serde(alias = "type", alias = "eventType", alias = "activityType")]
    pub activity_type: String,
    /// Provider activity price when present.
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub price: Option<f64>,
    /// Provider quantity or size when present.
    #[serde(
        default,
        alias = "size",
        alias = "amount",
        deserialize_with = "deserialize_optional_f64"
    )]
    pub quantity: Option<f64>,
    /// Provider observation timestamp in RFC3339 or unix seconds.
    #[serde(alias = "timestamp", alias = "createdAt", alias = "created_at")]
    pub timestamp: String,
}

/// Polymarket paginated activity response.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PolymarketActivityPageDto {
    /// Activity items in the current page.
    #[serde(alias = "data", alias = "history", alias = "activities")]
    pub activities: Vec<PolymarketActivityDto>,
    /// Cursor for the next page when present.
    #[serde(default, alias = "nextCursor", alias = "next_cursor", alias = "cursor")]
    pub next_cursor: Option<String>,
}

fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(number)) => number
            .as_f64()
            .ok_or_else(|| serde::de::Error::custom("numeric field is not representable as f64"))
            .map(Some),
        Some(serde_json::Value::String(string)) => {
            let trimmed = string.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }

            trimmed
                .parse::<f64>()
                .map(Some)
                .map_err(|_| serde::de::Error::custom("failed to parse numeric string"))
        }
        Some(_) => Err(serde::de::Error::custom(
            "expected numeric field as number, string, or null",
        )),
    }
}

fn pick_string(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter().find_map(|key| match object.get(*key) {
        Some(serde_json::Value::String(value)) if !value.trim().is_empty() => Some(value.clone()),
        Some(serde_json::Value::Number(value)) => Some(value.to_string()),
        Some(serde_json::Value::Bool(value)) => Some(value.to_string()),
        _ => None,
    })
}

fn pick_bool(object: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> bool {
    keys.iter()
        .find_map(|key| match object.get(*key) {
            Some(serde_json::Value::Bool(value)) => Some(*value),
            Some(serde_json::Value::String(value)) => value.trim().parse::<bool>().ok(),
            Some(serde_json::Value::Number(value)) => value.as_i64().map(|n| n != 0),
            _ => None,
        })
        .unwrap_or(false)
}

fn pick_optional_f64(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Result<Option<f64>, String> {
    for key in keys {
        match object.get(*key) {
            None | Some(serde_json::Value::Null) => continue,
            Some(serde_json::Value::Number(number)) => {
                return number
                    .as_f64()
                    .ok_or_else(|| "numeric field is not representable as f64".to_string())
                    .map(Some);
            }
            Some(serde_json::Value::String(string)) => {
                let trimmed = string.trim();
                if trimmed.is_empty() {
                    return Ok(None);
                }

                return trimmed
                    .parse::<f64>()
                    .map(Some)
                    .map_err(|_| "failed to parse numeric string".to_string());
            }
            Some(_) => {
                return Err("expected numeric field as number, string, or null".to_string());
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::{PolymarketActivityPageDto, PolymarketDiscoveryPageDto, PolymarketMarketDto};

    #[test]
    fn deserializes_numeric_strings() {
        let dto: PolymarketMarketDto = serde_json::from_str(
            r#"{
                "condition_id":"0xabc",
                "question":"Will BTC close above 100k?",
                "active":true,
                "best_bid":"0.45",
                "best_ask":"0.47",
                "last_trade_price":"0.46",
                "volume":"1234.5"
            }"#,
        )
        .unwrap();

        assert_eq!(dto.best_bid, Some(0.45));
        assert_eq!(dto.best_ask, Some(0.47));
        assert_eq!(dto.last_trade_price, Some(0.46));
        assert_eq!(dto.volume, Some(1234.5));
    }

    #[test]
    fn deserializes_discovery_item_with_both_id_fields() {
        let dto: PolymarketMarketDto = serde_json::from_str(
            r#"{
                "id":"legacy-id",
                "conditionId":"0xabc",
                "question":"Will BTC close above 100k?",
                "active":true
            }"#,
        )
        .unwrap();

        assert_eq!(dto.condition_id, "0xabc");
        assert_eq!(dto.question, "Will BTC close above 100k?");
        assert!(dto.active);
    }

    #[test]
    fn deserializes_activity_page_with_cursor() {
        let page: PolymarketActivityPageDto = serde_json::from_str(
            r#"{
                "data":[
                    {
                        "id":"trade-1",
                        "conditionId":"0xabc",
                        "type":"trade",
                        "price":"0.52",
                        "size":"100",
                        "timestamp":"2026-04-12T00:00:00Z"
                    }
                ],
                "nextCursor":"page-2"
            }"#,
        )
        .unwrap();

        assert_eq!(page.activities.len(), 1);
        assert_eq!(page.activities[0].quantity, Some(100.0));
        assert_eq!(page.next_cursor.as_deref(), Some("page-2"));
    }

    #[test]
    fn deserializes_discovery_page_with_cursor() {
        let page: PolymarketDiscoveryPageDto = serde_json::from_str(
            r#"{
                "data":[
                    {
                        "condition_id":"0xabc",
                        "question":"Will BTC close above 100k?",
                        "active":true
                    }
                ],
                "next_cursor":"page-2"
            }"#,
        )
        .unwrap();

        assert_eq!(page.markets.len(), 1);
        assert_eq!(page.markets[0].condition_id, "0xabc");
        assert_eq!(page.next_cursor.as_deref(), Some("page-2"));
    }
}
