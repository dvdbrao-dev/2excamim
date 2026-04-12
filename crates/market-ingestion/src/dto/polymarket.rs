use serde::Deserialize;

/// Polymarket market discovery response item.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PolymarketMarketDto {
    /// Provider market identifier.
    #[serde(alias = "id", alias = "conditionId")]
    pub condition_id: String,
    /// Provider title or question.
    #[serde(alias = "question", alias = "title")]
    pub question: String,
    /// Provider active flag.
    #[serde(default)]
    pub active: bool,
    /// Provider closed flag.
    #[serde(default)]
    pub closed: bool,
    /// Provider archived flag.
    #[serde(default)]
    pub archived: bool,
    /// Provider order acceptance flag.
    #[serde(default, alias = "acceptingOrders")]
    pub accepting_orders: bool,
    /// Provider best bid probability.
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub best_bid: Option<f64>,
    /// Provider best ask probability.
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub best_ask: Option<f64>,
    /// Provider last traded price or probability.
    #[serde(
        default,
        alias = "lastTradePrice",
        deserialize_with = "deserialize_optional_f64"
    )]
    pub last_trade_price: Option<f64>,
    /// Provider market volume.
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub volume: Option<f64>,
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

#[cfg(test)]
mod tests {
    use super::{PolymarketActivityPageDto, PolymarketMarketDto};

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
}
