use crate::dto::polymarket::{PolymarketActivityPageDto, PolymarketMarketDto};
use chrono::{DateTime, Utc};

/// Small HTTP response wrapper for Polymarket reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body as UTF-8 text.
    pub body: String,
}

/// Transport abstraction for read-only HTTP GET requests.
pub trait HttpGet {
    /// Executes a GET request.
    fn get(&self, url: &str) -> Result<HttpResponse, HttpTransportError>;
}

/// Transport error returned before payload decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpTransportError {
    message: String,
}

impl HttpTransportError {
    /// Creates a new transport error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the transport error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl core::fmt::Display for HttpTransportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "http transport error: {}", self.message)
    }
}

impl std::error::Error for HttpTransportError {}

/// Read-only ureq transport for Polymarket.
pub struct UreqHttpClient;

impl HttpGet for UreqHttpClient {
    fn get(&self, url: &str) -> Result<HttpResponse, HttpTransportError> {
        let response = ureq::get(url)
            .call()
            .map_err(|err| HttpTransportError::new(err.to_string()))?;
        let status = response.status();
        let body = response
            .into_string()
            .map_err(|err| HttpTransportError::new(err.to_string()))?;

        Ok(HttpResponse { status, body })
    }
}

/// Typed Polymarket adapter errors.
#[derive(Debug)]
pub enum PolymarketAdapterError {
    Transport(HttpTransportError),
    UnexpectedStatus(u16),
    Decode(serde_json::Error),
}

impl core::fmt::Display for PolymarketAdapterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "{err}"),
            Self::UnexpectedStatus(status) => {
                write!(f, "unexpected polymarket response status: {status}")
            }
            Self::Decode(err) => write!(f, "failed to decode polymarket discovery payload: {err}"),
        }
    }
}

impl std::error::Error for PolymarketAdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(err) => Some(err),
            Self::Decode(err) => Some(err),
            Self::UnexpectedStatus(_) => None,
        }
    }
}

/// Provider-specific Polymarket discovery fetcher.
pub trait FetchPolymarketDiscovery {
    /// Fetches provider DTOs for market discovery.
    fn fetch_discovery(&self) -> Result<Vec<PolymarketMarketDto>, PolymarketAdapterError>;
}

/// Provider-specific Polymarket discovery payload with raw body.
#[derive(Debug, Clone, PartialEq)]
pub struct PolymarketDiscoveryPayload {
    /// Raw response body.
    pub raw_body: String,
    /// Decoded provider markets.
    pub markets: Vec<PolymarketMarketDto>,
    /// Fetch timestamp in UTC.
    pub fetched_at: DateTime<Utc>,
}

/// Provider-specific Polymarket discovery fetcher with raw payload access.
pub trait FetchPolymarketDiscoveryPayload {
    /// Fetches decoded discovery data together with the raw response body.
    fn fetch_discovery_payload(&self)
        -> Result<PolymarketDiscoveryPayload, PolymarketAdapterError>;
}

/// Provider-specific Polymarket activity page with raw body.
#[derive(Debug, Clone, PartialEq)]
pub struct PolymarketActivityPayload {
    /// Raw response body.
    pub raw_body: String,
    /// Decoded provider activity page.
    pub page: PolymarketActivityPageDto,
    /// Fetch timestamp in UTC.
    pub fetched_at: DateTime<Utc>,
}

/// Provider-specific Polymarket paginated activity fetcher.
pub trait FetchPolymarketActivityPayload {
    /// Fetches one activity page and returns the raw body plus decoded DTOs.
    fn fetch_activity_payload(
        &self,
        cursor: Option<&str>,
    ) -> Result<PolymarketActivityPayload, PolymarketAdapterError>;
}

/// Read-only HTTP adapter for Polymarket market discovery.
pub struct PolymarketHttpAdapter<T> {
    base_url: String,
    http_client: T,
}

impl<T> PolymarketHttpAdapter<T> {
    /// Creates a new Polymarket adapter.
    pub fn new(base_url: impl Into<String>, http_client: T) -> Self {
        Self {
            base_url: base_url.into(),
            http_client,
        }
    }

    /// Returns the discovery endpoint URL.
    pub fn discovery_url(&self) -> String {
        format!("{}/markets", self.base_url.trim_end_matches('/'))
    }

    /// Returns the activity endpoint URL.
    pub fn activity_url(&self, cursor: Option<&str>) -> String {
        let base = format!("{}/activity", self.base_url.trim_end_matches('/'));
        match cursor {
            Some(cursor) if !cursor.trim().is_empty() => format!("{base}?cursor={cursor}"),
            _ => base,
        }
    }
}

impl<T> PolymarketHttpAdapter<T>
where
    T: HttpGet,
{
    /// Parses a Polymarket discovery payload body.
    pub fn parse_discovery_payload(
        &self,
        body: &str,
    ) -> Result<Vec<PolymarketMarketDto>, PolymarketAdapterError> {
        serde_json::from_str(body).map_err(PolymarketAdapterError::Decode)
    }

    /// Parses a Polymarket activity payload body.
    pub fn parse_activity_payload(
        &self,
        body: &str,
    ) -> Result<PolymarketActivityPageDto, PolymarketAdapterError> {
        serde_json::from_str(body).map_err(PolymarketAdapterError::Decode)
    }
}

impl<T> FetchPolymarketDiscovery for PolymarketHttpAdapter<T>
where
    T: HttpGet,
{
    fn fetch_discovery(&self) -> Result<Vec<PolymarketMarketDto>, PolymarketAdapterError> {
        let response = self
            .http_client
            .get(&self.discovery_url())
            .map_err(PolymarketAdapterError::Transport)?;

        if response.status != 200 {
            return Err(PolymarketAdapterError::UnexpectedStatus(response.status));
        }

        self.parse_discovery_payload(&response.body)
    }
}

impl<T> FetchPolymarketDiscoveryPayload for PolymarketHttpAdapter<T>
where
    T: HttpGet,
{
    fn fetch_discovery_payload(
        &self,
    ) -> Result<PolymarketDiscoveryPayload, PolymarketAdapterError> {
        let response = self
            .http_client
            .get(&self.discovery_url())
            .map_err(PolymarketAdapterError::Transport)?;

        if response.status != 200 {
            return Err(PolymarketAdapterError::UnexpectedStatus(response.status));
        }

        let markets = self.parse_discovery_payload(&response.body)?;

        Ok(PolymarketDiscoveryPayload {
            raw_body: response.body,
            markets,
            fetched_at: Utc::now(),
        })
    }
}

impl<T> FetchPolymarketActivityPayload for PolymarketHttpAdapter<T>
where
    T: HttpGet,
{
    fn fetch_activity_payload(
        &self,
        cursor: Option<&str>,
    ) -> Result<PolymarketActivityPayload, PolymarketAdapterError> {
        let response = self
            .http_client
            .get(&self.activity_url(cursor))
            .map_err(PolymarketAdapterError::Transport)?;

        if response.status != 200 {
            return Err(PolymarketAdapterError::UnexpectedStatus(response.status));
        }

        let page = self.parse_activity_payload(&response.body)?;

        Ok(PolymarketActivityPayload {
            raw_body: response.body,
            page,
            fetched_at: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FetchPolymarketActivityPayload, FetchPolymarketDiscovery, HttpGet, HttpResponse,
        HttpTransportError, PolymarketAdapterError, PolymarketHttpAdapter,
    };

    struct FakeHttpClient {
        response: Result<HttpResponse, HttpTransportError>,
    }

    impl HttpGet for FakeHttpClient {
        fn get(&self, _url: &str) -> Result<HttpResponse, HttpTransportError> {
            self.response.clone()
        }
    }

    #[test]
    fn fetches_and_decodes_discovery_payload() {
        let adapter = PolymarketHttpAdapter::new(
            "https://example.test",
            FakeHttpClient {
                response: Ok(HttpResponse {
                    status: 200,
                    body: r#"[{"condition_id":"0xabc","question":"Example market","active":true}]"#
                        .into(),
                }),
            },
        );

        let markets = adapter.fetch_discovery().unwrap();
        assert_eq!(markets.len(), 1);
        assert_eq!(markets[0].condition_id, "0xabc");
    }

    #[test]
    fn rejects_malformed_discovery_payload() {
        let adapter = PolymarketHttpAdapter::new(
            "https://example.test",
            FakeHttpClient {
                response: Ok(HttpResponse {
                    status: 200,
                    body: "{not-json}".into(),
                }),
            },
        );

        let err = adapter.fetch_discovery().unwrap_err();
        assert!(matches!(err, PolymarketAdapterError::Decode(_)));
    }

    #[test]
    fn fetches_and_decodes_activity_payload() {
        let adapter = PolymarketHttpAdapter::new(
            "https://example.test",
            FakeHttpClient {
                response: Ok(HttpResponse {
                    status: 200,
                    body: r#"{"data":[{"id":"trade-1","conditionId":"0xabc","type":"trade","price":"0.51","size":"10","timestamp":"2026-04-12T00:00:00Z"}],"nextCursor":"page-2"}"#.into(),
                }),
            },
        );

        let payload = adapter.fetch_activity_payload(None).unwrap();
        assert_eq!(payload.page.activities.len(), 1);
        assert_eq!(payload.page.next_cursor.as_deref(), Some("page-2"));
    }
}
